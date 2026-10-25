use noryn_core::{
    backup::{self, ImportDestination, Scope},
    commands::SavedCommand,
    crypto,
    model::{Team, VaultData},
    sync,
    sync_merge::{self, Merged},
    vault::Vault,
    workspace_transfer,
};
fn command(workspace: &str, text: &str) -> SavedCommand {
    SavedCommand {
        id: uuid::Uuid::new_v4().to_string(),
        workspace: workspace.into(),
        name: "Diagnose".into(),
        description: "Nur nach Auswahl ausführen".into(),
        command: text.into(),
    }
}
fn data() -> VaultData {
    VaultData {
        teams: vec![Team {
            id: "ops".into(),
            name: "Operations".into(),
            member_name: "Alice".into(),
            server_url: "https://sync.example.test".into(),
            token: "t".repeat(32),
            key: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                crypto::random_bytes::<32>(),
            ),
            revision: None,
            dirty: false,
            live_sync: false,
            sync_base: None,
        }],
        ..Default::default()
    }
}
#[test]
fn commands_stay_in_their_scope_and_history_never_contains_command_values() {
    let mut alice = data();
    let private = command("private", "printf PRIVATE_VALUE");
    alice.save_command(private.clone()).unwrap();
    assert!(!alice.teams[0].dirty);
    let shared = command("ops", "printf SHARED_VALUE");
    alice.teams[0].member_name.clear();
    assert!(alice.save_command(shared.clone()).is_err());
    assert_eq!(alice.commands.len(), 1);
    alice.teams[0].member_name = "Alice".into();
    alice.save_command(shared.clone()).unwrap();
    let snapshot = sync::snapshot(&alice, "ops");
    assert_eq!(snapshot.version, 4);
    assert_eq!(snapshot.commands.len(), 1);
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(!json.contains("PRIVATE_VALUE"));
    assert!(!serde_json::to_string(&snapshot.activity)
        .unwrap()
        .contains("SHARED_VALUE"));
    let mut bob = data();
    bob.save_command(command("private", "printf BOB_VALUE"))
        .unwrap();
    sync::apply_snapshot(&mut bob, "ops", snapshot.clone()).unwrap();
    assert_eq!(bob.commands.len(), 2);
    let before = serde_json::to_string(&bob).unwrap();
    let mut bad = snapshot.clone();
    bad.commands[0].id = bob
        .commands
        .iter()
        .find(|c| c.workspace == "private")
        .unwrap()
        .id
        .clone();
    assert!(sync::apply_snapshot(&mut bob, "ops", bad).is_err());
    assert_eq!(serde_json::to_string(&bob).unwrap(), before);
    let mut legacy = snapshot;
    legacy.version = 3;
    legacy.commands.clear();
    legacy.activity.clear();
    sync::apply_snapshot(&mut bob, "ops", legacy).unwrap();
    assert!(
        bob.team_activity["ops"]
            .iter()
            .any(|c| c.object == noryn_core::team_activity::ChangeObject::Command),
        "Legacy snapshots must preserve real command activity"
    );
    assert_eq!(
        bob.commands.len(),
        2,
        "Old snapshots cannot silently remove newer command data"
    );
    let mut invalid = command("private", "printf '\x1b[2J'");
    assert!(bob.save_command(invalid.clone()).is_err());
    invalid.command = "echo okay".into();
    invalid.workspace = "unknown".into();
    assert!(bob.save_command(invalid).is_err());
}
#[test]
fn command_merges_preserve_independent_edits_and_stop_on_command_conflicts() {
    let mut data = data();
    data.save_command(command("ops", "echo original")).unwrap();
    let base = sync::snapshot(&data, "ops");
    let mut local = base.clone();
    let mut remote = base.clone();
    local.commands[0].name = "Local name".into();
    remote.commands[0].command = "echo remote".into();
    let Merged::Ready(result) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("Independent fields must merge")
    };
    assert_eq!(result.commands[0].name, "Local name");
    assert_eq!(result.commands[0].command, "echo remote");
    local.commands[0].command = "echo local secret".into();
    let Merged::Conflicts(conflicts) = sync_merge::merge(&base, &local, &remote).unwrap() else {
        panic!("Conflicting command values must stop")
    };
    let description = serde_json::to_string(&conflicts).unwrap();
    assert!(description.contains("command"));
    assert!(!description.contains("local secret"));
    remote.commands.clear();
    assert!(matches!(
        sync_merge::merge(&base, &local, &remote).unwrap(),
        Merged::Conflicts(_)
    ));
}
#[test]
fn command_backups_import_with_fresh_ids_and_removal_only_affects_selected_team() {
    let mut data = data();
    data.save_command(command("private", "echo private"))
        .unwrap();
    data.save_command(command("ops", "echo shared")).unwrap();
    let private = backup::select(&data, Scope::Private).unwrap();
    assert_eq!(private.data.commands.len(), 1);
    assert_eq!(private.data.commands[0].workspace, "private");
    let team = backup::select(
        &data,
        Scope::Team {
            team_id: "ops".into(),
        },
    )
    .unwrap();
    assert_eq!(team.data.commands.len(), 1);
    assert_eq!(team.data.commands[0].workspace, "ops");
    let imported = private
        .import_into(
            &data,
            &ImportDestination {
                source_workspace: "private".into(),
                target_workspace: "ops".into(),
                include_credentials: false,
            },
        )
        .unwrap();
    assert_eq!(imported.commands.len(), 3);
    assert!(imported.commands.iter().any(|c| c.workspace == "ops"
        && c.command == "echo private"
        && c.id != private.data.commands[0].id));
    let restored = private.restore(&data, "Alice").unwrap();
    assert_eq!(restored.commands.len(), 2);
    workspace_transfer::remove_team(&mut data, "ops").unwrap();
    assert_eq!(data.commands.len(), 1);
    assert_eq!(data.commands[0].workspace, "private");
}
#[test]
fn commands_survive_encrypted_vault_reopen_and_scoped_backup_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vault");
    let password = "test vault password only";
    let mut vault = Vault::open(&path, password).unwrap();
    vault
        .change(|data| data.save_command(command("private", "echo ENCRYPTED_COMMAND_ONLY")))
        .unwrap();
    let json = std::fs::read_to_string(&path).unwrap();
    assert!(!json.contains("ENCRYPTED_COMMAND_ONLY"));
    drop(vault);
    let reopened = Vault::open(&path, password).unwrap();
    assert_eq!(reopened.data.commands.len(), 1);
    let archive = backup::select(&reopened.data, Scope::All).unwrap();
    let backup_path = directory.path().join("backup.noryn");
    backup::write_new(&backup_path, &archive, "separate backup password").unwrap();
    let archive = backup::read(&backup_path, "separate backup password").unwrap();
    assert_eq!(
        archive.data.commands[0].command,
        "echo ENCRYPTED_COMMAND_ONLY"
    );
}
