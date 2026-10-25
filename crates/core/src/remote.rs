use crate::{ftp, ssh};
use anyhow::{bail, Result};
use std::path::Path;

pub enum Remote {
    Ssh(ssh::Remote),
    Ftp(ftp::Remote),
}
impl Remote {
    pub fn read_text(&mut self, path: &str) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        match self {
            Self::Ssh(remote) => remote.read_text(path),
            Self::Ftp(remote) => remote.read_text(path),
        }
    }
    pub fn replace_text(
        &mut self,
        path: &str,
        expected: &crate::text_files::DigestBytes,
        bytes: &[u8],
        control: &crate::transfer::Control,
    ) -> Result<()> {
        match self {
            Self::Ssh(remote) => remote.replace_text(path, expected, bytes, control),
            Self::Ftp(remote) => remote.replace_text(path, expected, bytes, control),
        }
    }
    pub fn list(&mut self, path: &str) -> Result<Vec<ssh::RemoteEntry>> {
        match self {
            Self::Ssh(remote) => remote.list(path),
            Self::Ftp(remote) => remote.list(path),
        }
    }
    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        match self {
            Self::Ssh(remote) => remote.mkdir(path),
            Self::Ftp(remote) => remote.mkdir(path),
        }
    }
    pub fn upload(&mut self, local: &Path, path: &str) -> Result<u64> {
        match self {
            Self::Ssh(remote) => remote.upload(local, path),
            Self::Ftp(remote) => remote.upload(local, path),
        }
    }
    pub fn download_file(&mut self, path: &str, local: &Path, overwrite: bool) -> Result<u64> {
        match self {
            Self::Ssh(remote) => remote.download_file(path, local, overwrite),
            Self::Ftp(remote) => remote.download_file(path, local, overwrite),
        }
    }
    pub fn upload_controlled(
        &mut self,
        local: &Path,
        path: &str,
        control: &crate::transfer::Control,
    ) -> Result<u64> {
        match self {
            Self::Ssh(remote) => remote.upload_controlled(local, path, control),
            Self::Ftp(remote) => remote.upload_controlled(local, path, control),
        }
    }
    pub fn download_controlled(
        &mut self,
        path: &str,
        local: &Path,
        overwrite: bool,
        control: &crate::transfer::Control,
    ) -> Result<u64> {
        match self {
            Self::Ssh(remote) => remote.download_controlled(path, local, overwrite, control),
            Self::Ftp(remote) => remote.download_controlled(path, local, overwrite, control),
        }
    }
    pub fn sudo_auto(
        &mut self,
        enabled: bool,
        password: Option<String>,
    ) -> Result<ssh::SudoOutcome> {
        match self {
            Self::Ssh(remote) => remote.sudo_auto(enabled, password),
            Self::Ftp(_) => bail!("sudo ist nur über SSH/SFTP verfügbar"),
        }
    }
}
