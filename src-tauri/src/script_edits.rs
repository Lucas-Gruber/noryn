//! Protected, bounded editor copies; no executable file association and no SSH session required.
use crate::{
    external_editor::{self, Launch},
    task, vault_action, AppState, Result,
};
use anyhow::{ensure, Context};
use noryn_core::{model::VaultView, scripts, text_files, transfer::Control};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
};
use tauri::State;
type Exit = tokio::sync::oneshot::Receiver<std::result::Result<(), String>>;
pub type Documents = Arc<Mutex<HashMap<String, Arc<Document>>>>;
pub struct Document {
    team: String,
    entry: String,
    control: Control,
    draft: Mutex<Draft>,
    editor: Mutex<Option<Exit>>,
}
struct Draft {
    view: View,
    local: PathBuf,
    baseline: text_files::DigestBytes,
    _directory: tempfile::TempDir,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    id: String,
    team_id: String,
    name: String,
    auto_finish: bool,
    editor_note: String,
}
fn document(state: &AppState, id: &str) -> anyhow::Result<Arc<Document>> {
    let doc = state
        .script_documents
        .lock()
        .map_err(|_| anyhow::anyhow!("Skript-Arbeitskopien nicht verfügbar"))?
        .get(id)
        .cloned()
        .context("Skript-Arbeitskopie wurde geschlossen")?;
    doc.control.check()?;
    Ok(doc)
}
pub fn ensure_closed(state: &AppState, team: Option<&str>) -> anyhow::Result<()> {
    ensure!(
        !state
            .script_documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Skript-Arbeitskopien nicht verfügbar"))?
            .values()
            .any(|d| team.is_none_or(|id| d.team == id)),
        "Bitte zuerst externe Skript-Bearbeitungen beenden oder verwerfen"
    );
    Ok(())
}
pub fn clear(state: &AppState) -> Result<()> {
    let mut docs = state
        .script_documents
        .lock()
        .map_err(|_| "Skript-Arbeitskopien nicht verfügbar")?;
    for doc in docs.values() {
        doc.control.cancel();
    }
    docs.clear();
    Ok(())
}
fn open_copy(
    state: &AppState,
    team: &str,
    id: &str,
    expected: &str,
    mut opener: impl FnMut(&Path) -> anyhow::Result<Launch>,
) -> Result<View> {
    vault_action(state, |vault| {
        let entry = vault
            .data
            .script_libraries
            .get(team)
            .and_then(|c| {
                c.library
                    .entries
                    .iter()
                    .find(|e| e.id == id && e.kind == "script")
            })
            .context("Skript nicht gefunden")?;
        let mut docs = state
            .script_documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Skript-Arbeitskopien nicht verfügbar"))?;
        if let Some(doc) = docs.values().find(|d| d.team == team && d.entry == id) {
            let mut draft = doc
                .draft
                .lock()
                .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))?;
            if !draft.view.auto_finish {
                let launch = opener(&draft.local)?;
                draft.view.auto_finish = launch.completion.is_some();
                draft.view.editor_note = launch.note;
                *doc.editor
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Editor nicht verfügbar"))? = launch.completion;
            }
            return Ok(draft.view.clone());
        }
        ensure!(
            scripts::fingerprint(entry) == expected,
            "Skript wurde inzwischen geändert; bitte erneut auswählen"
        );
        ensure!(
            docs.len() < 8,
            "Höchstens acht Skripte gleichzeitig extern bearbeiten"
        );
        let directory = tempfile::Builder::new().prefix("noryn-script-").tempdir()?;
        let local = directory.path().join("script.bash.txt");
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&local)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(entry.body.as_bytes())?;
        file.sync_all()?;
        drop(file);
        let launch = opener(&local)?;
        let view = View {
            id: uuid::Uuid::new_v4().to_string(),
            team_id: team.into(),
            name: entry.name.clone(),
            auto_finish: launch.completion.is_some(),
            editor_note: launch.note,
        };
        docs.insert(
            view.id.clone(),
            Arc::new(Document {
                team: team.into(),
                entry: id.into(),
                control: Control::default(),
                editor: Mutex::new(launch.completion),
                draft: Mutex::new(Draft {
                    view: view.clone(),
                    local,
                    baseline: text_files::digest(entry.body.as_bytes()),
                    _directory: directory,
                }),
            }),
        );
        Ok(view)
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finished {
    view: VaultView,
    changed: bool,
}
fn finish(state: &AppState, id: &str, require_manual: bool) -> Result<Finished> {
    vault_action(state, |vault| {
        let doc = document(state, id)?;
        let draft = doc
            .draft
            .lock()
            .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))?;
        ensure!(
            !require_manual || !draft.view.auto_finish,
            "Bitte zuerst das externe Dokument bzw. Editorfenster schließen"
        );
        let bytes = text_files::read_local(&draft.local)?;
        ensure!(
            bytes.len() <= scripts::MAX_BODY,
            "Bash-Skript höchstens 512 KiB"
        );
        let body = std::str::from_utf8(&bytes)?.replace("\r\n", "\n");
        let changed = text_files::digest(body.as_bytes()) != draft.baseline;
        if changed {
            let mut current = vault
                .data
                .script_libraries
                .get(&doc.team)
                .and_then(|c| {
                    c.library
                        .entries
                        .iter()
                        .find(|e| e.id == doc.entry && e.kind == "script")
                })
                .cloned()
                .context("Skript wurde inzwischen gelöscht; Arbeitskopie bleibt erhalten")?;
            ensure!(text_files::digest(current.body.as_bytes())==draft.baseline,"Skriptinhalt wurde inzwischen geändert. Arbeitskopie bleibt erhalten; kopiere deinen Entwurf im Editor und speichere ihn als neues Skript");
            let expected = scripts::fingerprint(&current);
            current.body = body;
            doc.control.check()?;
            vault.change(|data| scripts::save(data, &doc.team, current, Some(&expected)))?;
        }
        doc.control.cancel();
        state
            .script_documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Skript-Arbeitskopien nicht verfügbar"))?
            .remove(id);
        Ok(Finished {
            view: vault.data.view(),
            changed,
        })
    })
}
#[tauri::command]
pub async fn script_edit_open(
    state: State<'_, AppState>,
    team_id: String,
    entry_id: String,
    expected: String,
) -> Result<View> {
    let state = state.inner().clone();
    task(move || {
        open_copy(
            &state,
            &team_id,
            &entry_id,
            &expected,
            external_editor::launch,
        )
    })
    .await
}
#[tauri::command]
pub fn script_edit_list(state: State<'_, AppState>) -> Result<Vec<View>> {
    vault_action(&state, |_| {
        let docs = state
            .script_documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Skript-Arbeitskopien nicht verfügbar"))?;
        docs.values()
            .map(|d| {
                d.draft
                    .lock()
                    .map(|d| d.view.clone())
                    .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))
            })
            .collect()
    })
}
#[tauri::command]
pub async fn script_edit_finish(
    state: State<'_, AppState>,
    document_id: String,
) -> Result<Finished> {
    let state = state.inner().clone();
    task(move || finish(&state, &document_id, true)).await
}
#[tauri::command]
pub fn script_edit_discard(state: State<'_, AppState>, document_id: String) -> Result<()> {
    vault_action(&state, |_| {
        let doc = document(&state, &document_id)?;
        doc.control.cancel();
        state
            .script_documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Skript-Arbeitskopien nicht verfügbar"))?
            .remove(&document_id);
        Ok(())
    })
}
#[tauri::command]
pub async fn script_edit_wait(state: State<'_, AppState>, document_id: String) -> Result<Finished> {
    complete(state.inner().clone(), document_id).await
}
async fn complete(state: AppState, id: String) -> Result<Finished> {
    let epoch = state.vault_epoch.load(Ordering::Acquire);
    let doc = document(&state, &id).map_err(|e| format!("{e:#}"))?;
    let mut receiver = doc
        .editor
        .lock()
        .map_err(|_| "Editor nicht verfügbar")?
        .take()
        .ok_or("Editor-Abschluss wird bereits überwacht oder ist nicht verfügbar")?;
    let result=async {
        let mut tick=tokio::time::interval(std::time::Duration::from_millis(250));
        loop { tokio::select! { result=&mut receiver=>{doc.control.check()?;return result.context("Editor-Abschluss nicht verfügbar")?.map_err(anyhow::Error::msg);},_=tick.tick()=>{doc.control.check()?;ensure!(state.vault_epoch.load(Ordering::Acquire)==epoch,"Tresor wurde gesperrt oder ersetzt");} } }
    }.await;
    let result = match result {
        Ok(()) => {
            task({
                let state = state.clone();
                move || finish(&state, &id, false)
            })
            .await
        }
        Err(e) => Err(format!("{e:#}")),
    };
    if result.is_err() {
        if let Ok(mut draft) = doc.draft.lock() {
            draft.view.auto_finish = false;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use noryn_core::{model::Team, scripts::Entry, vault::Vault};
    fn state(path: &Path) -> (AppState, String) {
        let state = AppState::default();
        let mut vault = Vault::open(&path.join("vault.json"), "test-vault-password").unwrap();
        vault.data.teams.push(Team {
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
        });
        let id = uuid::Uuid::new_v4().to_string();
        vault
            .change(|data| {
                scripts::save(
                    data,
                    "ops",
                    Entry {
                        id: id.clone(),
                        parent_id: None,
                        kind: "script".into(),
                        name: "Report".into(),
                        description: "Docs".into(),
                        body: "echo original\n".into(),
                        updated_at: 0,
                        updated_by: String::new(),
                    },
                    None,
                )
            })
            .unwrap();
        *state.vault.lock().unwrap() = Some(vault);
        (state, id)
    }
    fn info(state: &AppState, id: &str) -> scripts::EntryInfo {
        let mut view = state.vault.lock().unwrap().as_ref().unwrap().data.view();
        view.script_libraries
            .remove("ops")
            .unwrap()
            .entries
            .into_iter()
            .find(|e| e.id == id)
            .unwrap()
    }
    #[tokio::test]
    async fn external_script_editor_finishes_on_close_and_preserves_conflicts_and_failed_editor_copies(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (state, id) = state(dir.path());
        for scenario in [
            "changed",
            "unchanged",
            "metadata",
            "conflict",
            "failed",
            "locked",
        ] {
            let original = state
                .vault
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .data
                .script_libraries["ops"]
                .library
                .entries[0]
                .body
                .clone();
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let mut receiver = Some(receiver);
            let view = open_copy(&state, "ops", &id, &info(&state, &id).fingerprint, |path| {
                assert_eq!(path.file_name().unwrap(), "script.bash.txt");
                assert_eq!(std::fs::read_to_string(path).unwrap(), original);
                Ok(Launch {
                    completion: receiver.take(),
                    note: "test waiter".into(),
                })
            })
            .unwrap();
            let doc = document(&state, &view.id).unwrap();
            let local = doc.draft.lock().unwrap().local.clone();
            assert!(ensure_closed(&state, Some("ops")).is_err());
            assert!(ensure_closed(&state, Some("other")).is_ok());
            assert_eq!(
                open_copy(&state, "ops", &id, "stale", |_| panic!("no second editor"))
                    .unwrap()
                    .id,
                view.id
            );
            if scenario != "unchanged" {
                std::fs::write(&local, format!("echo {scenario}\r\n")).unwrap();
            }
            if ["conflict", "metadata"].contains(&scenario) {
                vault_action(&state, |vault| {
                    let mut current = vault.data.script_libraries["ops"].library.entries[0].clone();
                    let expected = scripts::fingerprint(&current);
                    if scenario == "conflict" {
                        current.body = "echo concurrent\n".into();
                    } else {
                        current.name = "Renamed meanwhile".into();
                    }
                    vault.change(|d| scripts::save(d, "ops", current, Some(&expected)))
                })
                .unwrap();
            }
            if scenario == "locked" {
                clear(&state).unwrap();
                state.vault_epoch.fetch_add(1, Ordering::Release);
            }
            let pending = complete(state.clone(), view.id.clone());
            sender
                .send(if scenario == "failed" {
                    Err("Editor failed".into())
                } else {
                    Ok(())
                })
                .unwrap();
            let result = pending.await;
            if ["changed", "unchanged", "metadata"].contains(&scenario) {
                let result = result.unwrap();
                assert_eq!(result.changed, scenario != "unchanged");
                let current = state
                    .vault
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .data
                    .script_libraries["ops"]
                    .library
                    .entries[0]
                    .clone();
                assert_eq!(
                    current.body,
                    if scenario == "unchanged" {
                        original
                    } else {
                        format!("echo {scenario}\n")
                    }
                );
                if scenario == "metadata" {
                    assert_eq!(current.name, "Renamed meanwhile");
                }
                drop(doc);
                assert!(!local.exists());
            } else {
                assert!(result.is_err());
                if scenario != "locked" {
                    assert!(local.exists());
                    assert!(!doc.draft.lock().unwrap().view.auto_finish);
                    clear(&state).unwrap();
                }
                drop(doc);
                assert!(!local.exists());
            }
        }
    }
    #[tokio::test]
    async fn locking_cancels_editor_waiters_and_invalid_utf8_and_stale_previews_never_overwrite_scripts(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (state, id) = state(dir.path());
        let mut cache_path = PathBuf::new();
        assert!(open_copy(&state, "ops", &id, "stale", |_| panic!("stale")).is_err());
        assert!(
            open_copy(&state, "ops", &id, &info(&state, &id).fingerprint, |p| {
                cache_path = p.into();
                anyhow::bail!("no editor")
            })
            .is_err()
        );
        assert!(!cache_path.exists());
        let view = open_copy(&state, "ops", &id, &info(&state, &id).fingerprint, |_| {
            Ok(Launch::manual())
        })
        .unwrap();
        let doc = document(&state, &view.id).unwrap();
        let local = doc.draft.lock().unwrap().local.clone();
        std::fs::write(&local, [0xff, 0xfe]).unwrap();
        assert!(finish(&state, &view.id, true).is_err());
        assert!(local.exists());
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
            "echo original\n"
        );
        clear(&state).unwrap();
        drop(doc);
        assert!(!local.exists());
        let (_sender, receiver) = tokio::sync::oneshot::channel();
        let mut receiver = Some(receiver);
        let view = open_copy(&state, "ops", &id, &info(&state, &id).fingerprint, |_| {
            Ok(Launch {
                completion: receiver.take(),
                note: String::new(),
            })
        })
        .unwrap();
        let doc = document(&state, &view.id).unwrap();
        let local = doc.draft.lock().unwrap().local.clone();
        drop(doc);
        let waiter = tokio::spawn(complete(state.clone(), view.id));
        tokio::task::yield_now().await;
        clear(&state).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(!local.exists());
    }
}
