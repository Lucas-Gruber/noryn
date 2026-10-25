//! Team-only Bash libraries: encrypted independently of connection snapshots.
use crate::{
    model::{Team, VaultData},
    team_activity,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, ZeroizeOnDrop};
pub const MAX_BODY: usize = 512 * 1024;
pub const MAX_TOTAL: usize = 8 * 1024 * 1024;

#[derive(Clone, Default, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub updated_at: u64,
    pub updated_by: String,
}
#[derive(Clone, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub id: String,
    pub actor: String,
    pub occurred_at: u64,
    pub action: String,
    pub target: String,
    pub fields: Vec<String>,
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Library {
    pub entries: Vec<Entry>,
    pub history: Vec<Change>,
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cache {
    pub library: Library,
    #[serde(default)]
    pub base: Option<Library>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub suspended: bool,
}
impl Cache {
    pub fn dirty(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b != &self.library)
            && (!self.library.entries.is_empty() || self.base.is_some())
    }
    pub fn archive(&mut self) {
        self.base = None;
        self.revision = None;
        self.origin.clear();
        self.suspended = true;
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInfo {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub bytes: usize,
    pub fingerprint: String,
    pub updated_at: u64,
    pub updated_by: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub entries: Vec<EntryInfo>,
    pub history: Vec<Change>,
    pub dirty: bool,
    pub suspended: bool,
    pub origin_changed: bool,
}
pub fn fingerprint(entry: &Entry) -> String {
    format!(
        "{:x}",
        Sha256::digest(&*zeroize::Zeroizing::new(
            serde_json::to_vec(entry).expect("serializable entry")
        ))
    )
}
pub fn origin(team: &Team) -> String {
    format!(
        "{:x}",
        Sha256::digest(&*zeroize::Zeroizing::new(format!(
            "{}\0{}\0{}",
            team.id,
            team.server_url.trim_end_matches('/'),
            team.key
        )))
    )
}
pub fn summaries(data: &VaultData) -> HashMap<String, Summary> {
    data.script_libraries
        .iter()
        .filter_map(|(id, cache)| {
            let team = data.teams.iter().find(|t| &t.id == id)?;
            Some((
                id.clone(),
                Summary {
                    entries: cache
                        .library
                        .entries
                        .iter()
                        .map(|e| EntryInfo {
                            id: e.id.clone(),
                            parent_id: e.parent_id.clone(),
                            kind: e.kind.clone(),
                            name: e.name.clone(),
                            description: e.description.clone(),
                            bytes: e.body.len(),
                            fingerprint: fingerprint(e),
                            updated_at: e.updated_at,
                            updated_by: e.updated_by.clone(),
                        })
                        .collect(),
                    history: cache.library.history.clone(),
                    dirty: cache.dirty(),
                    suspended: cache.suspended,
                    origin_changed: !cache.origin.is_empty() && cache.origin != origin(team),
                },
            ))
        })
        .collect()
}
pub fn validate(library: &Library) -> Result<()> {
    ensure!(
        library.entries.len() <= 512 && library.history.len() <= 1000,
        "Skript-Sammlung ist zu groß"
    );
    let mut ids = HashMap::new();
    let mut total = 0;
    for e in &library.entries {
        ensure!(
            uuid::Uuid::parse_str(&e.id).is_ok() && ids.insert(&e.id, e).is_none(),
            "Ungültige oder doppelte Skript-ID"
        );
        ensure!(
            ["folder", "script"].contains(&e.kind.as_str()),
            "Unbekannter Skript-Eintrag"
        );
        ensure!(
            !e.name.trim().is_empty()
                && e.name.len() <= 512
                && !e.name.chars().any(char::is_control),
            "Skriptname fehlt oder ist ungültig"
        );
        ensure!(
            e.description.len() <= 4096
                && e.body.len() <= MAX_BODY
                && [&e.body, &e.description].iter().all(|s| !s
                    .chars()
                    .any(|c| c.is_control() && !['\n', '\t'].contains(&c))),
            "Bash-Skript: UTF-8, höchstens 512 KiB, keine binären Steuerzeichen"
        );
        ensure!(
            e.kind != "folder" || e.body.is_empty(),
            "Ordner dürfen keinen Skriptinhalt enthalten"
        );
        team_activity::validate_member_name(&e.updated_by)?;
        total += e.body.len();
    }
    ensure!(total <= MAX_TOTAL, "Skriptinhalte zusammen höchstens 8 MiB");
    for e in &library.entries {
        let mut seen = HashSet::from([e.id.as_str()]);
        let mut parent = e.parent_id.as_ref();
        while let Some(id) = parent {
            ensure!(
                seen.insert(id.as_str()) && seen.len() <= 64,
                "Zyklus oder zu tiefe Skript-Ordnerstruktur"
            );
            let p = ids.get(id).context("Übergeordneter Skriptordner fehlt")?;
            ensure!(
                p.kind == "folder",
                "Skripte können keine Untereinträge haben"
            );
            parent = p.parent_id.as_ref();
        }
    }
    let mut events = HashSet::new();
    for c in &library.history {
        ensure!(
            uuid::Uuid::parse_str(&c.id).is_ok() && events.insert(&c.id),
            "Ungültige Skript-Verlaufs-ID"
        );
        team_activity::validate_member_name(&c.actor)?;
        ensure!(
            ["created", "updated", "deleted", "imported"].contains(&c.action.as_str())
                && c.target.len() <= 512
                && !c.target.chars().any(char::is_control)
                && c.fields.len() <= 5
                && c.fields.iter().all(
                    |f| ["name", "description", "body", "parent", "kind"].contains(&f.as_str())
                ),
            "Ungültiger Skript-Verlauf"
        );
    }
    Ok(())
}
pub fn validate_vault(data: &VaultData) -> Result<()> {
    ensure!(
        data.script_libraries.len() <= 64,
        "Höchstens 64 lokale Team-Sammlungen"
    );
    let mut total = 0;
    let mut base_bytes = 0;
    let mut metadata = 0;
    let mut entries = 0;
    let mut history = 0;
    for (id, cache) in &data.script_libraries {
        ensure!(
            id != "private" && data.teams.iter().any(|t| &t.id == id),
            "Skript-Sammlungen gehören ausschließlich zu vorhandenen Teams"
        );
        validate(&cache.library)?;
        total += cache
            .library
            .entries
            .iter()
            .map(|e| e.body.len())
            .sum::<usize>();
        if let Some(base) = &cache.base {
            validate(base)?;
            base_bytes += base.entries.iter().map(|e| e.body.len()).sum::<usize>();
        }
        for lib in std::iter::once(&cache.library).chain(cache.base.as_ref()) {
            entries += lib.entries.len();
            history += lib.history.len();
            metadata += lib
                .entries
                .iter()
                .map(|e| e.name.len() + e.description.len() + e.updated_by.len() + 256)
                .sum::<usize>();
            metadata += lib
                .history
                .iter()
                .map(|c| c.target.len() + c.actor.len() + 256)
                .sum::<usize>();
        }
        ensure!(
            cache.origin.is_empty()
                || (cache.origin.len() == 64
                    && cache.origin.bytes().all(|b| b.is_ascii_hexdigit())),
            "Ungültiges Skript-Sync-Ziel"
        );
        if let Some(revision) = &cache.revision {
            ensure!(valid_revision(revision), "Ungültige Skript-Revision");
        }
    }
    ensure!(
        total <= MAX_TOTAL && base_bytes <= MAX_TOTAL && metadata <= MAX_TOTAL && entries <= 4096 && history <= 16000,
        "Skript-Sammlungen überschreiten die lokalen Speichergrenzen (8 MiB Inhalt, 8 MiB Basis, 8 MiB Metadaten)"
    );
    Ok(())
}
pub fn valid_revision(revision: &str) -> bool {
    revision.len() == 66
        && revision.starts_with('"')
        && revision.ends_with('"')
        && revision.as_bytes()[1..65].iter().all(u8::is_ascii_hexdigit)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn record(library: &mut Library, actor: &str, action: &str, target: &str, fields: Vec<String>) {
    library.history.push(Change {
        id: uuid::Uuid::new_v4().to_string(),
        actor: actor.into(),
        occurred_at: now(),
        action: action.into(),
        target: target.into(),
        fields,
    });
    if library.history.len() > 1000 {
        library.history.drain(..library.history.len() - 1000);
    }
}
pub fn save(
    data: &mut VaultData,
    team_id: &str,
    mut entry: Entry,
    expected: Option<&str>,
) -> Result<()> {
    let team = data
        .teams
        .iter()
        .find(|t| t.id == team_id)
        .context("Team nicht gefunden")?;
    team_activity::validate_member_name(&team.member_name)?;
    let actor = team.member_name.clone();
    let target_origin = origin(team);
    validate_vault(data)?;
    let mut cache = data
        .script_libraries
        .get(team_id)
        .cloned()
        .unwrap_or_default();
    if cache.origin.is_empty() && !cache.suspended {
        cache.origin = target_origin;
    }
    entry.body = entry.body.replace("\r\n", "\n");
    let old = cache.library.entries.iter().find(|e| e.id == entry.id);
    ensure!(old.map(fingerprint).as_deref()==expected,"Skript wurde inzwischen geändert. Entwurf bleibt erhalten; bitte erneut öffnen oder als Kopie speichern");
    let fields: Vec<String> = if let Some(old) = old {
        [
            (old.name != entry.name, "name"),
            (old.description != entry.description, "description"),
            (old.body != entry.body, "body"),
            (old.parent_id != entry.parent_id, "parent"),
            (old.kind != entry.kind, "kind"),
        ]
        .into_iter()
        .filter(|(changed, _)| *changed)
        .map(|(_, f)| f.into())
        .collect()
    } else {
        vec![]
    };
    if old.is_some() && fields.is_empty() {
        return Ok(());
    }
    let action = if old.is_some() { "updated" } else { "created" };
    entry.updated_at = now();
    entry.updated_by = actor.clone();
    let mut library = cache.library.clone();
    library.entries.retain(|e| e.id != entry.id);
    library.entries.push(entry.clone());
    record(&mut library, &actor, action, &entry.name, fields);
    validate(&library)?;
    library.entries.sort_by(|a, b| a.id.cmp(&b.id));
    cache.library = library;
    let mut candidate = data.clone();
    candidate.script_libraries.insert(team_id.into(), cache);
    validate_vault(&candidate)?;
    *data = candidate;
    Ok(())
}
pub fn remove(data: &mut VaultData, team_id: &str, id: &str, expected: &str) -> Result<()> {
    let team = data
        .teams
        .iter()
        .find(|t| t.id == team_id)
        .context("Team nicht gefunden")?;
    team_activity::validate_member_name(&team.member_name)?;
    let actor = team.member_name.clone();
    let mut cache = data
        .script_libraries
        .get(team_id)
        .cloned()
        .context("Sammlung fehlt")?;
    let entry = cache
        .library
        .entries
        .iter()
        .find(|e| e.id == id)
        .context("Eintrag fehlt")?
        .clone();
    ensure!(
        fingerprint(&entry) == expected,
        "Eintrag wurde inzwischen geändert"
    );
    for child in &mut cache.library.entries {
        if child.parent_id.as_deref() == Some(id) {
            child.parent_id = entry.parent_id.clone();
            child.updated_by = actor.clone();
            child.updated_at = now();
        }
    }
    cache.library.entries.retain(|e| e.id != id);
    record(&mut cache.library, &actor, "deleted", &entry.name, vec![]);
    validate(&cache.library)?;
    let mut candidate = data.clone();
    candidate.script_libraries.insert(team_id.into(), cache);
    validate_vault(&candidate)?;
    *data = candidate;
    Ok(())
}
fn field<T: Clone + PartialEq>(base: &T, local: &T, remote: &T, label: &str) -> Result<T> {
    if local == remote || remote == base {
        Ok(local.clone())
    } else if local == base {
        Ok(remote.clone())
    } else {
        anyhow::bail!("Skript-Konflikt im Feld {label}. Beide Stände bleiben erhalten; keine automatische Auswahl")
    }
}
pub fn merge(base: &Library, local: &Library, remote: &Library) -> Result<Library> {
    for lib in [base, local, remote] {
        validate(lib)?;
    }
    let map = |lib: &Library| {
        lib.entries
            .iter()
            .map(|e| (e.id.clone(), e.clone()))
            .collect::<HashMap<_, _>>()
    };
    let (b, l, r) = (map(base), map(local), map(remote));
    let ids: HashSet<_> = b.keys().chain(l.keys()).chain(r.keys()).collect();
    let mut result = Library::default();
    for id in ids {
        let (old, here, there) = (b.get(id), l.get(id), r.get(id));
        let entry = if here == there || there == old {
            here.cloned()
        } else if here == old {
            there.cloned()
        } else if let (Some(old), Some(here), Some(there)) = (old, here, there) {
            let mut e = here.clone();
            e.name = field(&old.name, &here.name, &there.name, "Name")?;
            e.description = field(
                &old.description,
                &here.description,
                &there.description,
                "Beschreibung",
            )?;
            e.body = field(&old.body, &here.body, &there.body, "Inhalt")?;
            e.parent_id = field(&old.parent_id, &here.parent_id, &there.parent_id, "Ordner")?;
            e.kind = field(&old.kind, &here.kind, &there.kind, "Typ")?;
            // Self-reported display metadata only. Script values are never chosen by clocks.
            if there.updated_at > here.updated_at {
                e.updated_at = there.updated_at;
                e.updated_by = there.updated_by.clone();
            }
            Some(e)
        } else {
            anyhow::bail!("Skript-Konflikt: gleichzeitiges Löschen/Bearbeiten oder doppelte ID");
        };
        if let Some(entry) = entry {
            result.entries.push(entry);
        }
    }
    result.entries.sort_by(|a, b| a.id.cmp(&b.id));
    let mut events = HashMap::new();
    for c in base
        .history
        .iter()
        .chain(&local.history)
        .chain(&remote.history)
    {
        if let Some(old) = events.insert(&c.id, c) {
            ensure!(old == c, "Skript-Verlauf wurde nachträglich verändert");
        }
    }
    let base_ids: HashSet<_> = base.history.iter().map(|c| &c.id).collect();
    let remote_ids: HashSet<_> = remote.history.iter().map(|c| &c.id).collect();
    let mut seen = HashSet::new();
    // Preserve remote order and append unsent local changes. Acknowledged events
    // already pruned from the bounded remote history must not return from offline clients.
    for c in remote.history.iter().chain(&local.history) {
        if base_ids.contains(&c.id) && !remote_ids.contains(&c.id) {
            continue;
        }
        if seen.insert(&c.id) {
            result.history.push(c.clone());
        }
    }
    if result.history.len() > 1000 {
        result.history.drain(..result.history.len() - 1000);
    }
    validate(&result).context("Skript-Konflikt: gemeinsame Ordnerstruktur ist ungültig")?;
    Ok(result)
}
pub fn imported(cache: &Cache) -> Cache {
    let mut cache = cache.clone();
    cache.archive();
    cache
}
pub fn copy_entries(source: &Library, parent: Option<String>, actor: &str) -> Result<Library> {
    validate(source)?;
    let ids: HashMap<_, _> = source
        .entries
        .iter()
        .map(|e| (e.id.clone(), uuid::Uuid::new_v4().to_string()))
        .collect();
    let mut result = Library::default();
    for e in &source.entries {
        let mut e = e.clone();
        e.id = ids[&e.id].clone();
        e.parent_id = e
            .parent_id
            .as_ref()
            .and_then(|id| ids.get(id))
            .cloned()
            .or_else(|| parent.clone());
        e.updated_by = actor.into();
        e.updated_at = now();
        result.entries.push(e);
    }
    Ok(result)
}
pub fn keep_both(local: &Library, remote: &Library, actor: &str) -> Result<Library> {
    team_activity::validate_member_name(actor)?;
    let folder = Entry {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "folder".into(),
        name: "Lokaler Entwurf (Konfliktkopie)".into(),
        updated_by: actor.into(),
        updated_at: now(),
        parent_id: None,
        description: String::new(),
        body: String::new(),
    };
    let copied = copy_entries(local, Some(folder.id.clone()), actor)?;
    let mut result = remote.clone();
    result.entries.push(folder);
    result.entries.extend(copied.entries.clone());
    record(
        &mut result,
        actor,
        "imported",
        "Lokale Konfliktkopie",
        vec![],
    );
    result.entries.sort_by(|a, b| a.id.cmp(&b.id));
    validate(&result)?;
    Ok(result)
}
pub fn import_into(data: &mut VaultData, source: &Cache, team_id: &str) -> Result<()> {
    ensure!(
        team_id != "private",
        "Skript-Sammlungen können nur in einen Team-Tresor importiert werden"
    );
    let team = data
        .teams
        .iter()
        .find(|t| t.id == team_id)
        .context("Team fehlt")?;
    team_activity::validate_member_name(&team.member_name)?;
    let actor = team.member_name.clone();
    let copies = copy_entries(&source.library, None, &actor)?;
    let mut candidate = data.clone();
    let cache = candidate
        .script_libraries
        .entry(team_id.into())
        .or_default();
    cache.library.entries.extend(copies.entries.clone());
    cache.library.entries.sort_by(|a, b| a.id.cmp(&b.id));
    record(
        &mut cache.library,
        &actor,
        "imported",
        "Skript-Sammlung",
        vec![],
    );
    cache.suspended = true;
    validate_vault(&candidate)?;
    *data = candidate;
    Ok(())
}
