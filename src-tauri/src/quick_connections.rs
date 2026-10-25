use super::{connect_target, task, vault_action, AppState, AuthChallenge, ConnectResult, Result};
use noryn_core::ssh::TerminalEvent;
use noryn_core::{
    connection_credentials::Login,
    model::{validate_credential, validate_nodes, Credential, Node, VaultView},
    vault::Vault,
};
use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, State};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    node: Node,
    credential_id: Option<String>,
    credential: Option<Credential>,
}
pub enum Target {
    Saved(String),
    Quick(Box<Input>),
}
pub enum Origin {
    Saved,
    Quick { account_id: Option<String> },
}
pub struct Draft {
    node: Node,
    credential: Credential,
    account_id: Option<String>,
}

pub fn resolve(vault: &Vault, target: &Target) -> anyhow::Result<(Node, Credential, Origin)> {
    use anyhow::{ensure, Context};
    match target {
        Target::Saved(id) => {
            let node = vault
                .data
                .nodes
                .iter()
                .find(|n| &n.id == id)
                .context("Eintrag nicht gefunden")?
                .clone();
            ensure!(
                node.kind == "connection"
                    && ["ssh", "sftp", "ftp", "ftps"].contains(&node.protocol.as_str()),
                "Dieses Protokoll ist noch nicht implementiert"
            );
            Ok((node, vault.data.credential_for(id)?.clone(), Origin::Saved))
        }
        Target::Quick(input) => {
            let node = input.node.clone();
            ensure!(
                node.workspace == "private"
                    && node.kind == "connection"
                    && node.parent_id.is_none()
                    && ["ssh", "sftp", "ftp", "ftps"].contains(&node.protocol.as_str()),
                "Ungültige Schnellverbindung"
            );
            validate_nodes(std::slice::from_ref(&node))?;
            ensure!(
                input.credential_id.is_some() != input.credential.is_some(),
                "Genau einen Zugang für die Schnellverbindung wählen"
            );
            let credential = if let Some(id) = &input.credential_id {
                vault
                    .data
                    .credentials
                    .iter()
                    .find(|c| &c.id == id)
                    .context("Zugangsdaten nicht gefunden")?
                    .clone()
            } else {
                let mut value = input.credential.as_ref().unwrap().clone();
                ensure!(
                    value.team_id.is_none(),
                    "Direkte Schnellzugänge bleiben privat"
                );
                value.id = node.id.clone();
                value.name = node.name.clone();
                value
            };
            validate_credential(&credential)?;
            Ok((
                node,
                credential,
                Origin::Quick {
                    account_id: input.credential_id.clone(),
                },
            ))
        }
    }
}
pub fn draft(origin: &Origin, node: &Node, credential: &Credential) -> Option<Draft> {
    match origin {
        Origin::Saved => None,
        Origin::Quick { account_id } => Some(Draft {
            node: node.clone(),
            credential: credential.clone(),
            account_id: account_id.clone(),
        }),
    }
}

#[tauri::command]
pub async fn quick_connect(
    state: State<'_, AppState>,
    request: Input,
    trusted_fingerprint: Option<String>,
    output: Channel<TerminalEvent>,
    auth_output: Channel<AuthChallenge>,
) -> Result<ConnectResult> {
    let state = state.inner().clone();
    task(move || {
        connect_target(
            &state,
            Target::Quick(Box::new(request)),
            trusted_fingerprint,
            output,
            auth_output,
        )
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    view: VaultView,
    node: Node,
}
#[tauri::command]
pub async fn quick_save(
    state: State<'_, AppState>,
    session_id: String,
    name: String,
    parent_id: Option<String>,
) -> Result<Saved> {
    let state = state.inner().clone();
    task(move || save(&state, &session_id, name, parent_id)).await
}

fn save(
    state: &AppState,
    session_id: &str,
    name: String,
    parent_id: Option<String>,
) -> Result<Saved> {
    vault_action(state, |vault| {
        let mut connections = state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
        let connection = connections
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("Die Schnellverbindung wurde bereits geschlossen"))?;
        let draft = connection
            .quick
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Diese Verbindung ist bereits gespeichert"))?;
        let mut node = draft.node.clone();
        node.id = uuid::Uuid::new_v4().to_string();
        node.name = name.trim().into();
        node.parent_id = parent_id;
        node.workspace = "private".into();
        let login = if let Some(id) = &draft.account_id {
            Login::Account {
                credential_id: Some(id.clone()),
            }
        } else {
            Login::Direct {
                credential: draft.credential.clone(),
            }
        };
        vault.change(|data| {
            data.upsert_node(node.clone())?;
            data.set_connection_login(&node.id, login)
        })?;
        connection.quick.take();
        Ok(Saved {
            view: vault.data.view(),
            node,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ensure_connection_context, Connection};
    use noryn_core::{model::Credential, remote::Remote, ssh};
    use std::sync::{Arc, Mutex};

    fn node() -> Node {
        serde_json::from_value(serde_json::json!({
            "id":"temporary", "parentId":null, "workspace":"private", "kind":"connection",
            "name":"Quick server", "protocol":"ssh", "host":"localhost", "port":22,
            "initialPath":"", "notes":"", "tags":[], "favorite":false
        }))
        .unwrap()
    }
    fn credential() -> Credential {
        serde_json::from_value(serde_json::json!({"id":"account", "name":"My account", "username":"alice", "kind":"password", "password":"test-only-secret", "secret":"", "passphrase":"", "teamId":null})).unwrap()
    }
    #[test]
    fn quick_logins_are_ephemeral_validated_and_saved_once_without_reconnecting() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::default();
        let mut vault =
            Vault::open(&directory.path().join("vault.json"), "test-only-password").unwrap();
        let input = Input {
            node: node(),
            credential_id: None,
            credential: Some(credential()),
        };
        let (node, credential, origin) = resolve(&vault, &Target::Quick(Box::new(input))).unwrap();
        assert!(vault.data.nodes.is_empty());
        assert!(vault.data.credentials.is_empty());
        assert!(ensure_connection_context(&state, &vault, &node, &credential, 0, &origin).is_ok());
        assert!(ensure_connection_context(&state, &vault, &node, &credential, 1, &origin).is_err());
        state.connections.lock().unwrap().insert(
            "live".into(),
            Connection {
                health: None,
                quick: draft(&origin, &node, &credential),
                terminal: None,
                tools: None,
                remote: Arc::new(Mutex::new(Remote::Ssh(ssh::Remote::new(ssh2_session())))),
            },
        );
        vault.data.credentials.push(credential.clone());
        let account_origin = Origin::Quick {
            account_id: Some(credential.id.clone()),
        };
        vault.data.credentials[0].username = "changed".into();
        assert!(
            ensure_connection_context(&state, &vault, &node, &credential, 0, &account_origin)
                .is_err()
        );
        *state.vault.lock().unwrap() = Some(vault);
        assert!(save(
            &state,
            "live",
            "Saved".into(),
            Some("unknown-parent".into())
        )
        .is_err());
        assert!(state
            .vault
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .data
            .nodes
            .is_empty());
        assert!(state.connections.lock().unwrap()["live"].quick.is_some());
        let result = save(&state, "live", "Saved".into(), None).unwrap();
        assert_ne!(result.node.id, node.id);
        assert_eq!(result.node.workspace, "private");
        assert!(!serde_json::to_string(&result.view)
            .unwrap()
            .contains("test-only-secret"));
        assert!(save(&state, "live", "Duplicate".into(), None).is_err());
        let vault_guard = state.vault.lock().unwrap();
        let stored = vault_guard.as_ref().unwrap();
        assert_eq!(
            stored
                .data
                .credential_for(&result.node.id)
                .unwrap()
                .login_password(),
            "test-only-secret"
        );
        assert!(stored.data.bindings.is_empty());
        assert_eq!(state.connections.lock().unwrap().len(), 1);
    }
    // Unauthenticated, disconnected libssh2 object: no sockets, MFA or target system involved.
    fn ssh2_session() -> ssh2::Session {
        ssh2::Session::new().unwrap()
    }
    #[test]
    fn quick_requests_cannot_target_teams_or_mix_direct_and_saved_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let vault =
            Vault::open(&directory.path().join("vault.json"), "test-only-password").unwrap();
        let mut input = Input {
            node: node(),
            credential_id: None,
            credential: Some(credential()),
        };
        input.node.workspace = "team".into();
        assert!(resolve(&vault, &Target::Quick(Box::new(input))).is_err());
        let mut input = Input {
            node: node(),
            credential_id: Some("account".into()),
            credential: Some(credential()),
        };
        assert!(resolve(
            &vault,
            &Target::Quick(Box::new(Input {
                node: input.node.clone(),
                credential_id: input.credential_id.clone(),
                credential: input.credential.clone()
            }))
        )
        .is_err());
        input.credential_id = None;
        input.credential.as_mut().unwrap().team_id = Some("team".into());
        assert!(resolve(&vault, &Target::Quick(Box::new(input))).is_err());
    }
}
