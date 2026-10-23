use base64::{engine::general_purpose::STANDARD, Engine};
use harbor_core::{crypto, model::{Credential, Node, Team, VaultView}, ssh::{self, Remote, TerminalEvent, TerminalInput}, sync, vault::Vault};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf, sync::{mpsc::{self, SyncSender}, Arc, Mutex}};
use tauri::{ipc::Channel, Manager, State};
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, String>;

struct Connection { terminal: SyncSender<TerminalInput>, remote: Arc<Mutex<Remote>> }

#[derive(Clone, Default)]
struct AppState {
    vault: Arc<Mutex<Option<Vault>>>,
    connections: Arc<Mutex<HashMap<String, Connection>>>,
}

fn vault_action<T>(state: &AppState, action: impl FnOnce(&mut Vault) -> anyhow::Result<T>) -> Result<T> {
    let mut guard = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
    let vault = guard.as_mut().ok_or("Tresor ist gesperrt")?;
    action(vault).map_err(|e| format!("{e:#}"))
}

async fn task<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn vault_exists(app: tauri::AppHandle) -> Result<bool> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("vault.json").exists())
}

#[tauri::command]
async fn vault_unlock(app: tauri::AppHandle, state: State<'_, AppState>, password: String) -> Result<VaultView> {
    let state = state.inner().clone();
    let path = app.path().app_data_dir().map_err(|e| e.to_string())?.join("vault.json");
    task(move || {
        let password = Zeroizing::new(password);
        let mut guard = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
        if guard.is_some() { return Err("Tresor ist bereits entsperrt".into()); }
        let vault = Vault::open(&path, &password).map_err(|e| format!("{e:#}"))?;
        let view = vault.data.view();
        *guard = Some(vault);
        Ok(view)
    }).await
}

#[tauri::command]
fn vault_lock(state: State<'_, AppState>) -> Result<()> {
    let mut vault = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
    let mut connections = state.connections.lock().map_err(|_| "Sitzungen nicht verfügbar")?;
    for (_, connection) in connections.drain() { let _ = connection.terminal.try_send(TerminalInput::Close); }
    *vault = None;
    Ok(())
}

#[tauri::command]
async fn node_save(state: State<'_, AppState>, node: Node) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || vault_action(&state, |vault| { vault.change(|data| data.upsert_node(node))?; Ok(vault.data.view()) })).await
}

#[tauri::command]
async fn node_delete(state: State<'_, AppState>, id: String) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || vault_action(&state, |vault| { vault.change(|data| data.remove_node(&id))?; Ok(vault.data.view()) })).await
}

#[tauri::command]
async fn binding_set(state: State<'_, AppState>, node_id: String, credential_id: Option<String>) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || vault_action(&state, |vault| {
        vault.change(|data| {
            anyhow::ensure!(data.nodes.iter().any(|n| n.id == node_id), "Eintrag nicht gefunden");
            if let Some(id) = credential_id {
                anyhow::ensure!(data.credentials.iter().any(|c| c.id == id), "Zugangsdaten nicht gefunden");
                data.bindings.insert(node_id, id);
            } else { data.bindings.remove(&node_id); }
            Ok(())
        })?;
        Ok(vault.data.view())
    })).await
}

#[tauri::command]
async fn credential_save(state: State<'_, AppState>, mut credential: Credential) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || vault_action(&state, |vault| {
        anyhow::ensure!(!credential.name.trim().is_empty() && !credential.username.trim().is_empty(), "Name und Benutzername fehlen");
        anyhow::ensure!(["password", "key", "agent"].contains(&credential.kind.as_str()), "Unbekannter Zugangstyp");
        anyhow::ensure!(credential.kind == "agent" || !credential.secret.is_empty(), "Passwort oder privater Schlüssel fehlt");
        credential.id = uuid::Uuid::new_v4().to_string();
        vault.change(|data| { data.credentials.push(credential); Ok(()) })?;
        Ok(vault.data.view())
    })).await
}

#[tauri::command]
async fn credential_delete(state: State<'_, AppState>, id: String) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || vault_action(&state, |vault| {
        vault.change(|data| { data.credentials.retain(|c| c.id != id); data.bindings.retain(|_, c| c != &id); Ok(()) })?;
        Ok(vault.data.view())
    })).await
}

#[tauri::command]
fn team_key_generate() -> String { STANDARD.encode(crypto::random_bytes::<32>()) }

#[tauri::command]
async fn team_save(state: State<'_, AppState>, mut team: Team) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || vault_action(&state, |vault| {
        sync::validate_team_id(&team.id)?;
        sync::endpoint(&team.server_url, &team.id)?;
        anyhow::ensure!(!team.name.trim().is_empty(), "Teamname fehlt");
        if let Some(old) = vault.data.teams.iter().find(|t| t.id == team.id) {
            if team.key.is_empty() { team.key = old.key.clone(); }
            if team.token.is_empty() { team.token = old.token.clone(); }
            team.revision = if team.server_url == old.server_url && team.key == old.key { old.revision.clone() } else { None };
            team.dirty = old.dirty;
        } else { team.revision = None; team.dirty = true; }
        crypto::decode_key(&team.key)?;
        anyhow::ensure!(team.token.len() >= 32, "Server-Token muss mindestens 32 Zeichen enthalten");
        vault.change(|data| { data.teams.retain(|t| t.id != team.id); data.teams.push(team); Ok(()) })?;
        Ok(vault.data.view())
    })).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncResult { view: VaultView, message: String }

#[tauri::command]
async fn team_sync(state: State<'_, AppState>, team_id: String, direction: String) -> Result<SyncResult> {
    let state = state.inner().clone();
    task(move || vault_action(&state, |vault| {
        let message = vault.change(|data| sync::sync(data, &team_id, &direction))?;
        Ok(SyncResult { view: vault.data.view(), message })
    })).await
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ConnectResult {
    HostKey { fingerprint: String, previous: Option<String>, host: String },
    Connected { id: String, home: String, username: String },
}

#[tauri::command]
async fn connect_node(state: State<'_, AppState>, node_id: String, trusted_fingerprint: Option<String>, output: Channel<TerminalEvent>) -> Result<ConnectResult> {
    let state = state.inner().clone();
    task(move || {
        let (node, credential, known) = vault_action(&state, |vault| {
            let node = vault.data.nodes.iter().find(|n| n.id == node_id).ok_or_else(|| anyhow::anyhow!("Eintrag nicht gefunden"))?.clone();
            anyhow::ensure!(node.kind == "connection" && ["ssh", "sftp"].contains(&node.protocol.as_str()), "Dieses Protokoll ist noch nicht implementiert");
            let credential = vault.data.credential_for(&node_id)?.clone();
            let known = vault.data.known_hosts.get(&format!("{}:{}", node.host, node.port)).cloned();
            Ok((node, credential, known))
        })?;
        let (session, fingerprint) = ssh::handshake(&node.host, node.port).map_err(|e| format!("{e:#}"))?;
        if known.as_ref() != Some(&fingerprint) && trusted_fingerprint.as_ref() != Some(&fingerprint) {
            return Ok(ConnectResult::HostKey { fingerprint, previous: known, host: format!("{}:{}", node.host, node.port) });
        }
        ssh::authenticate(&session, &credential).map_err(|e| format!("{e:#}"))?;
        let file_session = ssh::authenticated(&node, &credential, &fingerprint).map_err(|e| format!("{e:#}"))?;
        let remote = Remote::new(file_session);
        let home = remote.home().unwrap_or_else(|_| "/".into());
        let (tx, rx) = mpsc::sync_channel(512);
        // Serialize final registration with vault locking so locking cannot race a new connection.
        vault_action(&state, |vault| {
            vault.change(|data| { data.known_hosts.insert(format!("{}:{}", node.host, node.port), fingerprint); Ok(()) })?;
            ssh::start_terminal(session, rx, move |event| { let _ = output.send(event); })?;
            let id = uuid::Uuid::new_v4().to_string();
            state.connections.lock().map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?.insert(id.clone(), Connection { terminal: tx, remote: Arc::new(Mutex::new(remote)) });
            Ok(ConnectResult::Connected { id, home, username: credential.username.clone() })
        })
    }).await
}

#[tauri::command]
fn terminal_input(state: State<'_, AppState>, id: String, data: String) -> Result<()> {
    let connections = state.connections.lock().map_err(|_| "Sitzungen nicht verfügbar")?;
    let connection = connections.get(&id).ok_or("Sitzung geschlossen")?;
    if data.len() > 1024 * 1024 { return Err("Terminal-Eingabe zu groß".into()); }
    connection.terminal.try_send(TerminalInput::Data(data.into_bytes())).map_err(|e| e.to_string())
}

#[tauri::command]
fn terminal_resize(state: State<'_, AppState>, id: String, cols: u32, rows: u32) -> Result<()> {
    let connections = state.connections.lock().map_err(|_| "Sitzungen nicht verfügbar")?;
    connections.get(&id).ok_or("Sitzung geschlossen")?.terminal.try_send(TerminalInput::Resize(cols.min(1000), rows.min(1000))).map_err(|e| e.to_string())
}

#[tauri::command]
fn connection_close(state: State<'_, AppState>, id: String) -> Result<()> {
    if let Some(connection) = state.connections.lock().map_err(|_| "Sitzungen nicht verfügbar")?.remove(&id) { let _ = connection.terminal.try_send(TerminalInput::Close); }
    Ok(())
}

fn remote_for(state: &AppState, id: &str) -> Result<Arc<Mutex<Remote>>> {
    Ok(state.connections.lock().map_err(|_| "Sitzungen nicht verfügbar")?.get(id).ok_or("Sitzung geschlossen")?.remote.clone())
}

#[tauri::command]
async fn remote_list(state: State<'_, AppState>, id: String, path: String) -> Result<Vec<ssh::RemoteEntry>> {
    let remote = remote_for(&state, &id)?;
    task(move || remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?.list(&path).map_err(|e| format!("{e:#}"))).await
}

#[tauri::command]
async fn remote_sudo(state: State<'_, AppState>, id: String, enabled: bool, password: String) -> Result<()> {
    let remote = remote_for(&state, &id)?;
    task(move || remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?.set_sudo(enabled, password).map_err(|e| format!("{e:#}"))).await
}

#[tauri::command]
async fn remote_transfer(state: State<'_, AppState>, id: String, direction: String, local: String, remote_path: String) -> Result<u64> {
    let remote = remote_for(&state, &id)?;
    task(move || {
        let remote = remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?;
        let path = PathBuf::from(local);
        match direction.as_str() {
            "upload" => remote.upload(&path, &remote_path),
            "download" => remote.download(&remote_path, &path),
            _ => return Err("Ungültige Übertragungsrichtung".into()),
        }.map_err(|e| format!("{e:#}"))
    }).await
}

#[tauri::command]
async fn remote_mkdir(state: State<'_, AppState>, id: String, path: String) -> Result<()> {
    let remote = remote_for(&state, &id)?;
    task(move || remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?.mkdir(&path).map_err(|e| format!("{e:#}"))).await
}

pub fn run() {
    tauri::Builder::default().plugin(tauri_plugin_dialog::init()).manage(AppState::default())
        .invoke_handler(tauri::generate_handler![vault_exists, vault_unlock, vault_lock, node_save, node_delete, binding_set, credential_save, credential_delete, team_key_generate, team_save, team_sync, connect_node, terminal_input, terminal_resize, connection_close, remote_list, remote_sudo, remote_transfer, remote_mkdir])
        .run(tauri::generate_context!()).expect("Harbor konnte nicht gestartet werden");
}
