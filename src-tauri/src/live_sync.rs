use super::{task, vault_action, AppState, Result};
use futures_util::StreamExt;
use noryn_core::{
    auto_sync,
    model::VaultView,
    sync,
    sync_merge::{self, Conflict, Merged},
};
use serde::Serialize;
use std::{
    collections::HashSet,
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};
use tauri::{ipc::Channel, State};
use zeroize::Zeroizing;

struct WatchConfig {
    id: String,
    server_url: String,
    token: Zeroizing<String>,
}

pub struct Job {
    jobs: Arc<Mutex<HashSet<String>>>,
    id: String,
}
impl Job {
    pub fn start(state: &AppState, id: &str) -> Result<Self> {
        let mut jobs = state
            .sync_jobs
            .lock()
            .map_err(|_| "Abgleich nicht verfügbar")?;
        if !jobs.insert(id.into()) {
            return Err("Für dieses Team läuft bereits ein Abgleich".into());
        }
        Ok(Self {
            jobs: state.sync_jobs.clone(),
            id: id.into(),
        })
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchEvent {
    kind: &'static str,
    revision: Option<String>,
    message: &'static str,
}

async fn subscribe(team: &WatchConfig, output: &Channel<WatchEvent>) -> anyhow::Result<()> {
    let url = sync::events_endpoint(&team.server_url, &team.id)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(75))
        .build()?;
    let response = client
        .get(url)
        .bearer_auth(team.token.as_str())
        .header("Accept", "text/event-stream")
        .send()
        .await?
        .error_for_status()?;
    anyhow::ensure!(
        response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.starts_with("text/event-stream")),
        "Server unterstützt keine Live-Benachrichtigungen"
    );
    output.send(WatchEvent {
        kind: "connected",
        revision: None,
        message: "Live-Verbindung aktiv",
    })?;
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut event = String::new();
    while let Some(chunk) = stream.next().await {
        for byte in chunk? {
            if byte == b'\n' {
                let line = std::str::from_utf8(&buffer)?.trim_end_matches('\r');
                if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim().into();
                }
                if event == "revision" {
                    if let Some(value) = line.strip_prefix("data:") {
                        let value = value.trim();
                        anyhow::ensure!(
                            value == "none"
                                || (value.len() == 66
                                    && value.starts_with('"')
                                    && value.ends_with('"')
                                    && value.as_bytes()[1..65].iter().all(u8::is_ascii_hexdigit)),
                            "Ungültige Live-Revision"
                        );
                        output.send(WatchEvent {
                            kind: "revision",
                            revision: Some(value.into()),
                            message: "Neue Server-Revision",
                        })?;
                    }
                }
                if line.is_empty() {
                    event.clear();
                }
                buffer.clear();
            } else {
                anyhow::ensure!(buffer.len() < 4096, "Live-Nachricht ist zu groß");
                buffer.push(byte);
            }
        }
    }
    anyhow::bail!("Live-Verbindung beendet")
}

pub fn stop(state: &AppState, id: &str) -> Result<()> {
    if let Some(handle) = state
        .sync_events
        .lock()
        .map_err(|_| "Live-Sync nicht verfügbar")?
        .remove(id)
    {
        handle.abort();
    }
    Ok(())
}

#[tauri::command]
pub fn team_events_stop(state: State<'_, AppState>, team_id: String) -> Result<()> {
    stop(&state, &team_id)
}

#[tauri::command]
pub fn team_events_start(
    state: State<'_, AppState>,
    team_id: String,
    output: Channel<WatchEvent>,
) -> Result<()> {
    vault_action(&state, |vault| {
        let team = vault
            .data
            .teams
            .iter()
            .find(|t| t.id == team_id)
            .ok_or_else(|| anyhow::anyhow!("Team nicht gefunden"))?;
        anyhow::ensure!(team.live_sync, "Live-Sync ist ausgeschaltet");
        noryn_core::team_activity::validate_member_name(&team.member_name)?;
        let team = WatchConfig {
            id: team.id.clone(),
            server_url: team.server_url.clone(),
            token: Zeroizing::new(team.token.clone()),
        };
        let mut events = state
            .sync_events
            .lock()
            .map_err(|_| anyhow::anyhow!("Live-Sync nicht verfügbar"))?;
        if let Some(handle) = events.get(&team_id) {
            if !handle.inner().is_finished() {
                return Ok(());
            }
        }
        let handle = tauri::async_runtime::spawn(async move {
            let mut delay = 2;
            loop {
                let error = match subscribe(&team, &output).await {
                    Ok(()) => return,
                    Err(error) => error,
                };
                let status = error
                    .downcast_ref::<reqwest::Error>()
                    .and_then(reqwest::Error::status);
                let fatal = match status {
                    Some(reqwest::StatusCode::NOT_FOUND) => Some("Der Sync-Server unterstützt noch keinen Live-Sync. Bitte den Server aktualisieren und erneut verbinden."),
                    Some(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN) => Some("Live-Sync wurde abgewiesen. Bitte Team-ID und Server-Token prüfen."),
                    _ => None,
                };
                if let Some(message) = fatal {
                    let _ = output.send(WatchEvent {
                        kind: "error",
                        revision: None,
                        message,
                    });
                    return;
                }
                if output
                    .send(WatchEvent {
                        kind: "error",
                        revision: None,
                        message:
                            "Live-Verbindung unterbrochen. Erneuter Versuch folgt automatisch.",
                    })
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(60);
            }
        });
        events.insert(team_id, handle);
        Ok(())
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReply {
    view: VaultView,
    message: String,
    conflicts: Vec<Conflict>,
}

#[tauri::command]
pub async fn team_auto_sync(state: State<'_, AppState>, team_id: String) -> Result<AutoReply> {
    let state = state.inner().clone();
    task(move || {
        let _job = Job::start(&state, &team_id)?;
        let (mut work, initial, epoch, reserved_nodes, reserved_credentials) =
            vault_action(&state, |vault| {
                let team = vault
                    .data
                    .teams
                    .iter()
                    .find(|t| t.id == team_id)
                    .ok_or_else(|| anyhow::anyhow!("Team nicht gefunden"))?;
                anyhow::ensure!(team.live_sync, "Live-Sync ist ausgeschaltet");
                let initial = sync::snapshot(&vault.data, &team_id);
                let work = sync::detached(&vault.data, &team_id)?;
                let nodes: HashSet<String> = vault
                    .data
                    .nodes
                    .iter()
                    .filter(|n| n.workspace != team_id)
                    .map(|n| n.id.clone())
                    .collect();
                let credentials: HashSet<String> = vault
                    .data
                    .credentials
                    .iter()
                    .filter(|c| c.team_id.as_ref() != Some(&team_id))
                    .map(|c| c.id.clone())
                    .collect();
                Ok((
                    work,
                    initial,
                    state.vault_epoch.load(Ordering::Acquire),
                    nodes,
                    credentials,
                ))
            })?;
        let outcome = auto_sync::reconcile_checked(&mut work, &team_id, |snapshot| {
            anyhow::ensure!(
                snapshot
                    .nodes
                    .iter()
                    .all(|n| !reserved_nodes.contains(&n.id)),
                "Server-Eintrag kollidiert mit einem anderen Workspace"
            );
            anyhow::ensure!(
                snapshot
                    .credentials
                    .iter()
                    .all(|c| !reserved_credentials.contains(&c.id)),
                "Server-Zugang kollidiert mit einem persönlichen oder anderen Team-Konto"
            );
            Ok(())
        })
        .map_err(|e| format!("{e:#}"))?;
        vault_action(&state, |vault| {
            anyhow::ensure!(
                epoch == state.vault_epoch.load(Ordering::Acquire),
                "Tresor wurde zwischenzeitlich gesperrt oder ersetzt"
            );
            let current = vault
                .data
                .teams
                .iter()
                .find(|t| t.id == team_id)
                .ok_or_else(|| anyhow::anyhow!("Team nicht gefunden"))?;
            let uploaded = &work.teams[0];
            anyhow::ensure!(
                current.live_sync
                    && current.server_url == uploaded.server_url
                    && current.token == uploaded.token
                    && current.key == uploaded.key,
                "Team-Konfiguration wurde geändert"
            );
            if !outcome.conflicts.is_empty() {
                return Ok(AutoReply {
                    view: vault.data.view(),
                    message: outcome.message,
                    conflicts: outcome.conflicts,
                });
            }
            let server = sync::snapshot(&work, &team_id);
            let local = sync::snapshot(&vault.data, &team_id);
            let merged = match sync_merge::merge(&initial, &local, &server)? {
                Merged::Ready(snapshot) => snapshot,
                Merged::Conflicts(conflicts) => {
                    return Ok(AutoReply {
                        view: vault.data.view(),
                        message: "Weitere lokale Änderungen stehen im Konflikt. Bitte prüfen."
                            .into(),
                        conflicts,
                    })
                }
            };
            let dirty = merged != server;
            if merged == local
                && current.revision == uploaded.revision
                && current.sync_base == uploaded.sync_base
                && current.dirty == dirty
            {
                return Ok(AutoReply {
                    view: vault.data.view(),
                    message: outcome.message,
                    conflicts: vec![],
                });
            }
            vault.change(|data| {
                sync::apply_snapshot(data, &team_id, merged)?;
                let team = data
                    .teams
                    .iter_mut()
                    .find(|t| t.id == team_id)
                    .ok_or_else(|| anyhow::anyhow!("Team fehlt"))?;
                team.revision = uploaded.revision.clone();
                team.sync_base = uploaded.sync_base.clone();
                team.dirty = dirty;
                Ok(())
            })?;
            Ok(AutoReply {
                view: vault.data.view(),
                message: outcome.message,
                conflicts: vec![],
            })
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_and_live_jobs_are_serialized_per_team_and_released_after_errors() {
        let state = AppState::default();
        let first = Job::start(&state, "ops").unwrap();
        assert!(Job::start(&state, "ops").is_err());
        let other = Job::start(&state, "other").unwrap();
        drop(first);
        assert!(Job::start(&state, "ops").is_ok());
        drop(other);
        assert!(state.sync_jobs.lock().unwrap().is_empty());
    }
}
