use crate::{model::Credential, ssh::RemoteEntry};
use anyhow::{bail, ensure, Context, Result};
use std::{
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    time::{Duration, UNIX_EPOCH},
};
use suppaftp::{
    list::{File, ListParser},
    native_tls::{Protocol, TlsConnector},
    types::FileType,
    FtpError, Mode, NativeTlsConnector, NativeTlsFtpStream,
};

pub struct Remote {
    stream: NativeTlsFtpStream,
    home: String,
    machine_listing: bool,
}

impl Remote {
    pub fn connect(host: &str, port: u16, credential: &Credential, secure: bool) -> Result<Self> {
        let tls = if secure {
            Some(
                TlsConnector::builder()
                    .min_protocol_version(Some(Protocol::Tlsv12))
                    .build()?,
            )
        } else {
            None
        };
        Self::connect_with_tls(host, port, credential, tls)
    }

    // Supplying a connector allows an explicitly trusted private CA, while
    // retaining certificate and hostname validation (also used by integration tests).
    pub fn connect_with_tls(
        host: &str,
        port: u16,
        credential: &Credential,
        tls: Option<TlsConnector>,
    ) -> Result<Self> {
        validate(&credential.username)?;
        validate(credential.login_password())?;
        ensure!(
            credential.kind == "password" || !credential.login_password().is_empty(),
            "FTP benötigt einen Zugang mit Benutzerpasswort; SSH-Schlüssel und SSH-Agent gelten nur für SSH"
        );
        let mut socket = None;
        for addr in (host, port)
            .to_socket_addrs()
            .context("FTP-Host nicht gefunden")?
        {
            if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(10)) {
                socket = Some(stream);
                break;
            }
        }
        let socket = socket.context("FTP-Verbindung fehlgeschlagen (10 Sekunden je Adresse)")?;
        socket.set_read_timeout(Some(Duration::from_secs(30)))?;
        socket.set_write_timeout(Some(Duration::from_secs(30)))?;
        let peer = socket.peer_addr()?;
        let mut stream = NativeTlsFtpStream::connect_with_stream(socket)?.passive_stream_builder(
            move |mut address| {
                // Passive replies may contain private/NAT or unrelated addresses.
                // Both data and control connections always use the connected server.
                address.set_ip(peer.ip());
                let socket = TcpStream::connect_timeout(&address, Duration::from_secs(10))
                    .map_err(FtpError::ConnectionError)?;
                socket
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .map_err(FtpError::ConnectionError)?;
                socket
                    .set_write_timeout(Some(Duration::from_secs(30)))
                    .map_err(FtpError::ConnectionError)?;
                Ok(socket)
            },
        );
        stream.set_passive_nat_workaround(true);
        stream.set_mode(if peer.is_ipv6() {
            Mode::ExtendedPassive
        } else {
            Mode::Passive
        });
        if let Some(tls) = tls {
            stream = stream
                .into_secure(NativeTlsConnector::from(tls), host)
                .context("FTPS-TLS-Verbindung oder Zertifikatsprüfung fehlgeschlagen")?;
        }
        // Never retry FTPS as plaintext when AUTH TLS or a certificate fails.
        stream
            .login(credential.username.as_str(), credential.login_password())
            .context("FTP-Anmeldung fehlgeschlagen")?;
        stream.transfer_type(FileType::Binary)?;
        let _ = stream.opts("UTF8", Some("ON"));
        let home = stream.pwd()?;
        validate_path(&home)?;
        Ok(Self {
            stream,
            home,
            machine_listing: true,
        })
    }

    pub fn home(&self) -> &str {
        &self.home
    }

    pub fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
        validate_path(path)?;
        self.stream
            .cwd(path)
            .context("FTP-Ordner nicht erreichbar")?;
        let lines = if self.machine_listing {
            match self.stream.mlsd(None) {
                Ok(lines) => lines,
                Err(FtpError::UnexpectedResponse(response))
                    if [500, 501, 502, 504].contains(&response.status.code()) =>
                {
                    self.machine_listing = false;
                    self.stream.list(None)?
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            self.stream.list(None)?
        };
        ensure!(
            lines.len() <= 100_000,
            "FTP-Verzeichnis enthält zu viele Einträge"
        );
        let mut entries = Vec::new();
        for line in lines {
            if line.trim().is_empty() || (!self.machine_listing && line.starts_with("total ")) {
                continue;
            }
            let file = if self.machine_listing {
                ListParser::parse_mlsd(&line)
            } else {
                File::try_from(line.as_str())
            }
            .context("FTP-Verzeichnisformat wird nicht unterstützt")?;
            if [".", ".."].contains(&file.name()) {
                continue;
            }
            validate(file.name())?;
            ensure!(
                !file.name().is_empty() && !file.name().contains('/'),
                "Ungültiger FTP-Dateiname"
            );
            entries.push(RemoteEntry {
                name: file.name().into(),
                path: join(path, file.name()),
                is_dir: file.is_directory(),
                is_file: file.is_file(),
                is_link: file.is_symlink(),
                size: file.size() as u64,
                modified: file
                    .modified()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                permissions: String::new(),
            });
        }
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(entries)
    }

    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        validate_path(path)?;
        self.stream
            .mkdir(path)
            .context("FTP-Ordner konnte nicht angelegt werden")?;
        Ok(())
    }

    pub fn upload(&mut self, local: &Path, remote: &str) -> Result<u64> {
        self.upload_controlled(local, remote, &crate::transfer::Control::default())
    }

    pub fn upload_controlled(
        &mut self,
        local: &Path,
        remote: &str,
        control: &crate::transfer::Control,
    ) -> Result<u64> {
        control.check()?;
        validate_path(remote)?;
        let (parent, name) = remote.rsplit_once('/').context("Ungültiger FTP-Pfad")?;
        ensure!(
            !name.is_empty() && name != "." && name != "..",
            "Ungültiger FTP-Dateiname"
        );
        let parent = if parent.is_empty() { "/" } else { parent };
        let mut input = control.reader(crate::transfer::open_regular(local)?);
        self.ensure_absent(parent, name)?;
        // Publish only a completely transferred file. FTP has no portable atomic
        // create-if-absent operation; check again immediately before RNTO.
        let temporary = join(parent, &format!(".noryn-upload-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut data = self.stream.put_with_stream(&temporary)?;
            let copied = std::io::copy(&mut input, &mut data);
            let finished = data.finish();
            let size = copied?;
            finished.context("FTP-Upload wurde vom Server nicht vollständig bestätigt")?;
            control.check()?;
            self.ensure_absent(parent, name)?;
            self.stream.rename(temporary.as_str(), remote)?;
            Ok(size)
        })();
        if result.is_err() {
            let _ = self.stream.rm(&temporary);
        }
        result
    }

    fn ensure_absent(&mut self, parent: &str, name: &str) -> Result<()> {
        ensure!(
            !self.list(parent)?.iter().any(|entry| entry.name == name),
            "Remote-Datei existiert bereits; Upload wurde nicht überschrieben"
        );
        Ok(())
    }

    pub fn download_file(&mut self, remote: &str, local: &Path, overwrite: bool) -> Result<u64> {
        self.download_controlled(
            remote,
            local,
            overwrite,
            &crate::transfer::Control::default(),
        )
    }

    pub fn download_controlled(
        &mut self,
        remote: &str,
        local: &Path,
        overwrite: bool,
        control: &crate::transfer::Control,
    ) -> Result<u64> {
        control.check()?;
        validate_path(remote)?;
        let parent = local.parent().context("Ungültiger lokaler Pfad")?;
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        let mut data = self.stream.retr_as_stream(remote)?;
        let result = std::io::copy(&mut data, &mut control.writer(output.as_file_mut()));
        let finished = data.finish();
        let size = result?;
        finished.context("FTP-Download wurde vom Server nicht vollständig bestätigt")?;
        control.check()?;
        output.as_file().sync_all()?;
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
}

fn join(parent: &str, name: &str) -> String {
    format!("{}/{name}", parent.trim_end_matches('/'))
}
fn validate(value: &str) -> Result<()> {
    ensure!(
        !value.contains(['\r', '\n', '\0']),
        "FTP unterstützt keine Zeilenumbrüche oder NUL in Zugangsdaten und Pfaden"
    );
    Ok(())
}
fn validate_path(path: &str) -> Result<()> {
    validate(path)?;
    if !path.starts_with('/') {
        bail!("FTP-Pfad muss absolut sein");
    }
    Ok(())
}
