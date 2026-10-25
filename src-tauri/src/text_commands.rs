//! Explicit, bounded local working copies. The webview receives metadata, never file bytes.
use super::{
    external_editor::{self, Launch},
    remote_for, task, AppState, Result,
};
use anyhow::{ensure, Context};
use noryn_core::{
    text_files::{self, DigestBytes},
    transfer::Control,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
};
use tauri::State;

pub type Documents = Arc<Mutex<HashMap<String, Arc<Document>>>>;
pub struct Document {
    session_id: String,
    control: Control,
    inner: Mutex<Draft>,
    editor: Mutex<Option<tokio::sync::oneshot::Receiver<std::result::Result<(), String>>>>,
}
struct Draft {
    view: View,
    original: PathBuf,
    local: PathBuf,
    baseline: DigestBytes,
    opening: bool,
    _directory: tempfile::TempDir,
}
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Local,
    Remote,
}
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct View {
    id: String,
    name: String,
    path: String,
    remote: bool,
    text_copy: bool,
    auto_finish: bool,
    editor_note: String,
}
fn active(state: &AppState, session: &str, epoch: u64) -> anyhow::Result<()> {
    ensure!(
        state.vault_epoch.load(Ordering::Acquire) == epoch
            && state
                .vault
                .lock()
                .map_err(|_| anyhow::anyhow!("Tresor nicht verfügbar"))?
                .is_some(),
        "Tresor ist gesperrt oder wurde ersetzt"
    );
    remote_for(state, session).map_err(anyhow::Error::msg)?;
    Ok(())
}
fn document_for(state: &AppState, session: &str, id: &str) -> anyhow::Result<Arc<Document>> {
    remote_for(state, session).map_err(anyhow::Error::msg)?;
    let doc = state
        .documents
        .lock()
        .map_err(|_| anyhow::anyhow!("Arbeitskopien nicht verfügbar"))?
        .get(id)
        .cloned()
        .context("Arbeitskopie wurde bereits geschlossen")?;
    doc.control.check()?;
    ensure!(
        doc.session_id == session,
        "Arbeitskopie gehört zu einer anderen Sitzung"
    );
    Ok(doc)
}
pub fn clear(state: &AppState, session: Option<&str>) -> Result<()> {
    let mut docs = state
        .documents
        .lock()
        .map_err(|_| "Arbeitskopien nicht verfügbar")?;
    docs.retain(|_, doc| {
        let remove = session.is_none_or(|session| doc.session_id == session);
        if remove {
            doc.control.cancel();
        }
        !remove
    });
    Ok(())
}
fn launch_copy(
    state: &AppState,
    session: &str,
    epoch: u64,
    doc: &Arc<Document>,
    created: bool,
    mut opener: impl FnMut(&Path, &Control) -> anyhow::Result<Launch>,
) -> anyhow::Result<View> {
    let local = doc
        .inner
        .lock()
        .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))?
        .local
        .clone();
    let result = (|| {
        doc.control.check()?;
        let launch = opener(&local, &doc.control)?;
        active(state, session, epoch)?;
        let mut draft = doc
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))?;
        doc.control.check()?;
        draft.view.auto_finish = launch.completion.is_some();
        draft.view.editor_note = launch.note;
        *doc.editor
            .lock()
            .map_err(|_| anyhow::anyhow!("Editor nicht verfügbar"))? = launch.completion;
        Ok(draft.view.clone())
    })();
    if let Ok(mut draft) = doc.inner.lock() {
        draft.opening = false;
    }
    if result.is_err() && created {
        doc.control.cancel();
        state
            .documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Arbeitskopien nicht verfügbar"))?
            .retain(|_, other| !Arc::ptr_eq(other, doc));
    }
    result
}
fn open_copy(
    state: &AppState,
    session: &str,
    side: Side,
    path: String,
    opener: impl FnMut(&Path, &Control) -> anyhow::Result<Launch>,
) -> anyhow::Result<View> {
    let epoch = state.vault_epoch.load(Ordering::Acquire);
    active(state, session, epoch)?;
    let remote = matches!(side, Side::Remote);
    let docs: Vec<_> = state
        .documents
        .lock()
        .map_err(|_| anyhow::anyhow!("Arbeitskopien nicht verfügbar"))?
        .values()
        .cloned()
        .collect();
    for doc in &docs {
        let mut draft = doc
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))?;
        if doc.session_id == session && draft.view.remote == remote && draft.view.path == path {
            doc.control.check()?;
            anyhow::ensure!(!draft.opening, "Datei wird bereits geöffnet");
            if draft.view.auto_finish {
                return Ok(draft.view.clone());
            }
            draft.opening = true;
            drop(draft);
            return launch_copy(state, session, epoch, doc, false, opener);
        }
    }
    anyhow::ensure!(
        docs.len() < 8,
        "Höchstens acht externe Arbeitskopien gleichzeitig öffnen"
    );
    let original = PathBuf::from(&path);
    let name = if remote {
        path.rsplit('/').next()
    } else {
        original.file_name().and_then(|s| s.to_str())
    }
    .context("Ungültiger Dateiname")?
    .to_string();
    let working_name = text_files::working_name(&name)?;
    let bytes = if remote {
        remote_for(state, session)
            .map_err(anyhow::Error::msg)?
            .lock()
            .map_err(|_| anyhow::anyhow!("Dateizugriff nicht verfügbar"))?
            .read_text(&path)?
    } else {
        ensure!(
            original.is_absolute(),
            "Lokaler Dateipfad muss absolut sein"
        );
        text_files::read_local(&original)?
    };
    let directory = external_editor::working_directory("noryn-text-")?;
    let local = directory.path().join(&working_name);
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&local)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    let view = View {
        id: uuid::Uuid::new_v4().to_string(),
        text_copy: working_name != name,
        name,
        path,
        remote,
        auto_finish: false,
        editor_note: String::new(),
    };
    let doc = Arc::new(Document {
        session_id: session.into(),
        control: Control::default(),
        editor: Mutex::new(None),
        inner: Mutex::new(Draft {
            view: view.clone(),
            original,
            local,
            baseline: text_files::digest(&bytes),
            opening: true,
            _directory: directory,
        }),
    });

    {
        let vault = state
            .vault
            .lock()
            .map_err(|_| anyhow::anyhow!("Tresor nicht verfügbar"))?;
        ensure!(
            vault.is_some() && state.vault_epoch.load(Ordering::Acquire) == epoch,
            "Tresor wurde während des Öffnens gesperrt"
        );
        let connections = state
            .connections
            .lock()
            .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?;
        ensure!(
            connections.contains_key(session),
            "Sitzung wurde während des Öffnens geschlossen"
        );
        let mut docs = state
            .documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Arbeitskopien nicht verfügbar"))?;
        ensure!(
            docs.len() < 8,
            "Höchstens acht externe Arbeitskopien gleichzeitig öffnen"
        );
        // Register cancellation before handing the file to a potentially interactive portal.
        docs.insert(view.id.clone(), doc.clone());
    }
    launch_copy(state, session, epoch, &doc, true, opener)
}
#[tauri::command]
pub async fn text_open(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    side: Side,
    path: String,
) -> Result<View> {
    let rules = crate::file_apps::load(&app)?;
    let state = state.inner().clone();
    task(move || {
        open_copy(&state, &id, side, path.clone(), |working, control| {
            crate::file_apps::launch(working, &path, &rules, control)
        })
        .map_err(|e| format!("{e:#}"))
    })
    .await
}
#[tauri::command]
pub async fn text_reopen(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    document_id: String,
) -> Result<View> {
    let rules = crate::file_apps::load(&app)?;
    let state = state.inner().clone();
    task(move || {
        let doc = document_for(&state, &id, &document_id).map_err(|e| format!("{e:#}"))?;
        let draft = doc
            .inner
            .lock()
            .map_err(|_| "Arbeitskopie nicht verfügbar")?;
        doc.control.check().map_err(|e| format!("{e:#}"))?;
        let side = if draft.view.remote {
            Side::Remote
        } else {
            Side::Local
        };
        let path = draft.view.path.clone();
        drop(draft);
        open_copy(&state, &id, side, path.clone(), |working, control| {
            crate::file_apps::launch(working, &path, &rules, control)
        })
        .map_err(|e| format!("{e:#}"))
    })
    .await
}
fn save_copy(state: &AppState, session: &str, id: &str) -> anyhow::Result<bool> {
    let epoch = state.vault_epoch.load(Ordering::Acquire);
    active(state, session, epoch)?;
    let doc = document_for(state, session, id)?;
    let mut draft = doc
        .inner
        .lock()
        .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))?;
    ensure!(
        !draft.opening,
        "Bitte zuerst das Öffnen im Standardprogramm abschließen"
    );
    let bytes = text_files::read_local(&draft.local)?;
    let digest = text_files::digest(&bytes);
    doc.control.check()?;
    if digest == draft.baseline {
        return Ok(false);
    }
    if draft.view.remote {
        let remote = remote_for(state, session).map_err(anyhow::Error::msg)?;
        let mut remote = remote
            .lock()
            .map_err(|_| anyhow::anyhow!("Dateizugriff nicht verfügbar"))?;
        doc.control.check()?;
        remote.replace_text(&draft.view.path, &draft.baseline, &bytes, &doc.control)?;
    } else {
        doc.control.check()?;
        text_files::replace_local(&draft.original, &draft.baseline, &bytes)?;
    }
    draft.baseline = digest;
    Ok(true)
}
#[tauri::command]
pub async fn text_save(
    state: State<'_, AppState>,
    id: String,
    document_id: String,
) -> Result<bool> {
    let state = state.inner().clone();
    task(move || save_copy(&state, &id, &document_id).map_err(|e| format!("{e:#}"))).await
}
#[tauri::command]
pub fn text_close(state: State<'_, AppState>, id: String, document_id: String) -> Result<()> {
    let doc = document_for(&state, &id, &document_id).map_err(|e| format!("{e:#}"))?;
    doc.control.cancel();
    state
        .documents
        .lock()
        .map_err(|_| "Arbeitskopien nicht verfügbar")?
        .remove(&document_id);
    Ok(())
}

fn finish_copy(state: &AppState, session: &str, id: &str) -> anyhow::Result<bool> {
    let changed = save_copy(state, session, id)?;
    // Never remove the only edited copy if conflict checking or publication fails.
    let doc = document_for(state, session, id)?;
    doc.control.cancel();
    state
        .documents
        .lock()
        .map_err(|_| anyhow::anyhow!("Arbeitskopien nicht verfügbar"))?
        .remove(id);
    Ok(changed)
}

#[tauri::command]
pub async fn text_finish(
    state: State<'_, AppState>,
    id: String,
    document_id: String,
) -> Result<bool> {
    let state = state.inner().clone();
    task(move || {
        let doc = document_for(&state, &id, &document_id).map_err(|e| format!("{e:#}"))?;
        if doc
            .inner
            .lock()
            .map_err(|_| "Arbeitskopie nicht verfügbar")?
            .view
            .auto_finish
        {
            return Err("Bitte zuerst das externe Dokument bzw. Editorfenster schließen".into());
        }
        finish_copy(&state, &id, &document_id).map_err(|e| format!("{e:#}"))
    })
    .await
}

type EditorExit = tokio::sync::oneshot::Receiver<std::result::Result<(), String>>;
async fn wait_editor(doc: &Document, mut receiver: EditorExit) -> anyhow::Result<()> {
    let mut cancellation = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        tokio::select! {
            result = &mut receiver => {
                doc.control.check()?;
                return result.context("Editor-Abschluss konnte nicht gelesen werden")?.map_err(anyhow::Error::msg);
            },
            _ = cancellation.tick() => doc.control.check()?,
        }
    }
}

#[tauri::command]
pub async fn text_wait(
    state: State<'_, AppState>,
    id: String,
    document_id: String,
) -> Result<bool> {
    complete_on_exit(state.inner().clone(), id, document_id).await
}

async fn complete_on_exit(state: AppState, id: String, document_id: String) -> Result<bool> {
    let doc = document_for(&state, &id, &document_id).map_err(|e| format!("{e:#}"))?;
    let receiver = doc
        .editor
        .lock()
        .map_err(|_| "Editor nicht verfügbar")?
        .take()
        .ok_or("Editor-Abschluss wird bereits überwacht oder ist nicht verfügbar")?;
    let result = wait_editor(&doc, receiver).await;
    if let Err(error) = result {
        if let Ok(mut draft) = doc.inner.lock() {
            draft.view.auto_finish = false;
        }
        return Err(format!("{error:#}"));
    }
    task(move || {
        let result = finish_copy(&state, &id, &document_id).map_err(|e| format!("{e:#}"));
        if result.is_err() {
            if let Ok(mut draft) = doc.inner.lock() {
                draft.view.auto_finish = false;
            }
        }
        result
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Connection;
    use noryn_core::{remote::Remote, ssh, vault::Vault};
    fn state(directory: &Path) -> AppState {
        let state = AppState::default();
        *state.vault.lock().unwrap() =
            Some(Vault::open(&directory.join("vault.json"), "test-only-password").unwrap());
        for session in ["a", "b"] {
            state.connections.lock().unwrap().insert(
                session.into(),
                Connection {
                    health: None,
                    quick: None,
                    terminal: None,
                    tools: None,
                    remote: Arc::new(Mutex::new(Remote::Ssh(ssh::Remote::new(
                        ssh2::Session::new().unwrap(),
                    )))),
                },
            );
        }
        state
    }
    #[tokio::test]
    async fn editor_close_publishes_saved_bytes_and_removes_the_cache_but_conflicts_keep_it() {
        let directory = tempfile::tempdir().unwrap();
        let state = state(directory.path());
        for scenario in [
            "changed",
            "unchanged",
            "conflict",
            "failed-editor",
            "locked",
        ] {
            let original = directory.path().join(format!("{scenario}.txt"));
            std::fs::write(&original, b"original").unwrap();
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let mut receiver = Some(receiver);
            let view = open_copy(
                &state,
                "a",
                Side::Local,
                original.to_str().unwrap().into(),
                |_, _| {
                    Ok(Launch {
                        completion: receiver.take(),
                        note: "test waiter".into(),
                    })
                },
            )
            .unwrap();
            assert!(view.auto_finish);
            let duplicate = open_copy(&state, "a", Side::Local, view.path.clone(), |_, _| {
                panic!("must not launch a second editor while waiting")
            })
            .unwrap();
            assert_eq!(duplicate.id, view.id);
            let doc = document_for(&state, "a", &view.id).unwrap();
            let local = doc.inner.lock().unwrap().local.clone();
            if scenario != "unchanged" {
                std::fs::write(&local, b"edited").unwrap();
            }
            assert_eq!(std::fs::read(&original).unwrap(), b"original");
            if scenario == "conflict" {
                std::fs::write(&original, b"concurrent").unwrap();
            }
            if scenario == "locked" {
                clear(&state, None).unwrap();
            }
            let pending = complete_on_exit(state.clone(), "a".into(), view.id.clone());
            if scenario == "failed-editor" {
                sender.send(Err("Editor failed".into())).unwrap();
            } else {
                sender.send(Ok(())).unwrap();
            }
            let result = pending.await;
            match scenario {
                "changed" | "unchanged" => {
                    assert_eq!(result.unwrap(), scenario == "changed");
                    assert_eq!(
                        std::fs::read(&original).unwrap(),
                        if scenario == "changed" {
                            b"edited".as_slice()
                        } else {
                            b"original".as_slice()
                        }
                    );
                    drop(doc);
                    assert!(!local.exists());
                }
                "conflict" | "failed-editor" => {
                    assert!(result.is_err());
                    assert!(local.exists());
                    assert_eq!(std::fs::read(&local).unwrap(), b"edited");
                    assert!(!doc.inner.lock().unwrap().view.auto_finish);
                    assert_eq!(
                        std::fs::read(&original).unwrap(),
                        if scenario == "conflict" {
                            b"concurrent".as_slice()
                        } else {
                            b"original".as_slice()
                        }
                    );
                    clear(&state, None).unwrap();
                    drop(doc);
                    assert!(!local.exists());
                }
                _ => {
                    assert!(result.is_err());
                    drop(doc);
                    assert!(!local.exists());
                    assert_eq!(std::fs::read(&original).unwrap(), b"original");
                }
            }
        }
    }

    #[tokio::test]
    async fn cancelling_a_waiter_releases_plaintext_without_waiting_for_the_editor() {
        let directory = tempfile::tempdir().unwrap();
        let state = state(directory.path());
        let original = directory.path().join("cancel.txt");
        std::fs::write(&original, b"original").unwrap();
        let (_sender, receiver) = tokio::sync::oneshot::channel();
        let mut receiver = Some(receiver);
        let view = open_copy(
            &state,
            "a",
            Side::Local,
            original.to_str().unwrap().into(),
            |_, _| {
                Ok(Launch {
                    completion: receiver.take(),
                    note: String::new(),
                })
            },
        )
        .unwrap();
        let doc = document_for(&state, "a", &view.id).unwrap();
        let local = doc.inner.lock().unwrap().local.clone();
        drop(doc);
        let pending = tokio::spawn(complete_on_exit(state.clone(), "a".into(), view.id));
        tokio::task::yield_now().await;
        clear(&state, None).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), pending)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(!local.exists());
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
    }
    #[test]
    fn working_copies_open_without_mutation_save_explicitly_reject_conflicts_and_clean_up() {
        let directory = tempfile::tempdir().unwrap();
        let state = state(directory.path());
        let original = directory.path().join("script.sh");
        std::fs::write(&original, b"echo initial\n").unwrap();
        let view = open_copy(
            &state,
            "a",
            Side::Local,
            original.to_str().unwrap().into(),
            |path, _| {
                assert_eq!(path.file_name().unwrap(), "script.sh.txt");
                assert_eq!(std::fs::read(path).unwrap(), b"echo initial\n");
                Ok(Launch::manual())
            },
        )
        .unwrap();
        assert!(view.text_copy);
        let duplicate = open_copy(&state, "a", Side::Local, view.path.clone(), |_, _| {
            Ok(Launch::manual())
        })
        .unwrap();
        assert_eq!(view.id, duplicate.id);
        assert!(document_for(&state, "b", &view.id).is_err());
        let doc = document_for(&state, "a", &view.id).unwrap();
        let local = doc.inner.lock().unwrap().local.clone();
        assert_ne!(local, original);
        assert!(!save_copy(&state, "a", &view.id).unwrap());
        std::fs::write(&local, b"echo edited\n").unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"echo initial\n");
        assert!(save_copy(&state, "a", &view.id).unwrap());
        assert_eq!(std::fs::read(&original).unwrap(), b"echo edited\n");
        std::fs::write(&original, b"concurrent change").unwrap();
        std::fs::write(&local, b"editor change").unwrap();
        assert!(
            format!("{:#}", save_copy(&state, "a", &view.id).unwrap_err()).contains("inzwischen")
        );
        assert_eq!(std::fs::read(&original).unwrap(), b"concurrent change");
        assert_eq!(std::fs::read(&local).unwrap(), b"editor change");
        clear(&state, Some("b")).unwrap();
        assert!(!doc.control.is_cancelled());
        clear(&state, Some("a")).unwrap();
        assert!(doc.control.is_cancelled());
        assert!(save_copy(&state, "a", &view.id).is_err());
        drop(doc);
        assert!(!local.exists());
    }
    #[test]
    fn cache_limits_lock_epoch_and_failed_openers_do_not_leave_plaintext_copies() {
        let directory = tempfile::tempdir().unwrap();
        let state = state(directory.path());
        let mut cache_path = PathBuf::new();
        for i in 0..8 {
            let file = directory.path().join(format!("file{i}.txt"));
            std::fs::write(&file, b"test").unwrap();
            open_copy(
                &state,
                "a",
                Side::Local,
                file.to_str().unwrap().into(),
                |_, _| Ok(Launch::manual()),
            )
            .unwrap();
        }
        let file = directory.path().join("last.txt");
        std::fs::write(&file, b"test").unwrap();
        assert!(open_copy(
            &state,
            "a",
            Side::Local,
            file.to_str().unwrap().into(),
            |_, _| panic!("limit must prevent opening")
        )
        .is_err());
        let doc = state
            .documents
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        let local = doc.inner.lock().unwrap().local.clone();
        clear(&state, None).unwrap();
        assert!(doc.control.is_cancelled());
        drop(doc);
        assert!(!local.exists());
        let failure = open_copy(
            &state,
            "a",
            Side::Local,
            file.to_str().unwrap().into(),
            |path, _| {
                cache_path.clone_from(&path.to_path_buf());
                anyhow::bail!("No default app")
            },
        );
        assert!(failure.is_err());
        assert!(!cache_path.exists());
        assert!(state.documents.lock().unwrap().is_empty());
        state.vault.lock().unwrap().take();
        state.vault_epoch.fetch_add(1, Ordering::Release);
        assert!(open_copy(
            &state,
            "a",
            Side::Local,
            file.to_str().unwrap().into(),
            |_, _| panic!("locked")
        )
        .is_err());
    }
    #[test]
    fn opening_a_portal_keeps_the_vault_available_and_lock_cancels_publication() {
        let directory = tempfile::tempdir().unwrap();
        let state = state(directory.path());
        let original = directory.path().join("portal.txt");
        std::fs::write(&original, b"test").unwrap();
        let mut local = PathBuf::new();
        let result = open_copy(
            &state,
            "a",
            Side::Local,
            original.to_str().unwrap().into(),
            |path, control| {
                local = path.into();
                assert!(state.vault.try_lock().is_ok());
                assert!(state.connections.try_lock().is_ok());
                clear(&state, None).unwrap();
                state.vault.lock().unwrap().take();
                state.vault_epoch.fetch_add(1, Ordering::Release);
                assert!(control.is_cancelled());
                Ok(Launch::manual())
            },
        );
        assert!(result.is_err());
        assert!(!local.exists());
        assert!(state.documents.lock().unwrap().is_empty());
    }
}
