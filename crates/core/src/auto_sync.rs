use crate::{
    crypto,
    model::VaultData,
    sync::{self, TeamSnapshot},
    sync_merge::{self, Conflict, Merged},
    team_activity,
};
use anyhow::{bail, ensure, Context, Result};
use reqwest::{
    blocking::Client,
    header::{ETAG, IF_MATCH, IF_NONE_MATCH},
    StatusCode,
};
use serde::Serialize;
use std::{io::Read, time::Duration};
use zeroize::Zeroizing;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub message: String,
    pub conflicts: Vec<Conflict>,
}

fn initial_conflict(team: &str) -> Outcome {
    Outcome { message: "Für den ersten Abgleich bitte den Serverstand bewusst abrufen oder lokale Daten vorher exportieren.".into(),
        conflicts: vec![Conflict { object: "workspace".into(), id: team.into(), name: "Erster Team-Abgleich".into(), fields: vec!["entry".into()] }] }
}

/// Network work runs against a detached team copy. Native callers commit separately.
pub fn reconcile(data: &mut VaultData, team_id: &str) -> Result<Outcome> {
    reconcile_checked(data, team_id, |_| Ok(()))
}

/// Detached jobs reserve foreign IDs without copying private records or secrets.
pub fn reconcile_checked(
    data: &mut VaultData,
    team_id: &str,
    validate: impl Fn(&TeamSnapshot) -> Result<()>,
) -> Result<Outcome> {
    let team = data
        .teams
        .iter()
        .find(|t| t.id == team_id)
        .context("Team nicht gefunden")?
        .clone();
    ensure!(
        team.live_sync,
        "Live-Sync ist für dieses Team ausgeschaltet"
    );
    team_activity::validate_member_name(&team.member_name)?;
    let local = sync::snapshot(data, team_id);
    let mut base: Option<TeamSnapshot> = team
        .sync_base
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    if let Some(base) = &mut base {
        let mut validated = VaultData {
            teams: vec![team.clone()],
            ..Default::default()
        };
        sync::apply_snapshot(&mut validated, team_id, base.clone())?;
        sync::canonicalize(base);
    }
    let url = sync::endpoint(&team.server_url, team_id)?;
    let key = crypto::decode_key(&team.key)?;
    let context = format!("noryn:team:v1:{team_id}");
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    for _ in 0..3 {
        let response = client.get(url.clone()).bearer_auth(&team.token).send()?;
        let remote = if response.status() == StatusCode::NOT_FOUND {
            ensure!(
                team.revision.is_none(),
                "Der bisherige Serverstand fehlt; bitte Server-Konfiguration prüfen"
            );
            None
        } else {
            response.error_for_status_ref()?;
            let revision = response
                .headers()
                .get(ETAG)
                .context("Server liefert keine Revision")?
                .to_str()?
                .to_string();
            ensure!(revision.len() <= 256, "Ungültige Server-Revision");
            let mut bytes = Vec::new();
            response
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 32 * 1024 * 1024, "Snapshot ist zu groß");
            let envelope = serde_json::from_slice(&bytes)?;
            let plaintext = crypto::unseal(&key, &envelope, context.as_bytes()).or_else(|_| {
                crypto::unseal(
                    &key,
                    &envelope,
                    crate::migration::previous_team_context(team_id).as_bytes(),
                )
            })?;
            let incoming: TeamSnapshot = serde_json::from_slice(&plaintext)?;
            let mut validated = sync::detached(data, team_id)?;
            sync::apply_snapshot(&mut validated, team_id, incoming)?;
            Some((sync::snapshot(&validated, team_id), revision))
        };
        let desired = match &remote {
            None => local.clone(),
            Some((remote, _)) if !team.dirty => remote.clone(),
            Some((remote, revision)) => {
                if let Some(base) = &base {
                    match sync_merge::merge(base, &local, remote)? {
                        Merged::Ready(snapshot) => snapshot,
                        Merged::Conflicts(conflicts) => return Ok(Outcome { message: "Parallele Änderungen widersprechen sich. Der automatische Abgleich wurde angehalten; es wurde nichts überschrieben.".into(), conflicts }),
                    }
                } else if team.revision.as_ref() == Some(revision) {
                    local.clone()
                } else if local.nodes.is_empty()
                    && local.commands.is_empty()
                    && local.credentials.is_empty()
                    && local.activity.is_empty()
                {
                    remote.clone()
                } else {
                    return Ok(initial_conflict(team_id));
                }
            }
        };
        let mut validated = data.clone();
        sync::apply_snapshot(&mut validated, team_id, desired.clone())?;
        validate(&desired)?;
        let revision = if let Some((remote, revision)) = &remote {
            if remote == &desired {
                Some(revision.clone())
            } else {
                None
            }
        } else {
            None
        };
        let revision = if let Some(revision) = revision {
            revision
        } else {
            let plaintext = Zeroizing::new(serde_json::to_vec(&desired)?);
            let envelope = crypto::seal(&key, &plaintext, context.as_bytes())?;
            let request = client
                .put(url.clone())
                .bearer_auth(&team.token)
                .json(&envelope);
            let request = if let Some((_, revision)) = &remote {
                request.header(IF_MATCH, revision)
            } else {
                request.header(IF_NONE_MATCH, "*")
            };
            let response = request.send()?;
            if response.status() == StatusCode::PRECONDITION_FAILED {
                continue;
            }
            let response = response.error_for_status()?;
            response
                .headers()
                .get(ETAG)
                .context("Server liefert keine Revision")?
                .to_str()?
                .to_string()
        };
        let serialized = Zeroizing::new(serde_json::to_string(&desired)?);
        sync::apply_snapshot(data, team_id, desired)?;
        let team = data
            .teams
            .iter_mut()
            .find(|t| t.id == team_id)
            .context("Team nicht gefunden")?;
        team.sync_base = Some(serialized.to_string());
        team.revision = Some(revision);
        team.dirty = false;
        return Ok(Outcome {
            message: "Team ist aktuell".into(),
            conflicts: vec![],
        });
    }
    bail!("Während des Abgleichs gab es weitere Änderungen; bitte erneut versuchen")
}
