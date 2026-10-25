//! Explicit, bounded local working copies. The webview receives metadata, never file bytes.
use super::{remote_for, task, AppState, Result};
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
}
struct Draft {
    view: View,
    original: PathBuf,
    local: PathBuf,
    baseline: DigestBytes,
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
}

fn launch(path: &Path) -> anyhow::Result<()> {
    tauri_plugin_opener::open_path(path,None::<&str>).context("Kein Standardprogramm verfügbar. Bitte eine Anwendung für diesen Dateityp im Betriebssystem festlegen")
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
fn open_copy(
    state: &AppState,
    session: &str,
    side: Side,
    path: String,
    mut opener: impl FnMut(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<View> {
    let epoch = state.vault_epoch.load(Ordering::Acquire);
    active(state, session, epoch)?;
    let remote = matches!(side, Side::Remote);
    {
        let docs: Vec<_> = state
            .documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Arbeitskopien nicht verfügbar"))?
            .values()
            .cloned()
            .collect();
        for doc in &docs {
            let draft = doc
                .inner
                .lock()
                .map_err(|_| anyhow::anyhow!("Arbeitskopie nicht verfügbar"))?;
            if doc.session_id == session && draft.view.remote == remote && draft.view.path == path {
                doc.control.check()?;
                opener(&draft.local)?;
                return Ok(draft.view.clone());
            }
        }
        ensure!(
            docs.len() < 8,
            "Höchstens acht externe Arbeitskopien gleichzeitig öffnen"
        );
    }
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
    let directory = tempfile::Builder::new().prefix("noryn-text-").tempdir()?;
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
    };
    let doc = Arc::new(Document {
        session_id: session.into(),
        control: Control::default(),
        inner: Mutex::new(Draft {
            view: view.clone(),
            original,
            local,
            baseline: text_files::digest(&bytes),
            _directory: directory,
        }),
    });
    // Serialize publication/opening with vault lock and connection close. No late plaintext copy after lock.
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
    // External editors may save by replacing their file. All subsequent reads reject symlinks.
    opener(&doc.inner.lock().unwrap().local)?;
    docs.insert(view.id.clone(), doc);
    Ok(view)
}
#[tauri::command]
pub async fn text_open(
    state: State<'_, AppState>,
    id: String,
    side: Side,
    path: String,
) -> Result<View> {
    let state = state.inner().clone();
    task(move || open_copy(&state, &id, side, path, launch).map_err(|e| format!("{e:#}"))).await
}
#[tauri::command]
pub async fn text_reopen(
    state: State<'_, AppState>,
    id: String,
    document_id: String,
) -> Result<()> {
    let state = state.inner().clone();
    task(move || {
        let doc = document_for(&state, &id, &document_id).map_err(|e| format!("{e:#}"))?;
        let draft = doc
            .inner
            .lock()
            .map_err(|_| "Arbeitskopie nicht verfügbar")?;
        doc.control.check().map_err(|e| format!("{e:#}"))?;
        launch(&draft.local).map_err(|e| format!("{e:#}"))
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
            |path| {
                assert_eq!(path.file_name().unwrap(), "script.sh.txt");
                assert_eq!(std::fs::read(path).unwrap(), b"echo initial\n");
                Ok(())
            },
        )
        .unwrap();
        assert!(view.text_copy);
        let duplicate = open_copy(&state, "a", Side::Local, view.path.clone(), |_| Ok(())).unwrap();
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
                |_| Ok(()),
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
            |_| panic!("limit must prevent opening")
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
            |path| {
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
            |_| panic!("locked")
        )
        .is_err());
    }
}
