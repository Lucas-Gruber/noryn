use noryn_core::{
    file_permissions::{tree, Change},
    remote::Remote,
};

pub fn round_trip(remote: &mut Remote, parent: &str, label: &str, known_mode: bool) {
    let path = format!(
        "{}/permissions-tree-{label} 'quoted';",
        parent.trim_end_matches('/')
    );
    let nested = format!("{path}/nested");
    let file = format!("{nested}/file.txt");
    remote.mkdir(&path).unwrap();
    remote.mkdir(&nested).unwrap();
    let source = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(source.path(), b"retained").unwrap();
    remote.upload(source.path(), &file).unwrap();
    let before = remote.permissions_read(&path).unwrap();
    let request = tree::Request {
        path: path.clone(),
        local: false,
        change: Change {
            mode: Some(0o750),
            uid: None,
            gid: None,
        },
        root_fingerprint: before.fingerprint,
    };
    let checked = tree::preview(remote, &request, &Default::default()).unwrap();
    assert_eq!((checked.files, checked.directories), (1, 2));
    assert_eq!(remote.permissions_read(&path).unwrap().mode, before.mode);
    if known_mode {
        let old = remote.permissions_read(&file).unwrap();
        remote
            .permissions_apply(
                &file,
                &old.fingerprint,
                &Change {
                    mode: Some(0o640),
                    uid: None,
                    gid: None,
                },
                &Default::default(),
            )
            .unwrap();
        assert!(
            tree::execute(remote, &request, &checked.fingerprint, &Default::default()).is_err()
        );
        assert_eq!(
            remote.permissions_read(&path).unwrap().mode,
            before.mode,
            "stale tree fails before writes"
        );
    }
    let checked = tree::preview(remote, &request, &Default::default()).unwrap();
    let cancelled = noryn_core::transfer::Control::default();
    cancelled.cancel();
    assert!(tree::execute(remote, &request, &checked.fingerprint, &cancelled).is_err());
    tree::execute(remote, &request, &checked.fingerprint, &Default::default()).unwrap();
    for entry in [&path, &nested, &file] {
        let result = remote.permissions_read(entry).unwrap();
        if known_mode {
            assert_eq!(result.mode, Some(0o750));
        }
    }
    assert_eq!(&*remote.read_text(&file).unwrap(), b"retained");
}
