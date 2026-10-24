use crate::{
    crypto::{self, Envelope},
    model::{validate_nodes, VaultData},
};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VaultFile {
    salt: String,
    sealed: Envelope,
}

pub struct Vault {
    pub data: VaultData,
    key: Zeroizing<[u8; 32]>,
    salt: [u8; 16],
    path: PathBuf,
}

impl Vault {
    pub fn open(path: &Path, password: &str) -> Result<Self> {
        ensure!(!password.is_empty(), "Tresor-Passwort fehlt");
        if path.exists() {
            ensure!(
                fs::metadata(path)?.len() <= 64 * 1024 * 1024,
                "Tresor-Datei ist zu groß"
            );
            let file: VaultFile = serde_json::from_slice(&fs::read(path)?)?;
            let salt: [u8; 16] = STANDARD
                .decode(file.salt)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Ungültiges Salz"))?;
            let key = crypto::derive_key(password, &salt)?;
            let plaintext =
                crypto::unseal(&key, &file.sealed, b"noryn:vault:v1").or_else(|_| {
                    crypto::unseal(&key, &file.sealed, crate::migration::PREVIOUS_VAULT_CONTEXT)
                })?;
            let data: VaultData = serde_json::from_slice(&plaintext)?;
            validate_nodes(&data.nodes)?;
            crate::credential_folders::validate(&data)?;
            for changes in data.team_activity.values() {
                crate::team_activity::validate_changes(changes)?;
            }
            Ok(Self {
                data,
                key,
                salt,
                path: path.to_path_buf(),
            })
        } else {
            let vault = Self::empty(path, password, 0)?;
            vault.save()?;
            Ok(vault)
        }
    }

    fn empty(path: &Path, password: &str, revision: u64) -> Result<Self> {
        ensure!(
            password.chars().count() >= 12,
            "Mindestens 12 Zeichen für einen neuen Tresor verwenden"
        );
        let salt = crypto::random_bytes();
        Ok(Self {
            data: VaultData {
                local_revision: revision,
                ..VaultData::default()
            },
            key: crypto::derive_key(password, &salt)?,
            salt,
            path: path.to_path_buf(),
        })
    }

    /// Explicit destructive replacement; never tries to decrypt or migrate the old file.
    /// An existing legacy migration backup is left untouched. The fresh primary vault
    /// prevents it from being automatically loaded after a reset.
    pub fn reset(
        path: &Path,
        previous: &Path,
        expected: &ResetSnapshot,
        password: &str,
        revision: u64,
    ) -> Result<Self> {
        ensure!(revision > 0, "Ungültige neue Tresor-Revision");
        ensure!(
            ResetSnapshot::inspect(path, previous)? == *expected,
            "Tresor-Datei wurde seit der Bestätigung geändert. Dialog erneut öffnen."
        );
        let fresh = Self::empty(path, password, revision)?;
        // KDF work takes time: check again immediately before atomic publication.
        ensure!(
            ResetSnapshot::inspect(path, previous)? == *expected,
            "Tresor-Datei wurde seit der Bestätigung geändert. Dialog erneut öffnen."
        );
        fresh.save()?;
        Ok(fresh)
    }

    pub fn save(&self) -> Result<()> {
        let plaintext = Zeroizing::new(serde_json::to_vec(&self.data)?);
        let file = VaultFile {
            salt: STANDARD.encode(self.salt),
            sealed: crypto::seal(&self.key, &plaintext, b"noryn:vault:v1")?,
        };
        let parent = self.path.parent().context("Tresor-Pfad ungültig")?;
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        let bytes = serde_json::to_vec(&file)?;
        ensure!(bytes.len() <= 64 * 1024 * 1024, "Tresor-Datei ist zu groß");
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }

    pub fn open_migrating(path: &Path, previous: &Path, password: &str) -> Result<Self> {
        if path.exists() || !previous.exists() {
            return Self::open(path, password);
        }
        let mut vault = Self::open(previous, password)?;
        vault.path = path.to_path_buf();
        vault.save()?;
        Ok(vault)
    }

    pub fn change<T>(&mut self, action: impl FnOnce(&mut VaultData) -> Result<T>) -> Result<T> {
        let previous = self.data.clone();
        let next_revision = self
            .data
            .local_revision
            .checked_add(1)
            .context("Lokale Tresor-Revision ist ausgeschöpft")?;
        match action(&mut self.data).and_then(|result| {
            self.data.local_revision = next_revision;
            self.save()?;
            Ok(result)
        }) {
            Ok(result) => Ok(result),
            Err(error) => {
                self.data = previous;
                Err(error)
            }
        }
    }
}

/// Ciphertext fingerprints stay native; no passwords or plaintext are needed for reset.
#[derive(Clone, PartialEq, Eq)]
pub struct ResetSnapshot {
    current: Option<[u8; 32]>,
    previous: Option<[u8; 32]>,
}
impl ResetSnapshot {
    pub fn inspect(path: &Path, previous: &Path) -> Result<Self> {
        let snapshot = Self {
            current: ciphertext_fingerprint(path)?,
            previous: ciphertext_fingerprint(previous)?,
        };
        ensure!(
            snapshot.current.is_some() || snapshot.previous.is_some(),
            "Kein lokaler Tresor vorhanden"
        );
        Ok(snapshot)
    }
}
fn ciphertext_fingerprint(path: &Path) -> Result<Option<[u8; 32]>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Tresor-Pfad ist keine reguläre Datei"
    );
    ensure!(
        metadata.len() <= 64 * 1024 * 1024,
        "Tresor-Datei ist zu groß"
    );
    let mut file = fs::File::open(path)?.take(64 * 1024 * 1024 + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count;
        ensure!(size <= 64 * 1024 * 1024, "Tresor-Datei ist zu groß");
        hash.update(&buffer[..count]);
    }
    Ok(Some(hash.finalize().into()))
}
