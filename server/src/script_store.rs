//! The server persists only an encrypted library file. It never executes scripts.
use super::*;
use axum::http::header::IF_NONE_MATCH;
fn path(state: &AppState, team: &str) -> PathBuf {
    file_path(state, team).with_extension("scripts.json")
}
pub async fn read(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &team, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match fs::read(path(&state, &team)) {
        Ok(bytes) => {
            let etag = revision(&bytes);
            if headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
                return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
            }
            (
                [
                    (header::ETAG, etag),
                    (header::CONTENT_TYPE, "application/json".into()),
                    (header::CACHE_CONTROL, "no-store".into()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
pub async fn write(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    if !authorized(&state, &team, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(envelope) = serde_json::from_slice::<Envelope>(&bytes) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if envelope.version != 1
        || STANDARD.decode(&envelope.nonce).map(|v| v.len()).ok() != Some(24)
        || !STANDARD
            .decode(&envelope.ciphertext)
            .is_ok_and(|v| v.len() >= 16)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(_guard) = state.write_lock.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let path = path(&state, &team);
    let previous = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let matches = match previous {
        Some(old) => headers
            .get(header::IF_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == revision(&old)),
        None => headers.get(IF_NONE_MATCH).is_some_and(|v| v == "*"),
    };
    if !matches {
        return StatusCode::PRECONDITION_FAILED.into_response();
    }
    let write = || -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&state.directory)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    };
    if write().is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (StatusCode::OK, [(header::ETAG, revision(&bytes))]).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    fn state(directory: &std::path::Path) -> AppState {
        AppState {
            directory: directory.into(),
            tokens: Arc::new(HashMap::from([(
                "ops".into(),
                Sha256::digest("integration-token-at-least-32-characters").into(),
            )])),
            write_lock: Arc::new(Mutex::new(())),
            notifications: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    #[tokio::test]
    async fn encrypted_library_has_independent_authentication_conditional_reads_and_cas() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let app = router(state.clone());
        let uri = "/v1/teams/ops/scripts";
        let request =
            |method: &str, token: &str, condition: Option<(&str, &str)>, body: Vec<u8>| {
                let mut request = Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"));
                if let Some((key, value)) = condition {
                    request = request.header(key, value);
                }
                request.body(Body::from(body)).unwrap()
            };
        let token = "integration-token-at-least-32-characters";
        assert_eq!(
            app.clone()
                .oneshot(request("GET", "wrong", None, vec![]))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone()
                .oneshot(request("GET", token, None, vec![]))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "PUT",
                    token,
                    Some(("if-none-match", "*")),
                    b"plaintext bash".to_vec()
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let bytes = serde_json::to_vec(
            &noryn_core::crypto::seal(&[11; 32], b"secret body", b"noryn:scripts:v1:ops").unwrap(),
        )
        .unwrap();
        let response = app
            .clone()
            .oneshot(request(
                "PUT",
                token,
                Some(("if-none-match", "*")),
                bytes.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()[header::ETAG]
            .to_str()
            .unwrap()
            .to_string();
        assert!(!file_path(&state, "ops").exists());
        assert!(path(&state, "ops").exists());
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "GET",
                    token,
                    Some(("if-none-match", &etag)),
                    vec![]
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_MODIFIED
        );
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "PUT",
                    token,
                    Some(("if-none-match", "*")),
                    bytes.clone()
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::PRECONDITION_FAILED
        );
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "PUT",
                    token,
                    Some(("if-match", "wrong")),
                    bytes.clone()
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::PRECONDITION_FAILED
        );
        assert_eq!(
            app.oneshot(request("PUT", token, Some(("if-match", &etag)), bytes))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "Requires localhost TCP listener; cargo test -p noryn-sync -- --ignored"]
    async fn real_script_clients_sync_ciphertext_merge_conflicts_and_preserve_missing_server_data()
    {
        use noryn_core::{
            model::{Team, VaultData},
            script_sync,
            scripts::{self, Cache, Entry},
        };
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = router(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::task::spawn_blocking(move || {
            let team = Team {
                id: "ops".into(),
                name: "Operations".into(),
                member_name: "Alice".into(),
                server_url: url,
                token: "integration-token-at-least-32-characters".into(),
                key: STANDARD.encode([29; 32]),
                revision: None,
                dirty: false,
                live_sync: false,
                sync_base: None,
            };
            let sync = |team: &Team, cache: &Cache| {
                script_sync::synchronize(team, cache, false, false, false, || Ok(()))
            };
            let mut alice = VaultData::default();
            alice.teams.push(team.clone());
            let id = "74bc17f8-a5ae-413c-94ec-9b48a53a68c8".to_owned();
            scripts::save(
                &mut alice,
                "ops",
                Entry {
                    id: id.clone(),
                    parent_id: None,
                    kind: "script".into(),
                    name: "secret-diagnostic-name".into(),
                    description: "private description".into(),
                    body: "echo secret-bash-body".into(),
                    updated_at: 0,
                    updated_by: String::new(),
                },
                None,
            )
            .unwrap();
            let a = sync(&team, &alice.script_libraries["ops"]).unwrap();
            let disk = fs::read(path(&state, "ops")).unwrap();
            let disk_text = String::from_utf8(disk.clone()).unwrap();
            for text in [
                "secret-diagnostic-name",
                "secret-bash-body",
                "private description",
                "Alice",
            ] {
                assert!(!disk_text.contains(text));
            }
            let mut bob_team = team.clone();
            bob_team.member_name = "Bob".into();
            let mut b = sync(&bob_team, &Cache::default()).unwrap();
            assert!(b.library == a.library);
            let unchanged = sync(&team, &a).unwrap();
            assert_eq!(unchanged.revision, a.revision);
            assert_eq!(fs::read(path(&state, "ops")).unwrap(), disk);
            let mut aedit = a.clone();
            aedit.library.entries[0].name = "Alice renamed".into();
            let a = sync(&team, &aedit).unwrap();
            b.library.entries[0].description = "Bob's docs".into();
            let b = sync(&bob_team, &b).unwrap();
            assert_eq!(b.library.entries[0].name, "Alice renamed");
            assert_eq!(b.library.entries[0].description, "Bob's docs");
            let mut a = sync(&team, &a).unwrap();
            let mut b = b;
            a.library.entries[0].body = "echo Alice".into();
            b.library.entries[0].body = "echo Bob".into();
            let a = sync(&team, &a).unwrap();
            let before = fs::read(path(&state, "ops")).unwrap();
            assert!(sync(&bob_team, &b)
                .err()
                .unwrap()
                .to_string()
                .contains("Konflikt"));
            assert_eq!(fs::read(path(&state, "ops")).unwrap(), before);
            let both =
                script_sync::synchronize(&bob_team, &b, false, false, true, || Ok(())).unwrap();
            assert!(both.library.entries.iter().any(|e| e.body == "echo Alice"));
            assert!(both
                .library
                .entries
                .iter()
                .any(|e| e.body == "echo Bob" && e.id != id));
            let mut wrong = team.clone();
            wrong.key = STANDARD.encode([30; 32]);
            assert!(sync(&wrong, &Cache::default()).is_err());
            assert!(sync(&wrong, &a).err().unwrap().to_string().contains("Ziel"));
            let before = fs::read(path(&state, "ops")).unwrap();
            let mut blocked = both.clone();
            blocked.library.entries[0].name = "never upload".into();
            assert!(script_sync::synchronize(
                &team,
                &blocked,
                false,
                false,
                false,
                || anyhow::bail!("locked")
            )
            .is_err());
            assert_eq!(fs::read(path(&state, "ops")).unwrap(), before);
            fs::remove_file(path(&state, "ops")).unwrap();
            assert!(sync(&team, &both)
                .err()
                .unwrap()
                .to_string()
                .contains("erneute Veröffentlichung"));
            assert!(!path(&state, "ops").exists());
            let restored =
                script_sync::synchronize(&team, &both, false, true, false, || Ok(())).unwrap();
            assert!(restored.library == both.library);
            assert!(path(&state, "ops").exists());
            let mut invalid = team;
            invalid.server_url = "http://not-local.example".into();
            assert!(sync(&invalid, &Cache::default()).is_err());
        })
        .await
        .unwrap();
        server.abort();
    }
}
