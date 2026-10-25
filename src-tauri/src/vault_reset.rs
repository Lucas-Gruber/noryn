use crate::{task, vault_paths, AppState, Result};
use noryn_core::{
    model::VaultView,
    vault::{ResetSnapshot, Vault},
};
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        MutexGuard,
    },
    time::{Duration, Instant},
};
use tauri::State;
use zeroize::Zeroizing;

pub struct PendingReset {
    id: String,
    snapshot: ResetSnapshot,
    epoch: u64,
    created: Instant,
}
#[derive(Serialize)]
pub struct Preview {
    id: String,
}

fn locked_and_idle<'a>(
    state: &'a AppState,
    installing: &AtomicBool,
) -> Result<MutexGuard<'a, Option<Vault>>> {
    let vault = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
    if vault.is_some() {
        return Err("Zum Zurücksetzen bitte zuerst den Tresor sperren".into());
    }
    if installing.load(Ordering::Acquire) {
        return Err("Ein Update wird gerade installiert".into());
    }
    if !state
        .connections
        .lock()
        .map_err(|_| "Sitzungen nicht verfügbar")?
        .is_empty()
        || !state
            .auth_requests
            .lock()
            .map_err(|_| "Anmeldungen nicht verfügbar")?
            .is_empty()
        || !state
            .transfers
            .lock()
            .map_err(|_| "Transfers nicht verfügbar")?
            .active
            .is_empty()
        || !state
            .sync_jobs
            .lock()
            .map_err(|_| "Abgleich nicht verfügbar")?
            .is_empty()
        || !state
            .sync_events
            .lock()
            .map_err(|_| "Live-Sync nicht verfügbar")?
            .is_empty()
        || crate::batch_commands::running(state)?
    {
        return Err("Vor dem Zurücksetzen bitte laufende Verbindungen, Transfers, Befehlsaufträge und Team-Abgleiche beenden".into());
    }
    Ok(vault)
}

#[tauri::command]
pub async fn vault_reset_preview(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    updates: State<'_, crate::updates::UpdateState>,
) -> Result<Preview> {
    let state = state.inner().clone();
    let installing = updates.installing.clone();
    let (path, previous) = vault_paths(&app)?;
    task(move || prepare(&state, &installing, &path, &previous)).await
}
fn prepare(
    state: &AppState,
    installing: &AtomicBool,
    path: &Path,
    previous: &Path,
) -> Result<Preview> {
    let _vault = locked_and_idle(state, installing)?;
    let mut pending = state
        .pending_reset
        .lock()
        .map_err(|_| "Zurücksetzen nicht verfügbar")?;
    // A failed refresh must not leave the older confirmation usable.
    pending.take();
    let snapshot = ResetSnapshot::inspect(path, previous).map_err(|e| format!("{e:#}"))?;
    let id = uuid::Uuid::new_v4().to_string();
    *pending = Some(PendingReset {
        id: id.clone(),
        snapshot,
        epoch: state.vault_epoch.load(Ordering::Acquire),
        created: Instant::now(),
    });
    Ok(Preview { id })
}

#[tauri::command]
pub fn vault_reset_cancel(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut pending = state
        .pending_reset
        .lock()
        .map_err(|_| "Zurücksetzen nicht verfügbar")?;
    if pending.as_ref().is_some_and(|pending| pending.id == id) {
        pending.take();
    }
    Ok(())
}

#[tauri::command]
pub async fn vault_reset_apply(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    updates: State<'_, crate::updates::UpdateState>,
    id: String,
    confirmation: String,
    password: String,
) -> Result<VaultView> {
    let state = state.inner().clone();
    let installing = updates.installing.clone();
    let password = Zeroizing::new(password);
    let (path, previous) = vault_paths(&app)?;
    task(move || {
        apply(
            &state,
            &installing,
            &path,
            &previous,
            &id,
            &confirmation,
            &password,
        )
    })
    .await
}
fn apply(
    state: &AppState,
    installing: &AtomicBool,
    path: &Path,
    previous: &Path,
    id: &str,
    confirmation: &str,
    password: &str,
) -> Result<VaultView> {
    if confirmation != "ZURÜCKSETZEN" {
        return Err("Zur Bestätigung bitte ZURÜCKSETZEN eingeben".into());
    }
    let mut guard = locked_and_idle(state, installing)?;
    let mut pending = state
        .pending_reset
        .lock()
        .map_err(|_| "Zurücksetzen nicht verfügbar")?;
    let prepared = pending
        .as_ref()
        .filter(|pending| pending.id == id)
        .ok_or("Bestätigung ist abgelaufen. Dialog erneut öffnen.")?;
    if prepared.created.elapsed() >= Duration::from_secs(300)
        || prepared.epoch != state.vault_epoch.load(Ordering::Acquire)
    {
        pending.take();
        return Err("Bestätigung ist abgelaufen. Dialog erneut öffnen.".into());
    }
    let revision = state
        .locked_revision
        .load(Ordering::Acquire)
        .checked_add(1)
        .ok_or("Lokale Tresor-Revision ist ausgeschöpft")?;
    // Acquire every fallible state lock before the atomic file replacement.
    let mut import = state
        .pending_import
        .lock()
        .map_err(|_| "Import nicht verfügbar")?;
    let fresh = Vault::reset(path, previous, &prepared.snapshot, password, revision)
        .map_err(|e| format!("{e:#}"))?;
    let view = fresh.data.view();
    *guard = Some(fresh);
    pending.take();
    import.take();
    state.locked_revision.store(revision, Ordering::Release);
    state.vault_epoch.fetch_add(1, Ordering::Release);
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use noryn_core::transfer::Control;
    use std::{fs, sync::Arc};
    const PASSWORD: &str = "new-local-password-123";
    #[test]
    fn reset_requires_locked_idle_state_and_matching_fresh_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.json");
        let previous = dir.path().join("previous.json");
        fs::write(&path, b"encrypted test data").unwrap();
        let state = AppState::default();
        let installing = AtomicBool::new(true);
        assert!(prepare(&state, &installing, &path, &previous).is_err());
        installing.store(false, Ordering::Release);
        state.sync_jobs.lock().unwrap().insert("team".into());
        assert!(prepare(&state, &installing, &path, &previous).is_err());
        state.sync_jobs.lock().unwrap().clear();
        state.transfers.lock().unwrap().active.insert(
            "job".into(),
            ("session".into(), Arc::new(Control::default())),
        );
        assert!(prepare(&state, &installing, &path, &previous).is_err());
        state.transfers.lock().unwrap().active.clear();
        let preview = prepare(&state, &installing, &path, &previous).unwrap();
        assert!(apply(
            &state,
            &installing,
            &path,
            &previous,
            "wrong-id",
            "ZURÜCKSETZEN",
            PASSWORD
        )
        .is_err());
        assert!(apply(
            &state,
            &installing,
            &path,
            &previous,
            &preview.id,
            "",
            PASSWORD
        )
        .is_err());
        assert!(apply(
            &state,
            &installing,
            &path,
            &previous,
            &preview.id,
            "ZURÜCKSETZEN",
            "asdf"
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), b"encrypted test data");
        state
            .pending_reset
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .created = Instant::now() - Duration::from_secs(301);
        assert!(apply(
            &state,
            &installing,
            &path,
            &previous,
            &preview.id,
            "ZURÜCKSETZEN",
            PASSWORD
        )
        .is_err());
        let preview = prepare(&state, &installing, &path, &previous).unwrap();
        state.vault_epoch.fetch_add(1, Ordering::Release);
        assert!(apply(
            &state,
            &installing,
            &path,
            &previous,
            &preview.id,
            "ZURÜCKSETZEN",
            PASSWORD
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), b"encrypted test data");
    }
    #[test]
    fn successful_reset_advances_the_revision_and_invalidates_the_preview() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.json");
        let previous = dir.path().join("previous.json");
        fs::write(&path, b"encrypted test data").unwrap();
        let state = AppState::default();
        state.locked_revision.store(500, Ordering::Release);
        let installing = AtomicBool::new(false);
        let preview = prepare(&state, &installing, &path, &previous).unwrap();
        let view = apply(
            &state,
            &installing,
            &path,
            &previous,
            &preview.id,
            "ZURÜCKSETZEN",
            PASSWORD,
        )
        .unwrap();
        assert_eq!(view.local_revision, 501);
        assert!(view.nodes.is_empty());
        assert_eq!(state.vault_epoch.load(Ordering::Acquire), 1);
        assert!(state.pending_reset.lock().unwrap().is_none());
        assert!(state.vault.lock().unwrap().is_some());
        let bytes = fs::read(&path).unwrap();
        assert!(prepare(&state, &installing, &path, &previous).is_err());
        assert!(apply(
            &state,
            &installing,
            &path,
            &previous,
            &preview.id,
            "ZURÜCKSETZEN",
            PASSWORD
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
