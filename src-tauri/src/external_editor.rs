//! Wait only for documented editor lifetimes, never for a generic opener's exit.
use anyhow::{Context, Result};
#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::oneshot;
static WAITERS: AtomicUsize = AtomicUsize::new(0);
pub struct Launch {
    pub completion: Option<oneshot::Receiver<std::result::Result<(), String>>>,
    pub note: String,
}
impl Launch {
    pub fn manual() -> Self {
        Self { completion: None, note: "Dieses Standardprogramm meldet keinen verlässlichen Dateiabschluss. Im Editor speichern und schließen, danach hier ‚Bearbeitung beenden‘ wählen.".into() }
    }
}
struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        WAITERS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn wait_args(program: &Path) -> Option<&'static [&'static str]> {
    match program.file_name()?.to_str()?.to_ascii_lowercase().as_str() {
        "kate" | "kwrite" | "kate.exe" | "kwrite.exe" => Some(&["--block", "--new"]),
        "code" | "code-insiders" | "codium" | "subl" | "sublime_text" | "sublime_text.exe" => {
            Some(&["--wait"])
        }
        "gedit" => Some(&["--standalone", "--wait"]),
        _ => None,
    }
}
#[cfg(target_os = "linux")]
fn default_program(path: &Path) -> Option<PathBuf> {
    use gio::prelude::AppInfoExt;
    // In Flatpak the host owns the association and process. The portal does not
    // expose document-close events, so retain an explicit finish action there.
    if Path::new("/.flatpak-info").exists() {
        return None;
    }
    let (mime, _) = gio::content_type_guess(Some(path), &[]);
    gio::AppInfo::default_for_type(&mime, false).map(|app| app.executable())
}
#[cfg(target_os = "windows")]
fn default_program(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    #[link(name = "shlwapi")]
    extern "system" {
        fn AssocQueryStringW(
            flags: u32,
            kind: i32,
            association: *const u16,
            extra: *const u16,
            output: *mut u16,
            length: *mut u32,
        ) -> i32;
    }
    let extension = format!(".{}", path.extension()?.to_str()?);
    let extension: Vec<u16> = std::ffi::OsStr::new(&extension)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut length = 0;
    // ASSOCSTR_EXECUTABLE; query size before writing into a bounded UTF-16 buffer.
    unsafe {
        AssocQueryStringW(
            0,
            2,
            extension.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut length,
        );
    }
    if !(2..=32768).contains(&length) {
        return None;
    }
    let mut buffer = vec![0; length as usize];
    let result = unsafe {
        AssocQueryStringW(
            0,
            2,
            extension.as_ptr(),
            std::ptr::null(),
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    if result != 0 {
        return None;
    }
    let end = buffer.iter().position(|c| *c == 0)?;
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..end])))
}
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn default_program(_: &Path) -> Option<PathBuf> {
    None
}

pub fn working_directory(prefix: &str) -> Result<tempfile::TempDir> {
    #[cfg(target_os = "linux")]
    if Path::new("/.flatpak-info").exists() {
        // Unlike sandbox-private /tmp, this private app-cache path is also usable
        // by an explicitly selected host editor. No broad host filesystem grant.
        let cache = gio::glib::user_cache_dir().join("noryn-edit");
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&cache)?;
        return Ok(tempfile::Builder::new().prefix(prefix).tempdir_in(cache)?);
    }
    Ok(tempfile::Builder::new().prefix(prefix).tempdir()?)
}

pub fn launch(path: &Path, control: &noryn_core::transfer::Control) -> Result<Launch> {
    control.check()?;
    #[cfg(target_os = "linux")]
    if Path::new("/.flatpak-info").exists() {
        return crate::file_portal::launch(path, control);
    }
    #[cfg(not(target_os = "macos"))]
    let plan = default_program(path).and_then(|program| {
        #[cfg(target_os = "linux")]
        let program = {
            // Desktop associations may name Electron itself; --wait belongs to
            // the editor's CLI, which creates and observes its wait marker.
            let name = program.file_name()?.to_str()?;
            if ["code", "code-insiders", "codium"].contains(&name) {
                let resolved = if program.is_absolute() {
                    program.clone()
                } else {
                    std::env::split_paths(&std::env::var_os("PATH")?)
                        .map(|dir| dir.join(&program))
                        .find(|candidate| candidate.is_file())?
                };
                let resolved = std::fs::canonicalize(resolved).ok()?;
                if resolved.parent()?.file_name()?.to_str()? == "bin" {
                    resolved
                } else {
                    let cli = resolved.parent()?.join("bin").join(name);
                    if !cli.is_file() {
                        return None;
                    }
                    cli
                }
            } else {
                program
            }
        };
        if let Some(args) = wait_args(&program) {
            let mut command = Command::new(&program);
            command.args(args).arg(path);
            return Some(command);
        }
        #[cfg(target_os = "windows")]
        {
            // Windows' Code.exe is Electron; invoke its actual CLI without cmd.exe
            // so spaces/metacharacters in filenames never become shell syntax.
            let name = program.file_name()?.to_str()?.to_ascii_lowercase();
            if ["code.exe", "code - insiders.exe", "vscodium.exe"].contains(&name.as_str()) {
                let cli = program.parent()?.join("resources/app/out/cli.js");
                if cli.is_file() {
                    let mut command = Command::new(&program);
                    command
                        .env("ELECTRON_RUN_AS_NODE", "1")
                        .arg(cli)
                        .arg("--wait")
                        .arg(path);
                    return Some(command);
                }
            }
        }
        None
    });
    #[cfg(target_os = "macos")]
    let plan = {
        let mut command = Command::new("/usr/bin/open");
        command.args(["-W", "-n"]).arg(path);
        Some(command)
    };
    if let Some(mut command) = plan {
        let mut launch = spawn_waiter(&mut command)?;
        if cfg!(target_os = "macos") {
            launch.note = "Im separaten Standardeditor speichern und die Anwendung beenden (⌘Q). Erst beim Beenden der App werden Änderungen übernommen und die Arbeitskopie gelöscht.".into();
        }
        return Ok(launch);
    }
    tauri_plugin_opener::open_path(path, None::<&str>).context("Kein Standardprogramm verfügbar. Bitte eine Anwendung für diesen Dateityp im Betriebssystem festlegen")?;
    Ok(Launch::manual())
}

pub fn spawn_waiter(command: &mut Command) -> Result<Launch> {
    if WAITERS.fetch_add(1, Ordering::AcqRel) >= 16 {
        WAITERS.fetch_sub(1, Ordering::AcqRel);
        anyhow::bail!(
            "Zu viele wartende Editor-Prozesse. Bitte zuvor geöffnete Editorfenster schließen"
        );
    }
    let slot = Slot;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Standardeditor konnte nicht gestartet werden")?;
    let (sender, receiver) = oneshot::channel();
    // This reaper owns only the process, never a document/path/file buffer. Lock
    // and session-close can remove cached plaintext while the editor stays open.
    std::thread::spawn(move || {
        let _slot = slot;
        let result = child.wait().map_err(|e| e.to_string()).and_then(|status| {
            if status.success() { Ok(()) } else { Err(format!("Editor wurde mit {status} beendet. Arbeitskopie zur Prüfung erhalten; kein automatischer Upload")) }
        });
        let _ = sender.send(result);
    });
    Ok(Launch { completion: Some(receiver), note: "Im Editor speichern und das Dokument bzw. das separate Editorfenster schließen. Danach werden Änderungen automatisch übernommen und die Arbeitskopie gelöscht.".into() })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_launchers_never_count_as_document_close() {
        for program in [
            "xdg-open",
            "gio",
            "notepad.exe",
            "open",
            "explorer.exe",
            "sh",
        ] {
            assert!(wait_args(Path::new(program)).is_none());
        }
        assert_eq!(wait_args(Path::new("code")), Some(&["--wait"][..]));
        assert_eq!(
            wait_args(Path::new("kate")),
            Some(&["--block", "--new"][..])
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn a_real_waiting_editor_reports_success_only_after_exit_and_reports_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("a file 'quoted';.txt");
        let gate = directory.path().join("editor-close");
        std::fs::write(&path, b"original").unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "n=0; while [ ! -f \"$2\" ]; do n=$((n + 1)); [ \"$n\" -lt 200 ] || exit 1; sleep 0.01; done; printf edited > \"$1\"",
                "noryn-test-editor",
            ])
            .arg(&path).arg(&gate);
        let mut launch = spawn_waiter(&mut command).unwrap();
        let mut receiver = launch.completion.take().unwrap();
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        std::fs::write(gate, b"close").unwrap();
        assert!(receiver.await.unwrap().is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), b"edited");
        let mut failing = Command::new("/bin/sh");
        failing.args(["-c", "exit 7"]);
        let result = spawn_waiter(&mut failing)
            .unwrap()
            .completion
            .unwrap()
            .await
            .unwrap();
        assert!(result.unwrap_err().contains("kein automatischer Upload"));
    }
}
