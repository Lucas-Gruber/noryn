use base64::{engine::general_purpose::STANDARD, Engine};
use harbor_core::{
    model::{Credential, Node},
    ssh::{self, Remote, TerminalEvent, TerminalInput},
};
use std::{
    fs,
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
#[ignore = "Run with scripts/test-ssh.sh against an isolated localhost OpenSSH server"]
fn actual_ssh_terminal_and_sftp_round_trip() {
    let port = std::env::var("HARBOR_TEST_SSH_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let username = std::env::var("HARBOR_TEST_SSH_USER").unwrap();
    let key = fs::read_to_string(std::env::var("HARBOR_TEST_SSH_KEY").unwrap()).unwrap();
    let node = Node {
        id: "test".into(),
        parent_id: None,
        workspace: "private".into(),
        kind: "connection".into(),
        name: "Integration".into(),
        protocol: "ssh".into(),
        host: "127.0.0.1".into(),
        port,
        notes: "".into(),
        tags: vec![],
        favorite: false,
    };
    let credential = Credential {
        id: "test".into(),
        name: "test".into(),
        username,
        kind: "key".into(),
        secret: key,
        passphrase: "".into(),
    };
    let (_, fingerprint) = ssh::handshake(&node.host, port).unwrap();
    assert!(ssh::authenticated(&node, &credential, "SHA256:wrong-host-key").is_err());
    let session = ssh::authenticated(&node, &credential, &fingerprint).unwrap();
    let mut remote = Remote::new(session);
    assert!(remote.home().unwrap().starts_with('/'));
    let dir = tempfile::tempdir().unwrap();
    let remote_path = dir
        .path()
        .join("a file with 'quotes';$(not-a-command).txt")
        .to_str()
        .unwrap()
        .to_string();
    let input = dir.path().join("input.txt");
    let output = dir.path().join("output.txt");
    let contents = b"Harbor SFTP round trip\n\x00\xffbinary data";
    fs::write(&input, contents).unwrap();
    assert_eq!(
        remote.upload(&input, &remote_path).unwrap(),
        contents.len() as u64
    );
    assert!(
        remote.upload(&input, &remote_path).is_err(),
        "existing remote files must not be overwritten"
    );
    assert!(remote
        .list(dir.path().to_str().unwrap())
        .unwrap()
        .iter()
        .any(|e| e.path == remote_path));
    remote.download(&remote_path, &output).unwrap();
    assert_eq!(fs::read(&output).unwrap(), contents);
    remote
        .mkdir(dir.path().join("new folder").to_str().unwrap())
        .unwrap();
    assert!(dir.path().join("new folder").is_dir());
    if let Ok(control) = std::env::var("HARBOR_TEST_SUDO_CONTROL") {
        shell_file_operations_over_ssh(&mut remote, &control, dir.path());
    }
    let session = ssh::authenticated(&node, &credential, &fingerprint).unwrap();
    let (tx, rx) = mpsc::sync_channel(16);
    let (events_tx, events_rx) = mpsc::channel();
    ssh::start_terminal(session, rx, move |event| {
        let _ = events_tx.send(event);
    })
    .unwrap();
    tx.send(TerminalInput::Resize(100, 30)).unwrap();
    tx.send(TerminalInput::Data(
        b"printf '\\110\\101\\122\\102\\117\\122\\137\\117\\113\\134\\156'\n".to_vec(),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        if let Ok(TerminalEvent::Data { data }) = events_rx.recv_timeout(Duration::from_millis(200))
        {
            output.extend(STANDARD.decode(data).unwrap());
            if String::from_utf8_lossy(&output).contains("HARBOR_OK") {
                break;
            }
        }
    }
    tx.send(TerminalInput::Close).unwrap();
    assert!(
        String::from_utf8_lossy(&output).contains("HARBOR_OK"),
        "terminal did not return command output: {:?}",
        String::from_utf8_lossy(&output)
    );
}

fn shell_file_operations_over_ssh(remote: &mut Remote, control: &str, directory: &std::path::Path) {
    // The test server emulates sudo prompts and root identity in its own PATH.
    // All filesystem operations still run as the current unprivileged user.
    let input = directory.join("shell-input");
    let bytes: Vec<_> = (0..512 * 1024).map(|i| (i % 256) as u8).collect();
    fs::write(&input, &bytes).unwrap();
    let remote_file = directory.join("sudo file '\"\n$(not-a-command)");
    let remote_path = remote_file.to_str().unwrap();
    let output = directory.join("shell-output");

    fs::write(control, "nopasswd").unwrap();
    // A supplied password must not be sent into stdin if sudo never asks for
    // it. In particular it must not become the prefix of an uploaded file.
    remote.set_sudo(true, "unused-password".into()).unwrap();
    assert_eq!(
        remote.upload(&input, remote_path).unwrap(),
        bytes.len() as u64
    );
    assert_eq!(fs::read(&remote_file).unwrap(), bytes);
    assert!(remote.upload(&input, remote_path).is_err());
    remote.download(remote_path, &output).unwrap();
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let entries = remote.list(directory.to_str().unwrap()).unwrap();
    assert!(entries
        .iter()
        .any(|entry| entry.path == remote_path && entry.size == bytes.len() as u64));
    let folder = directory.join("sudo directory\n'quoted'");
    remote.mkdir(folder.to_str().unwrap()).unwrap();
    assert!(remote.list(folder.to_str().unwrap()).unwrap().is_empty());

    remote.set_sudo(false, String::new()).unwrap();
    fs::write(control, "password").unwrap();
    assert!(remote.set_sudo(true, String::new()).is_err());
    // A failed transition leaves ordinary SFTP usable.
    remote.list(directory.to_str().unwrap()).unwrap();
    remote.set_sudo(true, "harbor-test-pass".into()).unwrap();
    remote.download(remote_path, &output).unwrap();
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let second = directory.join("password-upload");
    remote.upload(&input, second.to_str().unwrap()).unwrap();
    assert_eq!(fs::read(&second).unwrap(), bytes);
    assert!(remote.set_sudo(true, "wrong-password".into()).is_err());
    // A failed replacement also restores an already active sudo mode/password.
    remote.list(directory.to_str().unwrap()).unwrap();

    fs::write(control, "deny").unwrap();
    assert!(remote.set_sudo(true, "harbor-test-pass".into()).is_err());
    remote.set_sudo(false, String::new()).unwrap();
    remote.list(directory.to_str().unwrap()).unwrap();
    fs::write(control, "nopasswd").unwrap();
}
