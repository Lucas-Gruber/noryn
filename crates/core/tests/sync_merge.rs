use noryn_core::{
    model::{Credential, Node},
    sync::TeamSnapshot,
    sync_merge::{self, Merged},
};

fn snapshot() -> TeamSnapshot {
    TeamSnapshot {
        commands: vec![],
        version: 3,
        team_id: "ops".into(),
        nodes: vec![Node {
            id: "host".into(),
            parent_id: None,
            workspace: "ops".into(),
            kind: "connection".into(),
            name: "Server".into(),
            protocol: "ssh".into(),
            host: "example.test".into(),
            port: 22,
            initial_path: String::new(),
            notes: String::new(),
            tags: vec![],
            favorite: false,
        }],
        credentials: vec![Credential {
            id: "account".into(),
            name: "Shared account".into(),
            username: "ops".into(),
            kind: "password".into(),
            secret: "initial password".into(),
            password: String::new(),
            passphrase: String::new(),
            team_id: Some("ops".into()),
        }],
        activity: vec![],
    }
}

#[test]
fn independent_fields_and_new_entries_merge_without_clock_based_overwrites() {
    let base = snapshot();
    let mut local = base.clone();
    let mut remote = base.clone();
    local.nodes[0].port = 2222;
    local.credentials[0].username = "alice".into();
    remote.nodes[0].notes = "remote notes".into();
    remote.credentials[0].secret = "new shared password".into();
    let mut added = remote.nodes[0].clone();
    added.id = "new-host".into();
    remote.nodes.push(added);
    let Merged::Ready(merged) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("independent changes conflicted")
    };
    assert_eq!(
        merged.nodes.iter().find(|n| n.id == "host").unwrap().port,
        2222
    );
    assert_eq!(
        merged.nodes.iter().find(|n| n.id == "host").unwrap().notes,
        "remote notes"
    );
    assert_eq!(merged.nodes.len(), 2);
    assert_eq!(merged.credentials[0].username, "alice");
    assert_eq!(merged.credentials[0].secret, "new shared password");
}

#[test]
fn conflicting_secret_values_are_retained_and_never_in_conflict_metadata() {
    let base = snapshot();
    let mut local = base.clone();
    let mut remote = base.clone();
    local.credentials[0].secret = "alice secret value".into();
    remote.credentials[0].secret = "bob secret value".into();
    local.nodes[0].notes = "alice private note".into();
    remote.nodes[0].notes = "bob private note".into();
    let Merged::Conflicts(conflicts) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("conflict was overwritten")
    };
    let metadata = serde_json::to_string(&conflicts).unwrap();
    for value in [
        "alice secret value",
        "bob secret value",
        "alice private note",
        "bob private note",
    ] {
        assert!(!metadata.contains(value));
    }
    assert!(metadata.contains("secret") && metadata.contains("notes"));
    assert_eq!(local.credentials[0].secret, "alice secret value");
}

#[test]
fn deletion_versus_edit_and_deleted_parent_with_new_child_are_conflicts() {
    let mut base = snapshot();
    let mut local = base.clone();
    let mut remote = base.clone();
    local.nodes.clear();
    remote.nodes[0].name = "Edited".into();
    assert!(matches!(
        sync_merge::merge(&base, &local, &remote).unwrap(),
        Merged::Conflicts(_)
    ));
    remote = base.clone();
    let Merged::Ready(deleted) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("unchanged entry could not be deleted")
    };
    assert!(deleted.nodes.is_empty());
    base.nodes[0].kind = "folder".into();
    local = base.clone();
    local.nodes.clear();
    remote = base.clone();
    let mut child = snapshot().nodes.remove(0);
    child.id = "child".into();
    child.parent_id = Some("host".into());
    remote.nodes.push(child);
    assert!(matches!(
        sync_merge::merge(&base, &local, &remote).unwrap(),
        Merged::Conflicts(_)
    ));
}

#[test]
fn private_or_foreign_data_cannot_enter_a_merge() {
    let base = snapshot();
    let local = base.clone();
    let mut remote = base.clone();
    remote.credentials[0].team_id = None;
    assert!(sync_merge::merge(&base, &local, &remote).is_err());
    remote = base.clone();
    remote.nodes[0].workspace = "other".into();
    assert!(sync_merge::merge(&base, &local, &remote).is_err());
}

#[test]
fn key_and_passphrase_changes_by_different_members_require_review() {
    let mut base = snapshot();
    base.credentials[0].kind = "key".into();
    base.credentials[0].passphrase = "initial passphrase".into();
    let mut local = base.clone();
    let mut remote = base.clone();
    local.credentials[0].secret = "replaced private key".into();
    remote.credentials[0].passphrase = "different passphrase".into();
    let Merged::Conflicts(conflicts) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("key and passphrase were mixed")
    };
    assert!(conflicts[0]
        .fields
        .iter()
        .any(|field| field == "authentication"));
}

#[test]
fn history_union_preserves_recorded_order_and_rejects_rewritten_ids() {
    use noryn_core::team_activity::{ChangeAction, ChangeObject, TeamChange};
    let mut base = snapshot();
    let event = TeamChange {
        id: uuid::Uuid::new_v4().to_string(),
        actor: "Alice".into(),
        occurred_at: 5000,
        action: ChangeAction::Updated,
        object: ChangeObject::Connection,
        target_id: "host".into(),
        target_name: "Server".into(),
        fields: vec![],
    };
    base.activity.push(event.clone());
    let mut local = base.clone();
    let mut remote = base.clone();
    let mut next = event.clone();
    next.id = uuid::Uuid::new_v4().to_string();
    next.actor = "Bob".into();
    next.occurred_at = 1000;
    local.activity.push(next.clone());
    let Merged::Ready(merged) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("independent history conflicted")
    };
    assert_eq!(merged.activity, vec![event, next]);
    remote.activity[0].actor = "Forged".into();
    assert!(sync_merge::merge(&base, &local, &remote).is_err());
}

#[test]
fn event_url_preserves_a_team_named_snapshot_and_requires_secure_transport() {
    let url = noryn_core::sync::events_endpoint("https://example.test/base", "snapshot").unwrap();
    assert_eq!(
        url.as_str(),
        "https://example.test/base/v1/teams/snapshot/events"
    );
    assert!(noryn_core::sync::events_endpoint("http://example.test", "ops").is_err());
}

#[test]
fn an_open_editor_keeps_remote_field_changes_and_rejects_conflicting_saved_inputs() {
    let base = snapshot().nodes.remove(0);
    let mut edited = base.clone();
    edited.notes = "my unsaved note".into();
    let mut current = base.clone();
    current.port = 2222;
    let merged = sync_merge::merge_node_edit(&base, &edited, &current).unwrap();
    assert_eq!(merged.port, 2222);
    assert_eq!(merged.notes, "my unsaved note");
    current.notes = "another member's note".into();
    let error = sync_merge::merge_node_edit(&base, &edited, &current).unwrap_err();
    assert!(
        !error.to_string().contains("my unsaved note")
            && !error.to_string().contains("another member's note")
    );
}

#[test]
fn offline_members_do_not_resurrect_pruned_history_and_displace_new_events() {
    use noryn_core::team_activity::{ChangeAction, ChangeObject, TeamChange};
    let make = |time| TeamChange {
        id: uuid::Uuid::new_v4().to_string(),
        actor: "Alice".into(),
        occurred_at: time,
        action: ChangeAction::Updated,
        object: ChangeObject::Connection,
        target_id: "host".into(),
        target_name: "Server".into(),
        fields: vec![],
    };
    let mut base = snapshot();
    base.activity = (1..=1000).map(make).collect();
    let mut local = base.clone();
    local.activity.remove(0);
    local.activity.push(make(3000));
    let local_id = local.activity.last().unwrap().id.clone();
    let mut remote = base.clone();
    remote.activity = (1001..=2000).map(make).collect();
    let Merged::Ready(merged) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("history conflicted")
    };
    assert_eq!(merged.activity.len(), 1000);
    assert_eq!(merged.activity.last().unwrap().id, local_id);
    assert!(!merged
        .activity
        .iter()
        .any(|event| base.activity.iter().any(|old| old.id == event.id)));
}
