#[path = "common/file_actions.rs"]
mod file_actions_fixture;
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
    let mut remote = Remote::new(session.clone());
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
    recursive_round_trip(&mut remote, dir.path(), "sftp");
    text_round_trip(&mut remote, dir.path(), "sftp");
    let mut file_remote = noryn_core::remote::Remote::Ssh(Remote::new(session));
    file_actions_fixture::round_trip(&mut file_remote, dir.path().to_str().unwrap());
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

    recursive_round_trip(remote, directory, "sudo-nopasswd");
    text_round_trip(remote, directory, "sudo-nopasswd");
    remote.set_sudo(false, String::new()).unwrap();
    fs::write(control, "password").unwrap();
    assert!(remote.set_sudo(true, String::new()).is_err());
    // A failed transition leaves ordinary SFTP usable.
    remote.list(directory.to_str().unwrap()).unwrap();
    remote.set_sudo(true, "noryn-test-pass".into()).unwrap();
    remote.download(remote_path, &output).unwrap();
    assert_eq!(fs::read(&output).unwrap(), bytes);
    recursive_round_trip(remote, directory, "sudo-password");
    text_round_trip(remote, directory, "sudo-password");
    let created = directory.join("sudo-new-file");
    let moved = directory.join("sudo-move 'quoted';");
    remote.upload(&input, created.to_str().unwrap()).unwrap();
    remote
        .rename(created.to_str().unwrap(), moved.to_str().unwrap())
        .unwrap();
    assert_eq!(fs::read(&moved).unwrap(), bytes);
    assert!(remote.rename(moved.to_str().unwrap(), remote_path).is_err());
    remote.remove_file(moved.to_str().unwrap()).unwrap();
    let empty = directory.join("sudo-empty");
    remote.mkdir(empty.to_str().unwrap()).unwrap();
    remote.remove_dir(empty.to_str().unwrap()).unwrap();
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
fn login_health_is_cached_uses_read_only_queries_and_can_probe_a_forward_target() {
    use noryn_core::{login_health::LoginPoint, ssh_tools::Tools};
    use std::{io::Write, net::TcpListener, path::Path, thread};
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
    let previous = LoginPoint {
        identity: "test".into(),
        server_time: 1,
        active_services: vec![],
    };
    let report = tools.login_health(Some(&previous)).unwrap();
    assert!(
        report.uptime > 0.
            && report.cores > 0
            && report.services_available
            && report.journal_available
            && report.ports_available
    );
    assert_eq!(report.updates.as_ref().unwrap().total, 2);
    assert_eq!(report.updates.as_ref().unwrap().security, Some(1));
    assert_eq!(report.events[0].unit, "noryn-failed.service");
    assert!(report
        .ports
        .iter()
        .any(|p| p.port == 5432 && p.forward_host == "127.0.0.1"));
    assert!(tools
        .listening_ports()
        .unwrap()
        .iter()
        .any(|p| p.protocol == "udp" && p.port == 53));
    assert_eq!(
        tools.login_health(None).unwrap().captured_at,
        report.captured_at
    );
    let control = std::env::var("NORYN_TEST_SUDO_CONTROL").unwrap();
    let root = Path::new(&control).parent().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("update-calls"))
            .unwrap()
            .lines()
            .count(),
        1,
        "The login report must not poll or refresh repositories"
    );
    assert!(fs::read_to_string(root.join("journal-calls"))
        .unwrap()
        .contains("--since=@1"));
    fs::write(root.join("no-package-cache"), b"").unwrap();
    let missing_cache = Tools::new(session);
    assert!(
        missing_cache.login_health(None).unwrap().updates.is_none(),
        "Missing package sources must remain unknown, not zero updates"
    );
    assert_eq!(
        fs::read_to_string(root.join("update-calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    fs::remove_file(root.join("no-package-cache")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket.write_all(b"Noryn test banner\r\n").unwrap();
                    return;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("{e}"),
            }
        }
        panic!("No probe arrived");
    });
    let forward = tools.start_forward(0, "127.0.0.1".into(), port).unwrap();
    assert!(tools
        .probe_forward(&forward.id)
        .unwrap()
        .contains("Noryn test banner"));
    server.join().unwrap();
    tools.stop_forward(&forward.id).unwrap();
    assert!(tools.probe_forward(&forward.id).is_err());
    tools.close();
    assert!(tools.login_health(None).is_err());
    assert!(tools.listening_ports().is_err());
}

#[test]
#[ignore = "Run with scripts/test-ssh.sh against the isolated localhost server"]
fn sudo_password_fallback_supports_standard_and_sudo_rs_messages() {
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
    let control = std::env::var("NORYN_TEST_SUDO_CONTROL").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().to_str().unwrap();
    for mode in ["password", "password-rs"] {
        fs::write(&control, mode).unwrap();
        let mut remote = Remote::new(session.clone());
        assert!(matches!(
            remote.sudo_auto(true, None).unwrap(),
            ssh::SudoOutcome::PasswordRequired { .. }
        ));
        // The failed automatic attempt leaves ordinary SFTP usable until the
        // caller supplies the password through the existing dialog.
        remote.list(path).unwrap();
        assert!(matches!(
            remote
                .sudo_auto(true, Some("noryn-test-pass".into()))
                .unwrap(),
            ssh::SudoOutcome::Enabled
        ));
        remote.list(path).unwrap();
        assert!(matches!(
            remote.sudo_auto(false, None).unwrap(),
            ssh::SudoOutcome::Disabled
        ));

        let mut saved = Remote::new(session.clone()).with_password("noryn-test-pass".into());
        assert!(matches!(
            saved.sudo_auto(true, None).unwrap(),
            ssh::SudoOutcome::Enabled
        ));
        // Authentication bytes must not end up in the uploaded file stream.
        let input = directory.path().join(format!("input-{mode}"));
        let output = directory.path().join(format!("output-{mode}"));
        let contents: Vec<u8> = (0..=255).collect();
        fs::write(&input, &contents).unwrap();
        saved.upload(&input, output.to_str().unwrap()).unwrap();
        assert_eq!(fs::read(output).unwrap(), contents);

        let mut wrong = Remote::new(session.clone()).with_password("wrong-password".into());
        assert!(matches!(
            wrong.sudo_auto(true, None).unwrap(),
            ssh::SudoOutcome::PasswordRequired { .. }
        ));
        assert!(wrong
            .sudo_auto(true, Some("wrong-password".into()))
            .is_err());
        wrong.list(path).unwrap();
    }
    fs::write(&control, "deny").unwrap();
    let mut remote = Remote::new(session).with_password("noryn-test-pass".into());
    assert!(matches!(
        remote.sudo_auto(true, None).unwrap(),
        ssh::SudoOutcome::PasswordRequired { .. }
    ));
    remote.list(path).unwrap();
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

#[test]
#[ignore = "Run with scripts/test-ssh.sh; service and LVM tools are isolated stubs"]
fn interactive_management_uses_saved_sudo_password_without_repeating_started_actions() {
    use noryn_core::{
        ssh_tools::Tools,
        system_control::{Reply, Request},
        system_info::Section,
    };
    use std::{process::Command, thread};
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
    let tools = Tools::new(session.clone()).with_password("noryn-test-pass".into());
    let control = std::env::var("NORYN_TEST_SUDO_CONTROL").unwrap();
    let dir = std::path::Path::new(&control).parent().unwrap();
    let services = tools.snapshot(Section::Services).unwrap();
    assert!(services
        .services
        .iter()
        .any(|s| s.name == "noryn-mock.service" && s.file_state == "enabled"));
    assert!(services
        .services
        .iter()
        .any(|s| s.name == "noryn-inactive.service" && s.file_state == "disabled"));
    assert!(
        matches!(tools.control(&Request::ServiceFile {name:"noryn-mock.service".into()},false,None).unwrap(),Reply::Completed {output,..} if output.contains("ExecStart=/bin/true"))
    );
    assert!(
        matches!(tools.control(&Request::ServiceStatus {name:"noryn-mock.service".into()},false,None).unwrap(),Reply::Completed {output,..} if output.contains("UnitFileState=enabled"))
    );
    for request in [
        Request::ServiceStart {
            name: "noryn-mock.service".into(),
        },
        Request::ServiceStop {
            name: "noryn-mock.service".into(),
        },
        Request::ServiceReload {
            name: "noryn-mock.service".into(),
        },
        Request::ServiceEnable {
            name: "noryn-mock.service".into(),
        },
        Request::ServiceDisable {
            name: "noryn-mock.service".into(),
        },
    ] {
        assert!(matches!(
            tools.control(&request, true, None).unwrap(),
            Reply::Completed { .. }
        ));
    }
    let storage = tools.control(&Request::Storage, false, None).unwrap();
    assert!(
        matches!(storage,Reply::Completed {storage:Some(data),..} if !data.mounts.is_empty() && data.pvs.len()==1 && data.vgs.len()==1 && data.lvs.len()==1)
    );
    let restart = Request::ServiceRestart {
        name: "noryn-mock.service".into(),
    };
    fs::write(&control, "password").unwrap();
    fs::write(dir.join("service-calls"), "").unwrap();
    assert!(matches!(
        tools.control(&restart, true, None).unwrap(),
        Reply::Completed { .. }
    ));
    assert_eq!(
        fs::read_to_string(dir.join("service-calls"))
            .unwrap()
            .lines()
            .filter(|line| *line == "restart")
            .count(),
        1
    );
    let wrong = Tools::new(session.clone()).with_password("wrong".into());
    assert!(matches!(
        wrong.control(&restart, true, None).unwrap(),
        Reply::PasswordRequired { .. }
    ));
    assert!(matches!(
        wrong
            .control(&restart, true, Some("noryn-test-pass".into()))
            .unwrap(),
        Reply::Completed { .. }
    ));
    fs::write(&control, "nopasswd").unwrap();
    fs::write(dir.join("service-calls"), "").unwrap();
    fs::write(dir.join("fail-service"), "").unwrap();
    assert!(tools.control(&restart, true, None).is_err());
    assert_eq!(
        fs::read_to_string(dir.join("service-calls"))
            .unwrap()
            .lines()
            .filter(|line| *line == "restart")
            .count(),
        1,
        "a started mutation must never retry with a password"
    );
    fs::remove_file(dir.join("fail-service")).unwrap();
    assert!(tools
        .control(
            &Request::ServiceStop {
                name: "noryn-mock.service; touch /tmp/bad".into()
            },
            true,
            None
        )
        .is_err());
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let identity = tools
        .control(&Request::ProcessInfo { pid: child.id() }, false, None)
        .unwrap();
    let ticks = match identity {
        Reply::Completed {
            start_ticks: Some(ticks),
            output,
            ..
        } => {
            assert!(output.contains("sleep"));
            ticks
        }
        _ => panic!("missing process identity"),
    };
    assert!(tools
        .control(
            &Request::ProcessTerminate {
                pid: child.id(),
                start_ticks: ticks + 1
            },
            false,
            None
        )
        .is_err());
    assert!(child.try_wait().unwrap().is_none());
    tools
        .control(
            &Request::ProcessTerminate {
                pid: child.id(),
                start_ticks: ticks,
            },
            false,
            None,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        panic!("TERM did not reach test child");
    }
    child.wait().unwrap();
    tools.close();
    wrong.close();
}

fn recursive_round_trip(remote: &mut Remote, directory: &std::path::Path, mode: &str) {
    let source = directory.join(format!("source-{mode}"));
    fs::create_dir_all(source.join("nested/empty")).unwrap();
    let bytes = [0, 255, 10, 13, 128];
    let filename = "nested/ä 'quotes';$(no-command).bin";
    fs::write(source.join(filename), bytes).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, source.join("loop")).unwrap();
    let target = directory.join(format!("remote-{mode}"));
    let report = noryn_core::transfer::upload(
        remote,
        &source,
        target.to_str().unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!((report.files, report.directories, report.bytes), (1, 3, 5));
    #[cfg(unix)]
    assert_eq!(report.skipped, 1);
    assert!(noryn_core::transfer::upload(
        remote,
        &source,
        target.to_str().unwrap(),
        &Default::default()
    )
    .is_err());
    let output = directory.join(format!("output-{mode}"));
    noryn_core::transfer::download(
        remote,
        target.to_str().unwrap(),
        &output,
        false,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(fs::read(output.join(filename)).unwrap(), bytes);
    assert!(output.join("nested/empty").is_dir());
    assert!(!output.join("loop").exists());
    assert!(noryn_core::transfer::download(
        remote,
        target.to_str().unwrap(),
        &output,
        true,
        &Default::default()
    )
    .is_err());
}

fn text_round_trip(remote: &mut Remote, directory: &std::path::Path, mode: &str) {
    use noryn_core::{text_files, transfer::Control};
    let file = directory.join(format!("{mode} text 'quote';$(not-a-command).conf"));
    fs::write(&file, b"initial = true\n").unwrap();
    let path = file.to_str().unwrap();
    let initial = remote.read_text(path).unwrap();
    let expected = text_files::digest(&initial);
    let cancelled = Control::default();
    cancelled.cancel();
    assert!(remote
        .replace_text(path, &expected, b"cancelled", &cancelled)
        .is_err());
    assert_eq!(fs::read(&file).unwrap(), b"initial = true\n");
    remote
        .replace_text(path, &expected, b"edited = true\n", &Control::default())
        .unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"edited = true\n");
    assert!(remote
        .replace_text(path, &expected, b"stale edit", &Control::default())
        .is_err());
    assert_eq!(fs::read(&file).unwrap(), b"edited = true\n");
    assert!(remote
        .replace_text(
            path,
            &text_files::digest(b"edited = true\n"),
            &[0, 255],
            &Control::default()
        )
        .is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = directory.join(format!("{mode}-text-link"));
        symlink(&file, &link).unwrap();
        assert!(remote.read_text(link.to_str().unwrap()).is_err());
    }
    assert!(!remote
        .list(directory.to_str().unwrap())
        .unwrap()
        .iter()
        .any(|e| e.name.starts_with(".noryn-edit-")));
}

#[test]
#[ignore = "Run with scripts/test-ssh.sh against an isolated localhost OpenSSH server"]
fn parallel_commands_capture_each_stream_exit_code_limits_and_cancellation() {
    use noryn_core::ssh_tools::Tools;
    use std::sync::{atomic::AtomicBool, Arc};
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
    let tools = Arc::new(Tools::new(session));
    std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..4)
            .map(|i| {
                let tools = tools.clone();
                scope.spawn(move || {
                    let output = tools.run_command(
                        &format!(
                            "sleep 0.1; printf 'target-{i}'; printf 'error-{i}' >&2; exit {i}"
                        ),
                        &AtomicBool::new(false),
                        Duration::from_secs(5),
                    );
                    assert!(output.error.is_none(), "{:?}", output.error);
                    assert_eq!(output.stdout, format!("target-{i}"));
                    assert_eq!(output.stderr, format!("error-{i}"));
                    assert_eq!(output.exit_code, Some(i));
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
    });
    let flood = tools.run_command(
        "head -c 300000 /dev/zero | tr '\\000' x; head -c 300000 /dev/zero | tr '\\000' y >&2",
        &AtomicBool::new(false),
        Duration::from_secs(5),
    );
    assert!(flood.error.is_none(), "{:?}", flood.error);
    assert!(flood.truncated);
    assert_eq!(flood.stdout.len(), 256 * 1024);
    assert_eq!(flood.stderr.len(), 256 * 1024);
    let timeout = tools.run_command(
        "printf partial; sleep 2",
        &AtomicBool::new(false),
        Duration::from_millis(150),
    );
    assert_eq!(timeout.stdout, "partial");
    assert!(timeout.error.unwrap().contains("Zeitlimit"));
    let cancelled = tools.run_command("echo never", &AtomicBool::new(true), Duration::from_secs(5));
    assert!(cancelled.stdout.is_empty());
    assert!(cancelled.error.unwrap().contains("abgebrochen"));
    let eof = tools.run_command(
        "read input; printf eof",
        &AtomicBool::new(false),
        Duration::from_secs(5),
    );
    assert_eq!(eof.stdout, "eof");
    assert!(eof.error.is_none());
    let body = format!("set -euo pipefail\nprintf 'Grüße / %s' \"${{BASH_SOURCE[0]:-stdin}}\"\nprintf 'script-stderr' >&2\nhead -c 100000 /dev/zero | tr '\\000' x\n#{}\nprintf '\\nscript-end'\nexit 7\n", " padding".repeat(48000));
    let streamed = tools.run_script(&body, &AtomicBool::new(false), Duration::from_secs(10));
    assert!(streamed.error.is_none(), "{:?}", streamed.error);
    assert!(streamed.stdout.starts_with("Grüße / stdin"));
    assert!(streamed.stdout.ends_with("script-end"));
    assert_eq!(streamed.stderr, "script-stderr");
    assert_eq!(streamed.exit_code, Some(7));
    assert!(!streamed.truncated);
    assert!(tools
        .run_script("echo never", &AtomicBool::new(true), Duration::from_secs(5))
        .error
        .unwrap()
        .contains("abgebrochen"));
    tools.close();
    assert!(tools
        .run_command(
            "echo never",
            &AtomicBool::new(false),
            Duration::from_secs(5)
        )
        .error
        .unwrap()
        .contains("beendet"));
}
