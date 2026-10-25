use super::{live_sync, task, vault_action, AppState, Result};
use noryn_core::{model::VaultView, workspace_transfer};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    view: VaultView,
    roots: Vec<String>,
}

#[tauri::command]
pub async fn node_copy(
    state: State<'_, AppState>,
    source_ids: Vec<String>,
    workspace: String,
    parent_id: Option<String>,
    expected_revision: u64,
) -> Result<CopyResult> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            anyhow::ensure!(
                vault.data.local_revision == expected_revision,
                "Der Tresor wurde während der Auswahl geändert. Bitte das Kopieren erneut öffnen."
            );
            let roots = vault.change(|data| {
                workspace_transfer::copy_nodes(data, &source_ids, &workspace, parent_id.as_deref())
            })?;
            Ok(CopyResult {
                view: vault.data.view(),
                roots,
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn team_remove(
    state: State<'_, AppState>,
    team_id: String,
    expected_revision: u64,
) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        // Exclude both manual and live sync for this team before changing its configuration.
        let _job = live_sync::Job::start(&state, &team_id)?;
        vault_action(&state, |vault| {
            anyhow::ensure!(vault.data.local_revision == expected_revision, "Der Tresor wurde seit der Bestätigung geändert. Bitte das Entfernen erneut öffnen.");
            let connections = state
                .connections
                .lock()
                .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
            let auth = state
                .auth_requests
                .lock()
                .map_err(|_| anyhow::anyhow!("Anmeldungen nicht verfügbar"))?;
            anyhow::ensure!(
                connections.is_empty() && auth.is_empty(),
                "Vor dem Entfernen bitte alle Verbindungen und Anmeldungen schließen"
            );
            vault.change(|data| workspace_transfer::remove_team(data, &team_id))?;
            live_sync::stop(&state, &team_id).map_err(anyhow::Error::msg)?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use noryn_core::{
        backup,
        model::{Credential, Node, Team},
        vault::Vault,
    };
    use tauri::Manager;

    #[tokio::test]
    async fn stale_copies_and_removals_are_rejected_and_busy_teams_are_not_removed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vault.json");
        let mut vault = Vault::open(&path, "test-only-vault-password").unwrap();
        vault
            .change(|data| {
                data.nodes.push(Node {
                    id: "folder".into(),
                    parent_id: None,
                    workspace: "private".into(),
                    kind: "folder".into(),
                    name: "Servers".into(),
                    protocol: "ssh".into(),
                    host: "".into(),
                    port: 22,
                    initial_path: "".into(),
                    notes: "".into(),
                    tags: vec![],
                    favorite: false,
                });
                data.credentials.push(Credential {
                    id: "account".into(),
                    name: "Personal account".into(),
                    username: "alice".into(),
                    kind: "password".into(),
                    secret: "test-only-password".into(),
                    password: String::new(),
                    passphrase: String::new(),
                    team_id: None,
                });
                data.bindings.insert("folder".into(), "account".into());
                data.teams.push(Team {
                    id: "ops".into(),
                    name: "Operations".into(),
                    member_name: "Alice".into(),
                    server_url: "https://sync.example.test".into(),
                    token: "test-token-at-least-32-characters".into(),
                    key: STANDARD.encode(noryn_core::crypto::random_bytes::<32>()),
                    revision: None,
                    dirty: false,
                    live_sync: false,
                    sync_base: None,
                });
                Ok(())
            })
            .unwrap();
        let before = backup::fingerprint(&vault.data).unwrap();
        let state = AppState::default();
        *state.vault.lock().unwrap() = Some(vault);
        {
            let vault = state.vault.lock().unwrap();
            let vault = vault.as_ref().unwrap();
            let node = &vault.data.nodes[0];
            let credential = &vault.data.credentials[0];
            assert!(crate::ensure_connection_context(
                &state,
                vault,
                node,
                credential,
                0,
                &crate::quick_connections::Origin::Saved
            )
            .is_ok());
            let mut changed = node.clone();
            changed.id = "removed-node".into();
            assert!(crate::ensure_connection_context(
                &state,
                vault,
                &changed,
                credential,
                0,
                &crate::quick_connections::Origin::Saved
            )
            .is_err());
            let mut changed = credential.clone();
            changed.password = "changed-password".into();
            assert!(crate::ensure_connection_context(
                &state,
                vault,
                node,
                &changed,
                0,
                &crate::quick_connections::Origin::Saved
            )
            .is_err());
            assert!(crate::ensure_connection_context(
                &state,
                vault,
                node,
                credential,
                99,
                &crate::quick_connections::Origin::Saved
            )
            .is_err());
        }
        let app = tauri::test::mock_builder()
            .manage(state.clone())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert!(
            node_copy(app.state(), vec!["folder".into()], "ops".into(), None, 0)
                .await
                .is_err()
        );
        assert_eq!(
            backup::fingerprint(&state.vault.lock().unwrap().as_ref().unwrap().data).unwrap(),
            before
        );
        let copy = node_copy(app.state(), vec!["folder".into()], "ops".into(), None, 1)
            .await
            .unwrap();
        assert_eq!(copy.view.local_revision, 2);
        assert_eq!(copy.roots.len(), 1);
        assert!(team_remove(app.state(), "ops".into(), 1).await.is_err());
        let job = live_sync::Job::start(&state, "ops").unwrap();
        assert!(team_remove(app.state(), "ops".into(), 2).await.is_err());
        drop(job);
        let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
        state
            .auth_requests
            .lock()
            .unwrap()
            .insert("pending-auth".into(), sender);
        assert!(team_remove(app.state(), "ops".into(), 2).await.is_err());
        state.auth_requests.lock().unwrap().clear();
        let result = team_remove(app.state(), "ops".into(), 2).await.unwrap();
        assert!(result.teams.is_empty());
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].workspace, "private");
        let reopened = Vault::open(&path, "test-only-vault-password").unwrap();
        assert_eq!(reopened.data.local_revision, 3);
        assert!(reopened.data.teams.is_empty());
    }
}
