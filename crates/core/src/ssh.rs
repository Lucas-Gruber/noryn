use crate::model::{Credential, Node};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::{fs, io::{Read, Write}, net::{TcpStream, ToSocketAddrs}, path::Path, sync::mpsc::{Receiver, TryRecvError}, thread, time::Duration};
use zeroize::Zeroizing;

pub fn handshake(host: &str, port: u16) -> Result<(Session, String)> {
    let addresses = (host, port).to_socket_addrs().context("Host konnte nicht aufgelöst werden")?;
    let mut socket = None;
    for addr in addresses {
        if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(10)) { socket = Some(stream); break; }
    }
    let socket = socket.context("SSH-Verbindung fehlgeschlagen (10 Sekunden Timeout je Adresse)")?;
    socket.set_read_timeout(Some(Duration::from_secs(20)))?;
    socket.set_write_timeout(Some(Duration::from_secs(20)))?;
    let mut session = Session::new()?;
    session.set_timeout(20_000);
    session.set_tcp_stream(socket);
    session.handshake().context("SSH-Handshake fehlgeschlagen")?;
    let (key, _) = session.host_key().context("Server hat keinen Host-Schlüssel geliefert")?;
    let fingerprint = format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(key)));
    Ok((session, fingerprint))
}

pub fn authenticate(session: &Session, credential: &Credential) -> Result<()> {
    match credential.kind.as_str() {
        "password" => session.userauth_password(&credential.username, &credential.secret)?,
        "key" => session.userauth_pubkey_memory(&credential.username, None, &credential.secret,
            (!credential.passphrase.is_empty()).then_some(credential.passphrase.as_str()))?,
        "agent" => session.userauth_agent(&credential.username)?,
        _ => bail!("Unbekannte Authentifizierungsmethode"),
    }
    ensure!(session.authenticated(), "Authentifizierung fehlgeschlagen");
    Ok(())
}

pub fn authenticated(node: &Node, credential: &Credential, expected: &str) -> Result<Session> {
    let (session, fingerprint) = handshake(&node.host, node.port)?;
    ensure!(fingerprint == expected, "HOST-SCHLÜSSEL GEÄNDERT. Verbindung wurde abgebrochen.");
    authenticate(&session, credential)?;
    Ok(session)
}

pub enum TerminalInput { Data(Vec<u8>), Resize(u32, u32), Close }

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalEvent { Data { data: String }, Closed { message: String } }

pub fn start_terminal(session: Session, input: Receiver<TerminalInput>, output: impl Fn(TerminalEvent) + Send + 'static) -> Result<()> {
    let mut channel = session.channel_session()?;
    channel.request_pty("xterm-256color", None, Some((120, 32, 0, 0)))?;
    channel.shell()?;
    session.set_blocking(false);
    thread::spawn(move || {
        let mut buffer = [0; 16_384];
        let mut pending = Vec::new();
        let mut offset = 0;
        let mut resize = None;
        let message = 'running: loop {
            for _ in 0..64 {
                match input.try_recv() {
                    Ok(TerminalInput::Data(data)) => {
                        if pending.len() > 4 * 1024 * 1024 { break 'running "Terminal-Eingabepuffer ist voll".to_string(); }
                        pending.extend(data);
                    }
                    Ok(TerminalInput::Resize(cols, rows)) => resize = Some((cols, rows)),
                    Ok(TerminalInput::Close) | Err(TryRecvError::Disconnected) => break 'running "Verbindung geschlossen".to_string(),
                    Err(TryRecvError::Empty) => break,
                }
            }
            if let Some((cols, rows)) = resize {
                match channel.request_pty_size(cols.max(1), rows.max(1), None, None) {
                    Ok(()) => resize = None,
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {},
                    Err(e) => break format!("Terminalgröße: {e}"),
                }
            }
            if offset < pending.len() {
                match channel.write(&pending[offset..]) {
                    Ok(n) => { offset += n; if offset == pending.len() { pending.clear(); offset = 0; } },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {},
                    Err(e) => break e.to_string(),
                }
            }
            match channel.read(&mut buffer) {
                Ok(n) if n > 0 => output(TerminalEvent::Data { data: STANDARD.encode(&buffer[..n]) }),
                Ok(_) if channel.eof() => break "Remote-Sitzung beendet".to_string(),
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {},
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
    pub size: u64,
    pub modified: u64,
    pub permissions: String,
}

pub struct Remote {
    session: Session,
    sudo: Option<Zeroizing<String>>,
}

impl Remote {
    pub fn new(session: Session) -> Self { Self { session, sudo: None } }

    pub fn set_sudo(&mut self, enabled: bool, password: String) -> Result<()> {
        if !enabled { self.sudo = None; return Ok(()); }
        ensure!(!password.contains(['\n', '\r']), "sudo-Passwort enthält einen Zeilenumbruch");
        let old = self.sudo.take();
        self.sudo = Some(Zeroizing::new(password));
        if let Err(e) = self.helper("check", "/", None, &mut std::io::sink(), None) {
            self.sudo = old;
            return Err(e.context("sudo benötigt Python 3 und passende sudo-Rechte auf dem Zielsystem"));
        }
        Ok(())
    }

    pub fn home(&self) -> Result<String> {
        Ok(self.session.sftp()?.realpath(Path::new("."))?.to_string_lossy().to_string())
    }

    pub fn list(&self, path: &str) -> Result<Vec<RemoteEntry>> {
        ensure!(!path.contains('\0'), "Ungültiger Pfad");
        let mut entries = if self.sudo.is_some() {
            let mut output = Vec::new();
            self.helper("list", path, None, &mut output, None)?;
            serde_json::from_slice(&output)?
        } else {
            self.session.sftp()?.readdir(Path::new(path))?.into_iter().filter_map(|(path, stat)| {
                let name = path.file_name()?.to_string_lossy().to_string();
                if name == "." || name == ".." { return None; }
                Some(RemoteEntry { name, path: path.to_string_lossy().to_string(), is_dir: stat.is_dir(),
                    is_link: stat.file_type() == ssh2::FileType::Symlink, size: stat.size.unwrap_or(0),
                    modified: stat.mtime.unwrap_or(0), permissions: format!("{:03o}", stat.perm.unwrap_or(0) & 0o7777) })
            }).collect::<Vec<_>>()
        };
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    }

    pub fn upload(&self, local: &Path, remote: &str) -> Result<u64> {
        ensure!(local.is_file(), "Bitte einzelne Dateien hochladen; Ordner-Upload ist noch nicht implementiert");
        let mut input = fs::File::open(local)?;
        let size = input.metadata()?.len();
        if self.sudo.is_some() {
            self.helper("upload", remote, Some(&mut input), &mut std::io::sink(), Some(size))?;
        } else {
            let sftp = self.session.sftp()?;
            // Exclusive creation prevents silently overwriting existing remote files.
            let mut output = sftp.open_mode(Path::new(remote), ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::EXCLUSIVE, 0o600, ssh2::OpenType::File)?;
            std::io::copy(&mut input, &mut output)?;
            output.close()?;
        }
        Ok(size)
    }

    pub fn download(&self, remote: &str, local: &Path) -> Result<u64> {
        let parent = local.parent().context("Ungültiger lokaler Pfad")?;
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        if self.sudo.is_some() {
            self.helper("download", remote, None, &mut output, None)?;
        } else {
            let mut input = self.session.sftp()?.open(Path::new(remote))?;
            std::io::copy(&mut input, &mut output)?;
        }
        output.as_file().sync_all()?;
        let size = output.as_file().metadata()?.len();
        output.persist(local).map_err(|e| e.error)?;
        Ok(size)
    }

    pub fn mkdir(&self, path: &str) -> Result<()> {
        if self.sudo.is_some() { self.helper("mkdir", path, None, &mut std::io::sink(), None)?; }
        else { self.session.sftp()?.mkdir(Path::new(path), 0o755)?; }
        Ok(())
    }

    fn helper(&self, op: &str, path: &str, input: Option<&mut dyn Read>, output: &mut dyn Write, size: Option<u64>) -> Result<()> {
        let password = self.sudo.as_ref().context("sudo ist nicht aktiv")?;
        let args = STANDARD.encode(serde_json::to_vec(&serde_json::json!({ "op": op, "path": path, "size": size }))?);
        let flag = if password.is_empty() { "-n" } else { "-S -p ''" };
        let command = format!("sudo {flag} -- python3 -c {} {}", shell_quote(REMOTE_HELPER), shell_quote(&args));
        let mut channel = self.session.channel_session()?;
        channel.exec(&command)?;
        if !password.is_empty() { channel.write_all(password.as_bytes())?; channel.write_all(b"\n")?; }
        if let Some(input) = input { std::io::copy(input, &mut channel)?; }
        channel.send_eof()?;
        std::io::copy(&mut channel, output)?;
        let mut error = String::new();
        channel.stderr().take(16_384).read_to_string(&mut error)?;
        channel.wait_close()?;
        ensure!(channel.exit_status()? == 0, "Remote-Dateizugriff fehlgeschlagen: {}", error.trim());
        Ok(())
    }
}

fn shell_quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

// Paths are data in a base64 JSON argument, never interpolated into executable code.
const REMOTE_HELPER: &str = r#"import os,sys,json,base64,stat,shutil
a=json.loads(base64.b64decode(sys.argv[1])); p=a['path']; op=a['op']
if op=='check':
 assert os.geteuid()==0, 'sudo did not grant root'
elif op=='list':
 result=[]
 for e in os.scandir(p):
  s=e.stat(follow_symlinks=False)
  result.append(dict(name=e.name,path=e.path,isDir=e.is_dir(follow_symlinks=False),isLink=e.is_symlink(),size=s.st_size,modified=int(s.st_mtime),permissions=oct(stat.S_IMODE(s.st_mode))[2:]))
 print(json.dumps(result))
elif op=='download':
 with open(p,'rb') as f: shutil.copyfileobj(f,sys.stdout.buffer)
elif op=='upload':
 fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
 with os.fdopen(fd,'wb') as f:
  remaining=a['size']
  while remaining:
   chunk=sys.stdin.buffer.read(min(remaining,65536))
   if not chunk: raise IOError('incomplete upload')
   f.write(chunk); remaining-=len(chunk)
elif op=='mkdir': os.mkdir(p,0o755)
else: raise ValueError('unsupported operation')
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoting_does_not_execute_paths() { assert_eq!(shell_quote("x'; touch /tmp/oops"), "'x'\\''; touch /tmp/oops'"); }
}
