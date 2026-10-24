use crate::{
    model::{Credential, Node},
    shell_fs,
};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub fn handshake(host: &str, port: u16) -> Result<(Session, String)> {
    let addresses = (host, port)
        .to_socket_addrs()
        .context("Host konnte nicht aufgelöst werden")?;
    let mut socket = None;
    for addr in addresses {
        if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(10)) {
            socket = Some(stream);
            break;
        }
    }
    let socket =
        socket.context("SSH-Verbindung fehlgeschlagen (10 Sekunden Timeout je Adresse)")?;
    socket.set_read_timeout(Some(Duration::from_secs(20)))?;
    socket.set_write_timeout(Some(Duration::from_secs(20)))?;
    let mut session = Session::new()?;
    session.set_timeout(20_000);
    session.set_tcp_stream(socket);
    session
        .handshake()
        .context("SSH-Handshake fehlgeschlagen")?;
    let (key, _) = session
        .host_key()
        .context("Server hat keinen Host-Schlüssel geliefert")?;
    let fingerprint = format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(key))
    );
    Ok((session, fingerprint))
}

pub fn authenticate(session: &Session, credential: &Credential) -> Result<()> {
    authenticate_interactive(session, credential, |_, _, _| {
        bail!("Der Host benötigt eine interaktive Anmeldung (z. B. OTP)")
    })
}

#[derive(Clone, Serialize)]
pub struct AuthPrompt {
    pub text: String,
    pub echo: bool,
}

pub fn authenticate_interactive(
    session: &Session,
    credential: &Credential,
    mut answer: impl FnMut(&str, &str, Vec<AuthPrompt>) -> Result<Vec<String>>,
) -> Result<()> {
    let methods = session
        .auth_methods(&credential.username)
        .unwrap_or("")
        .to_string();
    let primary = match credential.kind.as_str() {
        "key" => Some(session.userauth_pubkey_memory(
            &credential.username,
            None,
            &credential.secret,
            (!credential.passphrase.is_empty()).then_some(credential.passphrase.as_str()),
        )),
        "agent" => Some(session.userauth_agent(&credential.username)),
        "password" => None,
        _ => bail!("Unbekannte Authentifizierungsmethode"),
    };
    if session.authenticated() {
        return Ok(());
    }
    let methods = session
        .auth_methods(&credential.username)
        .map(str::to_string)
        .unwrap_or(methods);
    let mut last_error = primary
        .and_then(std::result::Result::err)
        .map(|e| e.to_string());
    if methods.split(',').any(|method| method == "password")
        && !credential.login_password().is_empty()
    {
        if let Err(e) = session.userauth_password(&credential.username, credential.login_password())
        {
            last_error = Some(e.to_string());
        }
    }
    if session.authenticated() {
        return Ok(());
    }
    let methods = session
        .auth_methods(&credential.username)
        .map(str::to_string)
        .unwrap_or(methods);
    if methods
        .split(',')
        .any(|method| method == "keyboard-interactive")
    {
        struct Prompter<'a, F> {
            answer: &'a mut F,
            error: Option<anyhow::Error>,
        }
        impl<F: FnMut(&str, &str, Vec<AuthPrompt>) -> Result<Vec<String>>>
            ssh2::KeyboardInteractivePrompt for Prompter<'_, F>
        {
            fn prompt<'a>(
                &mut self,
                username: &str,
                instructions: &str,
                prompts: &[ssh2::Prompt<'a>],
            ) -> Vec<String> {
                if self.error.is_some() {
                    return vec![String::new(); prompts.len()];
                }
                let result = (self.answer)(
                    username,
                    instructions,
                    prompts
                        .iter()
                        .map(|p| AuthPrompt {
                            text: p.text.to_string(),
                            echo: p.echo,
                        })
                        .collect(),
                );
                match result {
                    Ok(answers) if answers.len() == prompts.len() => answers,
                    Ok(_) => {
                        self.error = Some(anyhow::anyhow!(
                            "Anzahl der Anmeldeantworten stimmt nicht überein"
                        ));
                        vec![String::new(); prompts.len()]
                    }
                    Err(error) => {
                        self.error = Some(error);
                        vec![String::new(); prompts.len()]
                    }
                }
            }
        }
        let mut prompter = Prompter {
            answer: &mut answer,
            error: None,
        };
        session.set_timeout(180_000);
        let result = session.userauth_keyboard_interactive(&credential.username, &mut prompter);
        session.set_timeout(20_000);
        if let Some(error) = prompter.error {
            return Err(error);
        }
        if let Err(error) = result {
            last_error = Some(error.to_string());
        }
    }
    ensure!(
        session.authenticated(),
        "Authentifizierung fehlgeschlagen: {}",
        last_error.unwrap_or_else(|| "Keine passende Anmeldemethode verfügbar".into())
    );
    Ok(())
}

pub(crate) fn retry<T>(
    mut operation: impl FnMut() -> std::result::Result<T, ssh2::Error>,
) -> Result<T> {
    let start = Instant::now();
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                ensure!(
                    start.elapsed() < Duration::from_secs(20),
                    "Zeitüberschreitung beim SSH-Dateizugriff"
                );
                thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

struct FileIo(ssh2::File);
impl Read for FileIo {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        retry_io(|| self.0.read(bytes))
    }
}
impl Write for FileIo {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        retry_io(|| self.0.write(bytes))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn retry_io<T>(mut operation: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let start = Instant::now();
    loop {
        match operation() {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= Duration::from_secs(20) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "SSH-Dateizugriff ohne Antwort",
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
            value => return value,
        }
    }
}

pub fn authenticated(node: &Node, credential: &Credential, expected: &str) -> Result<Session> {
    let (session, fingerprint) = handshake(&node.host, node.port)?;
    ensure!(
        fingerprint == expected,
        "HOST-SCHLÜSSEL GEÄNDERT. Verbindung wurde abgebrochen."
    );
    authenticate(&session, credential)?;
    Ok(session)
}

pub enum TerminalInput {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalEvent {
    Data { data: String },
    Closed { message: String },
}

pub fn start_terminal(
    session: Session,
    input: Receiver<TerminalInput>,
    output: impl Fn(TerminalEvent) + Send + 'static,
) -> Result<()> {
    session.set_blocking(false);
    let mut channel = retry(|| session.channel_session())?;
    retry(|| channel.request_pty("xterm-256color", None, Some((120, 32, 0, 0))))?;
    retry(|| channel.shell())?;
    thread::spawn(move || {
        let mut buffer = [0; 16_384];
        let mut pending = Vec::new();
        let mut queued = VecDeque::new();
        let mut queued_bytes = 0;
        let mut offset = 0;
        let mut resize = None;
        let mut latest_resize = None;
        let message = 'running: loop {
            for _ in 0..64 {
                match input.try_recv() {
                    Ok(TerminalInput::Data(data)) => {
                        if queued_bytes + pending.len() - offset + data.len() > 4 * 1024 * 1024 {
                            break 'running "Terminal-Eingabepuffer ist voll".to_string();
                        }
                        queued_bytes += data.len();
                        queued.push_back(data);
                    }
                    Ok(TerminalInput::Resize(cols, rows)) => latest_resize = Some((cols, rows)),
                    Ok(TerminalInput::Close) | Err(TryRecvError::Disconnected) => {
                        break 'running "Verbindung geschlossen".to_string()
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            if resize.is_none() {
                resize = latest_resize.take();
            }
            if let Some((cols, rows)) = resize {
                match channel.request_pty_size(cols.max(1), rows.max(1), None, None) {
                    Ok(()) => resize = None,
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                    Err(e) => break format!("Terminalgröße: {e}"),
                }
            }
            // Keep the current buffer and arguments stable across EAGAIN.
            // Newly typed or pasted data waits in its own queue until then.
            if pending.is_empty() {
                if let Some(data) = queued.pop_front() {
                    queued_bytes -= data.len();
                    pending = data;
                }
            }
            if offset < pending.len() {
                match channel.write(&pending[offset..]) {
                    Ok(n) => {
                        offset += n;
                        if offset == pending.len() {
                            pending.clear();
                            offset = 0;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => break e.to_string(),
                }
            }
            match channel.read(&mut buffer) {
                Ok(n) if n > 0 => output(TerminalEvent::Data {
                    data: STANDARD.encode(&buffer[..n]),
                }),
                Ok(_) if channel.eof() => break "Remote-Sitzung beendet".to_string(),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => break e.to_string(),
            }
            thread::sleep(Duration::from_millis(8));
        };
        let _ = channel.close();
        output(TerminalEvent::Closed { message });
    });
    Ok(())
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub is_file: bool,
    pub size: u64,
    pub modified: u64,
    pub permissions: String,
}

pub struct Remote {
    session: Session,
    sudo: Option<Zeroizing<String>>,
    saved_password: Zeroizing<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SudoOutcome {
    Enabled,
    Disabled,
    PasswordRequired { message: String },
}

impl Remote {
    pub fn new(session: Session) -> Self {
        session.set_blocking(false);
        Self {
            session,
            sudo: None,
            saved_password: Zeroizing::new(String::new()),
        }
    }

    pub fn with_password(mut self, password: String) -> Self {
        self.saved_password = Zeroizing::new(password);
        self
    }

    pub fn sudo_auto(&mut self, enabled: bool, password: Option<String>) -> Result<SudoOutcome> {
        if !enabled {
            self.set_sudo(false, String::new())?;
            return Ok(SudoOutcome::Disabled);
        }
        if let Some(password) = password {
            self.set_sudo(true, password)?;
            return Ok(SudoOutcome::Enabled);
        }
        match self.set_sudo(true, String::new()) {
            Ok(()) => return Ok(SudoOutcome::Enabled),
            Err(error) => {
                let message = format!("{error:#}");
                if !message.contains("password is required")
                    && !message.contains("password required")
                {
                    return Err(error);
                }
            }
        }
        if !self.saved_password.is_empty()
            && self.set_sudo(true, self.saved_password.to_string()).is_ok()
        {
            return Ok(SudoOutcome::Enabled);
        }
        Ok(SudoOutcome::PasswordRequired {
            message: "Für sudo wird ein zusätzliches oder anderes Passwort benötigt.".into(),
        })
    }

    pub fn set_sudo(&mut self, enabled: bool, password: String) -> Result<()> {
        if !enabled {
            self.sudo = None;
            return Ok(());
        }
        ensure!(
            !password.contains(['\n', '\r']),
            "sudo-Passwort enthält einen Zeilenumbruch"
        );
        let old = self.sudo.take();
        self.sudo = Some(Zeroizing::new(password));
        if let Err(e) = self.helper("check", "/", None, &mut std::io::sink(), None) {
            self.sudo = old;
            return Err(e.context(
                "sudo benötigt passende sudo-Rechte, eine Shell und GNU find auf dem Zielsystem",
            ));
        }
        Ok(())
    }

    pub fn home(&self) -> Result<String> {
        let sftp = retry(|| self.session.sftp())?;
        Ok(retry(|| sftp.realpath(Path::new(".")))?
            .to_string_lossy()
            .to_string())
    }

    pub fn list(&self, path: &str) -> Result<Vec<RemoteEntry>> {
        ensure!(!path.contains('\0'), "Ungültiger Pfad");
        let mut entries = if self.sudo.is_some() {
            let mut output = Vec::new();
            self.helper("list", path, None, &mut output, None)?;
            shell_fs::parse_entries(path, &output)?
        } else {
            let sftp = retry(|| self.session.sftp())?;
            let mut directory = retry(|| sftp.opendir(Path::new(path)))?;
            let mut files = Vec::new();
            loop {
                match retry(|| directory.readdir()) {
                    Ok((name, stat)) => {
                        // Check the raw entry before joining: Path::file_name on
                        // "/home/lucas/." resolves to "lucas", not ".".
                        if name.as_os_str() != "." && name.as_os_str() != ".." {
                            ensure!(
                                files.len() < 100_000,
                                "SFTP-Verzeichnis enthält zu viele Einträge"
                            );
                            files.push((Path::new(path).join(name), stat));
                        }
                    }
                    Err(error)
                        if error
                            .downcast_ref::<ssh2::Error>()
                            .is_some_and(|e| e.code() == ssh2::ErrorCode::Session(-16)) =>
                    {
                        break
                    }
                    Err(error) => return Err(error),
                }
            }
            retry(|| directory.close())?;
            files
                .into_iter()
                .map(|(path, stat)| {
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .context("Nicht-UTF-8-Dateiname wird nicht unterstützt")?
                        .to_string();
                    Ok(RemoteEntry {
                        name,
                        path: path.to_string_lossy().to_string(),
                        is_dir: stat.is_dir(),
                        is_file: stat.is_file(),
                        is_link: stat.file_type() == ssh2::FileType::Symlink,
                        size: stat.size.unwrap_or(0),
                        modified: stat.mtime.unwrap_or(0),
                        permissions: format!("{:03o}", stat.perm.unwrap_or(0) & 0o7777),
                    })
                })
                .collect::<Result<Vec<_>>>()?
        };
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(entries)
    }

    pub fn upload(&self, local: &Path, remote: &str) -> Result<u64> {
        self.upload_controlled(local, remote, &crate::transfer::Control::default())
    }

    pub fn upload_controlled(
        &self,
        local: &Path,
        remote: &str,
        control: &crate::transfer::Control,
    ) -> Result<u64> {
        control.check()?;
        let file = crate::transfer::open_regular(local)?;
        let size = file.metadata()?.len();
        let mut input = control.reader(file);
        if self.sudo.is_some() {
            self.helper(
                "upload",
                remote,
                Some(&mut input),
                &mut std::io::sink(),
                Some(size),
            )?;
        } else {
            let sftp = retry(|| self.session.sftp())?;
            // Exclusive creation prevents silently overwriting existing remote files.
            let mut output = FileIo(retry(|| {
                sftp.open_mode(
                    Path::new(remote),
                    ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::EXCLUSIVE,
                    0o600,
                    ssh2::OpenType::File,
                )
            })?);
            std::io::copy(&mut input, &mut output)?;
            retry(|| output.0.close())?;
        }
        Ok(size)
    }

    pub fn download(&self, remote: &str, local: &Path) -> Result<u64> {
        self.download_file(remote, local, true)
    }

    pub fn download_file(&self, remote: &str, local: &Path, overwrite: bool) -> Result<u64> {
        self.download_controlled(
            remote,
            local,
            overwrite,
            &crate::transfer::Control::default(),
        )
    }

    pub fn download_controlled(
        &self,
        remote: &str,
        local: &Path,
        overwrite: bool,
        control: &crate::transfer::Control,
    ) -> Result<u64> {
        control.check()?;
        let parent = local.parent().context("Ungültiger lokaler Pfad")?;
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        if self.sudo.is_some() {
            self.helper(
                "download",
                remote,
                None,
                &mut control.writer(&mut output),
                None,
            )?;
        } else {
            let sftp = retry(|| self.session.sftp())?;
            let mut input = FileIo(retry(|| sftp.open(Path::new(remote)))?);
            std::io::copy(&mut input, &mut control.writer(&mut output))?;
            retry(|| input.0.close())?;
        }
        control.check()?;
        output.as_file().sync_all()?;
        let size = output.as_file().metadata()?.len();
        if overwrite {
            output.persist(local).map_err(|e| e.error)?;
        } else {
            output
                .persist_noclobber(local)
                .map_err(|e| e.error)
                .context("Lokale Datei existiert bereits oder kann nicht angelegt werden")?;
        }
        Ok(size)
    }

    pub fn mkdir(&self, path: &str) -> Result<()> {
        if self.sudo.is_some() {
            self.helper("mkdir", path, None, &mut std::io::sink(), None)?;
        } else {
            let sftp = retry(|| self.session.sftp())?;
            retry(|| sftp.mkdir(Path::new(path), 0o755))?;
        }
        Ok(())
    }

    fn helper(
        &self,
        op: &str,
        path: &str,
        input: Option<&mut dyn Read>,
        output: &mut dyn Write,
        size: Option<u64>,
    ) -> Result<()> {
        let password = self.sudo.as_ref().context("sudo ist nicht aktiv")?;
        shell_fs::execute(&self.session, password, op, path, input, output, size)
    }
}
