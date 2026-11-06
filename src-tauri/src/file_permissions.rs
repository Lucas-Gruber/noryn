use crate::{task, transfer_commands, AppState, Result};
use noryn_core::{
    file_permissions::{self, Change, Snapshot},
    transfer::Control,
};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn file_permissions_tree(
    state: State<'_, AppState>,
    id: String,
    job_id: String,
    request: file_permissions::tree::Request,
    expected: Option<String>,
) -> Result<file_permissions::tree::Preview> {
    use file_permissions::tree;
    request.change.validate().map_err(|e| e.to_string())?;
    let control = Arc::new(Control::default());
    let (remote, registration) =
        transfer_commands::register(&state, &id, &job_id, control.clone())?;
    task(move || {
        let _registration = registration;
        let run = |backend: &mut dyn tree::Backend| {
            if let Some(expected) = &expected {
                tree::execute(backend, &request, expected, &control)
            } else {
                tree::preview(backend, &request, &control)
            }
        };
        let result = if request.local {
            run(&mut tree::Local)
        } else {
            let mut remote = remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?;
            run(&mut *remote)
        };
        let preview = result.map_err(|e| format!("{e:#}"))?;
        control.check().map_err(|e| e.to_string())?;
        Ok(preview)
    })
    .await
}

#[tauri::command]
pub async fn file_permissions(
    state: State<'_, AppState>,
    id: String,
    job_id: String,
    path: String,
    local: bool,
    change: Option<Change>,
    expected: Option<String>,
) -> Result<Snapshot> {
    if change.is_some() != expected.is_some() {
        return Err("Änderungen benötigen frisch geladene Dateimetadaten".into());
    }
    if let Some(change) = &change {
        change.validate().map_err(|e| e.to_string())?;
    }
    let control = Arc::new(Control::default());
    let (remote, registration) =
        transfer_commands::register(&state, &id, &job_id, control.clone())?;
    task(move || {
        let _registration = registration;
        let run = || -> anyhow::Result<Snapshot> {
            control.check()?;
            if local {
                return match (&change, &expected) {
                    (Some(change), Some(expected)) => {
                        file_permissions::local_apply(&path, expected, change, &control)
                    }
                    _ => file_permissions::local_read(&path),
                };
            }
            let mut remote = remote
                .lock()
                .map_err(|_| anyhow::anyhow!("Dateizugriff nicht verfügbar"))?;
            control.check()?;
            match (&change, &expected) {
                (Some(change), Some(expected)) => {
                    remote.permissions_apply(&path, expected, change, &control)
                }
                _ => remote.permissions_read(&path),
            }
        };
        let snapshot = run().map_err(|e| format!("{e:#}"))?;
        control.check().map_err(|e| e.to_string())?;
        Ok(snapshot)
    })
    .await
}
