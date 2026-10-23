use crate::ssh::RemoteEntry;
use anyhow::{ensure, Context, Result};
use ssh2::{Channel, Session};
use std::{
    io::{Read, Write},
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const READY: &[u8] = b"HARBOR_FS_READY_V1\n";
const PASSWORD_PROMPT: &str = "[harbor-sudo-password]";
const MAX_LIST_BYTES: usize = 32 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 64 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(20);

// Paths are separate quoted positional arguments, never inserted into the script.
// NUL-delimited metadata preserves filenames containing spaces, tabs or newlines.
const SCRIPT: &str = r#"set -eu
export LC_ALL=C
operation=$1
path=$2
expected_size=$3
case "$path" in /*) ;; *) path="./$path" ;; esac
printf 'HARBOR_FS_READY_V1\n'
case "$operation" in
 check)
  [ "$(id -u)" = 0 ] || { printf '%s\n' 'sudo hat keine root-Rechte erteilt' >&2; exit 1; }
  command -v find cat mkdir wc >/dev/null
  find / -maxdepth 0 -printf '' >/dev/null
  ;;
 list)
  [ -d "$path" ] || { printf '%s\n' 'Kein lesbares Verzeichnis' >&2; exit 1; }
  exec find "$path" -mindepth 1 -maxdepth 1 -printf '%f\0%y\0%s\0%T@\0%m\0'
  ;;
 download)
  [ -f "$path" ] || { printf '%s\n' 'Keine regulaere Datei' >&2; exit 1; }
  exec cat "$path"
  ;;
 upload)
  umask 077
  set -C
  cat > "$path"
  [ "$(wc -c < "$path")" -eq "$expected_size" ] || { printf '%s\n' 'Unvollstaendiger Upload' >&2; exit 1; }
  ;;
 mkdir) exec mkdir "$path" ;;
 *) printf '%s\n' 'Unbekannte Dateioperation' >&2; exit 1 ;;
esac
"#;

pub(crate) fn execute(
    session: &Session,
    password: &str,
    operation: &str,
    path: &str,
    input: Option<&mut dyn Read>,
    output: &mut dyn Write,
    size: Option<u64>,
) -> Result<()> {
    ensure!(
        ["check", "list", "download", "upload", "mkdir"].contains(&operation),
        "Unbekannte Dateioperation"
    );
    ensure!(!path.is_empty() && !path.contains('\0'), "Ungültiger Pfad");
    let flag = if password.is_empty() {
        "-n".to_string()
    } else {
        format!("-S -p {}", shell_quote(PASSWORD_PROMPT))
    };
    let command = format!(
        "LC_ALL=C sudo {flag} -- sh -c {} harbor-fs {} {} {}",
        shell_quote(SCRIPT),
        shell_quote(operation),
        shell_quote(path),
        size.unwrap_or(0),
    );
    let mut channel = crate::ssh::retry(|| session.channel_session())?;
    crate::ssh::retry(|| channel.exec(&command))?;
    let result = exchange(
        &mut channel,
        password,
        input,
        output,
        (operation == "list").then_some(MAX_LIST_BYTES),
    );
    // Keep the shared SSH transport nonblocking so terminal I/O can continue.
    let error = match result {
        Ok(error) => error,
        Err(error) => {
            let _ = channel.close();
            return Err(error);
        }
    };
    crate::ssh::retry(|| channel.wait_close())?;
    ensure!(
        channel.exit_status()? == 0,
        "Remote-Dateizugriff fehlgeschlagen: {}",
        String::from_utf8_lossy(&error).trim()
    );
    Ok(())
}

fn exchange(
    channel: &mut Channel,
    password: &str,
    mut input: Option<&mut dyn Read>,
    output: &mut dyn Write,
    output_limit: Option<usize>,
) -> Result<Vec<u8>> {
    let mut buffer = [0; 64 * 1024];
    let mut header = Vec::new();
    let mut error = Vec::new();
    let mut pending = Zeroizing::new(Vec::new());
    let mut offset = 0;
    let mut ready = false;
    let mut password_queued = false;
    let mut input_eof = false;
    let mut eof_sent = false;
    let mut output_bytes = 0;
    let mut last_progress = Instant::now();
    loop {
        let mut progress = false;
        // Drain both output streams while uploading so errors cannot fill the
        // SSH receive window and deadlock an otherwise valid file transfer.
        match channel.stderr().read(&mut buffer) {
            Ok(n) if n > 0 => {
                ensure!(
                    error.len() + n <= MAX_ERROR_BYTES,
                    "Remote-Fehlerausgabe ist zu groß"
                );
                error.extend_from_slice(&buffer[..n]);
                progress = true;
                let prompts = error
                    .windows(PASSWORD_PROMPT.len())
                    .filter(|part| *part == PASSWORD_PROMPT.as_bytes())
                    .count();
                ensure!(prompts <= 1, "sudo hat das Passwort abgelehnt");
                if prompts == 1 && !password_queued {
                    ensure!(
                        !password.is_empty() && !ready,
                        "Unerwartete sudo-Passwortabfrage"
                    );
                    pending.extend_from_slice(password.as_bytes());
                    pending.push(b'\n');
                    password_queued = true;
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        match channel.read(&mut buffer) {
            Ok(n) if n > 0 => {
                progress = true;
                let mut data = &buffer[..n];
                if !ready {
                    let needed = (READY.len() - header.len()).min(data.len());
                    header.extend_from_slice(&data[..needed]);
                    ensure!(
                        READY.starts_with(&header),
                        "Unerwartete Ausgabe vor dem sudo-Dateizugriff"
                    );
                    data = &data[needed..];
                    ready = header.len() == READY.len();
                }
                if ready {
                    output_bytes += data.len();
                    ensure!(
                        output_limit.is_none_or(|limit| output_bytes <= limit),
                        "Verzeichnisliste ist zu groß"
                    );
                    output.write_all(data)?;
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        if ready && pending.is_empty() && !input_eof {
            if let Some(input) = input.as_mut() {
                let n = input.read(&mut buffer)?;
                input_eof = n == 0;
                pending.extend_from_slice(&buffer[..n]);
            } else {
                input_eof = true;
            }
        }
        if offset < pending.len() {
            match channel.write(&pending[offset..]) {
                Ok(n) => {
                    progress |= n > 0;
                    offset += n;
                    if offset == pending.len() {
                        // Wipe the password as well as uploaded bytes before reuse.
                        use zeroize::Zeroize;
                        pending.zeroize();
                        pending.clear();
                        offset = 0;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
        }
        if ready && input_eof && pending.is_empty() && !eof_sent {
            match channel.send_eof() {
                Ok(()) => {
                    eof_sent = true;
                    progress = true;
                }
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if channel.eof() {
            ensure!(
                ready,
                "sudo-Dateizugriff konnte nicht gestartet werden: {}",
                String::from_utf8_lossy(&error).trim()
            );
            break;
        }
        if progress {
            last_progress = Instant::now();
        }
        ensure!(
            last_progress.elapsed() < IDLE_TIMEOUT,
            "Zeitüberschreitung beim sudo-Dateizugriff"
        );
        if !progress {
            thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(error)
}

pub(crate) fn parse_entries(path: &str, bytes: &[u8]) -> Result<Vec<RemoteEntry>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(bytes.last() == Some(&0), "Unvollständige Verzeichnisliste");
    let fields: Vec<_> = bytes[..bytes.len() - 1].split(|b| *b == 0).collect();
    ensure!(fields.len() % 5 == 0, "Ungültige Verzeichnisliste");
    fields
        .as_chunks::<5>()
        .0
        .iter()
        .map(|fields| {
            let name =
                std::str::from_utf8(fields[0]).context("Dateiname ist nicht UTF-8-kodiert")?;
            ensure!(
                !name.is_empty() && ![".", ".."].contains(&name) && !name.contains('/'),
                "Ungültiger Dateiname"
            );
            let kind = std::str::from_utf8(fields[1])?;
            ensure!(
                ["f", "d", "l", "b", "c", "p", "s", "D"].contains(&kind),
                "Ungültiger Dateityp"
            );
            let size = std::str::from_utf8(fields[2])?.parse()?;
            let modified = std::str::from_utf8(fields[3])?.parse::<f64>()?;
            ensure!(modified.is_finite(), "Ungültiger Änderungszeitpunkt");
            let permissions = u32::from_str_radix(std::str::from_utf8(fields[4])?, 8)?;
            ensure!(permissions <= 0o7777, "Ungültige Dateirechte");
            Ok(RemoteEntry {
                name: name.into(),
                path: format!("{}/{}", path.trim_end_matches('/'), name),
                is_dir: kind == "d",
                is_link: kind == "l",
                size,
                modified: modified.max(0.0) as u64,
                permissions: format!("{permissions:03o}"),
            })
        })
        .collect()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        process::{Command, Output, Stdio},
    };

    fn local(operation: &str, path: &Path, input: &[u8]) -> Output {
        let command = format!(
            "sh -c {} harbor-fs {} {} {}",
            shell_quote(SCRIPT),
            shell_quote(operation),
            shell_quote(path.to_str().unwrap()),
            input.len()
        );
        let mut child = Command::new("sh")
            .args(["-c", &command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn listing_preserves_unusual_names_and_symlinks_without_python() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "file with spaces",
            "line\nbreak\tname",
            "quotes'\";$(touch injected)",
            "-leading-option",
            "Grüße",
        ] {
            fs::write(dir.path().join(name), b"binary\0data").unwrap();
        }
        fs::create_dir(dir.path().join("a directory")).unwrap();
        std::os::unix::fs::symlink("a directory", dir.path().join("link")).unwrap();
        let output = local("list", dir.path(), b"");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let entries = parse_entries(
            dir.path().to_str().unwrap(),
            output.stdout.strip_prefix(READY).unwrap(),
        )
        .unwrap();
        assert_eq!(entries.len(), 7);
        assert!(entries
            .iter()
            .any(|e| e.name == "line\nbreak\tname" && e.size == 11));
        assert!(entries
            .iter()
            .any(|e| e.name == "link" && e.is_link && !e.is_dir));
        assert!(entries.iter().any(|e| e.name == "a directory" && e.is_dir));
        assert!(!dir.path().join("injected").exists());
    }

    #[test]
    fn binary_transfers_do_not_overwrite_files_or_follow_upload_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file '\"\n$(ignored)");
        let bytes = b"\0\xff\nall\r\nbinary\0bytes";
        let output = local("upload", &path, bytes);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(!local("upload", &path, b"overwrite").status.success());
        let download = local("download", &path, b"");
        assert!(download.status.success());
        assert_eq!(download.stdout.strip_prefix(READY).unwrap(), bytes);
        let link = dir.path().join("symlink");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(!local("upload", &link, b"overwrite").status.success());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let dangling = dir.path().join("dangling");
        let absent = dir.path().join("must-not-exist");
        std::os::unix::fs::symlink(&absent, &dangling).unwrap();
        assert!(!local("upload", &dangling, b"overwrite").status.success());
        assert!(!absent.exists());
        let folder = dir.path().join("new folder\n'quoted'");
        assert!(local("mkdir", &folder, b"").status.success());
        assert!(parse_entries(
            folder.to_str().unwrap(),
            local("list", &folder, b"")
                .stdout
                .strip_prefix(READY)
                .unwrap()
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn malformed_metadata_cannot_escape_the_selected_directory() {
        for data in [
            b"../escape\x00f\x000\x001\x00644\x00".as_slice(),
            b"incomplete\x00f\x00",
            b"name\x00f\x001\x00NaN\x00644\x00",
            b"name\x00f\x001\x001\x00999\x00",
        ] {
            assert!(parse_entries("/tmp/test", data).is_err());
        }
    }
}
