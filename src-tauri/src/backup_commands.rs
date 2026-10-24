use super::{task, vault_action, AppState, Result};
use noryn_core::{backup, model::VaultView};
use serde::Serialize;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::State;
use zeroize::Zeroizing;

pub(super) struct PendingImport {
    id: String,
    backup: backup::Backup,
    baseline: [u8; 32],
    created: Instant,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    id: String,
    summary: backup::Summary,
}

#[tauri::command]
pub async fn vault_export(
    state: State<'_, AppState>,
    path: String,
    scope: backup::Scope,
    password: String,
) -> Result<()> {
    let state = state.inner().clone();
    task(move || {
        let password = Zeroizing::new(password);
        let backup = vault_action(&state, |vault| backup::select(&vault.data, scope))?;
        backup::write_new(&PathBuf::from(path), &backup, &password).map_err(|e| format!("{e:#}"))
    })
    .await
}

#[tauri::command]
pub async fn vault_import_preview(
    state: State<'_, AppState>,
    path: String,
    password: String,
) -> Result<ImportPreview> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |_| {
            state
                .pending_import
                .lock()
                .map_err(|_| anyhow::anyhow!("Import nicht verfügbar"))?
                .take();
            Ok(())
        })?;
        let password = Zeroizing::new(password);
        let backup = backup::read(&PathBuf::from(path), &password).map_err(|e| format!("{e:#}"))?;
        vault_action(&state, |vault| {
            let summary = backup.summary(&vault.data);
            let id = uuid::Uuid::new_v4().to_string();
            *state
                .pending_import
                .lock()
                .map_err(|_| anyhow::anyhow!("Import nicht verfügbar"))? = Some(PendingImport {
                id: id.clone(),
                backup,
                baseline: backup::fingerprint(&vault.data)?,
                created: Instant::now(),
            });
            Ok(ImportPreview { id, summary })
        })
    })
    .await
}

#[tauri::command]
pub fn vault_import_cancel(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut pending = state
        .pending_import
        .lock()
        .map_err(|_| "Import nicht verfügbar")?;
    if pending.as_ref().is_some_and(|p| p.id == id) {
        pending.take();
    }
    Ok(())
}

#[tauri::command]
pub async fn vault_import_apply(
    state: State<'_, AppState>,
    id: String,
    member_name: String,
) -> Result<VaultView> {
    let state = state.inner().clone();
    task(move || {
        vault_action(&state, |vault| {
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
                "Vor dem Import bitte alle Verbindungen und Anmeldungen schließen"
            );
            let mut pending = state
                .pending_import
                .lock()
                .map_err(|_| anyhow::anyhow!("Import nicht verfügbar"))?;
            let prepared = pending
                .as_ref()
                .filter(|p| p.id == id)
                .ok_or_else(|| anyhow::anyhow!("Import-Vorschau ist abgelaufen"))?;
            anyhow::ensure!(
                prepared.created.elapsed() < Duration::from_secs(300),
                "Import-Vorschau ist abgelaufen; Datei erneut öffnen"
            );
            anyhow::ensure!(
                backup::fingerprint(&vault.data)? == prepared.baseline,
                "Der Tresor wurde seit der Vorschau geändert; Datei erneut öffnen"
            );
            let result = prepared.backup.restore(&vault.data, &member_name)?;
            anyhow::ensure!(
                state
                    .sync_jobs
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Abgleich nicht verfügbar"))?
                    .is_empty(),
                "Bitte zuerst laufende Team-Abgleiche abschließen"
            );
            vault.change(|data| {
                *data = result;
                Ok(())
            })?;
            pending.take();
            state
                .vault_epoch
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            state
                .sync_events
                .lock()
                .map_err(|_| anyhow::anyhow!("Live-Sync nicht verfügbar"))?
                .retain(|id, handle| {
                    let keep = vault
                        .data
                        .teams
                        .iter()
                        .any(|team| &team.id == id && team.live_sync);
                    if !keep {
                        handle.abort();
                    }
                    keep
                });
            Ok(vault.data.view())
        })
    })
    .await
}
