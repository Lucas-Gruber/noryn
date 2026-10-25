//! One authenticated ciphertext resource per team, separate from connection sync.
use crate::{
    crypto,
    model::Team,
    scripts::{self, Cache, Library},
};
use anyhow::{ensure, Context, Result};
use reqwest::{
    blocking::Client,
    header::{ETAG, IF_MATCH, IF_NONE_MATCH},
    StatusCode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
const LIMIT: u64 = 32 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload {
    version: u8,
    team_id: String,
    library: Library,
}

pub fn synchronize(
    team: &Team,
    cache: &Cache,
    adopt_origin: bool,
    republish_missing: bool,
    keep_both: bool,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Cache> {
    scripts::validate(&cache.library)?;
    if let Some(base) = &cache.base {
        scripts::validate(base)?;
    }
    check()?;
    let started = Instant::now();
    let origin = scripts::origin(team);
    ensure!(
        cache.origin.is_empty() || cache.origin == origin || adopt_origin,
        "Skript-Sync-Ziel wurde geändert. Zielwechsel zuerst ausdrücklich bestätigen"
    );
    let mut url = crate::sync::endpoint(&team.server_url, &team.id)?;
    let prefix = url
        .path()
        .strip_suffix("/snapshot")
        .context("Ungültiger Skript-Endpunkt")?
        .to_owned();
    url.set_path(&format!("{prefix}/scripts"));
    let key = crypto::decode_key(&team.key)?;
    let context = format!("noryn:scripts:v1:{}", team.id);
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut request = client.get(url.clone()).bearer_auth(&team.token);
    if !adopt_origin {
        if let Some(revision) = &cache.revision {
            request = request.header(IF_NONE_MATCH, revision);
        }
    }
    let mut response = request
        .send()
        .context("Skript-Sync-Server nicht erreichbar")?;
    ensure!(
        response.status() != StatusCode::NOT_FOUND,
        "Sync-Server unterstützt die Skript-Sammlung noch nicht. Bitte Server aktualisieren"
    );
    let (remote, revision) = match response.status() {
        StatusCode::NO_CONTENT => (Library::default(), None),
        StatusCode::NOT_MODIFIED => (
            cache
                .base
                .clone()
                .context("Skript-Basis fehlt für unveränderten Serverstand")?,
            cache.revision.clone(),
        ),
        _ => {
            response.error_for_status_ref()?;
            let revision = response
                .headers()
                .get(ETAG)
                .and_then(|v| v.to_str().ok())
                .context("Skript-Revision fehlt")?
                .to_owned();
            ensure!(
                scripts::valid_revision(&revision),
                "Ungültige Skript-Revision"
            );
            let mut bytes = Zeroizing::new(Vec::new());
            response.by_ref().take(LIMIT + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() as u64 <= LIMIT, "Skript-Antwort ist zu groß");
            ensure!(
                revision == format!("\"{:x}\"", Sha256::digest(&bytes)),
                "Skript-Revision passt nicht zum verschlüsselten Inhalt"
            );
            let envelope: crypto::Envelope = serde_json::from_slice(&bytes)?;
            let plaintext = crypto::unseal(&key, &envelope, context.as_bytes())?;
            let payload: Payload = serde_json::from_slice(&plaintext)?;
            ensure!(
                payload.version == 1 && payload.team_id == team.id,
                "Skript-Sammlung gehört zu einem anderen Team oder hat ein unbekanntes Format"
            );
            scripts::validate(&payload.library)?;
            (payload.library, Some(revision))
        }
    };
    check()?;
    let missing = !adopt_origin
        && revision.is_none()
        && cache
            .base
            .as_ref()
            .is_some_and(|base| !base.entries.is_empty());
    ensure!(!missing || republish_missing,"Skript-Sammlung fehlt auf dem Server. Lokale Kopie bleibt erhalten; erneute Veröffentlichung ausdrücklich bestätigen");
    let empty = Library::default();
    let base = if adopt_origin || missing {
        &empty
    } else {
        cache.base.as_ref().unwrap_or(&empty)
    };
    let merged = match scripts::merge(base, &cache.library, &remote) {
        Ok(merged) => merged,
        Err(_) if keep_both => scripts::keep_both(&cache.library, &remote, &team.member_name)?,
        Err(error) => return Err(error),
    };
    let revision = if merged != remote {
        crate::team_activity::validate_member_name(&team.member_name)?;
        ensure!(
            started.elapsed() < Duration::from_secs(60),
            "Zeitlimit beim Skript-Abgleich"
        );
        check()?;
        let payload = Payload {
            version: 1,
            team_id: team.id.clone(),
            library: merged.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&payload)?);
        let envelope = crypto::seal(&key, &bytes, context.as_bytes())?;
        let encrypted = serde_json::to_vec(&envelope)?;
        ensure!(encrypted.len() as u64 <= LIMIT, "Verschlüsselte Skript-Sammlung ist zu groß (höchstens 32 MiB); Inhalte oder Beschreibungen reduzieren");
        let expected_revision = format!("\"{:x}\"", Sha256::digest(&encrypted));
        let mut request = client
            .put(url)
            .bearer_auth(&team.token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(encrypted);
        request = if let Some(revision) = &revision {
            request.header(IF_MATCH, revision)
        } else {
            request.header(IF_NONE_MATCH, "*")
        };
        let response = request.send().context(
            "Skript-Upload konnte nicht bestätigt werden. Kein automatischer Schreib-Retry",
        )?;
        ensure!(response.status()!=StatusCode::PRECONDITION_FAILED,"Skript-Sammlung wurde parallel geändert. Lokal gespeicherte Entwürfe bleiben erhalten; erneut synchronisieren");
        response.error_for_status_ref()?;
        let revision = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .context("Bestätigung der Skript-Revision fehlt")?
            .to_owned();
        ensure!(
            scripts::valid_revision(&revision),
            "Ungültige bestätigte Skript-Revision"
        );
        ensure!(revision == expected_revision, "Bestätigte Skript-Revision stimmt nicht mit dem Upload überein; bitte erneut abgleichen");
        Some(revision)
    } else {
        revision
    };
    check()?;
    Ok(Cache {
        library: merged.clone(),
        base: Some(merged),
        revision,
        origin,
        suspended: false,
    })
}
