mod backup_commands;
mod batch_commands;
mod clipboard;
mod external_editor;
mod file_actions;
mod file_apps;
#[cfg(target_os = "linux")]
mod file_portal;
mod health_commands;
mod live_sync;
mod quick_connections;
mod script_commands;
mod script_edits;
mod text_commands;
mod transfer_commands;
mod updates;
mod vault_reset;
mod workspace_commands;

use base64::{engine::general_purpose::STANDARD, Engine};
use noryn_core::{
    crypto, ftp, local_fs,
    model::{Credential, CredentialFolder, Node, Team, VaultView},
    port_forward::ForwardInfo,
    remote::Remote,
    ssh::{self, TerminalEvent, TerminalInput},
    ssh_tools::Tools,
    sync,
    system_info::{Section, Snapshot},
    team_activity,
    vault::Vault,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
};
use tauri::{ipc::Channel, Manager, State};
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, String>;
type AuthRequests = Arc<Mutex<HashMap<String, SyncSender<Option<Vec<String>>>>>>;

struct Connection {
    health: Option<health_commands::Context>,
    quick: Option<quick_connections::Draft>,
    terminal: Option<SyncSender<TerminalInput>>,
    tools: Option<Arc<Tools>>,
    remote: Arc<Mutex<Remote>>,
}

#[derive(Clone, Default)]
struct AppState {
    vault: Arc<Mutex<Option<Vault>>>,
    connections: Arc<Mutex<HashMap<String, Connection>>>,
    auth_requests: AuthRequests,
    pending_import: Arc<Mutex<Option<backup_commands::PendingImport>>>,
    sync_events: Arc<Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>>,
    sync_jobs: Arc<Mutex<HashSet<String>>>,
    vault_epoch: Arc<AtomicU64>,
    locked_revision: Arc<AtomicU64>,
    pending_reset: Arc<Mutex<Option<vault_reset::PendingReset>>>,
    transfers: transfer_commands::Transfers,
    documents: text_commands::Documents,
    script_documents: script_edits::Documents,
    command_jobs: batch_commands::Jobs,
}

fn vault_action<T>(
    state: &AppState,
    action: impl FnOnce(&mut Vault) -> anyhow::Result<T>,
) -> Result<T> {
    let mut guard = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
    let vault = guard.as_mut().ok_or("Tresor ist gesperrt")?;
    action(vault).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn command_save(
    state: State<'_, AppState>,
    command: noryn_core::commands::SavedCommand,
    expected_revision: u64,
) -> Result<VaultView> {
    vault_action(&state, |vault| {
        anyhow::ensure!(vault.data.local_revision == expected_revision, "Tresor wurde inzwischen geändert. Bitte den Befehl erneut öffnen; deine Eingaben bleiben erhalten.");
        vault.change(|data| data.save_command(command))?;
        Ok(vault.data.view())
    })
}

#[tauri::command]
fn command_delete(
    state: State<'_, AppState>,
    id: String,
    expected_revision: u64,
) -> Result<VaultView> {
    vault_action(&state, |vault| {
        anyhow::ensure!(
            vault.data.local_revision == expected_revision,
            "Tresor wurde inzwischen geändert. Bitte erneut auswählen."
        );
        vault.change(|data| data.remove_command(&id))?;
        Ok(vault.data.view())
    })
}

fn ensure_connection_context(
    state: &AppState,
    vault: &Vault,
    node: &Node,
    credential: &Credential,
    epoch: u64,
    origin: &quick_connections::Origin,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.vault_epoch.load(Ordering::Acquire) == epoch,
        "Der Tresor wurde während des Verbindungsaufbaus gesperrt oder ersetzt"
    );
    match origin {
        quick_connections::Origin::Saved => {
            anyhow::ensure!(
                vault.data.nodes.iter().find(|n| n.id == node.id) == Some(node),
                "Der Eintrag wurde während des Verbindungsaufbaus geändert oder entfernt"
            );
            anyhow::ensure!(
                vault.data.credential_for(&node.id)? == credential,
                "Die persönlichen Zugangsdaten wurden während des Verbindungsaufbaus geändert"
            );
        }
        quick_connections::Origin::Quick {
            account_id: Some(id),
        } => {
            anyhow::ensure!(vault.data.credentials.iter().find(|c| &c.id == id) == Some(credential), "Die ausgewählten Zugangsdaten wurden während des Verbindungsaufbaus geändert oder entfernt");
        }
        quick_connections::Origin::Quick { account_id: None } => {}
    }
    Ok(())
}

async fn task<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn vault_exists(app: tauri::AppHandle) -> Result<bool> {
    let (path, previous) = vault_paths(&app)?;
    Ok(path.exists() || previous.exists())
}

fn vault_paths(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf)> {
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let previous = directory
        .parent()
        .ok_or("Tresor-Verzeichnis ungültig")?
        .join(noryn_core::migration::PREVIOUS_APP_ID)
        .join("vault.json");
    Ok((directory.join("vault.json"), previous))
}

#[tauri::command]
fn vault_location(app: tauri::AppHandle) -> Result<String> {
    Ok(vault_paths(&app)?.0.to_string_lossy().into())
}

#[tauri::command]
async fn vault_unlock(
    updates: State<'_, updates::UpdateState>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    password: String,
) -> Result<VaultView> {
    let state = state.inner().clone();
    let installing = updates.installing.clone();
    let (path, previous) = vault_paths(&app)?;
    task(move || {
        let password = Zeroizing::new(password);
        let mut guard = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
        if installing.load(Ordering::Acquire) {
            return Err("Ein Update wird gerade installiert".into());
        }
        if guard.is_some() {
            return Err("Tresor ist bereits entsperrt".into());
        }
        let vault =
            Vault::open_migrating(&path, &previous, &password).map_err(|e| format!("{e:#}"))?;
        let view = vault.data.view();
        state
            .pending_reset
            .lock()
            .map_err(|_| "Zurücksetzen nicht verfügbar")?
            .take();
        *guard = Some(vault);
        state.vault_epoch.fetch_add(1, Ordering::Release);
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
    if let Some(vault) = vault.as_ref() {
        state
            .locked_revision
            .fetch_max(vault.data.local_revision, Ordering::Release);
    }
    state
        .pending_reset
        .lock()
        .map_err(|_| "Zurücksetzen nicht verfügbar")?
        .take();
    transfer_commands::cancel(&state, None)?;
    batch_commands::cancel_all(&state)?;
    text_commands::clear(&state, None)?;
    script_edits::clear(&state)?;
    for (_, connection) in connections.drain() {
        if let Some(tools) = connection.tools {
            tools.close();
        }
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
    state
        .pending_import
        .lock()
        .map_err(|_| "Import nicht verfügbar")?
        .take();
    *vault = None;
    state.vault_epoch.fetch_add(1, Ordering::Release);
    for (_, handle) in state
        .sync_events
        .lock()
        .map_err(|_| "Live-Sync nicht verfügbar")?
        .drain()
    {
        handle.abort();
    }
    Ok(())
}

#[tauri::command]
async fn node_save(
    state: State<'_, AppState>,
    node: Node,
    base: Option<Node>,
    login: Option<noryn_core::connection_credentials::Login>,
    expected_revision: Option<u64>,
) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            if matches!(login, Some(noryn_core::connection_credentials::Login::Direct { .. })) {
                anyhow::ensure!(expected_revision == Some(vault.data.local_revision), "Der Tresor wurde während der Bearbeitung aktualisiert. Bitte die Verbindung erneut öffnen; deine Eingaben wurden nicht gespeichert.");
            }
            let node = if let Some(base) = base {
                let current = vault.data.nodes.iter().find(|n| n.id == base.id).ok_or_else(|| anyhow::anyhow!("Eintrag wurde zwischenzeitlich entfernt; deine Eingaben wurden nicht gespeichert"))?;
                noryn_core::sync_merge::merge_node_edit(&base, &node, current)?
            } else { node };
            let id = node.id.clone();
            vault.change(|data| {
                data.upsert_node(node)?;
                data.set_connection_login(&id, login.unwrap_or(noryn_core::connection_credentials::Login::Keep))
            })?;
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
                data.set_connection_login(
                    &node_id,
                    noryn_core::connection_credentials::Login::Account { credential_id },
                )
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
    folder_id: Option<String>,
    expected_revision: Option<u64>,
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
                anyhow::ensure!(old.team_id.is_none() || expected_revision == Some(vault.data.local_revision), "Der Tresor wurde während der Bearbeitung aktualisiert. Bitte den Team-Zugang erneut öffnen; deine Eingaben wurden nicht gespeichert.");
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
            let id = credential.id.clone();
            vault.change(|data| {
                data.upsert_credential(credential)?;
                data.set_credential_folder(&id, folder_id)
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
            vault.change(|data| data.remove_credential(&id))?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
async fn credential_folder_save(
    state: State<'_, AppState>,
    folder: CredentialFolder,
) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            vault.change(|data| data.upsert_credential_folder(folder))?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
async fn credential_folder_delete(state: State<'_, AppState>, id: String) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            vault.change(|data| data.remove_credential_folder(&id))?;
            Ok(vault.data.view())
        })
    })
    .await
}

#[tauri::command]
async fn credential_move(
    state: State<'_, AppState>,
    credential_id: String,
    folder_id: Option<String>,
) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
            vault.change(|data| data.set_credential_folder(&credential_id, folder_id))?;
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
            team.member_name = team.member_name.trim().into();
            team_activity::validate_member_name(&team.member_name)?;
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
                team.sync_base = if team.server_url == old.server_url && team.key == old.key {
                    old.sync_base.clone()
                } else {
                    None
                };
                team.dirty = old.dirty;
            } else {
                team.revision = None;
                team.sync_base = None;
                team.dirty = true;
            }
            crypto::decode_key(&team.key)?;
            anyhow::ensure!(
                team.token.len() >= 32,
                "Server-Token muss mindestens 32 Zeichen enthalten"
            );
            let team_id = team.id.clone();
            vault.change(|data| {
                data.teams.retain(|t| t.id != team.id);
                data.teams.push(team);
                Ok(())
            })?;
            live_sync::stop(&state, &team_id).map_err(anyhow::Error::msg)?;
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
        let _job = live_sync::Job::start(&state, &team_id)?;
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
        connect_target(
            &state,
            quick_connections::Target::Saved(node_id),
            trusted_fingerprint,
            output,
            auth_output,
        )
    })
    .await
}

fn connect_target(
    state: &AppState,
    target: quick_connections::Target,
    trusted_fingerprint: Option<String>,
    output: Channel<TerminalEvent>,
    auth_output: Channel<AuthChallenge>,
) -> Result<ConnectResult> {
    let (node, credential, known, epoch, origin) = vault_action(state, |vault| {
        let (node, credential, origin) = quick_connections::resolve(vault, &target)?;
        let known = vault
            .data
            .known_hosts
            .get(&format!("{}:{}", node.host, node.port))
            .cloned();
        Ok((
            node,
            credential,
            known,
            state.vault_epoch.load(Ordering::Acquire),
            origin,
        ))
    })?;
    if ["ftp", "ftps"].contains(&node.protocol.as_str()) {
        let mut remote =
            ftp::Remote::connect(&node.host, node.port, &credential, node.protocol == "ftps")
                .map_err(|e| format!("{e:#}"))?;
        let home = remote.home().to_string();
        if !node.initial_path.is_empty() {
            remote
                .list(&node.initial_path)
                .map_err(|e| format!("Startordner nicht erreichbar: {e:#}"))?;
        }
        return vault_action(state, |vault| {
            ensure_connection_context(state, vault, &node, &credential, epoch, &origin)?;
            let id = uuid::Uuid::new_v4().to_string();
            state
                .connections
                .lock()
                .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?
                .insert(
                    id.clone(),
                    Connection {
                        health: None,
                        quick: quick_connections::draft(&origin, &node, &credential),
                        terminal: None,
                        tools: None,
                        remote: Arc::new(Mutex::new(Remote::Ftp(remote))),
                    },
                );
            Ok(ConnectResult::Connected {
                id,
                home,
                username: credential.username.clone(),
            })
        });
    }
    let (session, fingerprint) =
        ssh::handshake(&node.host, node.port).map_err(|e| format!("{e:#}"))?;
    vault_action(state, |vault| {
        ensure_connection_context(state, vault, &node, &credential, epoch, &origin)
    })?;
    if known.as_ref() != Some(&fingerprint) && trusted_fingerprint.as_ref() != Some(&fingerprint) {
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
        vault_action(state, |vault| {
            ensure_connection_context(state, vault, &node, &credential, epoch, &origin)?;
            state
                .auth_requests
                .lock()
                .map_err(|_| anyhow::anyhow!("Anmeldung nicht verfügbar"))?
                .insert(id.clone(), sender);
            Ok(())
        })
        .map_err(anyhow::Error::msg)?;
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
    let remote =
        ssh::Remote::new(session.clone()).with_password(credential.login_password().into());
    let home = remote.home().unwrap_or_else(|_| "/".into());
    if node.protocol == "sftp" && !node.initial_path.is_empty() {
        remote
            .list(&node.initial_path)
            .map_err(|e| format!("Startordner nicht erreichbar: {e:#}"))?;
    }
    let (tx, rx) = mpsc::sync_channel(512);
    // Serialize final registration with vault locking so locking cannot race a new connection.
    vault_action(state, |vault| {
        ensure_connection_context(state, vault, &node, &credential, epoch, &origin)?;
        let health = if node.protocol == "ssh" && matches!(origin, quick_connections::Origin::Saved)
        {
            Some(health_commands::context(
                vault,
                &node,
                &credential.username,
                &fingerprint,
                epoch,
            )?)
        } else {
            None
        };
        vault.change(|data| {
            data.known_hosts
                .insert(format!("{}:{}", node.host, node.port), fingerprint);
            Ok(())
        })?;
        let tools = (node.protocol == "ssh").then(|| {
            Arc::new(Tools::new(session.clone()).with_password(credential.login_password().into()))
        });
        let terminal = if node.protocol == "ssh" {
            let terminal_tools = tools.clone();
            ssh::start_terminal(session, rx, move |event| {
                if matches!(event, TerminalEvent::Closed { .. }) {
                    if let Some(tools) = &terminal_tools {
                        tools.close();
                    }
                }
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
                    health,
                    quick: quick_connections::draft(&origin, &node, &credential),
                    terminal,
                    tools,
                    remote: Arc::new(Mutex::new(Remote::Ssh(remote))),
                },
            );
        Ok(ConnectResult::Connected {
            id,
            home,
            username: credential.username.clone(),
        })
    })
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
        transfer_commands::cancel(&state, Some(&id))?;
        text_commands::clear(&state, Some(&id))?;
        if let Some(tools) = connection.tools {
            tools.close();
        }
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

fn tools_for(state: &AppState, id: &str) -> Result<Arc<Tools>> {
    state
        .connections
        .lock()
        .map_err(|_| "Sitzungen nicht verfügbar")?
        .get(id)
        .ok_or("Sitzung geschlossen")?
        .tools
        .clone()
        .ok_or_else(|| "Diese Funktion benötigt eine SSH-Sitzung".into())
}

#[tauri::command]
async fn ssh_forward_start(
    state: State<'_, AppState>,
    id: String,
    local_port: u16,
    remote_host: String,
    remote_port: u16,
) -> Result<ForwardInfo> {
    let tools = tools_for(&state, &id)?;
    task(move || {
        tools
            .start_forward(local_port, remote_host, remote_port)
            .map_err(|e| format!("{e:#}"))
    })
    .await
}
#[tauri::command]
fn ssh_forward_list(state: State<'_, AppState>, id: String) -> Result<Vec<ForwardInfo>> {
    tools_for(&state, &id)?
        .list_forwards()
        .map_err(|e| format!("{e:#}"))
}
#[tauri::command]
async fn ssh_forward_stop(
    state: State<'_, AppState>,
    id: String,
    forward_id: String,
) -> Result<()> {
    let tools = tools_for(&state, &id)?;
    task(move || {
        tools
            .stop_forward(&forward_id)
            .map_err(|e| format!("{e:#}"))
    })
    .await
}
#[tauri::command]
async fn ssh_system_snapshot(
    state: State<'_, AppState>,
    id: String,
    section: Section,
) -> Result<Snapshot> {
    let tools = tools_for(&state, &id)?;
    task(move || tools.snapshot(section).map_err(|e| format!("{e:#}"))).await
}

#[tauri::command]
async fn ssh_system_control(
    state: State<'_, AppState>,
    id: String,
    request: noryn_core::system_control::Request,
    elevated: bool,
    password: Option<String>,
) -> Result<noryn_core::system_control::Reply> {
    let tools = tools_for(&state, &id)?;
    task(move || {
        tools
            .control(&request, elevated, password)
            .map_err(|e| format!("{e:#}"))
    })
    .await
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
    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::UpdateState::default())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            updates::update_info,
            updates::update_check,
            updates::update_download,
            updates::update_cancel,
            updates::update_install,
            updates::update_open_release,
            updates::program_open_project,
            auth_respond,
            local_locations,
            local_list,
            local_mkdir,
            vault_exists,
            vault_location,
            vault_unlock,
            vault_lock,
            vault_reset::vault_reset_preview,
            vault_reset::vault_reset_apply,
            vault_reset::vault_reset_cancel,
            backup_commands::vault_export,
            backup_commands::vault_import_preview,
            backup_commands::vault_import_apply,
            backup_commands::vault_import_cancel,
            node_save,
            node_delete,
            command_save,
            command_delete,
            script_edits::script_edit_open,
            script_edits::script_edit_list,
            script_edits::script_edit_finish,
            script_edits::script_edit_discard,
            script_edits::script_edit_wait,
            script_commands::script_get,
            script_commands::script_save,
            script_commands::script_delete,
            script_commands::script_sync,
            batch_commands::command_batch_run,
            batch_commands::command_batch_cancel,
            workspace_commands::node_copy,
            workspace_commands::team_remove,
            binding_set,
            credential_save,
            credential_delete,
            credential_folder_save,
            credential_folder_delete,
            credential_move,
            team_key_generate,
            team_save,
            team_sync,
            live_sync::team_events_start,
            live_sync::team_events_stop,
            live_sync::team_auto_sync,
            connect_node,
            quick_connections::quick_connect,
            quick_connections::quick_save,
            terminal_input,
            terminal_resize,
            connection_close,
            text_commands::text_open,
            text_commands::text_reopen,
            text_commands::text_save,
            text_commands::text_close,
            text_commands::text_wait,
            text_commands::text_finish,
            ssh_forward_start,
            ssh_forward_list,
            ssh_forward_stop,
            ssh_system_snapshot,
            health_commands::ssh_login_health,
            health_commands::ssh_listening_ports,
            health_commands::ssh_forward_probe,
            health_commands::ssh_forward_open,
            ssh_system_control,
            remote_list,
            clipboard::clipboard_read_text,
            clipboard::clipboard_write_text,
            file_actions::file_action,
            file_apps::file_apps_get,
            file_apps::file_apps_save,
            remote_sudo,
            transfer_commands::remote_transfer,
            transfer_commands::remote_transfer_cancel,
            remote_mkdir
        ])
        .run(tauri::generate_context!())
        .expect("Noryn konnte nicht gestartet werden");
}
