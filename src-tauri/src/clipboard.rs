//! Plain-text clipboard access only, on explicit copy/paste actions.
const MAX_TEXT: usize = 8 * 1024 * 1024;
fn bounded(text: String) -> crate::Result<String> {
    if text.len() > MAX_TEXT {
        return Err("Zwischenablage ist größer als 8 MiB".into());
    }
    Ok(text)
}
#[tauri::command]
pub async fn clipboard_write_text(app: tauri::AppHandle, text: String) -> crate::Result<()> {
    let text = bounded(text)?;
    #[cfg(target_os = "linux")]
    {
        let (send, receive) = tokio::sync::oneshot::channel();
        // Use the same GTK display/Wayland data-device as the WebView. No
        // privileged compositor data-control protocol or external utility needed.
        app.run_on_main_thread(move || {
            let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
            clipboard.set_text(&text);
            let _ = send.send(());
        })
        .map_err(|e| e.to_string())?;
        tokio::time::timeout(std::time::Duration::from_secs(10), receive)
            .await
            .map_err(|_| "Zwischenablage antwortet nicht")?
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        crate::task(move || app.clipboard().write_text(text).map_err(|e| e.to_string())).await
    }
}
#[tauri::command]
pub async fn clipboard_read_text(app: tauri::AppHandle) -> crate::Result<String> {
    #[cfg(target_os = "linux")]
    {
        let (send, receive) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).request_text(move |_, text| {
                let _ = send.send(bounded(text.unwrap_or_default().into()));
            });
        })
        .map_err(|e| e.to_string())?;
        tokio::time::timeout(std::time::Duration::from_secs(10), receive)
            .await
            .map_err(|_| "Zwischenablage antwortet nicht")?
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        bounded(crate::task(move || app.clipboard().read_text().map_err(|e| e.to_string())).await?)
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn clipboard_text_is_bounded_without_changing_unicode_or_line_breaks() {
        let text = "ä\nλ\r\n";
        assert_eq!(super::bounded(text.into()).unwrap(), text);
        assert!(super::bounded("x".repeat(super::MAX_TEXT + 1)).is_err());
    }
}
