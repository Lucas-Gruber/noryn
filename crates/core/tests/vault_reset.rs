use noryn_core::{
    model::{Credential, Team},
    vault::{ResetSnapshot, Vault},
};
use std::fs;

const OLD: &str = "old-test-password-123";
const NEW: &str = "new-test-password-456";
fn populated(path: &std::path::Path) -> Vault {
    let mut vault = Vault::open(path, OLD).unwrap();
    vault
        .change(|data| {
            data.credentials.push(Credential {
                id: "private-account".into(),
                name: "Private".into(),
                username: "alice".into(),
                kind: "password".into(),
                secret: String::new(),
                password: "login-password".into(),
                passphrase: String::new(),
                team_id: None,
            });
            data.known_hosts
                .insert("server:22".into(), "SHA256:pin".into());
            data.teams.push(Team {
                id: "team".into(),
                name: "Team".into(),
                member_name: "Alice".into(),
                server_url: "https://example.invalid".into(),
                token: "server-token".into(),
                key: "team-key".into(),
                revision: None,
                dirty: true,
                live_sync: false,
                sync_base: None,
            });
            Ok(())
        })
        .unwrap();
    vault
}
#[test]
fn reset_replaces_ciphertext_and_all_local_data_with_a_new_password() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vault.json");
    let previous = temp.path().join("previous.json");
    populated(&path);
    let backup = temp.path().join("separate.noryn");
    fs::write(&backup, b"separate encrypted backup").unwrap();
    let before = fs::read(&path).unwrap();
    let snapshot = ResetSnapshot::inspect(&path, &previous).unwrap();
    let fresh = Vault::reset(&path, &previous, &snapshot, NEW, 42).unwrap();
    assert_eq!(fresh.data.local_revision, 42);
    assert!(
        fresh.data.nodes.is_empty()
            && fresh.data.credentials.is_empty()
            && fresh.data.teams.is_empty()
    );
    assert!(
        fresh.data.bindings.is_empty()
            && fresh.data.known_hosts.is_empty()
            && fresh.data.team_activity.is_empty()
    );
    assert!(fresh.data.credential_folders.is_empty() && fresh.data.credential_locations.is_empty());
    assert_ne!(fs::read(&path).unwrap(), before);
    assert!(Vault::open(&path, OLD).is_err());
    assert!(Vault::open(&path, NEW).unwrap().data.credentials.is_empty());
    assert_eq!(fs::read(&backup).unwrap(), b"separate encrypted backup");
    assert_ne!(
        serde_json::from_slice::<serde_json::Value>(&before).unwrap()["salt"],
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap()["salt"]
    );
}
#[test]
fn reset_prevents_legacy_remigration_but_preserves_the_old_backup() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vault.json");
    let previous = temp.path().join("previous.json");
    populated(&previous);
    let backup = fs::read(&previous).unwrap();
    let snapshot = ResetSnapshot::inspect(&path, &previous).unwrap();
    Vault::reset(&path, &previous, &snapshot, NEW, 1).unwrap();
    assert_eq!(fs::read(&previous).unwrap(), backup);
    assert!(Vault::open_migrating(&path, &previous, OLD).is_err());
    assert!(Vault::open_migrating(&path, &previous, NEW)
        .unwrap()
        .data
        .teams
        .is_empty());
}
#[test]
fn changed_missing_and_invalid_sources_never_get_replaced_from_a_stale_preview() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vault.json");
    let previous = temp.path().join("previous.json");
    assert!(ResetSnapshot::inspect(&path, &previous).is_err());
    populated(&path);
    let snapshot = ResetSnapshot::inspect(&path, &previous).unwrap();
    fs::write(&path, b"another instance changed this file").unwrap();
    assert!(Vault::reset(&path, &previous, &snapshot, NEW, 1).is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        b"another instance changed this file"
    );
    fs::remove_file(&path).unwrap();
    assert!(Vault::reset(&path, &previous, &snapshot, NEW, 1).is_err());
    assert!(!path.exists());
    fs::create_dir(&path).unwrap();
    assert!(ResetSnapshot::inspect(&path, &previous).is_err());
}
#[test]
fn malformed_ciphertext_can_be_reset_without_the_forgotten_password() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vault.json");
    let previous = temp.path().join("previous.json");
    fs::write(&path, b"damaged ciphertext").unwrap();
    let snapshot = ResetSnapshot::inspect(&path, &previous).unwrap();
    assert!(Vault::reset(&path, &previous, &snapshot, "asdf", 1).is_err());
    assert!(Vault::reset(&path, &previous, &snapshot, NEW, 0).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"damaged ciphertext");
    Vault::reset(&path, &previous, &snapshot, NEW, 1).unwrap();
    assert!(Vault::open(&path, NEW).unwrap().data.nodes.is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[cfg(unix)]
#[test]
fn reset_refuses_links_instead_of_touching_an_unrelated_file() {
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("unrelated");
    fs::write(&outside, b"keep").unwrap();
    let path = temp.path().join("vault.json");
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(ResetSnapshot::inspect(&path, &temp.path().join("previous")).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"keep");
}
