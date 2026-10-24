use noryn_core::{model::Credential, ssh};

fn credential(username: &str) -> Credential {
    Credential {
        id: "test".into(),
        name: "OTP test".into(),
        username: username.into(),
        kind: if username == "key-user" {
            "key"
        } else {
            "password"
        }
        .into(),
        secret: if username == "key-user" {
            std::fs::read_to_string(std::env::var("NORYN_TEST_OTP_KEY").unwrap()).unwrap()
        } else {
            String::new()
        },
        password: if username == "password-user" {
            "stored-login-password"
        } else {
            ""
        }
        .into(),
        passphrase: String::new(),
        team_id: None,
    }
}

#[test]
#[ignore = "Run node scripts/test-otp.mjs for an isolated localhost SSH challenge server"]
fn actual_keyboard_interactive_handles_key_password_multiple_prompts_and_cancellation() {
    for profile in ["NORYN_TEST_OTP_PORT", "NORYN_TEST_OTP_MODERN_PORT"] {
        eprintln!("OTP transport: {profile}");
        let port = std::env::var(profile).unwrap().parse().unwrap();
        exercise_authentication(port);
    }
}

fn exercise_authentication(port: u16) {
    for username in ["key-user", "password-user", "multi-user"] {
        let (session, _) = ssh::handshake("127.0.0.1", port)
            .unwrap_or_else(|error| panic!("{username} handshake: {error:#}"));
        let credential = credential(username);
        let mut prompted = 0;
        ssh::authenticate_interactive(&session, &credential, |_, instructions, prompts| {
            prompted += 1;
            assert!(instructions.contains("authenticator"));
            assert!(prompts.iter().all(|p| !p.echo));
            assert!(prompts.last().unwrap().text.contains("Verification code"));
            Ok(if username == "multi-user" {
                vec!["interactive-password".into(), "123456".into()]
            } else {
                vec!["123456".into()]
            })
        })
        .unwrap_or_else(|error| panic!("{username}: {error:#}"));
        assert_eq!(prompted, 1);
        assert!(session.authenticated());
    }
    let (session, _) = ssh::handshake("127.0.0.1", port).unwrap();
    let error = ssh::authenticate_interactive(&session, &credential("cancel-user"), |_, _, _| {
        anyhow::bail!("Anmeldung abgebrochen")
    })
    .unwrap_err();
    assert!(error.to_string().contains("abgebrochen"));
    assert!(!session.authenticated());
    let (session, _) = ssh::handshake("127.0.0.1", port).unwrap();
    assert!(
        ssh::authenticate_interactive(&session, &credential("bad-user"), |_, _, _| Ok(vec![
            "wrong-code".into()
        ]))
        .is_err()
    );
    assert!(!session.authenticated());
}
