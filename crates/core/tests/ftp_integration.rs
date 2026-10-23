use noryn_core::{ftp::Remote, model::Credential};
use std::fs;
use suppaftp::native_tls::{Certificate, Protocol, TlsConnector};

fn credential() -> Credential {
    Credential {
        id: "test".into(),
        name: "FTP test".into(),
        username: "test-user".into(),
        kind: "password".into(),
        password: "test-password".into(),
        secret: String::new(),
        passphrase: String::new(),
        team_id: None,
    }
}
fn port(mode: &str) -> u16 {
    std::env::var(format!("NORYN_TEST_FTP_{mode}"))
        .unwrap()
        .parse()
        .unwrap()
}
fn trusted_connector() -> TlsConnector {
    TlsConnector::builder()
        .min_protocol_version(Some(Protocol::Tlsv12))
        .add_root_certificate(
            Certificate::from_pem(&fs::read(std::env::var("NORYN_TEST_FTP_CA").unwrap()).unwrap())
                .unwrap(),
        )
        .build()
        .unwrap()
}

#[test]
#[ignore = "Run python scripts/test-ftp.py with scripts/requirements-ftp.txt in a local venv"]
fn actual_ftp_and_ftps_round_trip_with_certificate_checks_and_legacy_listings() {
    for mode in ["MLSD", "POSIX", "DOS", "TLS"] {
        let mut remote = Remote::connect_with_tls(
            "127.0.0.1",
            port(mode),
            &credential(),
            (mode == "TLS").then(trusted_connector),
        )
        .unwrap_or_else(|e| panic!("{mode}: {e:#}"));
        assert_eq!(remote.home(), "/");
        let listing = remote.list("/").unwrap();
        assert!(listing[0].is_dir);
        assert!(listing.iter().any(|f| f.name == "ä space; file.txt"));
        assert!(listing.iter().any(|f| f.name == ".hidden"));
        assert!(
            listing
                .iter()
                .find(|f| f.name == "source.bin")
                .unwrap()
                .modified
                > 0
        );
        remote.mkdir("/new folder").unwrap();
        assert!(remote.list("/new folder").unwrap().is_empty());
        assert!(remote.list("/does-not-exist").is_err());
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("upload.bin");
        let bytes: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 256) as u8).collect();
        fs::write(&source, &bytes).unwrap();
        assert_eq!(
            remote
                .upload(&source, "/new folder/binary ä ; upload.bin")
                .unwrap(),
            bytes.len() as u64
        );
        assert!(remote
            .upload(&source, "/new folder/binary ä ; upload.bin")
            .is_err());
        let output = temp.path().join("output.bin");
        assert_eq!(
            remote
                .download_file("/new folder/binary ä ; upload.bin", &output, false)
                .unwrap(),
            bytes.len() as u64
        );
        assert_eq!(fs::read(&output).unwrap(), bytes);
        fs::write(&output, b"keep existing file").unwrap();
        assert!(remote.download_file("/source.bin", &output, false).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"keep existing file");
        remote.download_file("/source.bin", &output, true).unwrap();
        assert_eq!(fs::metadata(&output).unwrap().len(), 256 * 1024);
        assert!(remote.download_file("/missing.bin", &output, true).is_err());
        assert_eq!(fs::metadata(&output).unwrap().len(), 256 * 1024);
        assert!(remote.mkdir("/bad\r\nDELE /source.bin").is_err());
        assert!(!remote
            .list("/new folder")
            .unwrap()
            .iter()
            .any(|f| f.name.starts_with(".noryn-upload-")));
    }
    let mut invalid = credential();
    invalid.password = "wrong-password".into();
    assert!(Remote::connect("127.0.0.1", port("MLSD"), &invalid, false).is_err());
    invalid.username = "test-user\r\nDELE /source.bin".into();
    assert!(Remote::connect("127.0.0.1", port("MLSD"), &invalid, false).is_err());
    // FTPS must reject untrusted certs, hostname mismatch, and unsupported AUTH TLS.
    assert!(Remote::connect("127.0.0.1", port("TLS"), &credential(), true).is_err());
    assert!(Remote::connect_with_tls(
        "127.0.0.1",
        port("TLS_WRONG_HOST"),
        &credential(),
        Some(trusted_connector())
    )
    .is_err());
    assert!(Remote::connect("127.0.0.1", port("MLSD"), &credential(), true).is_err());
    let mut key_only = credential();
    key_only.kind = "key".into();
    key_only.password.clear();
    assert!(Remote::connect("127.0.0.1", port("MLSD"), &key_only, false).is_err());
}
