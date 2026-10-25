//! Explicit file-manager mutations with bounded preflight and no-clobber destinations.
use crate::{ssh::RemoteEntry, transfer::Control};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Copy,
    Move,
    Delete,
    NewFile,
    NewFolder,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub action: Action,
    pub source: String,
    pub destination: String,
    pub local: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub fingerprint: String,
    pub files: usize,
    pub directories: usize,
    pub links: usize,
    pub bytes: u64,
}
#[derive(Clone, Serialize)]
struct Item {
    entry: RemoteEntry,
    relative: Vec<String>,
}
pub trait Files {
    fn stat(&mut self, path: &str) -> Result<RemoteEntry>;
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>>;
    fn mkdir(&mut self, path: &str) -> Result<()>;
    fn new_file(&mut self, path: &str) -> Result<()>;
    fn unlink(&mut self, path: &str) -> Result<()>;
    fn rmdir(&mut self, path: &str) -> Result<()>;
    fn rename(&mut self, source: &str, target: &str) -> Result<()>;
    fn copy_file(&mut self, source: &str, target: &str, control: &Control) -> Result<()>;
}
fn split(path: &str, local: bool) -> Result<(String, String)> {
    ensure!(
        !path.contains(['\0', '\r', '\n']) && path.len() <= 32768,
        "Ungültiger Dateipfad"
    );
    if local {
        let p = Path::new(path);
        ensure!(
            p.is_absolute()
                && !p.components().any(|c| matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )),
            "Absoluten Pfad ohne Punktsegmente verwenden"
        );
        Ok((
            p.parent()
                .context("Dateisystemwurzel ist kein Aktionsziel")?
                .to_string_lossy()
                .into(),
            p.file_name()
                .and_then(|n| n.to_str())
                .context("Ungültiger Dateiname")?
                .into(),
        ))
    } else {
        ensure!(
            path.starts_with('/')
                && !path.ends_with('/')
                && path
                    .split('/')
                    .skip(1)
                    .all(|n| !n.is_empty() && n != "." && n != ".."),
            "Absoluten Remote-Pfad ohne Punktsegmente verwenden; die Wurzel ist kein Aktionsziel"
        );
        let (p, n) = path.rsplit_once('/').unwrap();
        Ok((if p.is_empty() { "/".into() } else { p.into() }, n.into()))
    }
}
fn join(parent: &str, name: &str, local: bool) -> String {
    if local {
        Path::new(parent).join(name).to_string_lossy().into()
    } else {
        format!("{}/{name}", parent.trim_end_matches('/'))
    }
}
fn child(source: &str, target: &str, local: bool) -> bool {
    if local {
        Path::new(target).starts_with(source)
    } else {
        target == source || target.starts_with(&format!("{}/", source.trim_end_matches('/')))
    }
}
fn plan(fs: &mut (impl Files + ?Sized), request: &Request, control: &Control) -> Result<Vec<Item>> {
    control.check()?;
    if matches!(request.action, Action::NewFile | Action::NewFolder) {
        let (parent, _) = split(&request.destination, request.local)?;
        let p = fs.stat(&parent)?;
        ensure!(
            p.is_dir && !p.is_link,
            "Zielordner fehlt oder ist eine Verknüpfung"
        );
        absent(fs, &request.destination)?;
        return Ok(vec![Item {
            entry: p,
            relative: vec![],
        }]);
    }
    split(&request.source, request.local)?;
    let root = fs.stat(&request.source)?;
    ensure!(
        root.is_file || root.is_dir || root.is_link,
        "Spezialdateien sind kein Aktionsziel"
    );
    if request.action == Action::Copy {
        ensure!(
            !root.is_link,
            "Verknüpfungen können nicht als Kopierwurzel verwendet werden"
        );
    }
    if matches!(request.action, Action::Copy | Action::Move) {
        let (parent, _) = split(&request.destination, request.local)?;
        ensure!(
            !child(&request.source, &request.destination, request.local),
            "Ziel darf weder Quelle noch ein Unterordner der Quelle sein"
        );
        let p = fs.stat(&parent)?;
        ensure!(
            p.is_dir && !p.is_link,
            "Zielordner fehlt oder ist eine Verknüpfung"
        );
        absent(fs, &request.destination)?;
    }
    let mut items = vec![Item {
        entry: root,
        relative: vec![],
    }];
    let mut index = 0;
    let mut names_bytes = 0;
    while index < items.len() {
        control.check()?;
        let item = items[index].clone();
        index += 1;
        if request.action == Action::Move || !item.entry.is_dir || item.entry.is_link {
            continue;
        }
        ensure!(
            item.relative.len() < 64,
            "Dateibaum ist zu tief (höchstens 64 Ebenen)"
        );
        let mut names = std::collections::HashSet::new();
        for mut entry in fs.list(&item.entry.path)? {
            crate::transfer::local_component(&entry.name)?;
            ensure!(
                names.insert(entry.name.clone()),
                "Doppelter Name im Dateibaum"
            );
            ensure!(
                entry.is_dir || entry.is_file || entry.is_link,
                "Spezialdatei im Dateibaum; Aktion wurde vor Änderungen abgebrochen"
            );
            names_bytes += item.entry.path.len() + entry.name.len() + 1;
            ensure!(
                items.len() < 100000 && names_bytes <= 16 * 1024 * 1024,
                "Dateibaum ist zu groß"
            );
            let mut relative = item.relative.clone();
            relative.push(entry.name.clone());
            entry.path = join(&item.entry.path, &entry.name, request.local);
            items.push(Item { entry, relative });
        }
    }
    Ok(items)
}
fn absent(fs: &mut (impl Files + ?Sized), path: &str) -> Result<()> {
    // Stat errors must not be mistaken for non-existence. List the existing parent.
    let local = Path::new(path).is_absolute() && !path.starts_with('/');
    let (parent, name) = split(path, local)?;
    ensure!(
        !fs.list(&parent)?.iter().any(|e| e.name == name),
        "Ziel existiert bereits; es wird nichts überschrieben oder zusammengeführt"
    );
    Ok(())
}
fn describe(request: &Request, items: &[Item]) -> Result<Preview> {
    let mut fingerprint_items = items.to_vec();
    // Listing order is not a filesystem identity. Hash canonical paths and metadata.
    fingerprint_items.sort_by(|a, b| a.entry.path.cmp(&b.entry.path));
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(request, fingerprint_items))?)
    );
    Ok(Preview {
        fingerprint,
        files: items
            .iter()
            .filter(|i| i.entry.is_file && !i.entry.is_link)
            .count(),
        directories: items
            .iter()
            .filter(|i| i.entry.is_dir && !i.entry.is_link)
            .count(),
        links: items.iter().filter(|i| i.entry.is_link).count(),
        bytes: items
            .iter()
            .filter(|i| i.entry.is_file && !i.entry.is_link)
            .try_fold(0u64, |sum, i| {
                sum.checked_add(i.entry.size)
                    .context("Dateibaum ist zu groß")
            })?,
    })
}
pub fn preview(
    fs: &mut (impl Files + ?Sized),
    request: &Request,
    control: &Control,
) -> Result<Preview> {
    let items = plan(fs, request, control)?;
    describe(request, &items)
}
pub fn execute(
    fs: &mut (impl Files + ?Sized),
    request: &Request,
    expected: &str,
    control: &Control,
) -> Result<Preview> {
    let items = plan(fs, request, control)?;
    let result = describe(request, &items)?;
    ensure!(
        result.fingerprint == expected,
        "Dateibaum wurde seit der Vorschau geändert; bitte erneut prüfen"
    );
    control.check()?;
    match request.action {
        Action::NewFile => fs.new_file(&request.destination)?,
        Action::NewFolder => fs.mkdir(&request.destination)?,
        Action::Move => fs.rename(&request.source, &request.destination)?,
        Action::Copy => {
            for item in &items {
                control.check()?;
                if item.entry.is_link {
                    continue;
                }
                verify(fs, request, &items, item)?;
                let target = item
                    .relative
                    .iter()
                    .fold(request.destination.clone(), |p, n| {
                        join(&p, n, request.local)
                    });
                if item.entry.is_dir {
                    fs.mkdir(&target)?;
                } else {
                    fs.copy_file(&item.entry.path, &target, control)?;
                }
            }
        }
        Action::Delete => {
            for item in items.iter().rev() {
                control.check()?;
                verify(fs, request, &items, item)?;
                if item.entry.is_dir && !item.entry.is_link {
                    fs.rmdir(&item.entry.path)?;
                } else {
                    fs.unlink(&item.entry.path)?;
                }
            }
        }
    }
    Ok(result)
}
fn verify(
    fs: &mut (impl Files + ?Sized),
    request: &Request,
    items: &[Item],
    item: &Item,
) -> Result<()> {
    for parent in items.iter().filter(|p| {
        p.entry.path != item.entry.path
            && p.entry.is_dir
            && !p.entry.is_link
            && child(&p.entry.path, &item.entry.path, request.local)
    }) {
        let now = fs.stat(&parent.entry.path)?;
        ensure!(
            now.is_dir && !now.is_link,
            "Quellordner wurde während der Aktion ersetzt"
        );
    }
    let now = fs.stat(&item.entry.path)?;
    let old = &item.entry;
    ensure!(
        now.is_link == old.is_link
            && now.is_dir == old.is_dir
            && now.is_file == old.is_file
            && (old.is_dir || now.size == old.size && now.modified == old.modified),
        "Datei wurde während der Aktion geändert; Teiländerungen bleiben erhalten"
    );
    Ok(())
}
pub struct Local;
impl Files for Local {
    fn stat(&mut self, path: &str) -> Result<RemoteEntry> {
        let meta = std::fs::symlink_metadata(path)?;
        Ok(RemoteEntry {
            name: Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            path: path.into(),
            is_dir: meta.is_dir(),
            is_link: meta.file_type().is_symlink(),
            is_file: meta.is_file(),
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
            permissions: String::new(),
        })
    }
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
        std::fs::read_dir(path)?
            .map(|entry| self.stat(entry?.path().to_str().context("Ungültiger Dateipfad")?))
            .collect()
    }
    fn mkdir(&mut self, path: &str) -> Result<()> {
        std::fs::create_dir(path)?;
        Ok(())
    }
    fn new_file(&mut self, path: &str) -> Result<()> {
        new_local(path)?.sync_all()?;
        Ok(())
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            if std::fs::symlink_metadata(path)?
                .file_type()
                .is_symlink_dir()
            {
                std::fs::remove_dir(path)?;
                return Ok(());
            }
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    fn rmdir(&mut self, path: &str) -> Result<()> {
        std::fs::remove_dir(path)?;
        Ok(())
    }
    fn rename(&mut self, source: &str, target: &str) -> Result<()> {
        rename_local(Path::new(source), Path::new(target))
    }
    fn copy_file(&mut self, source: &str, target: &str, control: &Control) -> Result<()> {
        let source = crate::transfer::open_regular(Path::new(source))?;
        let mut target = new_local(target)?;
        std::io::copy(&mut control.reader(source), &mut target)?;
        target.flush()?;
        target.sync_all()?;
        Ok(())
    }
}
fn new_local(path: &str) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
fn rename_local(source: &Path, target: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let s = std::ffi::CString::new(source.as_os_str().as_bytes())?;
        let t = std::ffi::CString::new(target.as_os_str().as_bytes())?;
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                s.as_ptr(),
                libc::AT_FDCWD,
                t.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let s = std::ffi::CString::new(source.as_os_str().as_bytes())?;
        let t = std::ffi::CString::new(target.as_os_str().as_bytes())?;
        if unsafe { libc::renamex_np(s.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(old: *const u16, new: *const u16, flags: u32) -> i32;
        }
        let s: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let t: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(s.as_ptr(), t.as_ptr(), 0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}
pub struct Remote<'a>(pub &'a mut crate::remote::Remote);
impl Files for Remote<'_> {
    fn stat(&mut self, path: &str) -> Result<RemoteEntry> {
        if path == "/" {
            self.0.list("/")?;
            return Ok(RemoteEntry {
                name: "/".into(),
                path: "/".into(),
                is_dir: true,
                is_file: false,
                is_link: false,
                size: 0,
                modified: 0,
                permissions: String::new(),
            });
        }
        let (parent, name) = split(path, false)?;
        self.0
            .list(&parent)?
            .into_iter()
            .find(|e| e.name == name)
            .context("Datei oder Ordner existiert nicht")
    }
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
        self.0.list(path)
    }
    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.0.mkdir(path)
    }
    fn new_file(&mut self, path: &str) -> Result<()> {
        let temp = tempfile::NamedTempFile::new()?;
        self.0.upload(temp.path(), path)?;
        Ok(())
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        self.0.remove_file(path)
    }
    fn rmdir(&mut self, path: &str) -> Result<()> {
        self.0.remove_dir(path)
    }
    fn rename(&mut self, source: &str, target: &str) -> Result<()> {
        self.0.rename(source, target)
    }
    fn copy_file(&mut self, source: &str, target: &str, control: &Control) -> Result<()> {
        let temporary = tempfile::NamedTempFile::new()?.into_temp_path();
        self.0
            .download_controlled(source, &temporary, true, control)?;
        control.check()?;
        self.0.upload_controlled(&temporary, target, control)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(action: Action, source: &Path, destination: &Path) -> Request {
        Request {
            action,
            source: source.to_string_lossy().into(),
            destination: destination.to_string_lossy().into(),
            local: true,
        }
    }
    fn apply(request: &Request) -> Result<Preview> {
        let preview = preview(&mut Local, request, &Control::default())?;
        execute(
            &mut Local,
            request,
            &preview.fingerprint,
            &Control::default(),
        )
    }
    #[test]
    fn recursive_actions_reject_stale_previews_existing_targets_and_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source 'quote';");
        std::fs::create_dir_all(source.join("nested/empty")).unwrap();
        std::fs::write(source.join("nested/a.txt"), b"example").unwrap();
        let target = temp.path().join("copy");
        let copy = request(Action::Copy, &source, &target);
        let before = preview(&mut Local, &copy, &Control::default()).unwrap();
        assert_eq!((before.files, before.directories, before.bytes), (1, 3, 7));
        std::fs::write(source.join("new"), b"x").unwrap();
        assert!(execute(&mut Local, &copy, &before.fingerprint, &Control::default()).is_err());
        assert!(!target.exists());
        let before = preview(&mut Local, &copy, &Control::default()).unwrap();
        let cancel = Control::default();
        cancel.cancel();
        assert!(execute(&mut Local, &copy, &before.fingerprint, &cancel).is_err());
        assert!(!target.exists());
        apply(&copy).unwrap();
        assert!(target.join("nested/empty").is_dir());
        assert_eq!(
            std::fs::read(target.join("nested/a.txt")).unwrap(),
            b"example"
        );
        assert!(apply(&copy).is_err());
        let moved = temp.path().join("moved");
        apply(&request(Action::Move, &target, &moved)).unwrap();
        assert!(!target.exists());
        assert!(Local
            .rename(moved.to_str().unwrap(), source.to_str().unwrap())
            .is_err());
        assert!(source.join("nested/a.txt").exists());
        apply(&request(Action::Delete, &moved, Path::new(""))).unwrap();
        assert!(!moved.exists());
        assert!(apply(&request(Action::Copy, &source, &source.join("subtree"))).is_err());
        let file = temp.path().join("empty.txt");
        apply(&request(Action::NewFile, Path::new(""), &file)).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"");
        assert!(apply(&request(Action::NewFile, Path::new(""), &file)).is_err());
        assert!(split(if cfg!(windows) { "C:\\" } else { "/" }, true).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn nested_links_are_never_traversed_and_special_files_fail_before_writing() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"private").unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        symlink(&outside, source.join("link")).unwrap();
        let target = temp.path().join("copy");
        let report = apply(&request(Action::Copy, &source, &target)).unwrap();
        assert_eq!(report.links, 1);
        assert!(!target.join("link").exists());
        apply(&request(Action::Delete, &source, Path::new(""))).unwrap();
        assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"private");
        std::fs::create_dir(&source).unwrap();
        let socket = std::os::unix::net::UnixListener::bind(source.join("socket")).unwrap();
        assert!(apply(&request(Action::Copy, &source, &temp.path().join("failed"))).is_err());
        assert!(!temp.path().join("failed").exists());
        drop(socket);
    }
}
