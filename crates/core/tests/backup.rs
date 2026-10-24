use base64::{engine::general_purpose::STANDARD, Engine};
use noryn_core::{
    backup::{self, Scope},
    crypto,
    model::{Credential, CredentialFolder, Node, Team, VaultData},
    team_activity::ChangeAction,
    vault::Vault,
};

fn fixture() -> VaultData {
    let mut data = VaultData::default();
    data.teams.push(Team {
        id: "ops".into(),
        name: "Operations".into(),
        member_name: "Alice".into(),
        server_url: "https://sync.example.test".into(),
        token: "test-server-token-at-least-32-characters".into(),
        key: STANDARD.encode(crypto::random_bytes::<32>()),
        revision: Some("\"rev1\"".into()),
        dirty: false,
        live_sync: false,
        sync_base: None,
    });
    for (id, workspace) in [("private-host", "private"), ("team-host", "ops")] {
        data.nodes.push(Node {
            id: id.into(),
            parent_id: None,
            workspace: workspace.into(),
            kind: "connection".into(),
            name: id.into(),
            protocol: "ssh".into(),
            host: "secret.example.test".into(),
            port: 22,
            initial_path: String::new(),
            notes: "private note".into(),
            tags: vec![],
            favorite: false,
        });
    }
    for (id, team_id) in [("private-account", None), ("team-account", Some("ops"))] {
        data.credentials.push(Credential {
            id: id.into(),
            name: id.into(),
            username: "alice".into(),
            kind: "password".into(),
            secret: "secret-login-password".into(),
            password: "independent-sudo-password".into(),
            passphrase: String::new(),
            team_id: team_id.map(str::to_owned),
        });
    }
    data.bindings
        .insert("private-host".into(), "private-account".into());
    data.bindings
        .insert("team-host".into(), "private-account".into());
    data.credential_folders.push(CredentialFolder {
        id: "personal-folder".into(),
        parent_id: None,
        name: "Personal organization".into(),
    });
    data.credential_locations
        .insert("private-account".into(), "personal-folder".into());
    data.credential_locations
        .insert("team-account".into(), "personal-folder".into());
    data.known_hosts
        .insert("example.test:22".into(), "SHA256:test".into());
    data
}

#[test]
fn encrypted_backup_round_trip_rejects_wrong_password_tampering_and_overwrite() {
    let data = fixture();
    let backup = backup::select(&data, Scope::All).unwrap();
    let bytes = backup::encode(&backup, "independent backup password").unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for secret in [
        "secret-login-password",
        "independent-sudo-password",
        "secret.example.test",
        "Alice",
        &data.teams[0].key,
    ] {
        assert!(!text.contains(secret));
    }
    let decoded = backup::decode(&bytes, "independent backup password").unwrap();
    assert_eq!(
        serde_json::to_value(&data).unwrap(),
        serde_json::to_value(&decoded.data).unwrap()
    );
    assert!(backup::decode(&bytes, "wrong password").is_err());
    let mut corrupt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let ciphertext = corrupt["sealed"]["ciphertext"].as_str().unwrap();
    let mut ciphertext = STANDARD.decode(ciphertext).unwrap();
    ciphertext[0] ^= 1;
    corrupt["sealed"]["ciphertext"] = STANDARD.encode(ciphertext).into();
    assert!(backup::decode(
        &serde_json::to_vec(&corrupt).unwrap(),
        "independent backup password"
    )
    .is_err());
    assert_ne!(
        bytes,
        backup::encode(&backup, "independent backup password").unwrap()
    );
    assert!(backup::encode(&backup, "short").is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backup.noryn");
    backup::write_new(&path, &backup, "independent backup password").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(backup::write_new(&path, &backup, "independent backup password").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn scoped_backups_exclude_personal_team_bindings_and_other_workspaces() {
    let data = fixture();
    let team = backup::select(
        &data,
        Scope::Team {
            team_id: "ops".into(),
        },
    )
    .unwrap();
    assert_eq!(team.data.nodes.len(), 1);
    assert_eq!(team.data.credentials.len(), 1);
    assert_eq!(team.data.credentials[0].id, "team-account");
    assert!(team.data.bindings.is_empty());
    assert!(team.data.credential_folders.is_empty());
    assert!(team.data.credential_locations.is_empty());
    assert!(team.data.known_hosts.is_empty());
    assert!(team.data.teams[0].member_name.is_empty());
    let private = backup::select(&data, Scope::Private).unwrap();
    assert_eq!(private.data.nodes[0].id, "private-host");
    assert_eq!(private.data.credentials[0].id, "private-account");
    assert!(private.data.teams.is_empty());
    assert!(private.data.team_activity.is_empty());
    assert!(private.data.known_hosts.is_empty());
    assert_eq!(private.data.bindings.len(), 1);
    assert_eq!(private.data.credential_locations.len(), 1);
}

#[test]
fn team_restore_keeps_personal_identity_configuration_and_organization() {
    let source = fixture();
    let backup = backup::select(
        &source,
        Scope::Team {
            team_id: "ops".into(),
        },
    )
    .unwrap();
    let mut current = source.clone();
    current.teams[0].member_name = "Bob".into();
    current.teams[0].token = "bobs-new-server-token-at-least-32-characters".into();
    current.nodes[1].name = "local change".into();
    let restored = backup.restore(&current, "Ignored local override").unwrap();
    assert_eq!(restored.teams[0].member_name, "Bob");
    assert_eq!(restored.teams[0].token, current.teams[0].token);
    assert!(restored.teams[0].dirty);
    assert!(!restored.teams[0].live_sync);
    assert!(restored.teams[0].sync_base.is_none());
    assert_eq!(restored.bindings, current.bindings);
    assert_eq!(restored.credential_locations, current.credential_locations);
    assert_eq!(restored.nodes[0], current.nodes[0]);
    assert_eq!(restored.team_activity["ops"].last().unwrap().actor, "Bob");
    assert_eq!(
        restored.team_activity["ops"].last().unwrap().action,
        ChangeAction::Imported
    );
    assert!(backup.restore(&VaultData::default(), "").is_err());
    let fresh = backup.restore(&VaultData::default(), "Charlie").unwrap();
    assert_eq!(fresh.teams[0].member_name, "Charlie");
    assert!(fresh.bindings.is_empty());
}

#[test]
fn private_restore_and_collisions_are_validated_without_mutation() {
    let source = fixture();
    let backup = backup::select(&source, Scope::Private).unwrap();
    let mut current = source.clone();
    current.nodes[0].name = "changed".into();
    let restored = backup.restore(&current, "").unwrap();
    assert_eq!(
        restored
            .nodes
            .iter()
            .find(|n| n.workspace == "private")
            .unwrap()
            .name,
        "private-host"
    );
    assert_eq!(restored.teams[0].dirty, current.teams[0].dirty);
    assert_eq!(
        restored.credential_locations["team-account"],
        "personal-folder"
    );
    current.nodes[0].workspace = "ops".into();
    let before = serde_json::to_value(&current).unwrap();
    assert!(backup.restore(&current, "").is_err());
    assert_eq!(serde_json::to_value(&current).unwrap(), before);
    let mut invalid = fixture();
    invalid.credentials.push(invalid.credentials[0].clone());
    assert!(backup::select(&invalid, Scope::All).is_err());
    invalid = fixture();
    invalid
        .bindings
        .insert("missing".into(), "private-account".into());
    // Full backups must reject invalid bindings, rather than silently dropping them.
    assert!(backup::validate(&invalid).is_err());
}

#[test]
fn restored_data_is_saved_with_the_current_vault_password() {
    let backup = backup::select(&fixture(), Scope::All).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.json");
    let mut vault = Vault::open(&path, "current vault password").unwrap();
    let restored = backup.restore(&vault.data, "Bob").unwrap();
    vault
        .change(|data| {
            *data = restored;
            Ok(())
        })
        .unwrap();
    drop(vault);
    let vault = Vault::open(&path, "current vault password").unwrap();
    assert_eq!(vault.data.credentials.len(), 2);
    assert!(Vault::open(&path, "independent backup password").is_err());
}

#[test]
fn sync_cache_never_leaves_in_an_export_or_frontend_view() {
    let mut data = fixture();
    data.teams[0].sync_base =
        Some(serde_json::to_string(&noryn_core::sync::snapshot(&data, "ops")).unwrap());
    assert!(!serde_json::to_string(&data.view())
        .unwrap()
        .contains("secret-login-password"));
    assert!(backup::select(&data, Scope::All).unwrap().data.teams[0]
        .sync_base
        .is_none());
    assert!(backup::select(
        &data,
        Scope::Team {
            team_id: "ops".into()
        }
    )
    .unwrap()
    .data
    .teams[0]
        .sync_base
        .is_none());
}

#[test]
fn vault_revisions_increase_and_cannot_be_reset_by_restored_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = Vault::open(&dir.path().join("vault.json"), "current vault password").unwrap();
    assert_eq!(vault.data.local_revision, 0);
    vault
        .change(|data| {
            data.local_revision = u64::MAX;
            Ok(())
        })
        .unwrap();
    assert_eq!(vault.data.local_revision, 1);
    assert!(vault
        .change(|_| -> anyhow::Result<()> { anyhow::bail!("failed change") })
        .is_err());
    assert_eq!(vault.data.local_revision, 1);
    vault
        .change(|data| {
            data.local_revision = 0;
            Ok(())
        })
        .unwrap();
    assert_eq!(vault.data.view().local_revision, 2);
}
