pub fn round_trip(remote: &mut noryn_core::remote::Remote, parent: &str) {
    use noryn_core::file_actions::{self, Action, Request};
    let mut files = file_actions::Remote(remote);
    let root = format!("{}/actions 'quoted';", parent.trim_end_matches('/'));
    let run =
        |files: &mut file_actions::Remote<'_>, action, source: String, destination: String| {
            let request = Request {
                action,
                source,
                destination,
                local: false,
            };
            let preview = file_actions::preview(files, &request, &Default::default()).unwrap();
            file_actions::execute(files, &request, &preview.fingerprint, &Default::default())
                .unwrap();
        };
    run(&mut files, Action::NewFolder, String::new(), root.clone());
    run(
        &mut files,
        Action::NewFolder,
        String::new(),
        format!("{root}/nested"),
    );
    run(
        &mut files,
        Action::NewFile,
        String::new(),
        format!("{root}/nested/file.txt"),
    );
    let copy = format!("{root}-copy");
    run(&mut files, Action::Copy, root.clone(), copy.clone());
    let moved = format!("{root}-moved");
    run(&mut files, Action::Move, copy.clone(), moved.clone());
    assert!(file_actions::Files::stat(&mut files, &copy).is_err());
    assert!(
        file_actions::Files::stat(&mut files, &format!("{moved}/nested/file.txt"))
            .unwrap()
            .is_file
    );
    assert!(file_actions::Files::rename(&mut files, &moved, &root).is_err());
    run(&mut files, Action::Delete, moved.clone(), String::new());
    run(&mut files, Action::Delete, root.clone(), String::new());
    assert!(file_actions::Files::stat(&mut files, &moved).is_err());
    assert!(file_actions::Files::stat(&mut files, &root).is_err());
}
