use base64::{engine::general_purpose::STANDARD, Engine};
use noryn_core::{
    backup::{self, ImportDestination, Scope},
    crypto,
    model::{Team, VaultData},
    scripts::{self, Cache, Entry, Library},
    vault::Vault,
    workspace_transfer,
};
fn fixture() -> VaultData {
    let mut data = VaultData::default();
    data.teams.push(Team {
        id: "ops".into(),
        name: "Operations".into(),
        member_name: "Alice".into(),
        server_url: "https://sync.example.test".into(),
        token: "test-token-at-least-32-characters-long".into(),
        key: STANDARD.encode([17; 32]),
        revision: None,
        dirty: false,
        live_sync: false,
        sync_base: None,
    });
    data
}
fn entry(kind: &str, parent: Option<&str>, name: &str, body: &str) -> Entry {
    Entry {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id: parent.map(str::to_owned),
        kind: kind.into(),
        name: name.into(),
        description: "Team documentation".into(),
        body: body.into(),
        updated_at: 1,
        updated_by: "Alice".into(),
    }
}
fn update(data: &mut VaultData, entry: Entry) {
    let expected = scripts::fingerprint(
        data.script_libraries["ops"]
            .library
            .entries
            .iter()
            .find(|e| e.id == entry.id)
            .unwrap(),
    );
    scripts::save(data, "ops", entry, Some(&expected)).unwrap();
}
#[test]
fn folders_validate_atomically_and_personal_state_and_connection_sync_stay_separate() {
    let mut data = fixture();
    let folder = entry("folder", None, "Diagnostics", "");
    scripts::save(&mut data, "ops", folder.clone(), None).unwrap();
    let script = entry(
        "script",
        Some(&folder.id),
        "Report",
        "printf 'secret-body'\r\n",
    );
    scripts::save(&mut data, "ops", script.clone(), None).unwrap();
    assert_eq!(
        data.script_libraries["ops"]
            .library
            .entries
            .iter()
            .find(|e| e.id == script.id)
            .unwrap()
            .body,
        "printf 'secret-body'\n"
    );
    assert!(!data.view().script_libraries["ops"].origin_changed);
    data.teams[0].server_url = "https://changed.example.test".into();
    assert!(data.view().script_libraries["ops"].origin_changed);
    assert!(!data.teams[0].dirty);
    assert!(data.team_activity.is_empty());
    assert!(data.bindings.is_empty());
    let before = serde_json::to_value(&data).unwrap();
    let expected = scripts::fingerprint(
        data.script_libraries["ops"]
            .library
            .entries
            .iter()
            .find(|e| e.id == folder.id)
            .unwrap(),
    );
    let mut cycle = folder.clone();
    cycle.parent_id = Some(script.id.clone());
    assert!(scripts::save(&mut data, "ops", cycle, Some(&expected)).is_err());
    assert_eq!(serde_json::to_value(&data).unwrap(), before);
    assert!(scripts::save(&mut data, "private", script.clone(), None).is_err());
    assert!(scripts::save(&mut data, "ops", script.clone(), None).is_err());
    let history = serde_json::to_string(&data.script_libraries["ops"].library.history).unwrap();
    assert!(!history.contains("secret-body"));
    assert!(!history.contains("Team documentation"));
    let current = data.script_libraries["ops"]
        .library
        .entries
        .iter()
        .find(|e| e.id == folder.id)
        .unwrap()
        .clone();
    scripts::remove(
        &mut data,
        "ops",
        &folder.id,
        &scripts::fingerprint(&current),
    )
    .unwrap();
    assert!(data.script_libraries["ops"].library.entries[0]
        .parent_id
        .is_none());
    data.teams[0].member_name.clear();
    assert!(scripts::save(&mut data, "ops", entry("script", None, "x", "echo x"), None).is_err());
}
#[test]
fn merging_combines_independent_fields_but_never_chooses_contents_by_clock() {
    let original = entry("script", None, "Report", "echo base");
    let base = Library {
        entries: vec![original.clone()],
        history: vec![],
    };
    let mut local = base.clone();
    local.entries[0].name = "New name".into();
    let mut remote = base.clone();
    remote.entries[0].body = "echo remote".into();
    remote.entries[0].updated_at = u64::MAX;
    let merged = scripts::merge(&base, &local, &remote).unwrap();
    assert_eq!(merged.entries[0].name, "New name");
    assert_eq!(merged.entries[0].body, "echo remote");
    local.entries[0].body = "echo local".into();
    assert!(scripts::merge(&base, &local, &remote).is_err());
    assert!(scripts::merge(&base, &Library::default(), &remote).is_err());
    let both = scripts::keep_both(&local, &remote, "Bob").unwrap();
    assert_eq!(
        both.entries
            .iter()
            .find(|e| e.id == original.id)
            .unwrap()
            .body,
        "echo remote"
    );
    assert!(both
        .entries
        .iter()
        .any(|e| e.id != original.id && e.body == "echo local" && e.parent_id.is_some()));
    scripts::validate(&both).unwrap();
}
#[test]
fn encrypted_vault_backups_and_imports_keep_libraries_scoped_without_sync_bases() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = fixture();
    let folder = entry("folder", None, "Ops", "");
    let script = entry(
        "script",
        Some(&folder.id),
        "Script",
        "printf 'secret-shell-body'",
    );
    scripts::save(&mut data, "ops", folder, None).unwrap();
    scripts::save(&mut data, "ops", script.clone(), None).unwrap();
    let cache = data.script_libraries.get_mut("ops").unwrap();
    cache.base = Some(cache.library.clone());
    cache.origin = scripts::origin(&data.teams[0]);
    cache.revision = Some(format!("\"{}\"", "a".repeat(64)));
    let mut vault = Vault::open(&dir.path().join("vault.json"), "test-vault-password").unwrap();
    vault
        .change(|d| {
            *d = data.clone();
            Ok(())
        })
        .unwrap();
    drop(vault);
    let disk = std::fs::read_to_string(dir.path().join("vault.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&disk).unwrap();
    let salt: [u8; 16] = STANDARD
        .decode(value["salt"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let key = crypto::derive_key("test-vault-password", &salt).unwrap();
    let sealed: crypto::Envelope = serde_json::from_value(value["sealed"].clone()).unwrap();
    for context in [b"noryn:vault:v1", b"noryn:vault:v2", b"noryn:vault:v3"] {
        assert!(crypto::unseal(&key, &sealed, context).is_err());
    }
    assert!(crypto::unseal(&key, &sealed, b"noryn:vault:v4").is_ok());
    assert!(!disk.contains("secret-shell-body"));
    let vault = Vault::open(&dir.path().join("vault.json"), "test-vault-password").unwrap();
    assert_eq!(vault.data.script_libraries["ops"].library.entries.len(), 2);
    let team = backup::select(
        &data,
        Scope::Team {
            team_id: "ops".into(),
        },
    )
    .unwrap();
    assert_eq!(team.summary(&data).scripts, 1);
    let archive = dir.path().join("scripts.noryn");
    backup::write_new(&archive, &team, "separate-export-password").unwrap();
    assert!(!std::fs::read_to_string(&archive)
        .unwrap()
        .contains("secret-shell-body"));
    let decoded = backup::read(&archive, "separate-export-password").unwrap();
    let restored = decoded.restore(&data, "").unwrap();
    let restored_cache = &restored.script_libraries["ops"];
    assert!(
        restored_cache.suspended
            && restored_cache.base.is_none()
            && restored_cache.revision.is_none()
            && restored_cache.origin.is_empty()
    );
    assert_eq!(restored.teams[0].member_name, "Alice");
    assert_eq!(
        backup::select(&data, Scope::Private)
            .unwrap()
            .summary(&data)
            .scripts,
        0
    );
    let mut target = fixture();
    let mut other = target.teams[0].clone();
    other.id = "other".into();
    other.member_name = "Bob".into();
    target.teams.push(other);
    let imported = decoded
        .import_into(
            &target,
            &ImportDestination {
                source_workspace: "ops".into(),
                target_workspace: "other".into(),
                include_credentials: false,
            },
        )
        .unwrap();
    let copies = &imported.script_libraries["other"];
    assert!(copies.suspended);
    assert!(copies.base.is_none());
    assert!(copies
        .library
        .entries
        .iter()
        .all(|e| e.id != script.id && e.updated_by == "Bob"));
    scripts::validate(&copies.library).unwrap();
    let private = decoded
        .import_into(
            &target,
            &ImportDestination {
                source_workspace: "ops".into(),
                target_workspace: "private".into(),
                include_credentials: false,
            },
        )
        .unwrap();
    assert!(private.script_libraries.is_empty());
    let mut removed = data.clone();
    workspace_transfer::remove_team(&mut removed, "ops").unwrap();
    assert!(removed.script_libraries.is_empty());
    // A malformed cache cannot become a private script library, even with a forged team record.
    removed
        .script_libraries
        .insert("private".into(), Cache::default());
    assert!(scripts::validate_vault(&removed).is_err());
    // Ordinary snapshots never contain library bodies or acknowledged script bases.
    let snapshot = serde_json::to_string(&noryn_core::sync::snapshot(&data, "ops")).unwrap();
    assert!(!snapshot.contains("secret-shell-body"));
}
#[test]
fn malformed_parent_cycles_binary_bodies_and_global_limits_are_rejected() {
    let mut data = fixture();
    let mut first = entry("folder", None, "a", "");
    let second = entry("folder", Some(&first.id), "b", "");
    first.parent_id = Some(second.id.clone());
    assert!(scripts::validate(&Library {
        entries: vec![first, second],
        history: vec![]
    })
    .is_err());
    let mut binary = entry("script", None, "bad", "echo\0bad");
    assert!(scripts::save(&mut data, "ops", binary.clone(), None).is_err());
    binary.body = "x".repeat(scripts::MAX_BODY + 1);
    assert!(scripts::save(&mut data, "ops", binary, None).is_err());
    assert!(data.script_libraries.is_empty());
    let script = entry("script", None, "ok", "echo base");
    scripts::save(&mut data, "ops", script.clone(), None).unwrap();
    let mut edit = script;
    edit.description = "updated".into();
    update(&mut data, edit);
    assert_eq!(
        data.script_libraries["ops"]
            .library
            .history
            .last()
            .unwrap()
            .fields,
        vec!["description"]
    );
}

#[test]
fn bounded_script_history_keeps_new_local_events_without_resurrecting_pruned_acknowledged_events() {
    let change = |target: &str| scripts::Change {
        id: uuid::Uuid::new_v4().to_string(),
        actor: "Alice".into(),
        occurred_at: 1,
        action: "updated".into(),
        target: target.into(),
        fields: vec!["body".into()],
    };
    let acknowledged = change("old");
    let fresh = change("new local");
    let base = Library {
        entries: vec![],
        history: vec![acknowledged.clone()],
    };
    let local = Library {
        entries: vec![],
        history: vec![acknowledged.clone(), fresh.clone()],
    };
    let remote = Library {
        entries: vec![],
        history: (0..1000).map(|_| change("remote")).collect(),
    };
    let merged = scripts::merge(&base, &local, &remote).unwrap();
    assert_eq!(merged.history.len(), 1000);
    assert_eq!(merged.history.last().unwrap().id, fresh.id);
    assert!(!merged.history.iter().any(|c| c.id == acknowledged.id));
    let mut tampered = base.clone();
    tampered.history[0].actor = "Someone else".into();
    assert!(scripts::merge(&base, &local, &tampered).is_err());
}
