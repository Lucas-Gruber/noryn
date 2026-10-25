use crate::{
    crypto::{self, Envelope},
    model::{validate_credential, validate_nodes, Credential, Node, VaultData},
    team_activity::{self, TeamChange},
};
use anyhow::{bail, ensure, Context, Result};
use reqwest::{
    blocking::Client,
    header::{ETAG, IF_MATCH, IF_NONE_MATCH},
    StatusCode, Url,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamSnapshot {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<crate::commands::SavedCommand>,
    pub version: u8,
    pub team_id: String,
    pub nodes: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<Credential>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity: Vec<TeamChange>,
}

pub fn snapshot(data: &VaultData, team_id: &str) -> TeamSnapshot {
    let mut snapshot = TeamSnapshot {
        commands: data
            .commands
            .iter()
            .filter(|c| c.workspace == team_id)
            .cloned()
            .collect(),
        version: 3,
        team_id: team_id.to_string(),
        nodes: data
            .nodes
            .iter()
            .filter(|n| n.workspace == team_id)
            .cloned()
            .collect(),
        credentials: data
            .credentials
            .iter()
            .filter(|c| c.team_id.as_deref() == Some(team_id))
            .cloned()
            .collect(),
        activity: data.team_activity.get(team_id).cloned().unwrap_or_default(),
    };
    if !snapshot.commands.is_empty()
        || snapshot
            .activity
            .iter()
            .any(|c| c.object == team_activity::ChangeObject::Command)
    {
        snapshot.version = 4;
    }
    canonicalize(&mut snapshot);
    snapshot
}

pub fn canonicalize(snapshot: &mut TeamSnapshot) {
    snapshot.commands.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.credentials.sort_by(|a, b| a.id.cmp(&b.id));
}

pub fn detached(data: &VaultData, team_id: &str) -> Result<VaultData> {
    let team = data
        .teams
        .iter()
        .find(|t| t.id == team_id)
        .context("Team nicht gefunden")?
        .clone();
    let mut work = VaultData {
        teams: vec![team],
        ..Default::default()
    };
    apply_snapshot(&mut work, team_id, snapshot(data, team_id))?;
    Ok(work)
}

pub fn apply_snapshot(data: &mut VaultData, team_id: &str, snapshot: TeamSnapshot) -> Result<()> {
    ensure!(
        [1, 2, 3, 4].contains(&snapshot.version) && snapshot.team_id == team_id,
        "Snapshot gehört zu einem anderen Team oder hat ein unbekanntes Format"
    );
    ensure!(
        snapshot.version >= 2 || snapshot.credentials.is_empty(),
        "Altes Snapshot-Format darf keine Zugangsdaten enthalten"
    );
    ensure!(
        snapshot.credentials.len() <= 10_000,
        "Zu viele Team-Zugangsdaten"
    );
    ensure!(
        snapshot.version >= 3 || snapshot.activity.is_empty(),
        "Altes Snapshot-Format darf kein Änderungsprotokoll enthalten"
    );
    team_activity::validate_changes(&snapshot.activity)?;
    ensure!(
        snapshot.version >= 4 || snapshot.commands.is_empty(),
        "Befehle benötigen Snapshot-Format 4"
    );
    crate::commands::validate(&snapshot.commands)?;
    ensure!(
        snapshot.commands.iter().all(|c| c.workspace == team_id
            && !data
                .commands
                .iter()
                .any(|old| old.id == c.id && old.workspace != team_id)),
        "Befehl gehört zu einem anderen Workspace oder kollidiert mit ihm"
    );
    let mut commands = data.commands.clone();
    if snapshot.version >= 4 {
        commands.retain(|c| c.workspace != team_id);
        commands.extend(snapshot.commands);
    }
    crate::commands::validate(&commands)?;
    let mut ids = std::collections::HashSet::new();
    for credential in &snapshot.credentials {
        validate_credential(credential)?;
        ensure!(
            credential.team_id.as_deref() == Some(team_id),
            "Snapshot enthält private oder fremde Zugangsdaten"
        );
        ensure!(ids.insert(&credential.id), "Doppelte Team-Zugangsdaten-ID");
        ensure!(
            !data
                .credentials
                .iter()
                .any(|c| c.id == credential.id && c.team_id.as_deref() != Some(team_id)),
            "Team-Zugangsdaten dürfen keine privaten oder fremden Konten ersetzen"
        );
    }
    ensure!(
        snapshot.nodes.iter().all(|n| n.workspace == team_id),
        "Snapshot enthält fremde oder private Einträge"
    );
    ensure!(
        data.teams.iter().any(|t| t.id == team_id),
        "Team nicht gefunden"
    );
    let mut nodes: Vec<_> = data
        .nodes
        .iter()
        .filter(|n| n.workspace != team_id)
        .cloned()
        .collect();
    nodes.extend(snapshot.nodes);
    validate_nodes(&nodes)?;
    data.nodes = nodes;
    data.commands = commands;
    if snapshot.version >= 2 {
        data.credentials
            .retain(|c| c.team_id.as_deref() != Some(team_id));
        data.credentials.extend(snapshot.credentials);
        data.prune_credential_locations();
        data.bindings
            .retain(|_, id| data.credentials.iter().any(|c| &c.id == id));
    }
    data.bindings
        .retain(|id, _| data.nodes.iter().any(|n| &n.id == id));
    if snapshot.version >= 3 {
        data.team_activity.insert(team_id.into(), snapshot.activity);
    }
    Ok(())
}

pub fn validate_team_id(id: &str) -> Result<()> {
    ensure!(
        (3..=80).contains(&id.len())
            && id != "private"
            && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        "Team-ID: 3–80 Buchstaben, Zahlen oder Bindestriche; 'private' ist reserviert"
    );
    Ok(())
}

pub fn endpoint(server: &str, team_id: &str) -> Result<Url> {
    validate_team_id(team_id)?;
    let mut url = Url::parse(server)?;
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Keine Zugangsdaten in der Server-URL angeben"
    );
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && local),
        "Team-Sync erfordert HTTPS (HTTP nur für localhost)"
    );
    ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "Server-URL darf keine Query oder Fragment enthalten"
    );
    let path = format!(
        "{}/v1/teams/{team_id}/snapshot",
        url.path().trim_end_matches('/')
    );
    url.set_path(&path);
    Ok(url)
}

pub fn events_endpoint(server: &str, team_id: &str) -> Result<Url> {
    let mut url = endpoint(server, team_id)?;
    let prefix = url
        .path()
        .strip_suffix("/snapshot")
        .context("Ungültiger Sync-Endpunkt")?
        .to_owned();
    url.set_path(&format!("{prefix}/events"));
    Ok(url)
}

pub fn sync(data: &mut VaultData, team_id: &str, direction: &str) -> Result<String> {
    let team = data
        .teams
        .iter()
        .find(|t| t.id == team_id)
        .context("Team nicht gefunden")?
        .clone();
    let url = endpoint(&team.server_url, team_id)?;
    let key = crypto::decode_key(&team.key)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let context = format!("noryn:team:v1:{team_id}");
    let revision;
    match direction {
        "push" => {
            team_activity::validate_member_name(&team.member_name)?;
            if let Some(changes) = data.team_activity.get(team_id) {
                team_activity::validate_changes(changes)?;
            }
            let plaintext = Zeroizing::new(serde_json::to_vec(&snapshot(data, team_id))?);
            let envelope = crypto::seal(&key, &plaintext, context.as_bytes())?;
            let mut request = client.put(url).bearer_auth(&team.token).json(&envelope);
            request = if let Some(revision) = &team.revision {
                request.header(IF_MATCH, revision)
            } else {
                request.header(IF_NONE_MATCH, "*")
            };
            let response = request.send()?;
            if response.status() == StatusCode::PRECONDITION_FAILED {
                bail!("Sync-Konflikt: Auf dem Server liegt eine neuere Version. Erst abrufen; lokale Änderungen vorher sichern.");
            }
            let response = response.error_for_status()?;
            revision = response
                .headers()
                .get(ETAG)
                .context("Server liefert keine Revision")?
                .to_str()?
                .to_string();
        }
        "pull" => {
            let mut response = client
                .get(url)
                .bearer_auth(&team.token)
                .send()?
                .error_for_status()?;
            revision = response
                .headers()
                .get(ETAG)
                .context("Server liefert keine Revision")?
                .to_str()?
                .to_string();
            let mut bytes = Vec::new();
            use std::io::Read;
            response
                .by_ref()
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 32 * 1024 * 1024, "Snapshot ist zu groß");
            let envelope: Envelope = serde_json::from_slice(&bytes)?;
            let plaintext = crypto::unseal(&key, &envelope, context.as_bytes()).or_else(|_| {
                crypto::unseal(
                    &key,
                    &envelope,
                    crate::migration::previous_team_context(team_id).as_bytes(),
                )
            })?;
            apply_snapshot(data, team_id, serde_json::from_slice(&plaintext)?)?;
        }
        _ => bail!("Ungültige Sync-Richtung"),
    }
    let base = Zeroizing::new(serde_json::to_string(&snapshot(data, team_id))?);
    let team = data
        .teams
        .iter_mut()
        .find(|t| t.id == team_id)
        .context("Team nicht gefunden")?;
    team.revision = Some(revision);
    team.dirty = false;
    team.sync_base = Some(base.to_string());
    Ok(if direction == "push" {
        "Team-Baum verschlüsselt hochgeladen"
    } else {
        "Team-Baum entschlüsselt und übernommen"
    }
    .into())
}
