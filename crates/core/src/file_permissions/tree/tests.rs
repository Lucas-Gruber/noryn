use super::*;
use std::collections::BTreeMap;

struct Memory {
    data: BTreeMap<String, Snapshot>,
    children: Vec<RemoteEntry>,
    writes: Vec<String>,
    cancel: bool,
    swap_parent: bool,
}
fn entry(name: &str) -> RemoteEntry {
    RemoteEntry {
        name: name.into(),
        path: "/untrusted-server-path".into(),
        is_dir: false,
        is_file: true,
        is_link: false,
        size: 1,
        modified: 0,
        permissions: "600".into(),
    }
}
fn memory() -> (Memory, Request) {
    let mut data = BTreeMap::new();
    for (path, dir) in [("/tree", true), ("/tree/a", false), ("/tree/b", false)] {
        data.insert(
            path.into(),
            Snapshot::new(
                path,
                dir,
                Some(0o700),
                Some(1000),
                Some(1000),
                (true, true),
                "original",
            )
            .unwrap(),
        );
    }
    let request = Request {
        path: "/tree".into(),
        local: false,
        change: Change {
            mode: Some(0o600),
            uid: None,
            gid: None,
        },
        root_fingerprint: data["/tree"].fingerprint.clone(),
    };
    (
        Memory {
            data,
            children: vec![entry("a"), entry("b")],
            writes: vec![],
            cancel: false,
            swap_parent: false,
        },
        request,
    )
}
impl Backend for Memory {
    fn read(&mut self, path: &str) -> Result<Snapshot> {
        self.data.get(path).cloned().context("missing")
    }
    fn list(&mut self, _path: &str) -> Result<Vec<RemoteEntry>> {
        Ok(self.children.clone())
    }
    fn apply(
        &mut self,
        path: &str,
        expected: &str,
        change: &Change,
        control: &Control,
    ) -> Result<()> {
        self.data[path].check(expected, change)?;
        let mut actual = self.data[path].clone();
        actual.mode = change.mode.or(actual.mode);
        actual.fingerprint = "changed".into();
        self.data.insert(path.into(), actual);
        self.writes.push(path.into());
        if self.cancel {
            control.cancel();
        }
        if self.swap_parent {
            self.data.get_mut("/tree").unwrap().is_dir = false;
        }
        Ok(())
    }
}
#[test]
fn previews_bind_changes_and_whole_tree_and_never_trust_listing_paths() {
    let (mut fs, mut request) = memory();
    let first = preview(&mut fs, &request, &Control::default()).unwrap();
    assert!(fs.writes.is_empty());
    assert_eq!((first.files, first.directories), (2, 1));
    fs.children.reverse();
    assert_eq!(
        preview(&mut fs, &request, &Control::default())
            .unwrap()
            .fingerprint,
        first.fingerprint
    );
    request.change.mode = Some(0o750);
    assert!(execute(&mut fs, &request, &first.fingerprint, &Control::default()).is_err());
    assert!(fs.writes.is_empty());
    request.change.mode = Some(0o600);
    fs.data.get_mut("/tree/a").unwrap().fingerprint = "someone edited child".into();
    assert!(execute(&mut fs, &request, &first.fingerprint, &Control::default()).is_err());
    assert!(fs.writes.is_empty());
    let fresh = preview(&mut fs, &request, &Control::default()).unwrap();
    execute(&mut fs, &request, &fresh.fingerprint, &Control::default()).unwrap();
    assert_eq!(fs.writes.last().unwrap(), "/tree", "root changes last");
    assert!(fs.writes.iter().all(|path| path.starts_with("/tree")));
}
#[test]
fn cancellation_and_replaced_ancestors_stop_without_repeating_completed_changes() {
    for swap in [false, true] {
        let (mut fs, request) = memory();
        let checked = preview(&mut fs, &request, &Control::default()).unwrap();
        fs.cancel = !swap;
        fs.swap_parent = swap;
        assert!(execute(&mut fs, &request, &checked.fingerprint, &Control::default()).is_err());
        assert_eq!(fs.writes, vec!["/tree/b"]);
        assert_eq!(fs.data["/tree/b"].mode, Some(0o600));
        assert_eq!(fs.data["/tree/a"].mode, Some(0o700));
    }
}
#[test]
fn hostile_names_tree_limits_file_roots_and_capabilities_fail_before_writes() {
    for name in ["..", ".", "../escape", "/outside", "nul\0"] {
        let (mut fs, request) = memory();
        fs.children = vec![entry(name)];
        assert!(preview(&mut fs, &request, &Control::default()).is_err());
        assert!(fs.writes.is_empty());
    }
    let (mut fs, request) = memory();
    fs.children = vec![entry("a"); MAX_ENTRIES];
    assert!(preview(&mut fs, &request, &Control::default()).is_err());
    assert!(fs.writes.is_empty());
    let (mut fs, request) = memory();
    fs.data.get_mut("/tree").unwrap().is_dir = false;
    assert!(preview(&mut fs, &request, &Control::default()).is_err());
    let (mut fs, request) = memory();
    fs.data.get_mut("/tree/a").unwrap().can_chmod = false;
    assert!(preview(&mut fs, &request, &Control::default()).is_err());
    assert!(fs.writes.is_empty());
}

#[cfg(unix)]
#[test]
fn real_local_tree_changes_children_before_restricting_directories_and_skips_links_and_specials() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("folder '\";$(ignored)");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let file = nested.join("file");
    std::fs::write(&file, b"keep").unwrap();
    let outside = temporary.path().join("outside");
    std::fs::write(&outside, b"outside").unwrap();
    let before = std::fs::metadata(&outside).unwrap().permissions().mode();
    symlink(&outside, root.join("link")).unwrap();
    symlink(temporary.path().join("absent"), root.join("dangling")).unwrap();
    let fifo = std::ffi::CString::new(root.join("fifo").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let metadata = super::super::local_read(root.to_str().unwrap()).unwrap();
    let request = Request {
        path: metadata.path,
        local: true,
        root_fingerprint: metadata.fingerprint,
        change: Change {
            mode: Some(0o600),
            uid: metadata.uid,
            gid: metadata.gid,
        },
    };
    let checked = preview(&mut Local, &request, &Control::default()).unwrap();
    assert_eq!(
        (
            checked.files,
            checked.directories,
            checked.skipped_links,
            checked.skipped_special
        ),
        (1, 2, 2, 1)
    );
    execute(
        &mut Local,
        &request,
        &checked.fingerprint,
        &Control::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        std::fs::metadata(&nested).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"keep");
    assert_eq!(
        std::fs::metadata(&outside).unwrap().permissions().mode(),
        before
    );
    // A changed child's metadata invalidates the full preview before any write.
    let mut fresh = request.clone();
    fresh.root_fingerprint = super::super::local_read(&fresh.path).unwrap().fingerprint;
    let checked = preview(&mut Local, &fresh, &Control::default()).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(execute(
        &mut Local,
        &fresh,
        &checked.fingerprint,
        &Control::default()
    )
    .is_err());
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o7777,
        0o700
    );
    fresh.path = root.join("link").to_str().unwrap().into();
    assert!(preview(&mut Local, &fresh, &Control::default()).is_err());
}
