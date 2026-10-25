use base64::{engine::general_purpose::STANDARD, Engine};
use noryn_core::{
    backup::{self, ImportDestination, Scope},
    crypto,
    model::{Credential, CredentialFolder, Node, Team, VaultData},
    team_activity::ChangeAction,
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
fn workspace_copies_remap_complete_forests_without_accounts_or_personal_bindings() {
    for (from, to) in [("private", "ops"), ("ops", "private"), ("ops", "other")] {
        let mut data = fixture();
        let mut other = data.teams[0].clone();
        other.id = "other".into();
        other.name = "Other team".into();
        other.member_name = "Bob".into();
        data.teams.push(other);
        let parent = data.nodes.iter_mut().find(|n| n.workspace == from).unwrap();
        parent.kind = "folder".into();
        let parent_id = parent.id.clone();
        let mut child = parent.clone();
        child.id = "child".into();
        child.name = "Nested host".into();
        child.kind = "connection".into();
        child.parent_id = Some(parent_id.clone());
        child.tags = vec!["production".into()];
        data.nodes.push(child.clone());
        let original = data.nodes.clone();
        let bindings = data.bindings.clone();
        let accounts = data.credentials.clone();
        let roots = workspace_transfer::copy_nodes(&mut data, &[parent_id], to, None).unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(&data.nodes[..original.len()], original.as_slice());
        let copies = &data.nodes[original.len()..];
        assert_eq!(copies.len(), 2);
        assert!(copies
            .iter()
            .all(|n| n.workspace == to && !original.iter().any(|old| old.id == n.id)));
        let copied_child = copies.iter().find(|n| n.name == child.name).unwrap();
        assert_eq!(copied_child.parent_id.as_ref(), Some(&roots[0]));
        assert_eq!(copied_child.notes, child.notes);
        assert_eq!(copied_child.tags, child.tags);
        assert_eq!(data.bindings, bindings);
        assert!(data.credentials == accounts);
        if to != "private" {
            let activity = &data.team_activity[to];
            assert_eq!(activity[0].action, ChangeAction::Created);
            let text = serde_json::to_string(activity).unwrap();
            assert!(!text.contains("private note") && !text.contains("secret-login-password"));
        }
        backup::validate(&data).unwrap();
    }
}

#[test]
fn workspace_copy_rejects_bad_targets_and_missing_identity_before_mutating() {
    let mut data = fixture();
    for (source, target, parent) in [
        ("private-host", "unknown", None),
        ("missing", "ops", None),
        ("private-host", "ops", Some("private-host")),
        ("private-host", "ops", Some("team-host")),
    ] {
        let before = backup::fingerprint(&data).unwrap();
        assert!(
            workspace_transfer::copy_nodes(&mut data, &[source.into()], target, parent).is_err()
        );
        assert_eq!(backup::fingerprint(&data).unwrap(), before);
    }
    data.teams[0].member_name.clear();
    let before = backup::fingerprint(&data).unwrap();
    assert!(
        workspace_transfer::copy_nodes(&mut data, &["private-host".into()], "ops", None).is_err()
    );
    assert_eq!(backup::fingerprint(&data).unwrap(), before);
}

#[test]
fn single_children_and_overlapping_roots_copy_into_a_selected_folder() {
    let mut data = fixture();
    data.nodes[0].kind = "folder".into();
    data.nodes[1].kind = "folder".into();
    let mut child = data.nodes[0].clone();
    child.kind = "connection".into();
    child.id = "child".into();
    child.parent_id = Some("private-host".into());
    data.nodes.push(child);
    let roots =
        workspace_transfer::copy_nodes(&mut data, &["child".into()], "ops", Some("team-host"))
            .unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(
        data.nodes.last().unwrap().parent_id.as_deref(),
        Some("team-host")
    );
    let before = data.nodes.len();
    let roots = workspace_transfer::copy_nodes(
        &mut data,
        &["private-host".into(), "child".into()],
        "ops",
        Some("team-host"),
    )
    .unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(data.nodes.len(), before + 2);
    assert_eq!(
        data.nodes.last().unwrap().parent_id.as_ref(),
        Some(&roots[0])
    );
    backup::validate(&data).unwrap();
}

#[test]
fn targeted_import_shares_credentials_only_when_explicitly_selected_and_preserves_configuration() {
    let mut data = fixture();
    data.credentials[0].kind = "key".into();
    data.credentials[0].secret = "private-key-material".into();
    data.credentials[0].passphrase = "independent-key-passphrase".into();
    let archive = backup::select(&data, Scope::Private).unwrap();
    let original = backup::fingerprint(&data).unwrap();
    for include_credentials in [false, true] {
        let result = archive
            .import_into(
                &data,
                &ImportDestination {
                    source_workspace: "private".into(),
                    target_workspace: "ops".into(),
                    include_credentials,
                },
            )
            .unwrap();
        assert_eq!(result.nodes.len(), data.nodes.len() + 1);
        assert_eq!(result.nodes.last().unwrap().workspace, "ops");
        assert_ne!(result.nodes.last().unwrap().id, "private-host");
        assert_eq!(result.bindings, data.bindings);
        assert_eq!(result.credential_locations, data.credential_locations);
        assert_eq!(result.known_hosts, data.known_hosts);
        assert_eq!(
            result.credentials.len(),
            data.credentials.len() + usize::from(include_credentials)
        );
        if include_credentials {
            let shared = result.credentials.last().unwrap();
            assert_eq!(shared.team_id.as_deref(), Some("ops"));
            assert_ne!(shared.id, "private-account");
            assert_eq!(shared.login_password(), "independent-sudo-password");
            assert_eq!(shared.secret, "private-key-material");
            assert_eq!(shared.passphrase, "independent-key-passphrase");
        }
        assert_eq!(result.teams[0].key, data.teams[0].key);
        assert_eq!(result.teams[0].token, data.teams[0].token);
        assert_eq!(result.teams[0].member_name, "Alice");
        assert!(!result.teams[0].live_sync);
        assert!(result.teams[0].dirty);
        assert_eq!(
            result.team_activity["ops"].last().unwrap().action,
            ChangeAction::Imported
        );
        let history = serde_json::to_string(&result.team_activity).unwrap();
        assert!(
            !history.contains("private-key-material")
                && !history.contains("independent-key-passphrase")
                && !history.contains("independent-sudo-password")
        );
        assert_eq!(backup::fingerprint(&data).unwrap(), original);
        backup::validate(&result).unwrap();
    }
}

#[test]
fn targeted_import_selects_one_source_excludes_agents_from_teams_and_never_copies_team_secrets() {
    let mut data = fixture();
    let mut agent = data.credentials[0].clone();
    agent.id = "local-agent".into();
    agent.kind = "agent".into();
    agent.secret.clear();
    data.credentials.push(agent);
    let mut other = data.teams[0].clone();
    other.id = "other".into();
    other.name = "Other".into();
    other.key = STANDARD.encode(crypto::random_bytes::<32>());
    other.member_name = "Bob".into();
    data.teams.push(other);
    let archive = backup::select(&data, Scope::All).unwrap();
    let into = |source: &str, target: &str| ImportDestination {
        source_workspace: source.into(),
        target_workspace: target.into(),
        include_credentials: true,
    };
    let shared = archive
        .import_into(&data, &into("private", "other"))
        .unwrap();
    assert_eq!(shared.credentials.len(), data.credentials.len() + 1);
    assert_eq!(shared.teams[1].key, data.teams[1].key);
    assert!(shared
        .credentials
        .iter()
        .all(|c| c.team_id.is_none() || c.kind != "agent"));
    let private = archive.import_into(&data, &into("ops", "private")).unwrap();
    assert_eq!(private.nodes.last().unwrap().workspace, "private");
    assert!(private.credentials.last().unwrap().team_id.is_none());
    assert_eq!(private.team_activity.len(), data.team_activity.len());
    let another = archive.import_into(&data, &into("ops", "other")).unwrap();
    assert_eq!(
        another.credentials.last().unwrap().team_id.as_deref(),
        Some("other")
    );
    assert_eq!(another.teams[1].key, data.teams[1].key);
    assert!(archive.import_into(&data, &into("missing", "ops")).is_err());
    assert!(archive
        .import_into(&data, &into("private", "missing"))
        .is_err());
}

#[test]
fn leaving_team_cleans_local_references_without_removing_private_accounts_or_other_teams() {
    let mut data = fixture();
    let mut other = data.teams[0].clone();
    other.id = "other".into();
    data.teams.push(other);
    // A private connection may personally use a shared account; remove this dangling binding.
    data.bindings
        .insert("private-host".into(), "team-account".into());
    let private = data.nodes[0].clone();
    let host_pins = data.known_hosts.clone();
    workspace_transfer::remove_team(&mut data, "ops").unwrap();
    assert_eq!(data.teams.len(), 1);
    assert_eq!(data.teams[0].id, "other");
    assert!(!data.teams[0].dirty);
    assert_eq!(data.nodes, vec![private]);
    assert_eq!(data.credentials.len(), 1);
    assert_eq!(data.credentials[0].id, "private-account");
    assert!(data.bindings.is_empty());
    assert!(data.team_activity.is_empty());
    assert_eq!(data.known_hosts, host_pins);
    assert!(!data.credential_locations.contains_key("team-account"));
    assert_eq!(data.credential_folders.len(), 1);
    backup::validate(&data).unwrap();
    let before = backup::fingerprint(&data).unwrap();
    assert!(workspace_transfer::remove_team(&mut data, "missing").is_err());
    assert_eq!(backup::fingerprint(&data).unwrap(), before);
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

#[test]
fn direct_connection_credentials_stay_private_and_are_never_in_shared_nodes_or_copies() {
    use noryn_core::connection_credentials::Login;
    let mut data = fixture();
    let mut direct = data.credentials[0].clone();
    direct.secret = "direct-private-key".into();
    direct.kind = "key".into();
    direct.passphrase = "private-key-passphrase".into();
    data.set_connection_login(
        "private-host",
        Login::Direct {
            credential: direct.clone(),
        },
    )
    .unwrap();
    assert!(!data.bindings.contains_key("private-host"));
    assert!(!data.teams[0].dirty);
    assert_eq!(
        data.credential_for("private-host").unwrap().secret,
        "direct-private-key"
    );
    let view = serde_json::to_string(&data.view()).unwrap();
    assert!(
        !view.contains("direct-private-key")
            && !view.contains("private-key-passphrase")
            && !view.contains("independent-sudo-password")
    );
    assert!(data
        .set_connection_login(
            "team-host",
            Login::Direct {
                credential: direct.clone()
            }
        )
        .is_err());
    let shared = serde_json::to_string(&noryn_core::sync::snapshot(&data, "ops")).unwrap();
    assert!(!shared.contains("connectionCredentials") && !shared.contains("direct-private-key"));
    let copied =
        workspace_transfer::copy_nodes(&mut data, &["private-host".into()], "ops", None).unwrap();
    assert!(!data.connection_credentials.contains_key(&copied[0]));
    assert!(data.team_activity["ops"]
        .iter()
        .all(|e| !serde_json::to_string(e)
            .unwrap()
            .contains("direct-private-key")));
    direct.secret.clear();
    direct.passphrase.clear();
    direct.password.clear();
    data.set_connection_login("private-host", Login::Direct { credential: direct })
        .unwrap();
    assert_eq!(
        data.credential_for("private-host").unwrap().passphrase,
        "private-key-passphrase"
    );
    assert_eq!(
        data.credential_for("private-host")
            .unwrap()
            .login_password(),
        "independent-sudo-password"
    );
    data.remove_node("private-host").unwrap();
    assert!(data.connection_credentials.is_empty());
}

#[test]
fn direct_credentials_round_trip_in_private_backups_and_only_import_with_explicit_private_consent()
{
    use noryn_core::connection_credentials::Login;
    let mut data = fixture();
    data.set_connection_login(
        "private-host",
        Login::Direct {
            credential: data.credentials[0].clone(),
        },
    )
    .unwrap();
    let archive = backup::select(&data, Scope::Private).unwrap();
    assert_eq!(serde_json::to_value(&archive).unwrap()["version"], 2);
    let encrypted = backup::encode(&archive, "backup-test-password-123").unwrap();
    let archive = backup::decode(&encrypted, "backup-test-password-123").unwrap();
    let restored = archive.restore(&data, "").unwrap();
    assert!(restored.connection_credentials == data.connection_credentials);
    let shared = backup::select(
        &data,
        Scope::Team {
            team_id: "ops".into(),
        },
    )
    .unwrap();
    assert!(!serde_json::to_string(&shared)
        .unwrap()
        .contains("connectionCredentials"));
    for (target, consent, expected) in [
        ("private", true, 2),
        ("private", false, 1),
        ("ops", true, 1),
    ] {
        let imported = archive
            .import_into(
                &data,
                &ImportDestination {
                    source_workspace: "private".into(),
                    target_workspace: target.into(),
                    include_credentials: consent,
                },
            )
            .unwrap();
        assert_eq!(imported.connection_credentials.len(), expected);
        noryn_core::connection_credentials::validate(&imported).unwrap();
    }
    let mut malicious = data.clone();
    malicious.connection_credentials.insert(
        "team-host".into(),
        data.credential_for("private-host").unwrap().clone(),
    );
    assert!(backup::validate(&malicious).is_err());
    let mut invalid = serde_json::to_value(&archive).unwrap();
    invalid["version"] = 1.into();
    let invalid: backup::Backup = serde_json::from_value(invalid).unwrap();
    assert!(backup::encode(&invalid, "backup-test-password-123").is_err());
}

#[test]
fn connection_owned_secrets_survive_vault_reopen_and_cannot_be_dropped_by_old_readers() {
    use noryn_core::connection_credentials::Login;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.json");
    let password = "test-only-vault-password";
    let mut vault = Vault::open(&path, password).unwrap();
    vault
        .change(|data| {
            *data = fixture();
            data.set_connection_login(
                "private-host",
                Login::Direct {
                    credential: data.credentials[0].clone(),
                },
            )
        })
        .unwrap();
    drop(vault);
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let salt = STANDARD.decode(stored["salt"].as_str().unwrap()).unwrap();
    let key = crypto::derive_key(password, &salt).unwrap();
    let envelope: crypto::Envelope = serde_json::from_value(stored["sealed"].clone()).unwrap();
    assert!(crypto::unseal(&key, &envelope, b"noryn:vault:v1").is_err());
    assert!(crypto::unseal(&key, &envelope, b"noryn:vault:v2").is_ok());
    let mut reopened = Vault::open(&path, password).unwrap();
    assert_eq!(
        reopened
            .data
            .credential_for("private-host")
            .unwrap()
            .login_password(),
        "independent-sudo-password"
    );
    reopened
        .change(|data| {
            data.set_connection_login(
                "private-host",
                Login::Account {
                    credential_id: None,
                },
            )
        })
        .unwrap();
    assert!(Vault::open(&path, password)
        .unwrap()
        .data
        .connection_credentials
        .is_empty());
}
