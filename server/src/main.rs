mod data_directory;

use anyhow::{ensure, Context, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::get,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use noryn_core::{crypto::Envelope, sync::validate_team_id};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use subtle::ConstantTimeEq;
use tokio::sync::watch;

#[derive(Clone)]
struct AppState {
    directory: PathBuf,
    tokens: Arc<HashMap<String, [u8; 32]>>,
    write_lock: Arc<Mutex<()>>,
    notifications: Arc<Mutex<HashMap<String, watch::Sender<String>>>>,
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
    if let Ok(notifications) = state.notifications.lock() {
        if let Some(sender) = notifications.get(&team) {
            sender.send_replace(revision(&bytes));
        }
    }
    (StatusCode::OK, [(header::ETAG, revision(&bytes))]).into_response()
}

async fn events(
    State(state): State<AppState>,
    Path(team): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &team, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let receiver = {
        // Subscribe and read the initial revision in the same order as writes.
        let Ok(_guard) = state.write_lock.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let current = match fs::read(file_path(&state, &team)) {
            Ok(bytes) => revision(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "none".into(),
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let Ok(mut notifications) = state.notifications.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let sender = notifications
            .entry(team)
            .or_insert_with(|| watch::channel(current.clone()).0);
        sender.send_replace(current);
        sender.subscribe()
    };
    let stream =
        futures_util::stream::unfold((true, receiver), |(initial, mut receiver)| async move {
            if !initial && receiver.changed().await.is_err() {
                return None;
            }
            let revision = receiver.borrow_and_update().clone();
            let event = Event::default().event("revision").data(revision);
            Some((Ok::<_, std::convert::Infallible>(event), (false, receiver)))
        });
    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(30)))
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-accel-buffering", "no".parse().unwrap());
    response
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/teams/{team}/events", get(events))
        .route(
            "/v1/teams/{team}/snapshot",
            get(read_snapshot).put(write_snapshot),
        )
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = std::env::var("NORYN_SYNC_TEAMS")
        .context("NORYN_SYNC_TEAMS=team-id:token (Token mindestens 32 Zeichen) setzen")?;
    let mut tokens = HashMap::new();
    for entry in config.split(',') {
        let (team, token) = entry
            .split_once(':')
            .context("NORYN_SYNC_TEAMS: ungültiges Format")?;
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
        std::env::var("NORYN_SYNC_DATA").unwrap_or_else(|_| "noryn-sync-data".into()),
    );
    data_directory::prepare(&directory)?;
    let bind = std::env::var("NORYN_SYNC_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!(
        "Noryn Sync: {bind} · {} Teams · HTTPS-Reverse-Proxy für externen Zugriff erforderlich",
        tokens.len()
    );
    let state = AppState {
        directory,
        tokens: Arc::new(tokens),
        write_lock: Arc::new(Mutex::new(())),
        notifications: Arc::new(Mutex::new(HashMap::new())),
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
            notifications: Arc::new(Mutex::new(HashMap::new())),
        });
        let uri = "/v1/teams/ops/snapshot";
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let envelope = noryn_core::crypto::seal(&[7; 32], b"team snapshot", b"ops").unwrap();
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

    #[tokio::test]
    async fn live_events_require_authentication_and_only_send_changed_revisions() {
        use http_body_util::BodyExt;
        let directory = tempfile::tempdir().unwrap();
        let token = "live-test-token-at-least-32-characters";
        let state = AppState {
            directory: directory.path().to_path_buf(),
            tokens: Arc::new(HashMap::from([(
                "ops".into(),
                Sha256::digest(token).into(),
            )])),
            write_lock: Arc::new(Mutex::new(())),
            notifications: Arc::new(Mutex::new(HashMap::new())),
        };
        let app = router(state);
        let uri = "/v1/teams/ops/events";
        let unauthorized = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let mut body = response.into_body();
        let initial = tokio::time::timeout(std::time::Duration::from_secs(1), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert!(std::str::from_utf8(&initial)
            .unwrap()
            .contains("data: none"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), body.frame())
                .await
                .is_err()
        );
        let envelope = noryn_core::crypto::seal(&[7; 32], b"encrypted data", b"ops").unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/teams/ops/snapshot")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("If-None-Match", "*")
                    .body(Body::from(serde_json::to_vec(&envelope).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let revision = response.headers()[header::ETAG]
            .to_str()
            .unwrap()
            .to_owned();
        let changed = tokio::time::timeout(std::time::Duration::from_secs(1), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        let changed = std::str::from_utf8(&changed).unwrap();
        assert!(changed.contains("event: revision") && changed.contains(&revision));
        assert!(!changed.contains("encrypted data") && !changed.contains(token));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), body.frame())
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "Requires a localhost TCP listener; run cargo test -p noryn-sync -- --ignored"]
    async fn encrypted_client_server_sync_preserves_local_identity_and_rejects_conflicts() {
        use noryn_core::model::{Credential, Node, Team, VaultData};
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
            notifications: Arc::new(Mutex::new(HashMap::new())),
        };
        let app = router(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::task::spawn_blocking(move || {
            let key = STANDARD.encode(noryn_core::crypto::random_bytes::<32>());
            let team = Team {
                id: "ops".into(),
                name: "Operations".into(),
                member_name: "Alice".into(),
                server_url: url,
                token: token.into(),
                key,
                revision: None,
                dirty: true,
                live_sync: false,
                sync_base: None,
            };
            let mut alice = VaultData::default();
            alice.teams.push(team.clone());
            alice
                .upsert_node(Node {
                    id: "shared-host".into(),
                    parent_id: None,
                    workspace: "ops".into(),
                    kind: "connection".into(),
                    name: "Private metadata".into(),
                    protocol: "ssh".into(),
                    host: "secret.example.internal".into(),
                    port: 22,
                    initial_path: String::new(),
                    notes: "first".into(),
                    tags: vec![],
                    favorite: false,
                })
                .unwrap();
            alice
                .upsert_credential(Credential {
                    id: "team-account".into(),
                    name: "Shared operations".into(),
                    username: "ops".into(),
                    kind: "password".into(),
                    secret: "shared-secret-stays-encrypted".into(),
                    password: String::new(),
                    passphrase: String::new(),
                    team_id: Some("ops".into()),
                })
                .unwrap();
            noryn_core::sync::sync(&mut alice, "ops", "push").unwrap();
            let stored = fs::read_to_string(file_path(&state, "ops")).unwrap();
            assert!(!stored.contains("secret.example.internal"));
            assert!(!stored.contains("Private metadata"));
            assert!(!stored.contains("shared-secret-stays-encrypted"));
            assert!(!stored.contains("Alice"));
            assert!(!stored.contains("activity"));
            let mut bob = VaultData::default();
            let mut bob_team = team;
            bob_team.member_name = "Bob".into();
            bob.teams.push(bob_team);
            bob.credentials.push(Credential {
                id: "bob-account".into(),
                name: "Bob".into(),
                username: "bob".into(),
                kind: "password".into(),
                secret: "bob-private-password".into(),
                passphrase: "".into(),
                password: String::new(),
                team_id: None,
            });
            bob.bindings
                .insert("shared-host".into(), "bob-account".into());
            noryn_core::sync::sync(&mut bob, "ops", "pull").unwrap();
            assert_eq!(bob.credential_for("shared-host").unwrap().username, "bob");
            assert_eq!(
                bob.credentials
                    .iter()
                    .find(|c| c.id == "team-account")
                    .unwrap()
                    .secret,
                "shared-secret-stays-encrypted"
            );
            assert_eq!(alice.teams[0].revision, bob.teams[0].revision);
            assert_eq!(alice.team_activity["ops"], bob.team_activity["ops"]);
            assert_eq!(bob.teams[0].member_name, "Bob");
            let mut edited = alice.nodes[0].clone();
            edited.notes = "second".into();
            alice.upsert_node(edited).unwrap();
            noryn_core::sync::sync(&mut alice, "ops", "push").unwrap();
            let mut concurrent = bob.nodes[0].clone();
            concurrent.notes = "concurrent edit".into();
            bob.upsert_node(concurrent).unwrap();
            assert!(noryn_core::sync::sync(&mut bob, "ops", "push")
                .unwrap_err()
                .to_string()
                .contains("Sync-Konflikt"));
            noryn_core::sync::sync(&mut bob, "ops", "pull").unwrap();
            assert_eq!(bob.nodes[0].notes, "second");
            assert_eq!(bob.credential_for("shared-host").unwrap().username, "bob");
            alice.remove_credential("team-account").unwrap();
            noryn_core::sync::sync(&mut alice, "ops", "push").unwrap();
            noryn_core::sync::sync(&mut bob, "ops", "pull").unwrap();
            assert_eq!(bob.credentials.len(), 1);
            assert_eq!(bob.credentials[0].id, "bob-account");
            // Both members edit the same host independently while offline.
            alice.teams[0].live_sync = true;
            bob.teams[0].live_sync = true;
            let mut alice_edit = alice.nodes[0].clone();
            alice_edit.port = 2222;
            alice.upsert_node(alice_edit).unwrap();
            let mut bob_edit = bob.nodes[0].clone();
            bob_edit.name = "Bob's host name".into();
            bob.upsert_node(bob_edit).unwrap();
            assert!(noryn_core::auto_sync::reconcile(&mut alice, "ops")
                .unwrap()
                .conflicts
                .is_empty());
            assert!(noryn_core::auto_sync::reconcile(&mut bob, "ops")
                .unwrap()
                .conflicts
                .is_empty());
            assert!(noryn_core::auto_sync::reconcile(&mut alice, "ops")
                .unwrap()
                .conflicts
                .is_empty());
            assert_eq!(alice.nodes[0].name, "Bob's host name");
            assert_eq!(bob.nodes[0].port, 2222);
            assert_eq!(bob.credential_for("shared-host").unwrap().username, "bob");
            let before_noop = fs::read(file_path(&state, "ops")).unwrap();
            noryn_core::auto_sync::reconcile(&mut alice, "ops").unwrap();
            assert_eq!(fs::read(file_path(&state, "ops")).unwrap(), before_noop);
            let mut blocked = alice.clone();
            let mut blocked_edit = blocked.nodes[0].clone();
            blocked_edit.notes = "pending but rejected by reserved namespace".into();
            blocked.upsert_node(blocked_edit).unwrap();
            let before_blocked = serde_json::to_value(&blocked).unwrap();
            assert!(noryn_core::auto_sync::reconcile_checked(
                &mut blocked,
                "ops",
                |_| anyhow::bail!("reserved personal ID")
            )
            .is_err());
            assert_eq!(serde_json::to_value(&blocked).unwrap(), before_blocked);
            assert_eq!(fs::read(file_path(&state, "ops")).unwrap(), before_noop);
            let mut alice_edit = alice.nodes[0].clone();
            alice_edit.notes = "alice conflicting note".into();
            alice.upsert_node(alice_edit).unwrap();
            let mut bob_edit = bob.nodes[0].clone();
            bob_edit.notes = "bob conflicting note".into();
            bob.upsert_node(bob_edit).unwrap();
            noryn_core::auto_sync::reconcile(&mut alice, "ops").unwrap();
            let before_conflict = fs::read(file_path(&state, "ops")).unwrap();
            let local_before = serde_json::to_value(&bob).unwrap();
            let conflict = noryn_core::auto_sync::reconcile(&mut bob, "ops").unwrap();
            assert_eq!(conflict.conflicts[0].fields, vec!["notes"]);
            assert_eq!(serde_json::to_value(&bob).unwrap(), local_before);
            assert_eq!(fs::read(file_path(&state, "ops")).unwrap(), before_conflict);
            noryn_core::sync::sync(&mut bob, "ops", "pull").unwrap();
            let shared = Credential {
                id: "revoked-account".into(),
                name: "Temporary share".into(),
                username: "ops".into(),
                kind: "password".into(),
                secret: "revoked shared password".into(),
                password: String::new(),
                passphrase: String::new(),
                team_id: Some("ops".into()),
            };
            alice.upsert_credential(shared.clone()).unwrap();
            noryn_core::auto_sync::reconcile(&mut alice, "ops").unwrap();
            noryn_core::auto_sync::reconcile(&mut bob, "ops").unwrap();
            assert!(bob.credentials.iter().any(|c| c.id == "revoked-account"));
            let mut unshared = shared;
            unshared.team_id = None;
            alice.upsert_credential(unshared).unwrap();
            assert!(noryn_core::auto_sync::reconcile(&mut alice, "ops")
                .unwrap()
                .conflicts
                .is_empty());
            assert!(noryn_core::auto_sync::reconcile(&mut bob, "ops")
                .unwrap()
                .conflicts
                .is_empty());
            assert!(!bob.credentials.iter().any(|c| c.id == "revoked-account"));
            assert!(alice
                .credentials
                .iter()
                .any(|c| c.id == "revoked-account" && c.team_id.is_none()));
            // Existing encrypted snapshots from the previous application remain readable.
            let legacy = serde_json::to_vec(
                &serde_json::json!({"version": 1, "teamId": "ops", "nodes": alice.nodes}),
            )
            .unwrap();
            let key = noryn_core::crypto::decode_key(&alice.teams[0].key).unwrap();
            let envelope = noryn_core::crypto::seal(
                &key,
                &legacy,
                noryn_core::migration::previous_team_context("ops").as_bytes(),
            )
            .unwrap();
            fs::write(
                file_path(&state, "ops"),
                serde_json::to_vec(&envelope).unwrap(),
            )
            .unwrap();
            noryn_core::sync::sync(&mut bob, "ops", "pull").unwrap();
            assert_eq!(bob.credential_for("shared-host").unwrap().username, "bob");
            let notes_before_wrong_key = bob.nodes[0].notes.clone();
            bob.teams[0].key = STANDARD.encode([8; 32]);
            assert!(noryn_core::sync::sync(&mut bob, "ops", "pull").is_err());
            assert_eq!(bob.nodes[0].notes, notes_before_wrong_key);
        })
        .await
        .unwrap();
        server.abort();
    }
}
