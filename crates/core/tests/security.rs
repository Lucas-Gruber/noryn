use harbor_core::{
    crypto,
    model::{Credential, Node, Team, VaultData},
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
    assert!(harbor_core::model::validate_nodes(&[entry.clone()]).is_err());
    for protocol in ["sftp", "ftp", "ftps"] {
        entry.protocol = protocol.into();
        harbor_core::model::validate_nodes(&[entry.clone()]).unwrap();
    }
    for invalid in ["relative/path", "/path\r\nDELE /file", "/path\0"] {
        let mut invalid_entry = entry.clone();
        invalid_entry.initial_path = invalid.into();
        assert!(harbor_core::model::validate_nodes(&[invalid_entry]).is_err());
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
        nodes: vec![
            node("folder", None, "ops", true),
            node("host", Some("folder"), "ops", false),
            node("personal", None, "private", false),
        ],
        credentials: vec![credential("alice", "alice"), credential("bob", "bob")],
        bindings: [("folder".into(), "alice".into())].into(),
        known_hosts: [("example.test:22".into(), "local-pin".into())].into(),
        teams: vec![Team {
            id: "ops".into(),
            name: "Operations".into(),
            server_url: "https://sync.example.test".into(),
            token: "private-server-token".into(),
            key: "private-encryption-key".into(),
            revision: None,
            dirty: false,
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
fn malicious_snapshot_cannot_inject_private_nodes_or_overwrite_other_workspace_ids() {
    let mut data = data();
    let before = data.nodes.clone();
    let invalid = TeamSnapshot {
        version: 1,
        team_id: "ops".into(),
        nodes: vec![node("injected", None, "private", false)],
    };
    assert!(sync::apply_snapshot(&mut data, "ops", invalid).is_err());
    assert_eq!(before, data.nodes);
    let collision = TeamSnapshot {
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
            Ok(())
        })
        .unwrap();
    let bytes = std::fs::read_to_string(&path).unwrap();
    assert!(!bytes.contains("never-sync-this-password"));
    assert!(!bytes.contains("example.test"));
    drop(vault);
    assert!(Vault::open(&path, "incorrect password").is_err());
    let vault = Vault::open(&path, password).unwrap();
    assert_eq!(vault.data.credential_for("host").unwrap().username, "alice");
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
