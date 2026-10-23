#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn configure_linux_graphics() {
    // Run before Tauri/GTK starts any threads. Keep user-selected backends intact.
    // WebKitGTK's DMABUF renderer can fail on NVIDIA/Wayland; XWayland provides
    // a compatible window backend when an X11 display is available.
    if std::env::var_os("GDK_BACKEND").is_none()
        && std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty())
    {
        std::env::set_var("GDK_BACKEND", "x11");
    }
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    configure_linux_graphics();

    harbor_app::run()
}
