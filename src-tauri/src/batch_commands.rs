//! Bounded parallel execution; outputs remain ephemeral and never enter the vault.
use crate::{task, vault_action, AppState, Result};
use noryn_core::{command_execution::Output, ssh_tools::Tools};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{ipc::Channel, State};

#[derive(Default)]
pub struct Registry {
    active: HashMap<String, Arc<AtomicBool>>,
    cancelled: VecDeque<String>,
}
pub type Jobs = Arc<Mutex<Registry>>;
struct Registration {
    jobs: Jobs,
    id: String,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.active.remove(&self.id);
        }
    }
}
pub fn cancel_all(state: &AppState) -> Result<()> {
    for flag in state
        .command_jobs
        .lock()
        .map_err(|_| "Befehlsaufträge nicht verfügbar")?
        .active
        .values()
    {
        flag.store(true, Ordering::Release);
    }
    Ok(())
}
pub fn running(state: &AppState) -> Result<bool> {
    Ok(!state
        .command_jobs
        .lock()
        .map_err(|_| "Befehlsaufträge nicht verfügbar")?
        .active
        .is_empty())
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    id: String,
    output: Output,
}

type Registered = (Registration, Arc<AtomicBool>, Vec<(String, Arc<Tools>)>);

fn register(
    state: &AppState,
    run_id: &str,
    ids: &[String],
    command_id: &str,
    expected: &str,
) -> Result<Registered> {
    vault_action(state, |vault| {
        anyhow::ensure!(
            uuid::Uuid::parse_str(run_id).is_ok(),
            "Ungültige Auftrags-ID"
        );
        let unique: HashSet<_> = ids.iter().collect();
        anyhow::ensure!(
            !ids.is_empty() && ids.len() <= 16 && unique.len() == ids.len(),
            "1–16 unterschiedliche SSH-Sitzungen auswählen"
        );
        let command = vault
            .data
            .commands
            .iter()
            .find(|c| c.id == command_id)
            .ok_or_else(|| anyhow::anyhow!("Befehl wurde entfernt"))?;
        anyhow::ensure!(
            command.command == expected,
            "Befehl wurde inzwischen geändert. Bitte erneut prüfen"
        );
        let connections = state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
        let targets = ids
            .iter()
            .map(|id| {
                let connection = connections
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("Sitzung wurde geschlossen"))?;
                anyhow::ensure!(
                    connection.terminal.is_some(),
                    "Nur offene SSH-Terminals sind auswählbar"
                );
                let tools = connection
                    .tools
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("SSH-Sitzung fehlt"))?;
                anyhow::ensure!(!tools.is_closed(), "SSH-Sitzung wurde beendet");
                Ok((id.clone(), tools))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut jobs = state
            .command_jobs
            .lock()
            .map_err(|_| anyhow::anyhow!("Befehlsaufträge nicht verfügbar"))?;
        if let Some(index) = jobs.cancelled.iter().position(|id| id == run_id) {
            jobs.cancelled.remove(index);
            anyhow::bail!("Ausführung abgebrochen");
        }
        anyhow::ensure!(
            jobs.active.is_empty(),
            "Es läuft bereits eine parallele Befehlsausführung"
        );
        let flag = Arc::new(AtomicBool::new(false));
        jobs.active.insert(run_id.into(), flag.clone());
        Ok((
            Registration {
                jobs: state.command_jobs.clone(),
                id: run_id.into(),
            },
            flag,
            targets,
        ))
    })
}
#[tauri::command]
pub async fn command_batch_run(
    state: State<'_, AppState>,
    run_id: String,
    ids: Vec<String>,
    command_id: String,
    expected_command: String,
    output: Channel<Reply>,
) -> Result<Vec<Reply>> {
    let (registration, cancelled, targets) =
        register(&state, &run_id, &ids, &command_id, &expected_command)?;
    task(move || {
        let _registration = registration;
        let command = zeroize::Zeroizing::new(expected_command);
        let next = AtomicUsize::new(0);
        let replies = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..targets.len().min(4) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((id, tools)) = targets.get(i) else {
                        break;
                    };
                    let reply = Reply {
                        id: id.clone(),
                        output: tools.run_command(&command, &cancelled, Duration::from_secs(120)),
                    };
                    if output.send(reply.clone()).is_err() {
                        cancelled.store(true, Ordering::Release);
                    }
                    if let Ok(mut replies) = replies.lock() {
                        replies.push(reply);
                    }
                });
            }
        });
        replies
            .into_inner()
            .map_err(|_| "Befehlsergebnisse nicht verfügbar".into())
    })
    .await
}
#[tauri::command]
pub fn command_batch_cancel(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let mut jobs = state
        .command_jobs
        .lock()
        .map_err(|_| "Befehlsaufträge nicht verfügbar")?;
    if let Some(flag) = jobs.active.get(&run_id) {
        flag.store(true, Ordering::Release);
    } else {
        if uuid::Uuid::parse_str(&run_id).is_err() {
            return Err("Ungültige Auftrags-ID".into());
        }
        if jobs.cancelled.len() >= 32 {
            jobs.cancelled.pop_front();
        }
        jobs.cancelled.push_back(run_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use noryn_core::{commands::SavedCommand, vault::Vault};
    #[test]
    fn invalid_stale_duplicate_and_cancelled_batches_never_start() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::default();
        let command_id = uuid::Uuid::new_v4().to_string();
        let mut vault =
            Vault::open(&directory.path().join("vault.json"), "test-only-password").unwrap();
        vault.data.commands.push(SavedCommand {
            id: command_id.clone(),
            workspace: "private".into(),
            name: "Test".into(),
            description: String::new(),
            command: "true".into(),
        });
        *state.vault.lock().unwrap() = Some(vault);
        let run_id = uuid::Uuid::new_v4().to_string();
        assert!(register(&state, &run_id, &[], &command_id, "true").is_err());
        let stale = register(&state, &run_id, &["missing".into()], &command_id, "changed");
        assert!(matches!(stale, Err(e) if e.contains("geändert")));
        assert!(register(
            &state,
            &run_id,
            &["same".into(), "same".into()],
            &command_id,
            "true"
        )
        .is_err());
        assert!(register(&state, &run_id, &["missing".into()], &command_id, "true").is_err());
        let session = ssh2::Session::new().unwrap();
        let tools = Arc::new(Tools::new(session.clone()));
        let (terminal, _receiver) = std::sync::mpsc::sync_channel(1);
        state.connections.lock().unwrap().insert(
            "a".into(),
            crate::Connection {
                health: None,
                quick: None,
                terminal: Some(terminal),
                tools: Some(tools.clone()),
                remote: Arc::new(Mutex::new(noryn_core::remote::Remote::Ssh(
                    noryn_core::ssh::Remote::new(session),
                ))),
            },
        );
        state
            .command_jobs
            .lock()
            .unwrap()
            .cancelled
            .push_back(run_id.clone());
        assert!(
            matches!(register(&state, &run_id, &["a".into()], &command_id, "true"), Err(e) if e.contains("abgebrochen"))
        );
        let (registration, flag, _) =
            register(&state, &run_id, &["a".into()], &command_id, "true").unwrap();
        assert!(running(&state).unwrap());
        assert!(register(
            &state,
            &uuid::Uuid::new_v4().to_string(),
            &["a".into()],
            &command_id,
            "true"
        )
        .is_err());
        cancel_all(&state).unwrap();
        assert!(flag.load(Ordering::Acquire));
        drop(registration);
        assert!(state.command_jobs.lock().unwrap().active.is_empty());
        tools.close();
        assert!(register(&state, &run_id, &["a".into()], &command_id, "true").is_err());
    }
}
