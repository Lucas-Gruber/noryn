use base64::{engine::general_purpose::STANDARD, Engine};
use harbor_core::{
    crypto, local_fs,
    model::{Credential, Node, Team, VaultView},
    ssh::{self, Remote, TerminalEvent, TerminalInput},
    sync,
    vault::Vault,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
};
use tauri::{ipc::Channel, Manager, State};
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, String>;
type AuthRequests = Arc<Mutex<HashMap<String, SyncSender<Option<Vec<String>>>>>>;

struct Connection {
    terminal: Option<SyncSender<TerminalInput>>,
    remote: Arc<Mutex<Remote>>,
}

#[derive(Clone, Default)]
struct AppState {
    vault: Arc<Mutex<Option<Vault>>>,
    connections: Arc<Mutex<HashMap<String, Connection>>>,
    auth_requests: AuthRequests,
}

fn vault_action<T>(
    state: &AppState,
    action: impl FnOnce(&mut Vault) -> anyhow::Result<T>,
) -> Result<T> {
    let mut guard = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
    let vault = guard.as_mut().ok_or("Tresor ist gesperrt")?;
    action(vault).map_err(|e| format!("{e:#}"))
}

async fn task<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn vault_exists(app: tauri::AppHandle) -> Result<bool> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("vault.json")
        .exists())
}

#[tauri::command]
async fn vault_unlock(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    password: String,
) -> Result<VaultView> {
    let state = state.inner().clone();
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("vault.json");
    task(move || {
        let password = Zeroizing::new(password);
        let mut guard = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
        if guard.is_some() {
            return Err("Tresor ist bereits entsperrt".into());
        }
        let vault = Vault::open(&path, &password).map_err(|e| format!("{e:#}"))?;
        let view = vault.data.view();
        *guard = Some(vault);
        Ok(view)
    })
    .await
}

#[tauri::command]
fn vault_lock(state: State<'_, AppState>) -> Result<()> {
    let mut vault = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
    let mut connections = state
        .connections
        .lock()
        .map_err(|_| "Sitzungen nicht verfügbar")?;
    for (_, connection) in connections.drain() {
        if let Some(terminal) = connection.terminal {
            let _ = terminal.try_send(TerminalInput::Close);
        }
    }
    for (_, pending) in state
        .auth_requests
        .lock()
        .map_err(|_| "Anmeldungen nicht verfügbar")?
        .drain()
    {
        let _ = pending.try_send(None);
    }
    *vault = None;
    Ok(())
}

#[tauri::command]
async fn node_save(state: State<'_, AppState>, node: Node) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            vault.change(|data| data.upsert_node(node))?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
async fn node_delete(state: State<'_, AppState>, id: String) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            vault.change(|data| data.remove_node(&id))?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
async fn binding_set(
    state: State<'_, AppState>,
    node_id: String,
    credential_id: Option<String>,
) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            vault.change(|data| {
                anyhow::ensure!(
                    data.nodes.iter().any(|n| n.id == node_id),
                    "Eintrag nicht gefunden"
                );
                if let Some(id) = credential_id {
                    anyhow::ensure!(
                        data.credentials.iter().any(|c| c.id == id),
                        "Zugangsdaten nicht gefunden"
                    );
                    data.bindings.insert(node_id, id);
                } else {
                    data.bindings.remove(&node_id);
                }
                Ok(())
            })?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
async fn credential_save(
    state: State<'_, AppState>,
    mut credential: Credential,
) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            anyhow::ensure!(
                !credential.name.trim().is_empty() && !credential.username.trim().is_empty(),
                "Name und Benutzername fehlen"
            );
            anyhow::ensure!(
                ["password", "key", "agent"].contains(&credential.kind.as_str()),
                "Unbekannter Zugangstyp"
            );
            if !credential.id.is_empty() {
                let old = vault
                    .data
                    .credentials
                    .iter()
                    .find(|c| c.id == credential.id)
                    .ok_or_else(|| anyhow::anyhow!("Zugangsdaten nicht gefunden"))?;
                let same_key = credential.kind == old.kind && credential.secret.is_empty();
                if same_key {
                    credential.secret = old.secret.clone();
                    if credential.passphrase.is_empty() {
                        credential.passphrase = old.passphrase.clone();
                    }
                }
                if credential.password.is_empty() {
                    credential.password = old.login_password().into();
                }
            } else {
                credential.id = uuid::Uuid::new_v4().to_string();
            }
            if credential.kind == "password" && !credential.password.is_empty() {
                credential.secret.clear();
            }
            anyhow::ensure!(
                credential.kind != "key" || !credential.secret.is_empty(),
                "Privater SSH-Schlüssel fehlt"
            );
            vault.change(|data| {
                data.credentials.retain(|c| c.id != credential.id);
                data.credentials.push(credential);
                Ok(())
            })?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
async fn credential_delete(state: State<'_, AppState>, id: String) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            vault.change(|data| {
                data.credentials.retain(|c| c.id != id);
                data.bindings.retain(|_, c| c != &id);
                Ok(())
            })?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
fn team_key_generate() -> String {
    STANDARD.encode(crypto::random_bytes::<32>())
}

#[tauri::command]
async fn team_save(state: State<'_, AppState>, mut team: Team) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            sync::validate_team_id(&team.id)?;
            sync::endpoint(&team.server_url, &team.id)?;
            anyhow::ensure!(!team.name.trim().is_empty(), "Teamname fehlt");
            if let Some(old) = vault.data.teams.iter().find(|t| t.id == team.id) {
                if team.key.is_empty() {
                    team.key = old.key.clone();
                }
                if team.token.is_empty() {
                    team.token = old.token.clone();
                }
                team.revision = if team.server_url == old.server_url && team.key == old.key {
                    old.revision.clone()
                } else {
                    None
                };
                team.dirty = old.dirty;
            } else {
                team.revision = None;
                team.dirty = true;
            }
            crypto::decode_key(&team.key)?;
            anyhow::ensure!(
                team.token.len() >= 32,
                "Server-Token muss mindestens 32 Zeichen enthalten"
            );
            vault.change(|data| {
                data.teams.retain(|t| t.id != team.id);
                data.teams.push(team);
                Ok(())
            })?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncResult {
    view: VaultView,
    message: String,
}

#[tauri::command]
async fn team_sync(
    state: State<'_, AppState>,
    team_id: String,
    direction: String,
) -> Result<SyncResult> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            let message = vault.change(|data| sync::sync(data, &team_id, &direction))?;
            Ok(SyncResult {
                view: vault.data.view(),
                message,
            })
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ConnectResult {
    HostKey {
        fingerprint: String,
        previous: Option<String>,
        host: String,
    },
    Connected {
        id: String,
        home: String,
        username: String,
    },
}

#[tauri::command]
async fn connect_node(
    state: State<'_, AppState>,
    node_id: String,
    trusted_fingerprint: Option<String>,
    output: Channel<TerminalEvent>,
    auth_output: Channel<AuthChallenge>,
) -> Result<ConnectResult> {
    let state = state.inner().clone();
    task(move || {
        let (node, credential, known) = vault_action(&state, |vault| {
            let node = vault
                .data
                .nodes
                .iter()
                .find(|n| n.id == node_id)
                .ok_or_else(|| anyhow::anyhow!("Eintrag nicht gefunden"))?
                .clone();
            anyhow::ensure!(
                node.kind == "connection" && ["ssh", "sftp"].contains(&node.protocol.as_str()),
                "Dieses Protokoll ist noch nicht implementiert"
            );
            let credential = vault.data.credential_for(&node_id)?.clone();
            let known = vault
                .data
                .known_hosts
                .get(&format!("{}:{}", node.host, node.port))
                .cloned();
            Ok((node, credential, known))
        })?;
        let (session, fingerprint) =
            ssh::handshake(&node.host, node.port).map_err(|e| format!("{e:#}"))?;
        if known.as_ref() != Some(&fingerprint)
            && trusted_fingerprint.as_ref() != Some(&fingerprint)
        {
            return Ok(ConnectResult::HostKey {
                fingerprint,
                previous: known,
                host: format!("{}:{}", node.host, node.port),
            });
        }
        ssh::authenticate_interactive(&session, &credential, |username, instructions, prompts| {
            anyhow::ensure!(
                prompts.len() <= 16
                    && instructions.len() <= 16_384
                    && prompts.iter().all(|p| p.text.len() <= 4096),
                "Anmeldeabfrage ist zu groß"
            );
            let id = uuid::Uuid::new_v4().to_string();
            let (sender, receiver) = mpsc::sync_channel(1);
            state
                .auth_requests
                .lock()
                .map_err(|_| anyhow::anyhow!("Anmeldung nicht verfügbar"))?
                .insert(id.clone(), sender);
            let challenge = AuthChallenge {
                id: id.clone(),
                host: format!("{}:{}", node.host, node.port),
                username: username.into(),
                instructions: instructions.into(),
                prompts,
            };
            let result = if auth_output.send(challenge).is_ok() {
                receiver
                    .recv_timeout(std::time::Duration::from_secs(180))
                    .ok()
                    .flatten()
                    .ok_or_else(|| anyhow::anyhow!("Anmeldung abgebrochen oder Zeitlimit erreicht"))
            } else {
                Err(anyhow::anyhow!("Anmeldedialog nicht erreichbar"))
            };
            if let Ok(mut requests) = state.auth_requests.lock() {
                requests.remove(&id);
            }
            result
        })
        .map_err(|e| format!("{e:#}"))?;
        // One authenticated transport, independent channels: OTP is requested once.
        let remote = Remote::new(session.clone()).with_password(credential.login_password().into());
        let home = remote.home().unwrap_or_else(|_| "/".into());
        let (tx, rx) = mpsc::sync_channel(512);
        // Serialize final registration with vault locking so locking cannot race a new connection.
        vault_action(&state, |vault| {
            vault.change(|data| {
                data.known_hosts
                    .insert(format!("{}:{}", node.host, node.port), fingerprint);
                Ok(())
            })?;
            let terminal = if node.protocol == "ssh" {
                ssh::start_terminal(session, rx, move |event| {
                    let _ = output.send(event);
                })?;
                Some(tx)
            } else {
                None
            };
            let id = uuid::Uuid::new_v4().to_string();
            state
                .connections
                .lock()
                .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?
                .insert(
                    id.clone(),
                    Connection {
                        terminal,
                        remote: Arc::new(Mutex::new(remote)),
                    },
                );
            Ok(ConnectResult::Connected {
                id,
                home,
                username: credential.username.clone(),
            })
        })
    })
    .await
}

#[tauri::command]
fn terminal_input(state: State<'_, AppState>, id: String, data: String) -> Result<()> {
    let connections = state
        .connections
        .lock()
        .map_err(|_| "Sitzungen nicht verfügbar")?;
    let connection = connections.get(&id).ok_or("Sitzung geschlossen")?;
    if data.len() > 1024 * 1024 {
        return Err("Terminal-Eingabe zu groß".into());
    }
    connection
        .terminal
        .as_ref()
        .ok_or("Diese Sitzung hat kein Terminal")?
        .try_send(TerminalInput::Data(data.into_bytes()))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn terminal_resize(state: State<'_, AppState>, id: String, cols: u32, rows: u32) -> Result<()> {
    let connections = state
        .connections
        .lock()
        .map_err(|_| "Sitzungen nicht verfügbar")?;
    connections
        .get(&id)
        .ok_or("Sitzung geschlossen")?
        .terminal
        .as_ref()
        .ok_or("Diese Sitzung hat kein Terminal")?
        .try_send(TerminalInput::Resize(cols.min(1000), rows.min(1000)))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn connection_close(state: State<'_, AppState>, id: String) -> Result<()> {
    if let Some(connection) = state
        .connections
        .lock()
        .map_err(|_| "Sitzungen nicht verfügbar")?
        .remove(&id)
    {
        if let Some(terminal) = connection.terminal {
            let _ = terminal.try_send(TerminalInput::Close);
        }
    }
    Ok(())
}

fn remote_for(state: &AppState, id: &str) -> Result<Arc<Mutex<Remote>>> {
    Ok(state
        .connections
        .lock()
        .map_err(|_| "Sitzungen nicht verfügbar")?
        .get(id)
        .ok_or("Sitzung geschlossen")?
        .remote
        .clone())
}

#[tauri::command]
async fn remote_list(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<Vec<ssh::RemoteEntry>> {
    let remote = remote_for(&state, &id)?;
    task(move || {
        remote
            .lock()
            .map_err(|_| "Dateizugriff nicht verfügbar")?
            .list(&path)
            .map_err(|e| format!("{e:#}"))
    })
    .await
}

#[tauri::command]
async fn remote_sudo(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    password: Option<String>,
) -> Result<ssh::SudoOutcome> {
    let remote = remote_for(&state, &id)?;
    task(move || {
        remote
            .lock()
            .map_err(|_| "Dateizugriff nicht verfügbar")?
            .sudo_auto(enabled, password)
            .map_err(|e| format!("{e:#}"))
    })
    .await
}

#[tauri::command]
async fn remote_transfer(
    state: State<'_, AppState>,
    id: String,
    direction: String,
    local: String,
    remote_path: String,
    overwrite: Option<bool>,
) -> Result<u64> {
    let remote = remote_for(&state, &id)?;
    task(move || {
        let remote = remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?;
        let path = PathBuf::from(local);
        match direction.as_str() {
            "upload" => remote.upload(&path, &remote_path),
            "download" => remote.download_file(&remote_path, &path, overwrite.unwrap_or(false)),
            _ => return Err("Ungültige Übertragungsrichtung".into()),
        }
        .map_err(|e| format!("{e:#}"))
    })
    .await
}

#[tauri::command]
async fn remote_mkdir(state: State<'_, AppState>, id: String, path: String) -> Result<()> {
    let remote = remote_for(&state, &id)?;
    task(move || {
        remote
            .lock()
            .map_err(|_| "Dateizugriff nicht verfügbar")?
            .mkdir(&path)
            .map_err(|e| format!("{e:#}"))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthChallenge {
    id: String,
    host: String,
    username: String,
    instructions: String,
    prompts: Vec<ssh::AuthPrompt>,
}

#[tauri::command]
fn auth_respond(
    state: State<'_, AppState>,
    id: String,
    answers: Option<Vec<String>>,
) -> Result<()> {
    if let Some(answers) = &answers {
        if answers.len() > 16 || answers.iter().map(String::len).sum::<usize>() > 32_768 {
            return Err("Anmeldeantwort zu groß".into());
        }
    }
    let sender = state
        .auth_requests
        .lock()
        .map_err(|_| "Anmeldung nicht verfügbar")?
        .remove(&id)
        .ok_or("Anmeldeabfrage ist abgelaufen")?;
    sender.try_send(answers).map_err(|e| e.to_string())
}

#[tauri::command]
async fn local_locations(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<local_fs::Location>> {
    vault_action(&state, |_| Ok(()))?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    task(move || Ok(local_fs::locations(&home))).await
}

#[tauri::command]
async fn local_list(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<local_fs::Directory> {
    vault_action(&state, |_| Ok(()))?;
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => app.path().home_dir().map_err(|e| e.to_string())?,
    };
    task(move || local_fs::list(&path).map_err(|e| format!("{e:#}"))).await
}

#[tauri::command]
async fn local_mkdir(state: State<'_, AppState>, parent: String, name: String) -> Result<()> {
    vault_action(&state, |_| Ok(()))?;
    task(move || {
        local_fs::mkdir(&PathBuf::from(parent), &name)
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    })
    .await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            auth_respond,
            local_locations,
            local_list,
            local_mkdir,
            vault_exists,
            vault_unlock,
            vault_lock,
            node_save,
            node_delete,
            binding_set,
            credential_save,
            credential_delete,
            team_key_generate,
            team_save,
            team_sync,
            connect_node,
            terminal_input,
            terminal_resize,
            connection_close,
            remote_list,
            remote_sudo,
            remote_transfer,
            remote_mkdir
        ])
        .run(tauri::generate_context!())
        .expect("Noryn konnte nicht gestartet werden");
}
