//! Explicit, non-interactive commands on existing authenticated SSH transports.
use anyhow::{ensure, Result};
use serde::Serialize;
use ssh2::{ErrorCode, Session};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
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
        retry(&mut || {
            channel = Some(session.channel_session()?);
            Ok(())
        })?;
        let ch = channel.as_mut().unwrap();
        // Retrying EAGAIN continues the same libssh2 operation, never a second exec.
        retry(&mut || ch.exec(command))?;
        retry(&mut || ch.send_eof())?;
        let mut buffer = [0; 8192];
        loop {
            check()?;
            let mut progress = false;
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
