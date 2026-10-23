use base64::{engine::general_purpose::STANDARD, Engine};
use noryn_core::{
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
    let port = std::env::var("NORYN_TEST_SSH_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let username = std::env::var("NORYN_TEST_SSH_USER").unwrap();
    let key = fs::read_to_string(std::env::var("NORYN_TEST_SSH_KEY").unwrap()).unwrap();
    let node = Node {
        id: "test".into(),
        parent_id: None,
        workspace: "private".into(),
        kind: "connection".into(),
        name: "Integration".into(),
        protocol: "ssh".into(),
        host: "127.0.0.1".into(),
        port,
        initial_path: String::new(),
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
        password: String::new(),
        team_id: None,
    };
    let (_, fingerprint) = ssh::handshake(&node.host, port).unwrap();
    assert!(ssh::authenticated(&node, &credential, "SHA256:wrong-host-key").is_err());
    let session = ssh::authenticated(&node, &credential, &fingerprint).unwrap();
    let mut remote = Remote::new(session);
    assert!(remote.home().unwrap().starts_with('/'));
    let dir = tempfile::tempdir().unwrap();
    let listing = remote.list(dir.path().to_str().unwrap()).unwrap();
    assert!(
        listing.is_empty(),
        "Empty directory must not contain itself as a child: {}",
        serde_json::to_string(&listing).unwrap()
    );
    let remote_path = dir
        .path()
        .join("a file with 'quotes';$(not-a-command).txt")
        .to_str()
        .unwrap()
        .to_string();
    let input = dir.path().join("input.txt");
    let output = dir.path().join("output.txt");
    let contents = b"Noryn SFTP round trip\n\x00\xffbinary data";
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
    if let Ok(control) = std::env::var("NORYN_TEST_SUDO_CONTROL") {
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
        b"printf '\\116\\117\\122\\131\\116\\137\\117\\113\\134\\156'\n".to_vec(),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        if let Ok(TerminalEvent::Data { data }) = events_rx.recv_timeout(Duration::from_millis(200))
        {
            output.extend(STANDARD.decode(data).unwrap());
            if String::from_utf8_lossy(&output).contains("NORYN_OK") {
                break;
            }
        }
    }
    tx.send(TerminalInput::Close).unwrap();
    assert!(
        String::from_utf8_lossy(&output).contains("NORYN_OK"),
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
    assert!(matches!(
        remote.sudo_auto(true, None).unwrap(),
        ssh::SudoOutcome::Enabled
    ));
    remote.set_sudo(false, String::new()).unwrap();
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
    fs::write(&output, b"keep my existing local file").unwrap();
    assert!(remote.download_file(remote_path, &output, false).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"keep my existing local file");
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
    remote.set_sudo(true, "noryn-test-pass".into()).unwrap();
    remote.download(remote_path, &output).unwrap();
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let second = directory.join("password-upload");
    remote.upload(&input, second.to_str().unwrap()).unwrap();
    assert_eq!(fs::read(&second).unwrap(), bytes);
    assert!(remote.set_sudo(true, "wrong-password".into()).is_err());
    // A failed replacement also restores an already active sudo mode/password.
    remote.list(directory.to_str().unwrap()).unwrap();

    fs::write(control, "deny").unwrap();
    assert!(remote.set_sudo(true, "noryn-test-pass".into()).is_err());
    remote.set_sudo(false, String::new()).unwrap();
    remote.list(directory.to_str().unwrap()).unwrap();
    fs::write(control, "nopasswd").unwrap();
}

#[test]
#[ignore = "Run with scripts/test-ssh.sh against the isolated localhost server"]
fn one_authenticated_transport_keeps_terminal_and_file_channels_usable_together() {
    let port = std::env::var("NORYN_TEST_SSH_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let credential = Credential {
        id: "test".into(),
        name: "test".into(),
        username: std::env::var("NORYN_TEST_SSH_USER").unwrap(),
        kind: "key".into(),
        secret: fs::read_to_string(std::env::var("NORYN_TEST_SSH_KEY").unwrap()).unwrap(),
        password: String::new(),
        passphrase: String::new(),
        team_id: None,
    };
    let (session, _) = ssh::handshake("127.0.0.1", port).unwrap();
    ssh::authenticate(&session, &credential).unwrap();
    let (tx, rx) = mpsc::sync_channel(16);
    let (events_tx, events_rx) = mpsc::channel();
    ssh::start_terminal(session.clone(), rx, move |event| {
        let _ = events_tx.send(event);
    })
    .unwrap();
    let mut remote = Remote::new(session).with_password("noryn-test-pass".into());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("input");
    let target = dir.path().join("remote");
    let output = dir.path().join("output");
    let contents = vec![57; 2 * 1024 * 1024];
    fs::write(&local, &contents).unwrap();
    tx.send(TerminalInput::Data(b"for i in 1 2 3 4 5 6 7 8 9 10; do printf '\\123\\110\\101\\122\\105\\104\\137\\117\\113\\n'; sleep 0.03; done\n".to_vec())).unwrap();
    remote.upload(&local, target.to_str().unwrap()).unwrap();
    remote.list(dir.path().to_str().unwrap()).unwrap();
    let control = std::env::var("NORYN_TEST_SUDO_CONTROL").unwrap();
    fs::write(&control, "password").unwrap();
    assert!(matches!(
        remote.sudo_auto(true, None).unwrap(),
        ssh::SudoOutcome::Enabled
    ));
    remote.download(target.to_str().unwrap(), &output).unwrap();
    assert_eq!(fs::read(output).unwrap(), contents);
    remote.set_sudo(false, String::new()).unwrap();
    fs::write(control, "nopasswd").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut bytes = Vec::new();
    while Instant::now() < deadline {
        if let Ok(TerminalEvent::Data { data }) = events_rx.recv_timeout(Duration::from_millis(100))
        {
            bytes.extend(STANDARD.decode(data).unwrap());
        }
        if String::from_utf8_lossy(&bytes).matches("SHARED_OK").count() >= 10 {
            break;
        }
    }
    tx.send(TerminalInput::Close).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&bytes).matches("SHARED_OK").count(),
        10
    );
}

#[test]
#[ignore = "Run with scripts/test-ssh.sh against an isolated localhost OpenSSH server"]
fn ssh_tools_forward_backpressure_half_close_and_collect_system_snapshots() {
    use noryn_core::{ssh_tools::Tools, system_info::Section};
    use std::{
        io::{Read, Write},
        net::{Shutdown, TcpListener, TcpStream},
        thread,
    };
    let credential = Credential {
        id: "test".into(),
        name: "test".into(),
        username: std::env::var("NORYN_TEST_SSH_USER").unwrap(),
        kind: "key".into(),
        secret: fs::read_to_string(std::env::var("NORYN_TEST_SSH_KEY").unwrap()).unwrap(),
        password: String::new(),
        passphrase: String::new(),
        team_id: None,
    };
    let (session, _) = ssh::handshake(
        "127.0.0.1",
        std::env::var("NORYN_TEST_SSH_PORT")
            .unwrap()
            .parse()
            .unwrap(),
    )
    .unwrap();
    ssh::authenticate(&session, &credential).unwrap();
    let tools = Tools::new(session.clone());
    let processes = tools.snapshot(Section::Processes).unwrap();
    assert!(!processes.processes.is_empty());
    let hardware = tools.snapshot(Section::Hardware).unwrap().hardware.unwrap();
    assert!(hardware.memory_total > 0 && hardware.logical_cpus > 0);
    let services = tools.snapshot(Section::Services).unwrap();
    assert!(services.message.is_some() || !services.services.is_empty());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|n| (n % 251) as u8).collect();
    let expected = payload.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut input = Vec::new();
        stream.read_to_end(&mut input).unwrap();
        assert_eq!(input, expected);
        stream.write_all(&input).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
    });
    let info = tools.start_forward(0, "127.0.0.1".into(), port).unwrap();
    assert!(tools
        .start_forward(info.local_port, "127.0.0.1".into(), port)
        .is_err());
    let mut client = TcpStream::connect(("127.0.0.1", info.local_port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    client.write_all(&payload).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    thread::sleep(Duration::from_millis(150)); // exercise downstream backpressure
    let mut output = Vec::new();
    client.read_to_end(&mut output).unwrap();
    assert_eq!(output, payload);
    server.join().unwrap();
    let stats = tools.list_forwards().unwrap();
    assert_eq!(stats[0].uploaded, payload.len() as u64);
    assert_eq!(stats[0].downloaded, payload.len() as u64);
    assert!(stats[0].last_error.is_none(), "{:?}", stats[0].last_error);
    tools.stop_forward(&info.id).unwrap();
    let reused = TcpListener::bind(("127.0.0.1", info.local_port)).unwrap();
    drop(reused);
    // A failed target must be reported without killing the SSH transport.
    let unreachable = tools.start_forward(0, "127.0.0.1".into(), port).unwrap();
    let mut failed_client = TcpStream::connect(("127.0.0.1", unreachable.local_port)).unwrap();
    failed_client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let _ = failed_client.read(&mut [0]);
    let deadline = Instant::now() + Duration::from_secs(2);
    while tools
        .list_forwards()
        .unwrap()
        .iter()
        .find(|f| f.id == unreachable.id)
        .unwrap()
        .last_error
        .is_none()
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(tools
        .list_forwards()
        .unwrap()
        .iter()
        .find(|f| f.id == unreachable.id)
        .unwrap()
        .last_error
        .is_some());
    let remote = Remote::new(session);
    assert!(remote.home().unwrap().starts_with('/'));
    tools.close();
    assert!(tools.list_forwards().unwrap().is_empty());
    assert!(TcpListener::bind(("127.0.0.1", unreachable.local_port)).is_ok());
    assert!(tools.start_forward(0, "127.0.0.1".into(), port).is_err());
    assert!(tools.snapshot(Section::Hardware).is_err());
}
