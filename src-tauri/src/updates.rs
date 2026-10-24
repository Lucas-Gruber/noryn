//! Signed application updates. No vault data or GitHub credentials enter this transport.
use super::{task, AppState, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::watch;

const MAX_PACKAGE: usize = 768 * 1024 * 1024;
const RELEASES: &str = "https://github.com/Lucas-Gruber/noryn/releases/latest";

#[derive(Clone, Default)]
pub struct UpdateState {
    pending: Arc<Mutex<Option<Pending>>>,
    busy: Arc<AtomicBool>,
    pub installing: Arc<AtomicBool>,
    cancel: Arc<Mutex<Option<watch::Sender<bool>>>>,
}
struct Pending {
    update: Update,
    bytes: Option<Vec<u8>>,
}
struct Busy(Arc<AtomicBool>);
impl Busy {
    fn enter(flag: &Arc<AtomicBool>) -> Result<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Es läuft bereits eine Update-Aktion")?;
        Ok(Self(flag.clone()))
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    automatic: bool,
    can_install: bool,
    install_hint: &'static str,
}
#[tauri::command]
pub fn update_info(app: tauri::AppHandle) -> UpdateInfo {
    use tauri::utils::{config::BundleType, platform::bundle_type};
    // The bundler also patches the EXE later copied as the portable download. A real
    // NSIS installation additionally has its uninstaller beside the executable.
    let installed_nsis = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("uninstall.exe")))
        .is_some_and(|p| p.is_file());
    let (can_install, install_hint) = if cfg!(debug_assertions) {
        (
            false,
            "Entwicklungsmodus: Updates werden hier nicht installiert.",
        )
    } else {
        match bundle_type() {
            Some(BundleType::AppImage | BundleType::App) => (true, "Das Update ersetzt die Anwendung. Dein Tresor bleibt erhalten."),
            Some(BundleType::Nsis) if installed_nsis => (true, "Das Update ersetzt die installierte Anwendung. Dein Tresor bleibt erhalten."),
            Some(BundleType::Deb | BundleType::Rpm) => (false, "Diese Version wird über ein Systempaket verwaltet. Lade das passende Paket aus dem Release herunter und installiere es über deinen Paketmanager."),
            _ => (false, "Portable Windows-EXE oder unpaketierte Anwendung: Lade die neue portable Datei aus dem Release herunter und ersetze sie bei geschlossener App. Alternativ kannst du den Installer verwenden."),
        }
    };
    UpdateInfo {
        version: app.package_info().version.to_string(),
        automatic: !cfg!(debug_assertions),
        can_install,
        install_hint,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    version: String,
    notes: String,
}

#[tauri::command]
pub async fn update_check(
    app: tauri::AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<Offer>> {
    let _busy = Busy::enter(&state.busy)?;
    let update = app.updater_builder()
        .timeout(Duration::from_secs(20))
        .configure_client(|client| client.https_only(true))
        .build().map_err(|e| e.to_string())?
        .check().await.map_err(|error| match error {
            tauri_plugin_updater::Error::ReleaseNotFound => "Noch kein öffentliches Update-Manifest verfügbar. Der erste signierte Release muss veröffentlicht und das Repository öffentlich erreichbar sein.".into(),
            _ => format!("Update-Prüfung fehlgeschlagen: {error}"),
        })?;
    if let Some(update) = &update {
        if update.download_url.scheme() != "https" {
            return Err("Update-Download muss HTTPS verwenden".into());
        }
    }
    let offer = update.as_ref().map(|u| Offer {
        version: u.version.clone(),
        notes: u
            .body
            .as_deref()
            .unwrap_or("Eine neue Noryn-Version ist verfügbar.")
            .chars()
            .take(16_384)
            .collect(),
    });
    *state.pending.lock().map_err(|_| "Update nicht verfügbar")? = update.map(|update| Pending {
        update,
        bytes: None,
    });
    Ok(offer)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

async fn download(update: &Update, output: Channel<Progress>) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(update.download_url.clone())
        .send()
        .await
        .map_err(|e| format!("Download fehlgeschlagen: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Download fehlgeschlagen: {e}"))?;
    let total = response.content_length();
    if total.is_some_and(|n| n > MAX_PACKAGE as u64) {
        return Err("Update-Paket ist zu groß".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    let mut sent = Instant::now();
    let _ = output.send(Progress {
        downloaded: 0,
        total,
    });
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download unterbrochen: {e}"))?;
        if bytes.len().saturating_add(chunk.len()) > MAX_PACKAGE {
            return Err("Update-Paket ist zu groß".into());
        }
        bytes.extend_from_slice(&chunk);
        if sent.elapsed() >= Duration::from_millis(100) {
            let _ = output.send(Progress {
                downloaded: bytes.len() as u64,
                total,
            });
            sent = Instant::now();
        }
    }
    // The official verifier checks package bytes AND the signed version before any installation.
    update.verify(&bytes).map_err(|_| {
        "Die Update-Signatur oder Versionsbindung ist ungültig. Es wurde nichts installiert."
    })?;
    let _ = output.send(Progress {
        downloaded: bytes.len() as u64,
        total,
    });
    Ok(bytes)
}

#[tauri::command]
pub async fn update_download(
    app: tauri::AppHandle,
    state: State<'_, UpdateState>,
    output: Channel<Progress>,
) -> Result<()> {
    if !update_info(app).can_install {
        return Err("Für diese Paketart bitte den Release-Download verwenden".into());
    }
    let _busy = Busy::enter(&state.busy)?;
    let update = state
        .pending
        .lock()
        .map_err(|_| "Update nicht verfügbar")?
        .as_ref()
        .ok_or("Bitte zuerst nach Updates suchen")?
        .update
        .clone();
    let (cancel, mut receiver) = watch::channel(false);
    *state.cancel.lock().map_err(|_| "Update nicht verfügbar")? = Some(cancel);
    let result = tokio::select! {
        result = download(&update, output) => result,
        _ = receiver.changed() => Err("Download abgebrochen".into()),
    };
    state
        .cancel
        .lock()
        .map_err(|_| "Update nicht verfügbar")?
        .take();
    let bytes = result?;
    if let Some(pending) = state
        .pending
        .lock()
        .map_err(|_| "Update nicht verfügbar")?
        .as_mut()
    {
        pending.bytes = Some(bytes);
    }
    Ok(())
}

#[tauri::command]
pub fn update_cancel(state: State<'_, UpdateState>) -> Result<()> {
    if let Some(cancel) = state
        .cancel
        .lock()
        .map_err(|_| "Update nicht verfügbar")?
        .as_ref()
    {
        let _ = cancel.send(true);
    }
    Ok(())
}

#[tauri::command]
pub async fn update_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    updates: State<'_, UpdateState>,
) -> Result<()> {
    if !update_info(app.clone()).can_install {
        return Err("Diese Paketart unterstützt keine direkte Installation".into());
    }
    let state = state.inner().clone();
    let updates = updates.inner().clone();
    task(move || {
        let _busy = Busy::enter(&updates.busy)?;
        let vault = state.vault.lock().map_err(|_| "Tresor nicht verfügbar")?;
        if vault.is_some() {
            return Err("Vor der Installation bitte den Tresor sperren".into());
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
                .sync_jobs
                .lock()
                .map_err(|_| "Abgleich nicht verfügbar")?
                .is_empty()
        {
            return Err(
                "Bitte laufende Verbindungen, Anmeldungen und Team-Abgleiche zuerst beenden".into(),
            );
        }
        let mut pending = updates
            .pending
            .lock()
            .map_err(|_| "Update nicht verfügbar")?;
        let pending = pending
            .as_mut()
            .ok_or("Bitte zuerst ein Update herunterladen")?;
        let bytes = pending
            .bytes
            .as_ref()
            .ok_or("Bitte zuerst das Update vollständig herunterladen")?;
        let _installing = Busy::enter(&updates.installing)?;
        pending.update.install(bytes).map_err(|error| {
            format!("Installation fehlgeschlagen: {error}. Der Tresor bleibt gesperrt.")
        })?;
        drop(vault);
        app.restart();
    })
    .await
}

#[tauri::command]
pub fn update_open_release() -> Result<()> {
    tauri_plugin_opener::open_url(RELEASES, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signed_packages_reject_tampering_wrong_versions_and_downgrades() {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            json!({
                "pubkey": include_str!("../tests/fixtures/updater/public.pub").trim(),
                "requireSignedVersion": true
            }),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let updater = app
            .updater_builder()
            .target("test-target")
            .endpoints(vec!["https://example.invalid/latest.json".parse().unwrap()])
            .unwrap()
            .build()
            .unwrap();
        let mut manifest = json!({"version":"99.0.0","platforms":{"test-target":{
            "url":"https://example.invalid/package",
            "signature":include_str!("../tests/fixtures/updater/payload.txt.sig").trim()
        }}});
        let update = updater.restore_update(manifest.clone()).unwrap().unwrap();
        let payload = include_bytes!("../tests/fixtures/updater/payload.txt");
        update.verify(payload).unwrap();
        assert!(update.verify(b"tampered package").is_err());
        manifest["version"] = json!("99.0.1");
        let wrong_version = updater.restore_update(manifest.clone()).unwrap().unwrap();
        assert!(matches!(
            wrong_version.verify(payload),
            Err(tauri_plugin_updater::Error::SignedVersionMismatch { .. })
        ));
        manifest["version"] = json!("0.0.0");
        assert!(updater.restore_update(manifest).unwrap().is_none());
    }

    #[test]
    fn update_actions_are_serialized_and_error_paths_release_the_gate() {
        let state = UpdateState::default();
        let first = Busy::enter(&state.busy).unwrap();
        assert!(Busy::enter(&state.busy).is_err());
        drop(first);
        assert!(Busy::enter(&state.busy).is_ok());
        assert!(!state.busy.load(Ordering::Acquire));
    }
}
