use crate::{task, vault_action, AppState, Result};
use noryn_core::transfer::{self, Control, Progress, Report};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{ipc::Channel, State};

#[derive(Default)]
pub struct Registry {
    pub active: HashMap<String, (String, Arc<Control>)>,
    // Cancellation may arrive before an async command acquires the vault lock.
    // Keep a bounded set of recent UUIDs so closing the panel cannot lose it.
    cancelled: VecDeque<String>,
}
pub type Transfers = Arc<Mutex<Registry>>;

pub fn cancel(state: &AppState, connection: Option<&str>) -> Result<()> {
    for (id, control) in state
        .transfers
        .lock()
        .map_err(|_| "Transfers nicht verfügbar")?
        .active
        .values()
    {
        if connection.is_none_or(|connection| connection == id) {
            control.cancel();
        }
    }
    Ok(())
}
struct Registration {
    transfers: Transfers,
    id: String,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut transfers) = self.transfers.lock() {
            transfers.active.remove(&self.id);
        }
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn remote_transfer(
    state: State<'_, AppState>,
    id: String,
    transfer_id: String,
    direction: String,
    local: String,
    remote_path: String,
    overwrite: Option<bool>,
    output: Channel<Progress>,
) -> Result<Report> {
    let control = Arc::new(Control::new(move |progress| {
        let _ = output.send(progress);
    }));
    let remote = vault_action(&state, |_| {
        anyhow::ensure!(
            uuid::Uuid::parse_str(&transfer_id).is_ok(),
            "Ungültige Transfer-ID"
        );
        anyhow::ensure!(
            direction == "upload" || direction == "download",
            "Ungültige Übertragungsrichtung"
        );
        let connections = state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
        let remote = connections
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("Sitzung geschlossen"))?
            .remote
            .clone();
        let mut transfers = state
            .transfers
            .lock()
            .map_err(|_| anyhow::anyhow!("Transfers nicht verfügbar"))?;
        if let Some(index) = transfers.cancelled.iter().position(|id| id == &transfer_id) {
            transfers.cancelled.remove(index);
            anyhow::bail!("Übertragung abgebrochen");
        }
        anyhow::ensure!(
            transfers.active.len() < 128
                && !transfers.active.contains_key(&transfer_id)
                && !transfers
                    .active
                    .values()
                    .any(|(connection, _)| connection == &id),
            "Für diese Sitzung läuft bereits ein Transfer"
        );
        transfers
            .active
            .insert(transfer_id.clone(), (id, control.clone()));
        Ok(remote)
    })?;
    let registration = Registration {
        transfers: state.transfers.clone(),
        id: transfer_id,
    };
    task(move || {
        let _registration = registration;
        let mut remote = remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?;
        let local = PathBuf::from(local);
        let result = if direction == "upload" {
            transfer::upload(&mut *remote, &local, &remote_path, &control)
        } else {
            transfer::download(
                &mut *remote,
                &remote_path,
                &local,
                overwrite.unwrap_or(false),
                &control,
            )
        };
        result.map_err(|e| format!("{e:#}"))
    })
    .await
}

#[tauri::command]
pub fn remote_transfer_cancel(state: State<'_, AppState>, transfer_id: String) -> Result<()> {
    if uuid::Uuid::parse_str(&transfer_id).is_err() {
        return Err("Ungültige Transfer-ID".into());
    }
    let mut registry = state
        .transfers
        .lock()
        .map_err(|_| "Transfers nicht verfügbar")?;
    cancel_job(&mut registry, transfer_id);

    Ok(())
}

fn cancel_job(registry: &mut Registry, id: String) {
    if let Some((_, control)) = registry.active.get(&id) {
        control.cancel();
    } else if !registry.cancelled.contains(&id) {
        if registry.cancelled.len() == 128 {
            registry.cancelled.pop_front();
        }
        registry.cancelled.push_back(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_before_registration_is_remembered_and_bounded() {
        let mut registry = Registry::default();
        cancel_job(&mut registry, "pending".into());
        cancel_job(&mut registry, "pending".into());
        assert_eq!(registry.cancelled.len(), 1);
        assert!(registry.active.is_empty());
        for n in 0..200 {
            cancel_job(&mut registry, n.to_string());
        }
        assert_eq!(registry.cancelled.len(), 128);
        assert!(!registry.cancelled.contains(&"pending".into()));
    }
    #[test]
    fn closing_cancels_only_its_connection_and_lock_cancels_all() {
        let state = AppState::default();
        let a = Arc::new(Control::default());
        let b = Arc::new(Control::default());
        state
            .transfers
            .lock()
            .unwrap()
            .active
            .insert("job-a".into(), ("a".into(), a.clone()));
        state
            .transfers
            .lock()
            .unwrap()
            .active
            .insert("job-b".into(), ("b".into(), b.clone()));
        cancel(&state, Some("a")).unwrap();
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
        cancel(&state, None).unwrap();
        assert!(b.is_cancelled());
        let registration = Registration {
            transfers: state.transfers.clone(),
            id: "job-a".into(),
        };
        drop(registration);
        let transfers = state.transfers.lock().unwrap();
        assert!(!transfers.active.contains_key("job-a"));
        assert!(transfers.active.contains_key("job-b"));
    }
}
