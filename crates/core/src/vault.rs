use crate::{
    crypto::{self, Envelope},
    model::{validate_nodes, VaultData},
};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
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
            let plaintext = crypto::unseal(&key, &file.sealed, b"harbor:vault:v1")?;
            let data: VaultData = serde_json::from_slice(&plaintext)?;
            validate_nodes(&data.nodes)?;
            Ok(Self {
                data,
                key,
                salt,
                path: path.to_path_buf(),
            })
        } else {
            ensure!(
                password.chars().count() >= 12,
                "Mindestens 12 Zeichen für einen neuen Tresor verwenden"
            );
            let salt = crypto::random_bytes();
            let key = crypto::derive_key(password, &salt)?;
            let vault = Self {
                data: VaultData::default(),
                key,
                salt,
                path: path.to_path_buf(),
            };
            vault.save()?;
            Ok(vault)
        }
    }

    pub fn save(&self) -> Result<()> {
        let plaintext = Zeroizing::new(serde_json::to_vec(&self.data)?);
        let file = VaultFile {
            salt: STANDARD.encode(self.salt),
            sealed: crypto::seal(&self.key, &plaintext, b"harbor:vault:v1")?,
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
        temp.write_all(&serde_json::to_vec(&file)?)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }

    pub fn change<T>(&mut self, action: impl FnOnce(&mut VaultData) -> Result<T>) -> Result<T> {
        let previous = self.data.clone();
        match action(&mut self.data).and_then(|result| {
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
