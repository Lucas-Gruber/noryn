//! Explicit, non-interactive commands on existing authenticated SSH transports.
use anyhow::{ensure, Result};
use serde::Serialize;
use ssh2::{ErrorCode, Session};
use std::{
    io::{Read, Write},
    sync::{atomic::{AtomicBool, Ordering}, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub truncated: bool,
}
const LIMIT: usize = 256 * 1024;

pub fn run(
    session: &Session,
    closed: &AtomicBool,
    cancelled: &AtomicBool,
    command: &str,
    timeout: Duration,
    setup: &Mutex<()>,
) -> Output {
    run_input(session, closed, cancelled, command, &[], timeout, setup)
}

pub fn run_script(
    session: &Session,
    closed: &AtomicBool,
    cancelled: &AtomicBool,
    body: &str,
    timeout: Duration,
) -> Output {
    if body.len() > crate::scripts::MAX_BODY || body.contains('\0') {
        return Output {
            error: Some("Ungültiges Bash-Skript".into()),
            ..Default::default()
        };
    }
    run_input(
        session,
        closed,
        cancelled,
        "bash -s --",
        body.as_bytes(),
        timeout,
        setup,
    )
}
fn run_input(
    session: &Session,
    closed: &AtomicBool,
    cancelled: &AtomicBool,
    command: &str,
    input: &[u8],
    timeout: Duration,
    setup: &Mutex<()>,
) -> Output {
    let mut output = Output::default();
    let start = Instant::now();
    let check = || -> Result<()> {
        ensure!(
            !cancelled.load(Ordering::Acquire),
            "Ausführung abgebrochen. Entfernte Prozesse können weiterlaufen"
        );
        ensure!(
            !closed.load(Ordering::Acquire),
            "SSH-Sitzung beendet. Entfernte Prozesse können weiterlaufen"
        );
        ensure!(
            start.elapsed() < timeout,
            "Zeitlimit erreicht. Entfernte Prozesse können weiterlaufen"
        );
        Ok(())
    };
    let retry =
        |operation: &mut dyn FnMut() -> std::result::Result<(), ssh2::Error>| -> Result<()> {
            loop {
                check()?;
                match operation() {
                    Ok(()) => return Ok(()),
                    Err(e) if e.code() == ErrorCode::Session(-37) => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };
    let mut channel = None;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = (|| -> Result<()> {
        ensure!(
            !command.trim().is_empty() && command.len() <= 65_536 && !command.contains('\0'),
            "Ungültiger Befehl"
        );
        // libssh2's shared nonblocking session can interleave channel-open and
        // exec handshakes. Serialize only setup; commands still run concurrently.
        {
            let _setup_guard = setup.lock().map_err(|_| anyhow::anyhow!("SSH channel setup lock poisoned"))?;
            retry(&mut || {
                channel = Some(session.channel_session()?);
                Ok(())
            })?;
            // Retrying EAGAIN continues the same libssh2 operation, never a second exec.
            retry(&mut || channel.as_mut().unwrap().exec(command))?;
        }
        let ch = channel.as_mut().unwrap();
        let mut sent = 0;
        let mut eof_sent = false;
        let mut buffer = [0; 8192];
        loop {
            check()?;
            let mut progress = false;
            if sent < input.len() {
                match ch.write(&input[sent..input.len().min(sent + 32 * 1024)]) {
                    Ok(n) => {
                        sent += n;
                        progress |= n > 0;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
            } else if !eof_sent {
                match ch.send_eof() {
                    Ok(()) => {
                        eof_sent = true;
                        progress = true;
                    }
                    Err(e) if e.code() == ErrorCode::Session(-37) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            for (stream, target) in [(0, &mut stdout), (1, &mut stderr)] {
                match ch.stream(stream).read(&mut buffer) {
                    Ok(n) => {
                        progress |= n > 0;
                        let available = LIMIT.saturating_sub(target.len());
                        target.extend_from_slice(&buffer[..n.min(available)]);
                        if n > available {
                            output.truncated = true;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
            }
            // EOF can be reported with buffered stdout/stderr still waiting.
            if ch.eof() && !progress {
                break;
            }
            if !progress {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        retry(&mut || ch.wait_close())?;
        output.exit_code = Some(ch.exit_status()?);
        Ok(())
    })();
    if let Some(mut channel) = channel {
        let _ = channel.close();
    }
    if let Err(error) = result {
        output.error = Some(format!("{error:#}"));
    }
    // Render as plain text: no terminal escapes, HTML or unbounded binary output.
    let clean = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .chars()
            .filter(|c| !c.is_control() || ['\n', '\t', '\r'].contains(c))
            .collect()
    };
    output.stdout = clean(&stdout);
    output.stderr = clean(&stderr);
    output
}
