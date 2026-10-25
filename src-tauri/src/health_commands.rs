use super::{task, vault_action, AppState, Result};
use noryn_core::{
    login_health::{LoginHealth, LoginPoint},
    model::{Node, VaultView},
    vault::Vault,
};
use serde::Serialize;
use std::sync::{atomic::Ordering, Arc};
use tauri::State;
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebScheme {
    Http,
    Https,
}
fn web_url(port: u16, scheme: WebScheme) -> anyhow::Result<String> {
    anyhow::ensure!(port > 0, "Weiterleitung hat keinen lokalen Port");
    Ok(format!(
        "{}://127.0.0.1:{port}/",
        match scheme {
            WebScheme::Http => "http",
            WebScheme::Https => "https",
        }
    ))
}

#[tauri::command]
pub fn ssh_forward_open(
    state: State<'_, AppState>,
    id: String,
    forward_id: String,
    scheme: WebScheme,
) -> Result<()> {
    let tools = super::tools_for(&state, &id)?;
    if tools.is_closed() {
        return Err("SSH-Sitzung ist beendet".into());
    }
    let forward = tools
        .list_forwards()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|f| f.id == forward_id)
        .ok_or("Weiterleitung nicht gefunden")?;
    let url = web_url(forward.local_port, scheme).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[derive(Clone)]
pub(super) struct Context {
    pub node: Node,
    pub identity: String,
    pub previous: Option<LoginPoint>,
    pub epoch: u64,
}

pub(super) fn context(
    vault: &Vault,
    node: &Node,
    username: &str,
    fingerprint: &str,
    epoch: u64,
) -> anyhow::Result<Context> {
    let identity = serde_json::to_string(&(&node.host, node.port, username, fingerprint))?;
    let previous = vault
        .data
        .health_logins
        .get(&node.id)
        .filter(|p| p.identity == identity)
        .cloned();
    Ok(Context {
        node: node.clone(),
        identity,
        previous,
        epoch,
    })
}

fn remember(vault: &mut Vault, context: &Context, report: &LoginHealth) -> anyhow::Result<()> {
    if vault.data.nodes.iter().find(|n| n.id == context.node.id) != Some(&context.node) {
        return Ok(());
    }
    if vault
        .data
        .health_logins
        .get(&context.node.id)
        .is_some_and(|p| {
            p.identity == context.identity
                && (p.server_time == report.captured_at
                    || (p.server_time > report.captured_at
                        && context
                            .previous
                            .as_ref()
                            .map(|previous| previous.server_time)
                            != Some(p.server_time)))
        })
    {
        return Ok(());
    }
    vault.change(|data| {
        data.health_logins.insert(
            context.node.id.clone(),
            LoginPoint {
                identity: context.identity.clone(),
                server_time: report.captured_at,
                active_services: report.active_services.clone(),
            },
        );
        noryn_core::login_health::validate_logins(data)
    })
}

#[derive(Serialize)]
pub struct HealthResult {
    report: LoginHealth,
    view: VaultView,
}

#[tauri::command]
pub async fn ssh_login_health(state: State<'_, AppState>, id: String) -> Result<HealthResult> {
    let state = state.inner().clone();
    let (tools, context) = {
        let connections = state
            .connections
            .lock()
            .map_err(|_| "Sitzungen nicht verfügbar")?;
        let connection = connections.get(&id).ok_or("Sitzung geschlossen")?;
        (
            connection.tools.clone().ok_or("SSH-Sitzung erforderlich")?,
            connection.health.clone(),
        )
    };
    task(move || {
        let report = tools
            .login_health(context.as_ref().and_then(|c| c.previous.as_ref()))
            .map_err(|e| format!("{e:#}"))?;
        let view = vault_action(&state, |vault| {
            anyhow::ensure!(
                !tools.is_closed()
                    && state
                        .connections
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Sitzungen nicht verfügbar"))?
                        .get(&id)
                        .and_then(|c| c.tools.as_ref())
                        .is_some_and(|current| Arc::ptr_eq(current, &tools)),
                "Sitzung wurde inzwischen beendet"
            );
            if let Some(context) = &context {
                anyhow::ensure!(
                    context.epoch == state.vault_epoch.load(Ordering::Acquire),
                    "Tresor wurde inzwischen gesperrt oder ersetzt"
                );
                remember(vault, context, &report)?;
            }
            Ok(vault.data.view())
        })?;
        Ok(HealthResult { report, view })
    })
    .await
}

#[tauri::command]
pub async fn ssh_listening_ports(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<noryn_core::login_health::ListeningPort>> {
    let tools = super::tools_for(&state, &id)?;
    task(move || tools.listening_ports().map_err(|e| format!("{e:#}"))).await
}

#[tauri::command]
pub async fn ssh_forward_probe(
    state: State<'_, AppState>,
    id: String,
    forward_id: String,
) -> Result<String> {
    let tools = super::tools_for(&state, &id)?;
    task(move || {
        tools
            .probe_forward(&forward_id)
            .map_err(|e| format!("{e:#}"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forwarded_web_links_are_restricted_to_http_loopback_addresses() {
        assert_eq!(
            web_url(5432, WebScheme::Http).unwrap(),
            "http://127.0.0.1:5432/"
        );
        assert_eq!(
            web_url(8443, WebScheme::Https).unwrap(),
            "https://127.0.0.1:8443/"
        );
        assert!(web_url(0, WebScheme::Http).is_err());
        assert!(serde_json::from_str::<WebScheme>("\"file\"").is_err());
    }
    fn report(at: u64) -> LoginHealth {
        LoginHealth {
            captured_at: at,
            uptime: 100.,
            cores: 2,
            previous_login: None,
            services_available: true,
            journal_available: true,
            ports_available: true,
            services: vec![],
            events: vec![],
            updates: None,
            peaks: vec![],
            load_samples: 0,
            load_window: 0,
            ports: vec![],
            notes: vec![],
            active_services: vec!["sshd.service".into()],
        }
    }
    #[test]
    fn login_comparisons_are_personal_match_identity_and_ignore_stale_results() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vault.json");
        let mut vault = Vault::open(&path, "test-only vault password").unwrap();
        let node: Node=serde_json::from_value(serde_json::json!({"id":"host", "parentId":null,"workspace":"private","kind":"connection","name":"Host","protocol":"ssh","host":"example.test","port":22,"initialPath":"","notes":"","tags":[],"favorite":false})).unwrap();
        vault.change(|data| data.upsert_node(node.clone())).unwrap();
        let first = context(&vault, &node, "alice", "fingerprint-a", 0).unwrap();
        assert!(first.previous.is_none());
        remember(&mut vault, &first, &report(2000)).unwrap();
        let same = context(&vault, &node, "alice", "fingerprint-a", 0).unwrap();
        assert_eq!(same.previous.as_ref().unwrap().server_time, 2000);
        assert!(context(&vault, &node, "bob", "fingerprint-a", 0)
            .unwrap()
            .previous
            .is_none());
        assert!(context(&vault, &node, "alice", "fingerprint-b", 0)
            .unwrap()
            .previous
            .is_none());
        let archive =
            noryn_core::backup::select(&vault.data, noryn_core::backup::Scope::All).unwrap();
        assert!(archive.data.health_logins.is_empty());
        remember(&mut vault, &first, &report(1500)).unwrap();
        assert_eq!(vault.data.health_logins["host"].server_time, 2000);
        // A real clock rollback against the captured baseline can reset it.
        remember(&mut vault, &same, &report(1500)).unwrap();
        assert_eq!(vault.data.health_logins["host"].server_time, 1500);
        let revision = vault.data.local_revision;
        remember(&mut vault, &same, &report(1500)).unwrap();
        assert_eq!(vault.data.local_revision, revision);
        let mut changed = node.clone();
        changed.host = "other.test".into();
        vault.change(|data| data.upsert_node(changed)).unwrap();
        let revision = vault.data.local_revision;
        remember(&mut vault, &same, &report(3000)).unwrap();
        assert_eq!(vault.data.local_revision, revision);
        drop(vault);
        let reopened = Vault::open(&path, "test-only vault password").unwrap();
        assert_eq!(reopened.data.health_logins["host"].server_time, 1500);
        assert!(!std::fs::read_to_string(path)
            .unwrap()
            .contains("sshd.service"));
    }
}
