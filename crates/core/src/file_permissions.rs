//! Explicit Unix metadata changes. Recursive plans are opt-in; no ACL traversal.
pub mod tree;
#[cfg(unix)]
use anyhow::Context;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const STALE: &str = "Datei oder Berechtigungen wurden inzwischen geändert. Bitte erneut laden.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}
impl Change {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.mode.is_some() || self.uid.is_some() || self.gid.is_some(),
            "Keine Berechtigungsänderung angegeben"
        );
        ensure!(
            self.mode.is_none_or(|mode| mode <= 0o7777),
            "Ungültige oktale Dateirechte"
        );
        ensure!(
            self.uid != Some(u32::MAX) && self.gid != Some(u32::MAX),
            "Ungültige UID oder GID"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub path: String,
    pub is_dir: bool,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub can_chmod: bool,
    pub can_chown: bool,
    pub fingerprint: String,
}
impl Snapshot {
    pub(crate) fn new(
        path: &str,
        is_dir: bool,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        capabilities: (bool, bool),
        identity: impl Serialize,
    ) -> Result<Self> {
        validate_path(path)?;
        Ok(Self {
            path: path.into(),
            is_dir,
            mode,
            uid,
            gid,
            can_chmod: capabilities.0,
            can_chown: capabilities.1,
            fingerprint: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(
                    path, is_dir, mode, uid, gid, identity
                ))?)
            ),
        })
    }
    pub(crate) fn check(&self, expected: &str, change: &Change) -> Result<()> {
        change.validate()?;
        ensure!(self.fingerprint == expected, STALE);
        ensure!(
            change.mode.is_none() || self.can_chmod,
            "chmod wird für dieses Dateisystem nicht unterstützt"
        );
        ensure!(
            (change.uid.is_none() && change.gid.is_none()) || self.can_chown,
            "Eigentümerwechsel wird für dieses Dateisystem nicht unterstützt"
        );
        Ok(())
    }
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 32768 && !path.contains('\0'),
        "Ungültiger Dateipfad"
    );
    // A trailing slash can make lstat follow a leaf symlink on Unix.
    ensure!(
        !path.ends_with('/'),
        "Bitte einen einzelnen Datei- oder Ordnerpfad wählen"
    );
    ensure!(
        ![".", ".."].contains(&path.rsplit('/').next().unwrap_or_default()),
        "Ungültiger Dateipfad"
    );
    Ok(())
}

pub fn local_read(path: &str) -> Result<Snapshot> {
    validate_path(path)?;
    #[cfg(windows)]
    validate_path(&path.replace('\\', "/"))?;
    ensure!(
        std::path::Path::new(path).is_absolute(),
        "Dateipfad muss absolut sein"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() || metadata.is_dir(),
        "Berechtigungen sind nur für reguläre Dateien und Ordner verfügbar; keine Verknüpfungen"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Snapshot::new(
            path,
            metadata.is_dir(),
            Some(metadata.mode() & 0o7777),
            Some(metadata.uid()),
            Some(metadata.gid()),
            (true, true),
            (
                "local",
                metadata.dev(),
                metadata.ino(),
                metadata.mode(),
                metadata.len(),
                metadata.mtime(),
                metadata.mtime_nsec(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        )
    }
    #[cfg(not(unix))]
    {
        Snapshot::new(
            path,
            metadata.is_dir(),
            None,
            None,
            None,
            (false, false),
            (
                "local",
                metadata.len(),
                metadata.is_dir(),
                metadata
                    .modified()?
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos(),
            ),
        )
    }
}

pub fn local_apply(
    path: &str,
    expected: &str,
    change: &Change,
    control: &crate::transfer::Control,
) -> Result<Snapshot> {
    control.check()?;
    local_read(path)?.check(expected, change)?;
    #[cfg(unix)]
    {
        let path_c = std::ffi::CString::new(path)?;
        // AT_SYMLINK_NOFOLLOW prevents a replaced leaf from redirecting changes
        // to its target. Never fall back to an operation which follows links.
        if change.uid.is_some() || change.gid.is_some() {
            control.check()?;
            let result = unsafe {
                libc::fchownat(
                    libc::AT_FDCWD,
                    path_c.as_ptr(),
                    change.uid.unwrap_or(u32::MAX),
                    change.gid.unwrap_or(u32::MAX),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if result != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("Eigentümer oder Gruppe konnten nicht geändert werden");
            }
        }
        if let Some(mode) = change.mode {
            control.check()?;
            let result = unsafe {
                libc::fchmodat(
                    libc::AT_FDCWD,
                    path_c.as_ptr(),
                    mode as libc::mode_t,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if result != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("Dateirechte konnten nicht geändert werden");
            }
        }
        local_read(path)
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!("Unix-Berechtigungen sind lokal unter Windows nicht verfügbar")
    }
}

pub(crate) fn posix_mode(value: &str) -> Option<u32> {
    let bits = value.as_bytes().get(..10)?;
    if !b"-d".contains(&bits[0]) {
        return None;
    }
    let mut mode = 0;
    for (index, byte) in bits[1..].iter().enumerate() {
        let allowed: &[u8] = match index % 3 {
            0 => b"r-",
            1 => b"w-",
            _ if index == 8 => b"xtT-",
            _ => b"xsS-",
        };
        if !allowed.contains(byte) {
            return None;
        }
        if matches!(byte, b'r' | b'w' | b'x' | b's' | b't') {
            mode |= 1 << (8 - index);
        }
    }
    if matches!(bits[3], b's' | b'S') {
        mode |= 0o4000;
    }
    if matches!(bits[6], b's' | b'S') {
        mode |= 0o2000;
    }
    if matches!(bits[9], b't' | b'T') {
        mode |= 0o1000;
    }
    Some(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_octal_modes_ids_paths_and_special_bits() {
        for mode in [0, 0o755, 0o4755, 0o7777] {
            Change {
                mode: Some(mode),
                uid: None,
                gid: None,
            }
            .validate()
            .unwrap();
        }
        assert!(Change {
            mode: Some(0o10000),
            uid: None,
            gid: None
        }
        .validate()
        .is_err());
        assert!(Change {
            mode: None,
            uid: Some(u32::MAX),
            gid: None
        }
        .validate()
        .is_err());
        for path in ["", "/tmp/link/", "/tmp/..", "/tmp/.", "/x\0"] {
            assert!(validate_path(path).is_err());
        }
        assert_eq!(posix_mode("drwxr-x---"), Some(0o750));
        assert_eq!(posix_mode("-rwsr-Sr-t"), Some(0o7745));
        assert_eq!(posix_mode("not-posix"), None);
    }
    #[cfg(unix)]
    #[test]
    fn local_permissions_are_non_recursive_reject_links_stale_previews_and_cancellation() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("folder '\";$(ignored)");
        std::fs::create_dir(&folder).unwrap();
        let file = folder.join("keep");
        std::fs::write(&file, b"data").unwrap();
        let child_mode = std::fs::metadata(&file).unwrap().mode();
        let path = folder.to_str().unwrap();
        let old = local_read(path).unwrap();
        let change = Change {
            mode: Some(0o750),
            uid: None,
            gid: None,
        };
        local_apply(path, &old.fingerprint, &change, &Default::default()).unwrap();
        assert_eq!(std::fs::metadata(&folder).unwrap().mode() & 0o7777, 0o750);
        assert_eq!(std::fs::metadata(&file).unwrap().mode(), child_mode);
        assert!(local_apply(path, &old.fingerprint, &change, &Default::default()).is_err());
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(local_read(link.to_str().unwrap()).is_err());
        let cancelled = crate::transfer::Control::default();
        cancelled.cancel();
        let old = local_read(file.to_str().unwrap()).unwrap();
        assert!(local_apply(
            file.to_str().unwrap(),
            &old.fingerprint,
            &change,
            &cancelled
        )
        .is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o0)).unwrap();
        let old = local_read(file.to_str().unwrap()).unwrap();
        let result = local_apply(
            file.to_str().unwrap(),
            &old.fingerprint,
            &Change {
                mode: Some(0o600),
                uid: Some(old.uid.unwrap()),
                gid: Some(old.gid.unwrap()),
            },
            &Default::default(),
        )
        .unwrap();
        assert_eq!(result.mode, Some(0o600));
        assert_eq!(std::fs::read(&file).unwrap(), b"data");
    }
}
