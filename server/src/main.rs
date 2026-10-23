use anyhow::{ensure, Context, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use harbor_core::{crypto::Envelope, sync::validate_team_id};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use subtle::ConstantTimeEq;

#[derive(Clone)]
struct AppState {
    directory: PathBuf,
    tokens: Arc<HashMap<String, [u8; 32]>>,
    write_lock: Arc<Mutex<()>>,
}

fn authorized(state: &AppState, team: &str, headers: &HeaderMap) -> bool {
    if validate_team_id(team).is_err() {
        return false;
    }
    let Some(expected) = state.tokens.get(team) else {
        return false;
    };
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    let actual: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    bool::from(actual.ct_eq(expected))
}

fn file_path(state: &AppState, team: &str) -> PathBuf {
    state
        .directory
        .join(format!("{:x}.json", Sha256::digest(team.as_bytes())))
}

fn revision(bytes: &[u8]) -> String {
    format!("\"{:x}\"", Sha256::digest(bytes))
}

async fn read_snapshot(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &team, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match fs::read(file_path(&state, &team)) {
        Ok(bytes) => (
            [
                (header::ETAG, revision(&bytes)),
                (header::CONTENT_TYPE, "application/json".into()),
                (header::CACHE_CONTROL, "no-store".into()),
            ],
            bytes,
        )
            .into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn write_snapshot(
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
    let path = file_path(&state, &team);
    let old = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let condition_ok = match &old {
        Some(old) => headers
            .get(header::IF_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == revision(old)),
        None => headers.get(header::IF_NONE_MATCH).is_some_and(|v| v == "*"),
    };
    if !condition_ok {
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

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/v1/teams/{team}/snapshot",
            get(read_snapshot).put(write_snapshot),
        )
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = std::env::var("HARBOR_SYNC_TEAMS")
        .context("HARBOR_SYNC_TEAMS=team-id:token (Token mindestens 32 Zeichen) setzen")?;
    let mut tokens = HashMap::new();
    for entry in config.split(',') {
        let (team, token) = entry
            .split_once(':')
            .context("HARBOR_SYNC_TEAMS: ungültiges Format")?;
        validate_team_id(team)?;
        ensure!(
            token.len() >= 32,
            "Sync-Token muss mindestens 32 Zeichen enthalten"
        );
        ensure!(
            tokens
                .insert(team.to_string(), Sha256::digest(token.as_bytes()).into())
                .is_none(),
            "Doppelte Team-ID"
        );
    }
    let directory = PathBuf::from(
        std::env::var("HARBOR_SYNC_DATA").unwrap_or_else(|_| "harbor-sync-data".into()),
    );
    fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    }
    let bind = std::env::var("HARBOR_SYNC_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!(
        "Harbor Sync: {bind} · {} Teams · HTTPS-Reverse-Proxy für externen Zugriff erforderlich",
        tokens.len()
    );
    let state = AppState {
        directory,
        tokens: Arc::new(tokens),
        write_lock: Arc::new(Mutex::new(())),
    };
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn sync_authentication_and_compare_and_swap() {
        let dir = tempfile::tempdir().unwrap();
        let token = "test-token-at-least-32-characters-long";
        let app = router(AppState {
            directory: dir.path().to_path_buf(),
            tokens: Arc::new(HashMap::from([(
                "ops".into(),
                Sha256::digest(token).into(),
            )])),
            write_lock: Arc::new(Mutex::new(())),
        });
        let uri = "/v1/teams/ops/snapshot";
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let envelope = harbor_core::crypto::seal(&[7; 32], b"team snapshot", b"ops").unwrap();
        let body = serde_json::to_vec(&envelope).unwrap();
        let request = || {
            Request::builder()
                .method("PUT")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::IF_NONE_MATCH, "*")
                .body(Body::from(body.clone()))
                .unwrap()
        };
        let response = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers().get(header::ETAG).unwrap().clone();
        assert_eq!(
            app.clone().oneshot(request()).await.unwrap().status(),
            StatusCode::PRECONDITION_FAILED
        );
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::ETAG).unwrap(), &etag);
        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::IF_MATCH, etag)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "Requires a localhost TCP listener; run cargo test -p harbor-sync -- --ignored"]
    async fn encrypted_client_server_sync_preserves_local_identity_and_rejects_conflicts() {
        use harbor_core::model::{Credential, Node, Team, VaultData};
        let dir = tempfile::tempdir().unwrap();
        let token = "integration-token-at-least-32-characters";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = AppState {
            directory: dir.path().to_path_buf(),
            tokens: Arc::new(HashMap::from([(
                "ops".into(),
                Sha256::digest(token).into(),
            )])),
            write_lock: Arc::new(Mutex::new(())),
        };
        let app = router(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::task::spawn_blocking(move || {
            let key = STANDARD.encode(harbor_core::crypto::random_bytes::<32>());
            let team = Team {
                id: "ops".into(),
                name: "Operations".into(),
                server_url: url,
                token: token.into(),
                key,
                revision: None,
                dirty: true,
            };
            let mut alice = VaultData::default();
            alice.teams.push(team.clone());
            alice.nodes.push(Node {
                id: "shared-host".into(),
                parent_id: None,
                workspace: "ops".into(),
                kind: "connection".into(),
                name: "Private metadata".into(),
                protocol: "ssh".into(),
                host: "secret.example.internal".into(),
                port: 22,
                notes: "first".into(),
                tags: vec![],
                favorite: false,
            });
            harbor_core::sync::sync(&mut alice, "ops", "push").unwrap();
            let stored = fs::read_to_string(file_path(&state, "ops")).unwrap();
            assert!(!stored.contains("secret.example.internal"));
            assert!(!stored.contains("Private metadata"));
            let mut bob = VaultData::default();
            bob.teams.push(team);
            bob.credentials.push(Credential {
                id: "bob-account".into(),
                name: "Bob".into(),
                username: "bob".into(),
                kind: "password".into(),
                secret: "bob-private-password".into(),
                passphrase: "".into(),
                password: String::new(),
            });
            bob.bindings
                .insert("shared-host".into(), "bob-account".into());
            harbor_core::sync::sync(&mut bob, "ops", "pull").unwrap();
            assert_eq!(bob.credential_for("shared-host").unwrap().username, "bob");
            assert_eq!(alice.teams[0].revision, bob.teams[0].revision);
            alice.nodes[0].notes = "second".into();
            harbor_core::sync::sync(&mut alice, "ops", "push").unwrap();
            bob.nodes[0].notes = "concurrent edit".into();
            assert!(harbor_core::sync::sync(&mut bob, "ops", "push")
                .unwrap_err()
                .to_string()
                .contains("Sync-Konflikt"));
            harbor_core::sync::sync(&mut bob, "ops", "pull").unwrap();
            assert_eq!(bob.nodes[0].notes, "second");
            assert_eq!(bob.credential_for("shared-host").unwrap().username, "bob");
            bob.teams[0].key = STANDARD.encode([8; 32]);
            assert!(harbor_core::sync::sync(&mut bob, "ops", "pull").is_err());
            assert_eq!(bob.nodes[0].notes, "second");
        })
        .await
        .unwrap();
        server.abort();
    }
}
