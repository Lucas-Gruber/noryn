use noryn_core::{
    crypto,
    model::{Credential, CredentialFolder, Node, Team, VaultData},
    sync::{self, TeamSnapshot},
    vault::Vault,
};

fn node(id: &str, parent: Option<&str>, workspace: &str, folder: bool) -> Node {
    Node {
        id: id.into(),
        parent_id: parent.map(str::to_string),
        workspace: workspace.into(),
        kind: if folder { "folder" } else { "connection" }.into(),
        name: id.into(),
        protocol: "ssh".into(),
        host: if folder { "" } else { "example.test" }.into(),
        port: 22,
        initial_path: String::new(),
        notes: "team note".into(),
        tags: vec!["production".into()],
        favorite: false,
    }
}

fn credential(id: &str, username: &str) -> Credential {
    Credential {
        id: id.into(),
        name: "local identity".into(),
        username: username.into(),
        kind: "password".into(),
        secret: "never-sync-this-password".into(),
        passphrase: "".into(),
        password: String::new(),
        team_id: None,
    }
}

#[test]
fn file_start_folders_are_backward_compatible_validated_and_shared() {
    let mut legacy = serde_json::to_value(node("host", None, "ops", false)).unwrap();
    legacy.as_object_mut().unwrap().remove("initialPath");
    assert!(serde_json::from_value::<Node>(legacy)
        .unwrap()
        .initial_path
        .is_empty());
    let mut entry = node("file-host", None, "ops", false);
    entry.initial_path = "/foldery/folderx".into();
    assert!(noryn_core::model::validate_nodes(&[entry.clone()]).is_err());
    for protocol in ["sftp", "ftp", "ftps"] {
        entry.protocol = protocol.into();
        noryn_core::model::validate_nodes(&[entry.clone()]).unwrap();
    }
    for invalid in ["relative/path", "/path\r\nDELE /file", "/path\0"] {
        let mut invalid_entry = entry.clone();
        invalid_entry.initial_path = invalid.into();
        assert!(noryn_core::model::validate_nodes(&[invalid_entry]).is_err());
    }
    let mut sender = data();
    sender.nodes.push(entry);
    let snapshot = sync::snapshot(&sender, "ops");
    let mut receiver = data();
    sync::apply_snapshot(&mut receiver, "ops", snapshot).unwrap();
    assert_eq!(
        receiver
            .nodes
            .iter()
            .find(|n| n.id == "file-host")
            .unwrap()
            .initial_path,
        "/foldery/folderx"
    );
    assert!(!receiver.bindings.contains_key("file-host"));
}

#[test]
fn legacy_password_accounts_and_combined_key_password_accounts_remain_private() {
    let legacy: Credential = serde_json::from_value(serde_json::json!({"id":"old", "name":"Legacy", "username":"alice", "kind":"password", "secret":"old-login-password", "passphrase":""})).unwrap();
    assert_eq!(legacy.login_password(), "old-login-password");
    let mut combined = credential("combined", "alice");
    combined.kind = "key".into();
    combined.secret = "encrypted-private-key-material".into();
    combined.passphrase = "key-decryption-passphrase".into();
    combined.password = "separate-user-password".into();
    assert_eq!(combined.login_password(), "separate-user-password");
    let mut vault = VaultData::default();
    vault.credentials.push(combined);
    let view = serde_json::to_string(&vault.view()).unwrap();
    assert!(vault.view().credentials[0].has_password);
    for secret in [
        "encrypted-private-key-material",
        "key-decryption-passphrase",
        "separate-user-password",
    ] {
        assert!(!view.contains(secret));
    }
}

fn data() -> VaultData {
    VaultData {
        local_revision: 0,
        nodes: vec![
            node("folder", None, "ops", true),
            node("host", Some("folder"), "ops", false),
            node("personal", None, "private", false),
        ],
        credentials: vec![credential("alice", "alice"), credential("bob", "bob")],
        bindings: [("folder".into(), "alice".into())].into(),
        known_hosts: [("example.test:22".into(), "local-pin".into())].into(),
        credential_folders: vec![],
        credential_locations: Default::default(),
        team_activity: Default::default(),
        teams: vec![Team {
            id: "ops".into(),
            name: "Operations".into(),
            member_name: "Alice".into(),
            server_url: "https://sync.example.test".into(),
            token: "private-server-token".into(),
            key: "private-encryption-key".into(),
            revision: None,
            dirty: false,
            live_sync: false,
            sync_base: None,
        }],
    }
}

#[test]
fn authenticated_encryption_rejects_tampering_wrong_key_and_wrong_team() {
    let key = crypto::random_bytes();
    let mut envelope = crypto::seal(&key, b"private data", b"team-a").unwrap();
    assert_eq!(
        &*crypto::unseal(&key, &envelope, b"team-a").unwrap(),
        b"private data"
    );
    assert!(crypto::unseal(&[1; 32], &envelope, b"team-a").is_err());
    assert!(crypto::unseal(&key, &envelope, b"team-b").is_err());
    envelope.ciphertext.replace_range(..4, "AAAA");
    assert!(crypto::unseal(&key, &envelope, b"team-a").is_err());
}

#[test]
fn encryption_never_reuses_a_nonce() {
    let a = crypto::seal(&[5; 32], b"same", b"context").unwrap();
    let b = crypto::seal(&[5; 32], b"same", b"context").unwrap();
    assert_ne!(a.nonce, b.nonce);
    assert_ne!(a.ciphertext, b.ciphertext);
}

#[test]
fn shared_snapshot_has_no_credentials_bindings_private_entries_or_pins() {
    let snapshot = sync::snapshot(&data(), "ops");
    let text = serde_json::to_string(&snapshot).unwrap();
    for private in [
        "credentials",
        "bindings",
        "alice",
        "bob",
        "personal",
        "local-pin",
        "never-sync",
        "private-server-token",
        "private-encryption-key",
    ] {
        assert!(!text.contains(private), "snapshot leaked {private}");
    }
    assert_eq!(snapshot.nodes.len(), 2);
}

#[test]
fn identity_inherits_and_can_be_overridden_without_marking_team_dirty() {
    let mut data = data();
    assert_eq!(data.credential_for("host").unwrap().username, "alice");
    data.bindings.insert("host".into(), "bob".into());
    assert_eq!(data.credential_for("host").unwrap().username, "bob");
    assert!(!data.teams[0].dirty);
    assert_eq!(sync::snapshot(&data, "ops").nodes.len(), 2);
}

#[test]
fn members_keep_their_own_accounts_after_sync() {
    let alice = data();
    let mut bob = data();
    bob.bindings.insert("folder".into(), "bob".into());
    sync::apply_snapshot(&mut bob, "ops", sync::snapshot(&alice, "ops")).unwrap();
    assert_eq!(bob.credential_for("host").unwrap().username, "bob");
    assert_eq!(alice.credential_for("host").unwrap().username, "alice");
    assert!(bob.nodes.iter().any(|n| n.id == "personal"));
    assert_eq!(bob.known_hosts["example.test:22"], "local-pin");
}

#[test]
fn only_explicit_team_credentials_sync_and_bindings_remain_personal() {
    let mut alice = data();
    let mut shared = credential("shared-account", "shared-user");
    shared.team_id = Some("ops".into());
    alice.upsert_credential(shared).unwrap();
    assert!(alice.teams[0].dirty);
    alice
        .bindings
        .insert("host".into(), "shared-account".into());
    let snapshot = sync::snapshot(&alice, "ops");
    assert_eq!(snapshot.credentials.len(), 1);
    assert_eq!(snapshot.credentials[0].id, "shared-account");
    let mut bob = data();
    bob.bindings.insert("folder".into(), "bob".into());
    sync::apply_snapshot(&mut bob, "ops", snapshot).unwrap();
    assert_eq!(bob.credential_for("host").unwrap().username, "bob");
    assert!(bob.credentials.iter().any(|c| c.id == "shared-account"));
    assert!(!serde_json::to_string(&bob.view())
        .unwrap()
        .contains("never-sync-this-password"));
    let mut collision = sync::snapshot(&alice, "ops");
    collision.credentials[0].id = "bob".into();
    assert!(sync::apply_snapshot(&mut bob, "ops", collision).is_err());
    assert_eq!(bob.credential_for("host").unwrap().username, "bob");
    let mut foreign = sync::snapshot(&alice, "ops");
    foreign.credentials[0].team_id = None;
    assert!(sync::apply_snapshot(&mut bob, "ops", foreign).is_err());
    bob.bindings.insert("host".into(), "shared-account".into());
    alice.remove_credential("shared-account").unwrap();
    sync::apply_snapshot(&mut bob, "ops", sync::snapshot(&alice, "ops")).unwrap();
    assert!(!bob.credentials.iter().any(|c| c.id == "shared-account"));
    assert!(!bob.bindings.contains_key("host"));
    assert!(bob.credentials.iter().any(|c| c.id == "bob"));
}

#[test]
fn legacy_snapshots_cannot_remove_team_credentials_and_agents_stay_private() {
    let mut vault = data();
    let mut shared = credential("shared", "user");
    shared.team_id = Some("ops".into());
    vault.upsert_credential(shared.clone()).unwrap();
    let mut legacy = sync::snapshot(&data(), "ops");
    legacy.version = 1;
    sync::apply_snapshot(&mut vault, "ops", legacy).unwrap();
    assert!(vault.credentials.iter().any(|c| c.id == "shared"));
    shared.kind = "agent".into();
    assert!(vault.upsert_credential(shared).is_err());
}

#[test]
fn malicious_snapshot_cannot_inject_private_nodes_or_overwrite_other_workspace_ids() {
    let mut data = data();
    let before = data.nodes.clone();
    let invalid = TeamSnapshot {
        activity: vec![],
        credentials: vec![],
        version: 1,
        team_id: "ops".into(),
        nodes: vec![node("injected", None, "private", false)],
    };
    assert!(sync::apply_snapshot(&mut data, "ops", invalid).is_err());
    assert_eq!(before, data.nodes);
    let collision = TeamSnapshot {
        activity: vec![],
        credentials: vec![],
        version: 1,
        team_id: "ops".into(),
        nodes: vec![node("personal", None, "ops", false)],
    };
    assert!(sync::apply_snapshot(&mut data, "ops", collision).is_err());
    assert_eq!(before, data.nodes);
}

#[test]
fn snapshots_reject_accidental_local_fields() {
    let mut json = serde_json::to_value(sync::snapshot(&data(), "ops")).unwrap();
    json["bindings"] = serde_json::json!({"host": "local-credential"});
    assert!(serde_json::from_value::<TeamSnapshot>(json).is_err());
}

#[test]
fn tree_rejects_cycles_cross_workspace_parents_and_connection_parents() {
    let mut data = data();
    assert!(data
        .upsert_node(node("folder", Some("folder"), "ops", true))
        .is_err());
    assert!(data
        .upsert_node(node("personal", Some("folder"), "private", false))
        .is_err());
    assert!(data
        .upsert_node(node("another", Some("host"), "ops", false))
        .is_err());
    assert!(data
        .upsert_node(node("orphan", Some("missing"), "ops", false))
        .is_err());
}

#[test]
fn recursive_delete_cleans_local_bindings_and_marks_team_dirty() {
    let mut data = data();
    data.remove_node("folder").unwrap();
    assert_eq!(data.nodes.len(), 1);
    assert_eq!(data.nodes[0].id, "personal");
    assert!(data.bindings.is_empty());
    assert!(data.teams[0].dirty);
    assert_eq!(data.credentials.len(), 2);
}

#[test]
fn server_url_requires_tls_and_rejects_embedded_credentials() {
    assert!(sync::endpoint("http://sync.example.test", "ops").is_err());
    assert!(sync::endpoint("https://user:password@example.test", "ops").is_err());
    assert!(sync::endpoint("https://example.test?token=secret", "ops").is_err());
    assert!(sync::endpoint("http://localhost:8787", "ops").is_ok());
    assert!(sync::endpoint("https://sync.example.test/base", "ops")
        .unwrap()
        .as_str()
        .ends_with("/base/v1/teams/ops/snapshot"));
    assert!(sync::endpoint("https://sync.example.test", "../private").is_err());
    assert!(sync::endpoint("https://sync.example.test", "private").is_err());
}

#[test]
fn vault_round_trip_is_encrypted_and_wrong_password_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.json");
    let password = "a strong test vault passphrase";
    let mut vault = Vault::open(&path, password).unwrap();
    vault
        .change(|d| {
            *d = data();
            d.upsert_credential_folder(CredentialFolder {
                id: "personal-folder".into(),
                parent_id: None,
                name: "private credential organizer".into(),
            })?;
            d.set_credential_folder("alice", Some("personal-folder".into()))?;
            let mut edited = d
                .nodes
                .iter()
                .find(|node| node.id == "host")
                .unwrap()
                .clone();
            edited.port = 2222;
            d.upsert_node(edited)?;
            Ok(())
        })
        .unwrap();
    let bytes = std::fs::read_to_string(&path).unwrap();
    assert!(!bytes.contains("never-sync-this-password"));
    assert!(!bytes.contains("example.test"));
    assert!(!bytes.contains("private credential organizer"));
    drop(vault);
    assert!(Vault::open(&path, "incorrect password").is_err());
    let vault = Vault::open(&path, password).unwrap();
    assert_eq!(vault.data.credential_for("host").unwrap().username, "alice");
    assert_eq!(
        vault.data.credential_folders[0].name,
        "private credential organizer"
    );
    assert_eq!(vault.data.credential_locations["alice"], "personal-folder");
    assert_eq!(vault.data.team_activity["ops"][0].actor, "Alice");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn failed_vault_transaction_rolls_back_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault =
        Vault::open(&dir.path().join("vault.json"), "a strong test passphrase").unwrap();
    let result: anyhow::Result<()> = vault.change(|data| {
        data.nodes.push(node("private", None, "private", false));
        anyhow::bail!("failure")
    });
    assert!(result.is_err());
    assert!(vault.data.nodes.is_empty());
}

#[test]
fn old_vault_migrates_without_overwriting_backup_or_reviving_stale_data() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::fs;
    let directory = tempfile::tempdir().unwrap();
    let previous = directory.path().join("old/vault.json");
    let current = directory.path().join("dev.noryn.client/vault.json");
    let password = "migration-test-password";
    let salt = [31; 16];
    let key = crypto::derive_key(password, &salt).unwrap();
    let original = data();
    let sealed = crypto::seal(
        &key,
        &serde_json::to_vec(&original).unwrap(),
        noryn_core::migration::PREVIOUS_VAULT_CONTEXT,
    )
    .unwrap();
    let bytes =
        serde_json::to_vec(&serde_json::json!({"salt": STANDARD.encode(salt), "sealed": sealed}))
            .unwrap();
    fs::create_dir_all(previous.parent().unwrap()).unwrap();
    fs::write(&previous, &bytes).unwrap();
    assert!(Vault::open_migrating(&current, &previous, "wrong-password").is_err());
    assert!(!current.exists());
    let mut migrated = Vault::open_migrating(&current, &previous, password).unwrap();
    assert_eq!(
        migrated.data.credentials[0].secret,
        original.credentials[0].secret
    );
    assert_eq!(migrated.data.bindings, original.bindings);
    assert_eq!(fs::read(&previous).unwrap(), bytes);
    let file: serde_json::Value = serde_json::from_slice(&fs::read(&current).unwrap()).unwrap();
    let envelope = serde_json::from_value(file["sealed"].clone()).unwrap();
    crypto::unseal(&key, &envelope, b"noryn:vault:v1").unwrap();
    assert!(crypto::unseal(
        &key,
        &envelope,
        noryn_core::migration::PREVIOUS_VAULT_CONTEXT
    )
    .is_err());
    migrated
        .change(|data| {
            data.nodes.clear();
            Ok(())
        })
        .unwrap();
    assert!(Vault::open_migrating(&current, &previous, password)
        .unwrap()
        .data
        .nodes
        .is_empty());
    assert!(Vault::open_migrating(&current, &previous, "wrong-password").is_err());
    assert_eq!(fs::read(&previous).unwrap(), bytes);
}

#[test]
fn credential_folders_are_personal_and_never_change_connection_identity_or_team_sharing() {
    let mut alice = data();
    let mut shared = credential("shared-account", "shared-user");
    shared.team_id = Some("ops".into());
    alice.upsert_credential(shared).unwrap();
    alice.teams[0].dirty = false;
    let root = CredentialFolder {
        id: "organizer-root".into(),
        parent_id: None,
        name: "Never sync this folder".into(),
    };
    let child = CredentialFolder {
        id: "organizer-child".into(),
        parent_id: Some(root.id.clone()),
        name: "Nested folder".into(),
    };
    alice.upsert_credential_folder(root.clone()).unwrap();
    alice.upsert_credential_folder(child.clone()).unwrap();
    alice
        .set_credential_folder("alice", Some(child.id.clone()))
        .unwrap();
    alice
        .set_credential_folder("shared-account", Some(root.id.clone()))
        .unwrap();
    let before = serde_json::to_value(&alice.credentials).unwrap();
    let bindings = alice.bindings.clone();
    assert!(!alice.teams[0].dirty);
    assert_eq!(alice.credential_for("host").unwrap().username, "alice");
    let encoded = serde_json::to_string(&sync::snapshot(&alice, "ops")).unwrap();
    for private in [
        "credentialFolders",
        "credentialLocations",
        "organizer-root",
        "organizer-child",
        "Never sync this folder",
    ] {
        assert!(!encoded.contains(private));
    }
    alice.remove_credential_folder(&root.id).unwrap();
    assert_eq!(alice.credential_folders[0].parent_id, None);
    assert_eq!(alice.credential_locations["alice"], child.id);
    assert!(!alice.credential_locations.contains_key("shared-account"));
    alice.remove_credential_folder(&child.id).unwrap();
    assert!(alice.credential_locations.is_empty());
    assert_eq!(before, serde_json::to_value(&alice.credentials).unwrap());
    assert_eq!(bindings, alice.bindings);
    assert!(!alice.teams[0].dirty);

    let mut bob = data();
    sync::apply_snapshot(&mut bob, "ops", sync::snapshot(&alice, "ops")).unwrap();
    bob.upsert_credential_folder(root.clone()).unwrap();
    bob.set_credential_folder("shared-account", Some(root.id.clone()))
        .unwrap();
    bob.set_credential_folder("bob", Some(root.id.clone()))
        .unwrap();
    sync::apply_snapshot(&mut bob, "ops", sync::snapshot(&alice, "ops")).unwrap();
    assert_eq!(bob.credential_locations["shared-account"], root.id);
    assert_eq!(bob.credential_for("host").unwrap().username, "alice");
    alice.remove_credential("shared-account").unwrap();
    sync::apply_snapshot(&mut bob, "ops", sync::snapshot(&alice, "ops")).unwrap();
    assert!(!bob.credential_locations.contains_key("shared-account"));
    assert_eq!(bob.credential_locations["bob"], root.id);
    assert_eq!(bob.credential_folders, vec![root]);
}

#[test]
fn credential_folder_validation_rejects_cycles_missing_targets_and_excessive_depth() {
    let mut vault = data();
    let root = CredentialFolder {
        id: "root".into(),
        parent_id: None,
        name: "Root".into(),
    };
    vault.upsert_credential_folder(root.clone()).unwrap();
    vault
        .upsert_credential_folder(CredentialFolder {
            id: "child".into(),
            parent_id: Some("root".into()),
            name: "Child".into(),
        })
        .unwrap();
    assert!(vault
        .upsert_credential_folder(CredentialFolder {
            parent_id: Some("child".into()),
            ..root.clone()
        })
        .is_err());
    assert_eq!(vault.credential_folders[0], root);
    assert!(vault
        .upsert_credential_folder(CredentialFolder {
            id: "missing-parent".into(),
            parent_id: Some("missing".into()),
            name: "Missing".into()
        })
        .is_err());
    assert!(vault
        .set_credential_folder("alice", Some("missing".into()))
        .is_err());
    assert!(vault
        .set_credential_folder("missing", Some("root".into()))
        .is_err());
    assert!(vault.remove_credential_folder("missing").is_err());
    let mut parent = "root".to_string();
    for index in 1..=32 {
        let id = format!("depth-{index}");
        vault
            .upsert_credential_folder(CredentialFolder {
                id: id.clone(),
                parent_id: Some(parent),
                name: id.clone(),
            })
            .unwrap();
        parent = id;
    }
    assert!(vault
        .upsert_credential_folder(CredentialFolder {
            id: "too-deep".into(),
            parent_id: Some(parent),
            name: "Too deep".into()
        })
        .is_err());
    noryn_core::credential_folders::validate(&vault).unwrap();
}

#[test]
fn existing_vaults_default_to_an_empty_credential_organization() {
    let mut old = serde_json::to_value(data()).unwrap();
    old.as_object_mut().unwrap().remove("credentialFolders");
    old.as_object_mut().unwrap().remove("credentialLocations");
    let loaded: VaultData = serde_json::from_value(old).unwrap();
    assert!(loaded.credential_folders.is_empty());
    assert!(loaded.credential_locations.is_empty());
    assert_eq!(loaded.credential_for("host").unwrap().username, "alice");
    noryn_core::credential_folders::validate(&loaded).unwrap();
}

#[test]
fn team_changes_require_a_name_track_actual_fields_and_keep_secret_values_out_of_history() {
    use noryn_core::team_activity::{ChangeAction, ChangedField};
    let mut vault = data();
    vault.teams[0].member_name.clear();
    let before = serde_json::to_value(&vault).unwrap();
    let mut edited = vault.nodes.iter().find(|n| n.id == "host").unwrap().clone();
    edited.port = 2222;
    assert!(vault
        .upsert_node(edited.clone())
        .unwrap_err()
        .to_string()
        .contains("Anzeigenamen"));
    assert_eq!(before, serde_json::to_value(&vault).unwrap());
    assert!(vault.remove_node("folder").is_err());
    let mut account = credential("shared", "shared-user");
    account.team_id = Some("ops".into());
    assert!(vault.upsert_credential(account.clone()).is_err());
    assert_eq!(before, serde_json::to_value(&vault).unwrap());
    vault.teams[0].member_name = "Alice Example".into();
    vault.upsert_node(edited.clone()).unwrap();
    assert_eq!(vault.team_activity["ops"][0].actor, "Alice Example");
    assert_eq!(
        vault.team_activity["ops"][0].fields,
        vec![ChangedField::Port]
    );
    assert!(vault.team_activity["ops"][0].occurred_at > 0);
    vault.teams[0].dirty = false;
    vault.upsert_node(edited).unwrap();
    assert_eq!(vault.team_activity["ops"].len(), 1);
    assert!(!vault.teams[0].dirty);
    vault.upsert_credential(account.clone()).unwrap();
    assert_eq!(vault.team_activity["ops"][1].action, ChangeAction::Created);
    account.secret = "old-and-new-password-must-not-be-in-history".into();
    vault.upsert_credential(account.clone()).unwrap();
    assert_eq!(
        vault.team_activity["ops"][2].fields,
        vec![ChangedField::Password]
    );
    let history = serde_json::to_string(&vault.team_activity).unwrap();
    assert!(!history.contains("never-sync-this-password"));
    assert!(!history.contains("old-and-new-password"));
    let mut bob = data();
    bob.teams[0].member_name = "Bob Example".into();
    sync::apply_snapshot(&mut bob, "ops", sync::snapshot(&vault, "ops")).unwrap();
    assert_eq!(bob.teams[0].member_name, "Bob Example");
    assert_eq!(bob.team_activity["ops"], vault.team_activity["ops"]);
    bob.remove_node("host").unwrap();
    assert_eq!(
        bob.team_activity["ops"].last().unwrap().actor,
        "Bob Example"
    );
    assert_eq!(
        bob.team_activity["ops"].last().unwrap().action,
        ChangeAction::Deleted
    );
    assert_eq!(bob.team_activity["ops"][0].actor, "Alice Example");
    assert_eq!(sync::snapshot(&bob, "ops").version, 3);
}

#[test]
fn malformed_history_is_rejected_atomically_and_old_snapshots_keep_known_history() {
    let mut alice = data();
    let mut edited = alice.nodes[1].clone();
    edited.notes = "Changed".into();
    alice.upsert_node(edited).unwrap();
    let mut bob = data();
    let before = serde_json::to_value(&bob).unwrap();
    let mut invalid = sync::snapshot(&alice, "ops");
    invalid.nodes[0].name = "Must not replace local data".into();
    invalid.activity[0].actor = "   ".into();
    assert!(sync::apply_snapshot(&mut bob, "ops", invalid).is_err());
    assert_eq!(serde_json::to_value(&bob).unwrap(), before);
    let mut duplicate = sync::snapshot(&alice, "ops");
    duplicate.activity.push(duplicate.activity[0].clone());
    assert!(sync::apply_snapshot(&mut bob, "ops", duplicate).is_err());
    assert_eq!(serde_json::to_value(&bob).unwrap(), before);
    let mut raw = serde_json::to_value(sync::snapshot(&alice, "ops")).unwrap();
    raw["activity"][0]["password"] = "must-not-be-a-log-field".into();
    assert!(serde_json::from_value::<TeamSnapshot>(raw).is_err());
    sync::apply_snapshot(&mut bob, "ops", sync::snapshot(&alice, "ops")).unwrap();
    let known = bob.team_activity["ops"].clone();
    let mut legacy = sync::snapshot(&alice, "ops");
    legacy.version = 2;
    assert!(sync::apply_snapshot(&mut bob, "ops", legacy).is_err());
    let mut legacy = sync::snapshot(&alice, "ops");
    legacy.version = 2;
    legacy.activity.clear();
    sync::apply_snapshot(&mut bob, "ops", legacy).unwrap();
    assert_eq!(bob.team_activity["ops"], known);
    let mut old_vault = serde_json::to_value(data()).unwrap();
    old_vault.as_object_mut().unwrap().remove("teamActivity");
    old_vault["teams"][0]
        .as_object_mut()
        .unwrap()
        .remove("memberName");
    let loaded: VaultData = serde_json::from_value(old_vault).unwrap();
    assert!(loaded.teams[0].member_name.is_empty());
    assert!(loaded.team_activity.is_empty());
}

#[test]
fn history_is_bounded_and_personal_organization_does_not_add_events() {
    use noryn_core::team_activity::MAX_CHANGES;
    let mut vault = data();
    let mut edited = vault.nodes[1].clone();
    edited.notes = "First".into();
    vault.upsert_node(edited.clone()).unwrap();
    let first_id = vault.team_activity["ops"][0].id.clone();
    for index in 0..MAX_CHANGES {
        edited.notes = format!("Edit {index}");
        vault.upsert_node(edited.clone()).unwrap();
    }
    assert_eq!(vault.team_activity["ops"].len(), MAX_CHANGES);
    assert!(vault.team_activity["ops"]
        .iter()
        .all(|entry| entry.id != first_id));
    let before = vault.team_activity.clone();
    vault.teams[0].dirty = false;
    vault
        .upsert_credential_folder(CredentialFolder {
            id: "folder-for-accounts".into(),
            parent_id: None,
            name: "My accounts".into(),
        })
        .unwrap();
    vault
        .set_credential_folder("alice", Some("folder-for-accounts".into()))
        .unwrap();
    vault.bindings.insert("host".into(), "bob".into());
    let mut private = vault
        .nodes
        .iter()
        .find(|n| n.id == "personal")
        .unwrap()
        .clone();
    private.name = "Personal change".into();
    vault.upsert_node(private).unwrap();
    assert_eq!(before, vault.team_activity);
    assert!(!vault.teams[0].dirty);
    noryn_core::team_activity::validate_changes(&vault.team_activity["ops"]).unwrap();
}
