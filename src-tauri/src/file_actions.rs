use crate::{task, transfer_commands, AppState, Result};
use noryn_core::{
    file_actions::{self, Preview, Request},
    transfer::{Control, Progress},
};
use std::sync::Arc;
use tauri::{ipc::Channel, State};
#[tauri::command]
pub async fn file_action(
    state: State<'_, AppState>,
    id: String,
    job_id: String,
    request: Request,
    expected: Option<String>,
    output: Channel<Progress>,
) -> Result<Preview> {
    let control = Arc::new(Control::new(move |p| {
        let _ = output.send(p);
    }));
    let (remote, registration) =
        transfer_commands::register(&state, &id, &job_id, control.clone())?;
    task(move || {
        let _registration = registration;
        let run = |fs: &mut dyn file_actions::Files| -> anyhow::Result<Preview> {
            if let Some(expected) = expected {
                file_actions::execute(fs, &request, &expected, &control)
            } else {
                file_actions::preview(fs, &request, &control)
            }
        };
        let result = if request.local {
            run(&mut file_actions::Local)
        } else {
            let mut remote = remote.lock().map_err(|_| "Dateizugriff nicht verfügbar")?;
            run(&mut file_actions::Remote(&mut remote))
        };
        result.map_err(|e| format!("{e:#}"))
    })
    .await
}
