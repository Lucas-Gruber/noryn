use crate::{live_sync, task, vault_action, AppState, Result};
use noryn_core::{
    model::{Team, VaultView},
    script_sync,
    scripts::{self, Cache, Entry},
};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::State;

#[tauri::command]
pub fn script_get(state: State<'_, AppState>, team_id: String, entry_id: String) -> Result<Entry> {
    vault_action(&state, |vault| {
        vault
            .data
            .script_libraries
            .get(&team_id)
            .and_then(|c| c.library.entries.iter().find(|e| e.id == entry_id))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Skript nicht gefunden"))
    })
}
#[tauri::command]
pub fn script_save(
    state: State<'_, AppState>,
    team_id: String,
    entry: Entry,
    expected: Option<String>,
) -> Result<VaultView> {
    save_entry(&state, &team_id, entry, expected.as_deref())
}
pub fn save_entry(
    state: &AppState,
    team_id: &str,
    entry: Entry,
    expected: Option<&str>,
) -> Result<VaultView> {
    vault_action(state, |vault| {
        vault.change(|data| scripts::save(data, team_id, entry, expected))?;
        Ok(vault.data.view())
    })
}
#[tauri::command]
pub fn script_delete(
    state: State<'_, AppState>,
    team_id: String,
    entry_id: String,
    expected: String,
) -> Result<VaultView> {
    vault_action(&state, |vault| {
        vault.change(|data| scripts::remove(data, &team_id, &entry_id, &expected))?;
        Ok(vault.data.view())
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReply {
    view: VaultView,
    message: String,
}
fn check_epoch(state: &AppState, epoch: u64) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.vault_epoch.load(Ordering::Acquire) == epoch,
        "Tresor wurde während des Skript-Abgleichs gesperrt oder ersetzt"
    );
    Ok(())
}
fn apply_result(
    state: &AppState,
    team: &Team,
    before: &Cache,
    after: Cache,
    epoch: u64,
) -> Result<VaultView> {
    vault_action(state, |vault| {
        check_epoch(state, epoch)?;
        let current_team = vault
            .data
            .teams
            .iter()
            .find(|t| t.id == team.id)
            .ok_or_else(|| anyhow::anyhow!("Team wurde inzwischen entfernt"))?;
        anyhow::ensure!(
            scripts::origin(current_team) == scripts::origin(team),
            "Skript-Sync-Ziel wurde während des Abgleichs geändert; Ergebnis wird nicht lokal übernommen"
        );
        let current = vault
            .data
            .script_libraries
            .get(&team.id)
            .cloned()
            .unwrap_or_default();
        let mut applied = after;
        applied.library = scripts::merge(&before.library, &current.library, &applied.library)
            .map_err(|_| anyhow::anyhow!("Skript-Konflikt: Während des Abgleichs wurde dasselbe Skript erneut bearbeitet. Lokaler Entwurf bleibt erhalten; Serverstand erneut prüfen"))?;
        applied.suspended = current.suspended != before.suspended && current.suspended;
        if current != applied
            && (!applied.library.entries.is_empty()
                || !applied.library.history.is_empty()
                || vault.data.script_libraries.contains_key(&team.id))
        {
            vault.change(|data| {
                data.script_libraries.insert(team.id.clone(), applied);
                scripts::validate_vault(data)
            })?;
        }
        Ok(vault.data.view())
    })
}
#[tauri::command]
pub async fn script_sync(
    state: State<'_, AppState>,
    team_id: String,
    manual: bool,
    adopt_origin: bool,
    republish_missing: bool,
    keep_both: bool,
) -> Result<SyncReply> {
    let state = state.inner().clone();
    let epoch = state.vault_epoch.load(Ordering::Acquire);
    task(move || {
        let mut job = None;
        // Wait only on a local job gate. No network retry or snapshot polling.
        for _ in 0..300 {
            check_epoch(&state, epoch).map_err(|e| e.to_string())?;
            if let Ok(guard) = live_sync::Job::start(&state, &team_id) {
                job = Some(guard);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _job = job.ok_or(
            "Für dieses Team läuft noch ein Abgleich. Bitte später erneut synchronisieren",
        )?;
        let (team, before) = vault_action(&state, |vault| {
            let team = vault
                .data
                .teams
                .iter()
                .find(|t| t.id == team_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Team nicht gefunden"))?;
            Ok((
                team,
                vault
                    .data
                    .script_libraries
                    .get(&team_id)
                    .cloned()
                    .unwrap_or_default(),
            ))
        })?;
        if before.suspended && !manual {
            return vault_action(&state, |vault| {
                Ok(SyncReply {
                    view: vault.data.view(),
                    message: "Importierte Sammlung wartet auf einen manuellen Abgleich".into(),
                })
            });
        }
        if !manual && (adopt_origin || republish_missing || keep_both) {
            return Err(
                "Skript-Konflikte und Zielwechsel müssen ausdrücklich bestätigt werden".into(),
            );
        }
        let after = script_sync::synchronize(
            &team,
            &before,
            adopt_origin,
            republish_missing,
            keep_both,
            || check_epoch(&state, epoch),
        )
        .map_err(|e| format!("{e:#}"))?;
        let view = apply_result(&state, &team, &before, after, epoch)?;
        Ok(SyncReply {
            view,
            message: "Skript-Sammlung verschlüsselt abgeglichen".into(),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use noryn_core::vault::Vault;
    #[test]
    fn detached_sync_keeps_edits_made_in_flight_and_rejects_stale_epochs_and_changed_targets() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::default();
        let mut vault = Vault::open(&dir.path().join("vault.json"), "test-vault-password").unwrap();
        let team = Team {
            id: "ops".into(),
            name: "Ops".into(),
            member_name: "Alice".into(),
            server_url: "https://sync.example.test".into(),
            token: "test-token-at-least-32-characters-long".into(),
            key: STANDARD.encode([19; 32]),
            revision: None,
            dirty: false,
            live_sync: false,
            sync_base: None,
        };
        vault.data.teams.push(team.clone());
        let entry = Entry {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            kind: "script".into(),
            name: "Report".into(),
            description: String::new(),
            body: "echo original".into(),
            updated_at: 0,
            updated_by: String::new(),
        };
        scripts::save(&mut vault.data, "ops", entry, None).unwrap();
        let before = vault.data.script_libraries["ops"].clone();
        *state.vault.lock().unwrap() = Some(vault);
        let mut after = before.clone();
        after.library.entries[0].body = "echo remote".into();
        after.base = Some(after.library.clone());
        after.origin = scripts::origin(&team);
        vault_action(&state, |vault| {
            vault.change(|data| {
                data.script_libraries
                    .get_mut("ops")
                    .unwrap()
                    .library
                    .entries[0]
                    .name = "Edited while syncing".into();
                Ok(())
            })
        })
        .unwrap();
        apply_result(&state, &team, &before, after.clone(), 0).unwrap();
        let current = state
            .vault
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .data
            .script_libraries["ops"]
            .clone();
        assert_eq!(current.library.entries[0].name, "Edited while syncing");
        assert_eq!(current.library.entries[0].body, "echo remote");
        assert!(current.dirty());
        let saved = state
            .vault
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .data
            .local_revision;
        state.vault_epoch.store(1, Ordering::Release);
        assert!(apply_result(&state, &team, &before, after.clone(), 0).is_err());
        assert_eq!(
            state
                .vault
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .data
                .local_revision,
            saved
        );
        vault_action(&state, |vault| {
            vault.data.teams[0].server_url = "https://different.example.test".into();
            Ok(())
        })
        .unwrap();
        assert!(apply_result(&state, &team, &before, after.clone(), 1).is_err());
        vault_action(&state, |vault| {
            vault.data.teams[0].server_url.clone_from(&team.server_url);
            vault
                .data
                .script_libraries
                .get_mut("ops")
                .unwrap()
                .library
                .entries[0]
                .body = "echo local conflicting edit".into();
            Ok(())
        })
        .unwrap();
        assert!(apply_result(&state, &team, &before, after, 1).is_err());
        assert_eq!(
            state
                .vault
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .data
                .script_libraries["ops"]
                .library
                .entries[0]
                .body,
            "echo local conflicting edit"
        );
    }
}
