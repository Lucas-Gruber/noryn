//! Bounded, explicit Linux management operations over the existing SSH transport.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::{
    io::{Read, Write},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    ServiceStatus {
        name: String,
    },
    ServiceFile {
        name: String,
    },
    ServiceStart {
        name: String,
    },
    ServiceStop {
        name: String,
    },
    ServiceRestart {
        name: String,
    },
    ServiceReload {
        name: String,
    },
    ServiceEnable {
        name: String,
    },
    ServiceDisable {
        name: String,
    },
    ProcessInfo {
        pid: u32,
    },
    ProcessTerminate {
        pid: u32,
        #[serde(rename = "startTicks")]
        start_ticks: u64,
    },
    ProcessKill {
        pid: u32,
        #[serde(rename = "startTicks")]
        start_ticks: u64,
    },
    Storage,
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Reply {
    Completed {
        output: String,
        #[serde(rename = "startTicks")]
        start_ticks: Option<u64>,
        storage: Option<crate::storage_info::Storage>,
    },
    PasswordRequired {
        message: String,
    },
}
const PROMPT: &str = "[noryn-system-sudo-password]";
const READY: &[u8] = b"NORYN_SYSTEM_READY_V1\n";
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn unit(name: &str) -> Result<String> {
    ensure!(
        !name.starts_with('-')
            && name.ends_with(".service")
            && name.len() <= 256
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"@_.:-\\".contains(&b)),
        "Ungültiger systemd-Dienstname"
    );
    Ok(quote(name))
}
fn process(pid: u32) -> Result<()> {
    ensure!(pid > 1, "PID 0 und 1 können hier nicht verwaltet werden");
    Ok(())
}
fn command(request: &Request) -> Result<String> {
    Ok(match request {
        Request::ServiceStatus {name} => format!("systemctl --no-pager show -p Id -p Description -p LoadState -p ActiveState -p SubState -p UnitFileState -p FragmentPath -p DropInPaths -p MainPID -p User -p Group -p Restart -p MemoryCurrent -p TasksCurrent -- {0}; printf '\\n--- STATUS & LETZTE LOGS ---\\n'; systemctl --no-pager --full --lines=40 status -- {0}; result=$?; case \"$result\" in 0|3|4) exit 0 ;; *) exit \"$result\" ;; esac", unit(name)?),
        Request::ServiceFile {name} => format!("exec systemctl --no-pager cat -- {}", unit(name)?),
        Request::ServiceStart {name} | Request::ServiceStop {name} | Request::ServiceRestart {name} | Request::ServiceReload {name} | Request::ServiceEnable {name} | Request::ServiceDisable {name} => {
            let action = match request { Request::ServiceStart {..} => "start", Request::ServiceStop {..} => "stop", Request::ServiceRestart {..} => "restart", Request::ServiceReload {..} => "reload", Request::ServiceEnable {..} => "enable", _ => "disable" };
            format!("systemctl --no-ask-password --no-block {action} -- {} && printf '%s\\n' 'Auftrag an systemd übergeben. Status bitte aktualisieren.'", unit(name)?)
        }
        Request::ProcessInfo {pid} => {
            ensure!(*pid > 0, "Ungültige PID");
            format!("set -f; [ -r /proc/{pid}/stat ] || {{ echo 'Prozess nicht mehr vorhanden' >&2; exit 1; }}; awk '{{sub(/^.*\\) /,\"\");print $20}}' /proc/{pid}/stat; printf 'Executable: '; readlink /proc/{pid}/exe || true; printf 'Arbeitsverzeichnis: '; readlink /proc/{pid}/cwd || true; printf '\\n'; cat /proc/{pid}/status")
        }
        Request::ProcessTerminate {pid,start_ticks} | Request::ProcessKill {pid,start_ticks} => {
            process(*pid)?; ensure!(*start_ticks > 0, "Prozessidentität fehlt; Details erneut laden");
            let signal = if matches!(request, Request::ProcessKill {..}) { "KILL" } else { "TERM" };
            format!("current=$(awk '{{sub(/^.*\\) /,\"\");print $20}}' /proc/{pid}/stat) || exit 1; [ \"$current\" = {start_ticks} ] || {{ echo 'Prozess wurde inzwischen ersetzt; Details erneut laden' >&2; exit 1; }}; /bin/kill -s {signal} -- {pid} && printf '%s\\n' '{signal} an PID {pid} gesendet.'")
        }
        Request::Storage => crate::storage_info::SCRIPT.into(),
    })
}
struct Output {
    text: String,
    error: String,
    status: i32,
    started: bool,
}

pub fn run(
    session: &Session,
    closed: &AtomicBool,
    saved_password: &str,
    request: &Request,
    elevated: bool,
    password: Option<String>,
) -> Result<Reply> {
    let password = password.map(Zeroizing::new);
    ensure!(
        password.as_ref().is_none_or(|value| value.len() <= 65536),
        "sudo-Passwort ist zu lang"
    );
    let script = command(request)?;
    let execute = |password: &str| execute(session, closed, &script, elevated, password);
    let result = if !elevated {
        execute("")?
    } else if let Some(password) = &password {
        execute(password)?
    } else {
        let first = execute("")?;
        // Only retry when sudo did not start the operation. Never repeat a started mutation.
        if !first.started && !saved_password.is_empty() {
            execute(saved_password)?
        } else {
            first
        }
    };
    if !result.started && elevated && password.is_none() {
        return Ok(Reply::PasswordRequired {
            message: result.error,
        });
    }
    ensure!(
        result.started && result.status == 0,
        "Remote-Aktion fehlgeschlagen: {}",
        result.error.trim()
    );
    let (output, start_ticks) = if matches!(request, Request::ProcessInfo { .. }) {
        let (ticks, text) = result
            .text
            .split_once('\n')
            .context("Prozessidentität fehlt")?;
        (text.to_string(), Some(ticks.trim().parse()?))
    } else {
        (result.text, None)
    };
    let storage = if matches!(request, Request::Storage) {
        Some(crate::storage_info::parse(&output)?)
    } else {
        None
    };
    Ok(Reply::Completed {
        output,
        start_ticks,
        storage,
    })
}

fn execute(
    session: &Session,
    closed: &AtomicBool,
    script: &str,
    elevated: bool,
    password: &str,
) -> Result<Output> {
    let script = format!("export LC_ALL=C SYSTEMD_COLORS=0 SYSTEMD_PAGER=cat; printf 'NORYN_SYSTEM_READY_V1\\n'; {script}");
    let base = format!("sh -c {} noryn-system", quote(&script));
    let command = if elevated {
        let flag = if password.is_empty() {
            "-n".into()
        } else {
            format!("-S -p {}", quote(PROMPT))
        };
        format!("if [ \"$(id -u)\" = 0 ]; then exec {base}; else exec sudo {flag} -- {base}; fi")
    } else {
        base
    };
    ensure!(!closed.load(Ordering::Acquire), "SSH-Sitzung ist beendet");
    let mut channel = crate::ssh::retry(|| session.channel_session())?;
    crate::ssh::retry(|| channel.exec(&command))?;
    let result = (|| -> Result<Output> {
        let mut text = Vec::new();
        let mut errors = Vec::new();
        let mut header = Vec::new();
        let mut pending = Zeroizing::new(Vec::new());
        let mut offset = 0;
        let mut sent_password = false;
        let mut eof_sent = false;
        let mut buffer = [0u8; 8192];
        let start = Instant::now();
        loop {
            ensure!(!closed.load(Ordering::Acquire), "SSH-Sitzung ist beendet");
            ensure!(start.elapsed() < Duration::from_secs(20), "Zeitlimit; der Ausgang der Remote-Aktion ist unbekannt. Vor erneutem Ausführen den Status prüfen.");
            let mut progress = false;
            match channel.stderr().read(&mut buffer) {
                Ok(n) => {
                    ensure!(errors.len() + n <= 256 * 1024, "Fehlerausgabe ist zu groß");
                    errors.extend_from_slice(&buffer[..n]);
                    progress |= n > 0;
                    let prompts = errors
                        .windows(PROMPT.len())
                        .filter(|b| *b == PROMPT.as_bytes())
                        .count();
                    if prompts > 1 && header.len() < READY.len() {
                        return Ok(Output {
                            text: String::new(),
                            error: "sudo hat das Passwort abgelehnt".into(),
                            status: 1,
                            started: false,
                        });
                    }
                    ensure!(
                        prompts <= 1,
                        "Unerwartete sudo-Abfrage nach Beginn der Aktion"
                    );
                    if prompts == 1 && !sent_password {
                        ensure!(
                            !password.is_empty() && header.len() < READY.len(),
                            "Unerwartete sudo-Abfrage"
                        );
                        pending.extend_from_slice(password.as_bytes());
                        pending.push(b'\n');
                        sent_password = true;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            match channel.read(&mut buffer) {
                Ok(n) => {
                    progress |= n > 0;
                    let mut bytes = &buffer[..n];
                    if header.len() < READY.len() {
                        let count = bytes.len().min(READY.len() - header.len());
                        header.extend_from_slice(&bytes[..count]);
                        bytes = &bytes[count..];
                        ensure!(
                            READY.starts_with(&header),
                            "Unerwartete Remote-Ausgabe; Aktion nicht automatisch wiederholen"
                        );
                    }
                    ensure!(
                        text.len() + bytes.len() <= 256 * 1024,
                        "Remote-Ausgabe ist zu groß"
                    );
                    text.extend_from_slice(bytes);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            if offset < pending.len() {
                match channel.write(&pending[offset..]) {
                    Ok(n) => {
                        offset += n;
                        progress |= n > 0;
                        if offset == pending.len() {
                            pending.zeroize();
                            pending.clear();
                            offset = 0;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
            }
            if header.len() == READY.len() && pending.is_empty() && !eof_sent {
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
                break;
            }
            if !progress {
                thread::sleep(Duration::from_millis(5));
            }
        }
        crate::ssh::retry(|| channel.wait_close())?;
        Ok(Output {
            text: String::from_utf8_lossy(&text).into(),
            error: String::from_utf8_lossy(&errors).replace(PROMPT, ""),
            status: channel.exit_status()?,
            started: header.len() == READY.len(),
        })
    })();
    let _ = channel.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn management_targets_and_actions_cannot_inject_shell_commands() {
        for name in [
            "x.service; touch /tmp/x",
            "--all.service",
            "*.service",
            "$(id).service",
            "x\n.service",
        ] {
            assert!(command(&Request::ServiceRestart { name: name.into() }).is_err());
        }
        assert!(command(&Request::ServiceRestart {
            name: "postgresql@16-main.service".into()
        })
        .unwrap()
        .contains("'postgresql@16-main.service'"));
        assert!(command(&Request::ProcessInfo { pid: 1 }).is_ok());
        assert!(command(&Request::ProcessKill {
            pid: 1,
            start_ticks: 2
        })
        .is_err());
        assert!(command(&Request::ProcessKill {
            pid: 10,
            start_ticks: 0
        })
        .is_err());
    }
}
