//! Bounded recursive transfers over an existing authenticated file connection.
use crate::ssh::RemoteEntry;
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 64;
const MAX_PATH_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub planning: bool,
    pub directory: bool,
    pub bytes: u64,
    pub total_bytes: u64,
    pub files: u64,
    pub total_files: u64,
    pub current_path: String,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub directory: bool,
    pub bytes: u64,
    pub files: u64,
    pub directories: u64,
    pub skipped: usize,
    pub skipped_paths: Vec<String>,
}

pub struct Control {
    cancelled: Arc<AtomicBool>,
    state: Mutex<(Progress, Instant)>,
    callback: Box<dyn Fn(Progress) + Send + Sync>,
}
impl Default for Control {
    fn default() -> Self {
        Self::new(|_| {})
    }
}
impl Control {
    pub fn new(callback: impl Fn(Progress) + Send + Sync + 'static) -> Self {
        Self {
            cancelled: Arc::default(),
            state: Mutex::new((Progress::default(), Instant::now())),
            callback: Box::new(callback),
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn check(&self) -> Result<()> {
        ensure!(
            !self.is_cancelled(),
            "Übertragung abgebrochen. Bereits übertragene Teile bleiben im Zielordner erhalten."
        );
        Ok(())
    }
    fn update(&self, force: bool, change: impl FnOnce(&mut Progress)) {
        let event = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            change(&mut state.0);
            if force || state.1.elapsed() >= Duration::from_millis(100) {
                state.1 = Instant::now();
                Some(state.0.clone())
            } else {
                None
            }
        };
        if let Some(event) = event {
            (self.callback)(event);
        }
    }
    fn io_check(&self) -> io::Result<()> {
        // Interrupted is retried by io::copy and cannot represent cancellation.
        if self.is_cancelled() {
            Err(io::Error::other("Übertragung abgebrochen"))
        } else {
            Ok(())
        }
    }
    pub(crate) fn reader<'a, R: Read>(&'a self, inner: R) -> CountingReader<'a, R> {
        CountingReader {
            inner,
            control: self,
        }
    }
    pub(crate) fn writer<'a, W: Write>(&'a self, inner: W) -> CountingWriter<'a, W> {
        CountingWriter {
            inner,
            control: self,
        }
    }
}
pub(crate) struct CountingReader<'a, R> {
    inner: R,
    control: &'a Control,
}
impl<R: Read> Read for CountingReader<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.control.io_check()?;
        let n = self.inner.read(bytes)?;
        self.control
            .update(false, |p| p.bytes = p.bytes.saturating_add(n as u64));
        Ok(n)
    }
}
pub(crate) struct CountingWriter<'a, W> {
    inner: W,
    control: &'a Control,
}
impl<W: Write> Write for CountingWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.control.io_check()?;
        let n = self.inner.write(bytes)?;
        self.control
            .update(false, |p| p.bytes = p.bytes.saturating_add(n as u64));
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub trait Backend {
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>>;
    fn mkdir(&mut self, path: &str) -> Result<()>;
    fn upload_file(&mut self, local: &Path, remote: &str, control: &Control) -> Result<u64>;
    fn download_file(
        &mut self,
        remote: &str,
        local: &Path,
        overwrite: bool,
        control: &Control,
    ) -> Result<u64>;
}

// Leaf links and special files must never be opened as transfer sources.
pub(crate) fn open_regular(path: &Path) -> Result<File> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "Quelle ist keine reguläre Datei: {}",
        path.display()
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    ensure!(
        opened.is_file() && !opened.file_type().is_symlink(),
        "Quelldatei wurde während der Übertragung ersetzt"
    );
    Ok(file)
}

#[derive(Clone)]
struct Item {
    relative: String,
    directory: bool,
    size: u64,
}
struct Plan {
    items: Vec<Item>,
    report: Report,
    count: usize,
    path_bytes: usize,
}
impl Plan {
    fn new(directory: bool) -> Self {
        Self {
            items: Vec::new(),
            report: Report {
                directory,
                ..Report::default()
            },
            count: 0,
            path_bytes: 0,
        }
    }
    fn add(&mut self, relative: String, directory: bool, size: u64, skip: bool) -> Result<()> {
        self.count += 1;
        self.path_bytes = self.path_bytes.saturating_add(relative.len());
        ensure!(
            self.count <= MAX_ENTRIES && self.path_bytes <= MAX_PATH_BYTES,
            "Ordnertransfer überschreitet das Limit von 100.000 Einträgen / 16 MiB Pfadnamen"
        );
        ensure!(
            relative.split('/').count() <= MAX_DEPTH,
            "Ordnertransfer überschreitet die maximale Tiefe von 64"
        );
        if skip {
            self.report.skipped += 1;
            if self.report.skipped_paths.len() < 100 {
                self.report.skipped_paths.push(relative);
            }
        } else {
            self.items.push(Item {
                relative,
                directory,
                size,
            });
        }
        Ok(())
    }
}
fn component(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0']),
        "Unsicherer Dateiname: {name:?}"
    );
    Ok(())
}
#[cfg(windows)]
pub(crate) fn local_component(name: &str) -> Result<()> {
    component(name)?;
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        !name.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c))
            && !name.ends_with(['.', ' '])
            && !["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            && !(stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9')),
        "Dateiname wird unter Windows nicht unterstützt: {name:?}"
    );
    Ok(())
}
#[cfg(not(windows))]
pub(crate) fn local_component(name: &str) -> Result<()> {
    component(name)
}
fn child(parent: &str, name: &str) -> String {
    format!("{}/{name}", parent.trim_end_matches('/'))
}
fn remote_root(path: &str) -> Result<(String, String)> {
    ensure!(
        path.starts_with('/') && !path.contains(['\0', '\\']) && path != "/",
        "Bitte einen absoluten Datei- oder Ordnerpfad wählen"
    );
    let path = path.trim_end_matches('/');
    for part in path.split('/').skip(1) {
        component(part)?;
    }
    let (parent, name) = path.rsplit_once('/').context("Ungültiger Remote-Pfad")?;
    Ok((
        if parent.is_empty() { "/" } else { parent }.into(),
        name.into(),
    ))
}
fn local_target(path: &Path) -> Result<PathBuf> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .context("Ungültiger lokaler Zielpfad")?;
    local_component(name)?;
    Ok(fs::canonicalize(path.parent().context("Zielordner fehlt")?)?.join(name))
}
fn check_local_chain(root: &Path, path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor)?;
        ensure!(
            !meta.file_type().is_symlink(),
            "Pfad wurde durch eine Verknüpfung ersetzt: {}",
            ancestor.display()
        );
        if ancestor == root {
            break;
        }
    }
    Ok(())
}
fn local_plan(path: &Path, control: &Control) -> Result<Plan> {
    let root = fs::symlink_metadata(path)?;
    ensure!(
        !root.file_type().is_symlink() && (root.is_dir() || root.is_file()),
        "Verknüpfungen und Spezialdateien können nicht als Transferquelle verwendet werden"
    );
    let mut plan = Plan::new(root.is_dir());
    plan.add(String::new(), root.is_dir(), root.len(), false)?;
    let mut pending = if root.is_dir() {
        vec![String::new()]
    } else {
        Vec::new()
    };
    while let Some(relative) = pending.pop() {
        control.check()?;
        let directory = if relative.is_empty() {
            path.to_path_buf()
        } else {
            path.join(&relative)
        };
        check_local_chain(path, &directory)?;
        for entry in fs::read_dir(directory)? {
            control.check()?;
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Nicht-UTF-8-Dateiname wird nicht unterstützt"))?;
            component(&name)?;
            let name = if relative.is_empty() {
                name
            } else {
                child(&relative, &name)
            };
            let meta = fs::symlink_metadata(entry.path())?;
            let skip = meta.file_type().is_symlink() || (!meta.is_file() && !meta.is_dir());
            plan.add(name.clone(), meta.is_dir(), meta.len(), skip)?;
            if meta.is_dir() && !skip {
                pending.push(name);
            }
        }
    }
    Ok(plan)
}
fn remote_plan(backend: &mut impl Backend, path: &str, control: &Control) -> Result<Plan> {
    let (parent, name) = remote_root(path)?;
    let root = backend
        .list(&parent)?
        .into_iter()
        .find(|e| e.name == name)
        .context("Remote-Quelle nicht gefunden")?;
    ensure!(
        !root.is_link && (root.is_dir || root.is_file),
        "Verknüpfungen und Spezialdateien können nicht als Transferquelle verwendet werden"
    );
    let mut plan = Plan::new(root.is_dir);
    plan.add(String::new(), root.is_dir, root.size, false)?;
    let mut pending = if root.is_dir {
        vec![String::new()]
    } else {
        Vec::new()
    };
    while let Some(relative) = pending.pop() {
        control.check()?;
        let directory = if relative.is_empty() {
            path.into()
        } else {
            child(path, &relative)
        };
        let mut names = HashSet::new();
        for entry in backend.list(&directory)? {
            control.check()?;
            local_component(&entry.name)?;
            ensure!(
                names.insert(entry.name.clone()),
                "Doppelter Eintrag in Remote-Verzeichnis"
            );
            let name = if relative.is_empty() {
                entry.name
            } else {
                child(&relative, &entry.name)
            };
            let skip = entry.is_link || (!entry.is_file && !entry.is_dir);
            plan.add(name.clone(), entry.is_dir, entry.size, skip)?;
            if entry.is_dir && !skip {
                pending.push(name);
            }
        }
    }
    Ok(plan)
}

pub fn upload(
    backend: &mut impl Backend,
    local: &Path,
    remote: &str,
    control: &Control,
) -> Result<Report> {
    execute(backend, local, remote, true, false, control)
}
pub fn download(
    backend: &mut impl Backend,
    remote: &str,
    local: &Path,
    overwrite: bool,
    control: &Control,
) -> Result<Report> {
    execute(backend, local, remote, false, overwrite, control)
}
fn execute(
    backend: &mut impl Backend,
    local: &Path,
    remote: &str,
    upload: bool,
    overwrite: bool,
    control: &Control,
) -> Result<Report> {
    control.check()?;
    let (parent, name) = remote_root(remote)?;
    let remote = child(&parent, &name);
    control.update(true, |p| {
        *p = Progress {
            planning: true,
            current_path: remote.clone(),
            ..Progress::default()
        }
    });
    let local = local_target(local)?;
    let mut plan = if upload {
        local_plan(&local, control)?
    } else {
        remote_plan(backend, &remote, control)?
    };
    control.update(true, |p| {
        p.planning = false;
        p.directory = plan.report.directory;
        p.total_files = plan.items.iter().filter(|i| !i.directory).count() as u64;
        p.total_bytes = plan
            .items
            .iter()
            .filter(|i| !i.directory)
            .fold(0u64, |sum, i| sum.saturating_add(i.size));
    });
    control.check()?;
    if upload {
        ensure!(!backend.list(&parent)?.iter().any(|e| e.name == name), "Remote-Ziel existiert bereits. Ordner werden nicht zusammengeführt und Dateien nicht überschrieben.");
    } else if plan.report.directory || !overwrite {
        ensure!(
            fs::symlink_metadata(&local).is_err_and(|e| e.kind() == io::ErrorKind::NotFound),
            "Lokales Ziel existiert bereits. Ordner werden nicht zusammengeführt."
        );
    }
    for item in &plan.items {
        control.check()?;
        let local_path = local.join(&item.relative);
        // Path::join("") adds a trailing separator; keep file roots unchanged.
        let local_path = if item.relative.is_empty() {
            local.clone()
        } else {
            local_path
        };
        let remote_path = if item.relative.is_empty() {
            remote.clone()
        } else {
            child(&remote, &item.relative)
        };
        control.update(false, |p| {
            p.current_path = if upload {
                local_path.to_string_lossy().into()
            } else {
                remote_path.clone()
            }
        });
        if upload {
            check_local_chain(&local, &local_path)?;
            if item.directory {
                backend.mkdir(&remote_path)?;
            } else {
                plan.report.bytes += backend
                    .upload_file(&local_path, &remote_path, control)
                    .with_context(|| {
                        format!(
                            "Upload: {}. Bereits übertragene Teile bleiben erhalten.",
                            local_path.display()
                        )
                    })?;
            }
        } else if item.directory {
            if !item.relative.is_empty() {
                check_local_chain(&local, local_path.parent().unwrap())?;
            }
            fs::create_dir(&local_path)?;
        } else {
            if plan.report.directory {
                check_local_chain(&local, local_path.parent().unwrap())?;
            }
            plan.report.bytes += backend
                .download_file(
                    &remote_path,
                    &local_path,
                    overwrite && !plan.report.directory,
                    control,
                )
                .with_context(|| {
                    format!("Download: {remote_path}. Bereits übertragene Teile bleiben erhalten.")
                })?;
        }
        if item.directory {
            plan.report.directories += 1;
        } else {
            plan.report.files += 1;
            control.update(false, |p| p.files += 1);
        }
    }
    control.check()?;
    control.update(true, |_| {});
    Ok(plan.report)
}

macro_rules! backend {
    ($kind:ty) => {
        impl Backend for $kind {
            fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
                <$kind>::list(self, path)
            }
            fn mkdir(&mut self, path: &str) -> Result<()> {
                <$kind>::mkdir(self, path)
            }
            fn upload_file(
                &mut self,
                local: &Path,
                remote: &str,
                control: &Control,
            ) -> Result<u64> {
                self.upload_controlled(local, remote, control)
            }
            fn download_file(
                &mut self,
                remote: &str,
                local: &Path,
                overwrite: bool,
                control: &Control,
            ) -> Result<u64> {
                self.download_controlled(remote, local, overwrite, control)
            }
        }
    };
}
backend!(crate::ssh::Remote);
backend!(crate::ftp::Remote);
backend!(crate::remote::Remote);

#[cfg(test)]
mod tests {
    use super::*;
    struct Disk {
        root: tempfile::TempDir,
        hostile: Option<String>,
        mutations: usize,
        cancel_file: bool,
    }
    impl Disk {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().unwrap(),
                hostile: None,
                mutations: 0,
                cancel_file: false,
            }
        }
        fn path(&self, path: &str) -> PathBuf {
            self.root.path().join(path.trim_start_matches('/'))
        }
    }
    impl Backend for Disk {
        fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
            let mut entries = crate::local_fs::list(&self.path(path))?.entries;
            if path == "/tree" {
                if let Some(name) = &self.hostile {
                    entries[0].name = name.clone();
                }
            }
            Ok(entries)
        }
        fn mkdir(&mut self, path: &str) -> Result<()> {
            self.mutations += 1;
            Ok(fs::create_dir(self.path(path))?)
        }
        fn upload_file(&mut self, local: &Path, remote: &str, control: &Control) -> Result<u64> {
            self.mutations += 1;
            let mut source = control.reader(open_regular(local)?);
            let mut output = File::options()
                .write(true)
                .create_new(true)
                .open(self.path(remote))?;
            Ok(io::copy(&mut source, &mut output)?)
        }
        fn download_file(
            &mut self,
            remote: &str,
            local: &Path,
            overwrite: bool,
            control: &Control,
        ) -> Result<u64> {
            self.mutations += 1;
            let mut source = open_regular(&self.path(remote))?;
            let mut output = tempfile::NamedTempFile::new_in(local.parent().unwrap())?;
            if self.cancel_file {
                control.cancel();
            }
            let n = io::copy(&mut source, &mut control.writer(&mut output))?;
            if overwrite {
                output.persist(local).map_err(|e| e.error)?;
            } else {
                output.persist_noclobber(local).map_err(|e| e.error)?;
            }
            Ok(n)
        }
    }
    fn fixture(path: &Path) {
        fs::create_dir_all(path.join("nested/empty")).unwrap();
        fs::write(path.join("nested/ä 'quotes';.bin"), [0, 255, 12, 0]).unwrap();
        fs::write(path.join("top.txt"), b"hello").unwrap();
    }
    #[test]
    fn recursive_round_trip_preserves_empty_directories_and_binary_data_without_merging() {
        let source = tempfile::tempdir().unwrap();
        fixture(source.path());
        let mut disk = Disk::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let control = Control::new(move |p| observed.lock().unwrap().push(p));
        let report = upload(&mut disk, source.path(), "/tree", &control).unwrap();
        assert_eq!((report.files, report.directories, report.bytes), (2, 3, 9));
        assert!(upload(&mut disk, source.path(), "/tree", &Control::default()).is_err());
        let output = tempfile::tempdir().unwrap();
        let target = output.path().join("result");
        download(&mut disk, "/tree", &target, false, &Control::default()).unwrap();
        assert!(target.join("nested/empty").is_dir());
        assert_eq!(
            fs::read(target.join("nested/ä 'quotes';.bin")).unwrap(),
            [0, 255, 12, 0]
        );
        fs::write(target.join("top.txt"), b"keep").unwrap();
        assert!(download(&mut disk, "/tree", &target, true, &Control::default()).is_err());
        assert_eq!(fs::read(target.join("top.txt")).unwrap(), b"keep");
        let events = events.lock().unwrap();
        let last = events.last().unwrap();
        assert_eq!(
            (last.files, last.total_files, last.bytes, last.total_bytes),
            (2, 2, 9, 9)
        );
        assert!(!last.planning && last.directory);
    }
    #[test]
    fn hostile_remote_names_are_rejected_before_creating_any_destination() {
        let mut disk = Disk::new();
        fixture(&disk.path("/tree"));
        let output = tempfile::tempdir().unwrap();
        for name in ["../escape", "/absolute", "a/b", "a\\b", "..", ".", "nul\0x"] {
            disk.hostile = Some(name.into());
            assert!(
                download(
                    &mut disk,
                    "/tree",
                    &output.path().join("target"),
                    false,
                    &Control::default()
                )
                .is_err(),
                "{name:?}"
            );
            assert!(!output.path().join("target").exists());
            assert_eq!(disk.mutations, 0);
        }
    }
    #[cfg(unix)]
    #[test]
    fn links_and_special_files_are_reported_without_following_them() {
        use std::os::unix::{fs::symlink, net::UnixListener};
        let source = tempfile::tempdir().unwrap();
        fixture(source.path());
        symlink(source.path(), source.path().join("loop")).unwrap();
        symlink("/etc/passwd", source.path().join("outside")).unwrap();
        let _socket = UnixListener::bind(source.path().join("socket")).unwrap();
        let mut disk = Disk::new();
        let report = upload(&mut disk, source.path(), "/tree", &Control::default()).unwrap();
        assert_eq!(report.skipped, 3);
        assert!(!disk.path("/tree/loop").exists());
        assert!(upload(
            &mut disk,
            &source.path().join("outside"),
            "/link",
            &Control::default()
        )
        .is_err());
        symlink("/etc/passwd", disk.path("/tree/outside")).unwrap();
        let output = tempfile::tempdir().unwrap();
        let report = download(
            &mut disk,
            "/tree",
            &output.path().join("result"),
            false,
            &Control::default(),
        )
        .unwrap();
        assert_eq!(report.skipped, 1);
        assert!(!output.path().join("result/outside").exists());
    }
    #[test]
    fn depth_and_plan_limits_are_checked_before_writing() {
        let source = tempfile::tempdir().unwrap();
        let mut path = source.path().to_path_buf();
        for _ in 0..65 {
            path.push("a");
            fs::create_dir(&path).unwrap();
        }
        let mut disk = Disk::new();
        assert!(upload(&mut disk, source.path(), "/too-deep", &Control::default()).is_err());
        assert_eq!(disk.mutations, 0);
        let mut plan = Plan::new(true);
        plan.count = MAX_ENTRIES;
        assert!(plan.add("a".into(), false, 0, false).is_err());
        let mut plan = Plan::new(true);
        plan.path_bytes = MAX_PATH_BYTES;
        assert!(plan.add("a".into(), false, 0, false).is_err());
    }
    #[test]
    fn cancellation_preserves_existing_files_and_cleans_temporary_downloads() {
        let mut disk = Disk::new();
        fixture(&disk.path("/tree"));
        let output = tempfile::tempdir().unwrap();
        let target = output.path().join("file");
        fs::write(&target, b"existing").unwrap();
        disk.cancel_file = true;
        assert!(download(
            &mut disk,
            "/tree/top.txt",
            &target,
            true,
            &Control::default()
        )
        .is_err());
        assert_eq!(fs::read(&target).unwrap(), b"existing");
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 1);
        let control = Control::default();
        control.cancel();
        let before = disk.mutations;
        assert!(download(
            &mut disk,
            "/tree",
            &output.path().join("cancelled"),
            false,
            &control
        )
        .is_err());
        assert_eq!(disk.mutations, before);
        let control = Control::default();
        let mut reader = control.reader(&b"data"[..]);
        assert_eq!(reader.read(&mut [0; 2]).unwrap(), 2);
        control.cancel();
        assert_ne!(
            reader.read(&mut [0; 2]).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }
}
