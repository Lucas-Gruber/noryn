use tauri::AppHandle;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use tauri_plugin_deep_link::DeepLinkExt;

#[tauri::command]
pub fn links_register(app: AppHandle) -> crate::Result<String> {
    #[cfg(target_os = "linux")]
    if std::path::Path::new("/.flatpak-info").exists() {
        return Ok("Die Flatpak-App bietet Verbindungslinks an. Bitte Noryn in den Standardanwendungen des Systems auswählen.".into());
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        app.deep_link().register_all().map_err(|_| "Verbindungslinks konnten nicht registriert werden. Bitte Noryn in den Standardanwendungen des Systems auswählen.".to_string())?;
        Ok("Verbindungslinks werden jetzt mit Noryn geöffnet.".into())
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = app;
        Ok("Noryn bietet Verbindungslinks über die installierte App an. Bitte Noryn im System als gewünschte Anwendung auswählen.".into())
    }
}
