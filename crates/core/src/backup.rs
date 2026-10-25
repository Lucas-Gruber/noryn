//! Password-encrypted, explicitly scoped backups. These are never sync payloads.
use crate::{
    credential_folders, crypto,
    model::{validate_credential, validate_nodes, VaultData},
    sync,
    team_activity::{self, ChangeAction, ChangeObject},
};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, io::Read, io::Write, path::Path};
use zeroize::Zeroizing;

const MAX_BYTES: usize = 64 * 1024 * 1024;
const CONTEXT: &[u8] = b"noryn:backup:v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Scope {
    All,
    Private,
    Team {
        #[serde(rename = "teamId")]
        team_id: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupFile {
    format: String,
    version: u8,
    salt: String,
    sealed: crypto::Envelope,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backup {
    version: u8,
    pub scope: Scope,
    pub data: VaultData,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub commands: usize,
    pub scope: Scope,
    pub label: String,
    pub nodes: usize,
    pub credentials: usize,
    pub teams: usize,
    pub replaced_nodes: usize,
    pub replaced_credentials: usize,
    pub replaced_commands: usize,
    pub sources: Vec<SourceSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub commands: usize,
    pub workspace: String,
    pub name: String,
    pub nodes: usize,
    pub credentials: usize,
    pub agent_credentials: usize,
    pub inline_credentials: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportDestination {
    pub source_workspace: String,
    pub target_workspace: String,
    pub include_credentials: bool,
}

pub fn validate(data: &VaultData) -> Result<()> {
    crate::commands::validate_vault(data)?;
    validate_nodes(&data.nodes)?;
    crate::connection_credentials::validate(data)?;
    ensure!(
        data.teams.len() <= 1_000 && data.credentials.len() <= 20_000,
        "Zu viele Tresor-Einträge"
    );
    let mut teams = HashSet::new();
    for team in &data.teams {
        sync::validate_team_id(&team.id)?;
        ensure!(teams.insert(team.id.as_str()), "Doppelte Team-ID");
        ensure!(
            !team.name.trim().is_empty() && team.name.len() <= 512,
            "Ungültiger Teamname"
        );
        sync::endpoint(&team.server_url, &team.id)?;
        ensure!(
            (32..=65_536).contains(&team.token.len()),
            "Ungültiger Server-Token"
        );
        crypto::decode_key(&team.key)?;
        if let Some(base) = &team.sync_base {
            ensure!(base.len() <= 32 * 1024 * 1024, "Sync-Basis ist zu groß");
            let snapshot: sync::TeamSnapshot = serde_json::from_str(base)?;
            let mut context = VaultData {
                teams: vec![team.clone()],
                ..Default::default()
            };
            sync::apply_snapshot(&mut context, &team.id, snapshot)?;
        }
        if !team.member_name.is_empty() {
            team_activity::validate_member_name(&team.member_name)?;
        }
        ensure!(
            team.revision
                .as_ref()
                .is_none_or(|r| r.len() <= 256 && !r.chars().any(char::is_control)),
            "Ungültige Revision"
        );
    }
    ensure!(
        data.nodes
            .iter()
            .all(|n| n.workspace == "private" || teams.contains(n.workspace.as_str())),
        "Unbekannter Workspace"
    );
    let mut credentials = HashSet::new();
    for credential in &data.credentials {
        validate_credential(credential)?;
        ensure!(
            credentials.insert(credential.id.as_str()),
            "Doppelte Zugangsdaten-ID"
        );
        ensure!(
            credential
                .team_id
                .as_deref()
                .is_none_or(|id| teams.contains(id)),
            "Unbekannter Team-Tresor"
        );
    }
    let nodes: HashSet<_> = data.nodes.iter().map(|n| n.id.as_str()).collect();
    ensure!(
        data.bindings
            .iter()
            .all(|(node, credential)| nodes.contains(node.as_str())
                && credentials.contains(credential.as_str())),
        "Ungültige persönliche Zuordnung"
    );
    credential_folders::validate(data)?;
    ensure!(
        data.known_hosts.len() <= 20_000
            && data
                .known_hosts
                .iter()
                .all(|(host, fingerprint)| host.len() <= 1024 && fingerprint.len() <= 256),
        "Ungültige Hostschlüssel-Liste"
    );
    for (team, changes) in &data.team_activity {
        ensure!(
            teams.contains(team.as_str()),
            "Verlauf gehört zu unbekanntem Team"
        );
        team_activity::validate_changes(changes)?;
    }
    Ok(())
}

pub fn select(data: &VaultData, scope: Scope) -> Result<Backup> {
    validate(data)?;
    let mut selected = match &scope {
        Scope::All => data.clone(),
        Scope::Private => {
            let mut selected = data.clone();
            selected.nodes.retain(|n| n.workspace == "private");
            selected.commands.retain(|c| c.workspace == "private");
            selected.credentials.retain(|c| c.team_id.is_none());
            selected.teams.clear();
            selected.team_activity.clear();
            selected.known_hosts.clear();
            selected.prune_credential_locations();
            selected
        }
        Scope::Team { team_id } => {
            let team = data
                .teams
                .iter()
                .find(|t| &t.id == team_id)
                .context("Team nicht gefunden")?;
            let mut team = team.clone();
            team.member_name.clear();
            team.sync_base = None;
            team.live_sync = false;
            VaultData {
                commands: data
                    .commands
                    .iter()
                    .filter(|c| &c.workspace == team_id)
                    .cloned()
                    .collect(),
                nodes: data
                    .nodes
                    .iter()
                    .filter(|n| &n.workspace == team_id)
                    .cloned()
                    .collect(),
                credentials: data
                    .credentials
                    .iter()
                    .filter(|c| c.team_id.as_ref() == Some(team_id))
                    .cloned()
                    .collect(),
                teams: vec![team],
                team_activity: data
                    .team_activity
                    .iter()
                    .filter(|(id, _)| *id == team_id)
                    .map(|(id, changes)| (id.clone(), changes.clone()))
                    .collect(),
                ..Default::default()
            }
        }
    };
    // Exclude the cached sync base, which can contain previously shared secrets.
    for team in &mut selected.teams {
        team.sync_base = None;
    }
    selected.bindings.retain(|node, credential| {
        selected.nodes.iter().any(|n| &n.id == node)
            && selected.credentials.iter().any(|c| &c.id == credential)
    });
    let backup = Backup {
        version: if !selected.commands.is_empty() {
            3
        } else if selected.connection_credentials.is_empty() {
            1
        } else {
            2
        },
        scope,
        data: selected,
    };
    backup.validate()?;
    Ok(backup)
}

impl Backup {
    fn validate(&self) -> Result<()> {
        ensure!(
            [1, 2, 3].contains(&self.version),
            "Unbekanntes Backup-Format"
        );
        ensure!(
            self.version >= 3 || self.data.commands.is_empty(),
            "Befehle benötigen Backup-Format 3"
        );
        ensure!(
            self.version >= 2 || self.data.connection_credentials.is_empty(),
            "Direkte Verbindungszugänge benötigen Backup-Format 2"
        );
        validate(&self.data)?;
        match &self.scope {
            Scope::All => {}
            Scope::Private => ensure!(
                self.data.teams.is_empty()
                    && self.data.commands.iter().all(|c| c.workspace == "private")
                    && self.data.nodes.iter().all(|n| n.workspace == "private")
                    && self.data.credentials.iter().all(|c| c.team_id.is_none())
                    && self.data.team_activity.is_empty()
                    && self.data.known_hosts.is_empty(),
                "Privates Backup enthält fremde Daten"
            ),
            Scope::Team { team_id } => ensure!(
                self.data.teams.len() == 1
                    && self.data.commands.iter().all(|c| &c.workspace == team_id)
                    && self.data.teams[0].id == *team_id
                    && self.data.nodes.iter().all(|n| &n.workspace == team_id)
                    && self
                        .data
                        .credentials
                        .iter()
                        .all(|c| c.team_id.as_ref() == Some(team_id))
                    && self.data.bindings.is_empty()
                    && self.data.credential_folders.is_empty()
                    && self.data.credential_locations.is_empty()
                    && self.data.known_hosts.is_empty(),
                "Team-Backup enthält persönliche oder fremde Daten"
            ),
        }
        Ok(())
    }

    pub fn summary(&self, current: &VaultData) -> Summary {
        let contains = |workspace: &str| match &self.scope {
            Scope::All => true,
            Scope::Private => workspace == "private",
            Scope::Team { team_id } => workspace == team_id,
        };
        Summary {
            replaced_commands: current
                .commands
                .iter()
                .filter(|c| contains(&c.workspace))
                .count(),
            commands: self.data.commands.len(),
            scope: self.scope.clone(),
            label: match &self.scope {
                Scope::All => "Alle Tresore".into(),
                Scope::Private => "Privater Tresor".into(),
                Scope::Team { .. } => self.data.teams[0].name.clone(),
            },
            nodes: self.data.nodes.len(),
            credentials: self.data.credentials.len() + self.data.connection_credentials.len(),
            teams: self.data.teams.len(),
            replaced_nodes: current
                .nodes
                .iter()
                .filter(|n| contains(&n.workspace))
                .count(),
            replaced_credentials: current
                .credentials
                .iter()
                .filter(|c| contains(c.team_id.as_deref().unwrap_or("private")))
                .count()
                + current
                    .connection_credentials
                    .iter()
                    .filter(|(id, _)| {
                        current
                            .nodes
                            .iter()
                            .any(|n| &n.id == *id && contains(&n.workspace))
                    })
                    .count(),
            sources: self.sources(),
        }
    }

    pub fn sources(&self) -> Vec<SourceSummary> {
        let mut workspaces = Vec::new();
        if !matches!(self.scope, Scope::Team { .. }) {
            workspaces.push(("private".to_owned(), "Privater Tresor".to_owned()));
        }
        workspaces.extend(
            self.data
                .teams
                .iter()
                .map(|team| (team.id.clone(), team.name.clone())),
        );
        workspaces
            .into_iter()
            .map(|(workspace, name)| {
                let credentials: Vec<_> = self
                    .data
                    .credentials
                    .iter()
                    .filter(|c| c.team_id.as_deref().unwrap_or("private") == workspace)
                    .collect();
                SourceSummary {
                    commands: self
                        .data
                        .commands
                        .iter()
                        .filter(|c| c.workspace == workspace)
                        .count(),
                    nodes: self
                        .data
                        .nodes
                        .iter()
                        .filter(|n| n.workspace == workspace)
                        .count(),
                    credentials: credentials.len()
                        + if workspace == "private" {
                            self.data.connection_credentials.len()
                        } else {
                            0
                        },
                    agent_credentials: credentials.iter().filter(|c| c.kind == "agent").count(),
                    inline_credentials: if workspace == "private" {
                        self.data.connection_credentials.len()
                    } else {
                        0
                    },
                    workspace,
                    name,
                }
            })
            .collect()
    }

    /// Add a single source workspace with fresh IDs. Existing data/configuration is preserved.
    pub fn import_into(
        &self,
        current: &VaultData,
        destination: &ImportDestination,
    ) -> Result<VaultData> {
        self.validate()?;
        validate(current)?;
        let ImportDestination {
            source_workspace,
            target_workspace,
            include_credentials,
        } = destination;
        ensure!(
            self.sources()
                .iter()
                .any(|s| &s.workspace == source_workspace),
            "Quell-Tresor ist nicht in dieser Sicherung enthalten"
        );
        crate::workspace_transfer::validate_destination(current, target_workspace, None)?;
        let source: Vec<_> = self
            .data
            .nodes
            .iter()
            .filter(|n| &n.workspace == source_workspace)
            .cloned()
            .collect();
        let nodes = crate::workspace_transfer::remap_nodes(&source, target_workspace, None)?;
        let remapped: std::collections::HashMap<_, _> = source
            .iter()
            .zip(&nodes)
            .map(|(old, new)| (old.id.as_str(), new.id.clone()))
            .collect();
        let mut result = current.clone();
        result.nodes.extend(nodes);
        for command in self
            .data
            .commands
            .iter()
            .filter(|c| &c.workspace == source_workspace)
        {
            let mut copy = command.clone();
            copy.id = uuid::Uuid::new_v4().to_string();
            copy.workspace = target_workspace.clone();
            result.commands.push(copy);
        }
        if *include_credentials {
            if target_workspace == "private" {
                for (id, credential) in &self.data.connection_credentials {
                    if let Some(new_id) = remapped.get(id.as_str()) {
                        let mut copy = credential.clone();
                        copy.id = new_id.clone();
                        result.connection_credentials.insert(new_id.clone(), copy);
                    }
                }
            }
            for credential in self
                .data
                .credentials
                .iter()
                .filter(|c| c.team_id.as_deref().unwrap_or("private") == source_workspace)
            {
                // An agent identity refers to this device and cannot be shared as raw credentials.
                if target_workspace != "private" && credential.kind == "agent" {
                    continue;
                }
                let mut copy = credential.clone();
                copy.id = uuid::Uuid::new_v4().to_string();
                copy.team_id = (target_workspace != "private").then(|| target_workspace.clone());
                result.credentials.push(copy);
            }
        }
        if target_workspace != "private" {
            let team = result
                .teams
                .iter_mut()
                .find(|t| &t.id == target_workspace)
                .context("Team fehlt")?;
            team.live_sync = false;
            team.sync_base = None;
            let name = team.name.clone();
            let change = result.team_change(
                target_workspace,
                ChangeAction::Imported,
                ChangeObject::Workspace,
                target_workspace,
                &name,
                vec![],
            )?;
            result.record_team_change(target_workspace, change);
        }
        // No bindings, organization, host pins, source team keys/tokens or past activity are copied.
        validate(&result)?;
        Ok(result)
    }

    /// Build and validate a replacement without changing the current vault.
    pub fn restore(&self, current: &VaultData, member_name: &str) -> Result<VaultData> {
        self.validate()?;
        let mut result = current.clone();
        match &self.scope {
            Scope::All => result = self.data.clone(),
            Scope::Private => {
                ensure!(
                    !self.data.commands.iter().any(|c| current
                        .commands
                        .iter()
                        .any(|old| old.id == c.id && old.workspace != "private")),
                    "Befehls-ID kollidiert mit einem Team"
                );
                result.commands.retain(|c| c.workspace != "private");
                result.commands.extend(self.data.commands.clone());
                ensure!(
                    !self.data.nodes.iter().any(|n| current
                        .nodes
                        .iter()
                        .any(|old| old.id == n.id && old.workspace != "private")),
                    "Eintrags-ID kollidiert mit einem Team"
                );
                ensure!(
                    !self.data.credentials.iter().any(|c| current
                        .credentials
                        .iter()
                        .any(|old| old.id == c.id && old.team_id.is_some())),
                    "Zugangsdaten-ID kollidiert mit einem Team"
                );
                result.connection_credentials = self.data.connection_credentials.clone();
                result.nodes.retain(|n| n.workspace != "private");
                result.nodes.extend(self.data.nodes.clone());
                result.credentials.retain(|c| c.team_id.is_some());
                result.credentials.extend(self.data.credentials.clone());
                result.bindings.retain(|id, _| {
                    !current
                        .nodes
                        .iter()
                        .any(|n| &n.id == id && n.workspace == "private")
                });
                result.bindings.extend(self.data.bindings.clone());
                for folder in &self.data.credential_folders {
                    if let Some(old) = result
                        .credential_folders
                        .iter()
                        .find(|old| old.id == folder.id)
                    {
                        ensure!(old == folder, "Persönlicher Zugangsdaten-Ordner wurde geändert; bitte vollständige Sicherung verwenden");
                    } else {
                        result.credential_folders.push(folder.clone());
                    }
                }
                result.credential_locations.retain(|id, _| {
                    !current
                        .credentials
                        .iter()
                        .any(|c| &c.id == id && c.team_id.is_none())
                });
                result
                    .credential_locations
                    .extend(self.data.credential_locations.clone());
            }
            Scope::Team { team_id } => {
                let snapshot = sync::snapshot(&self.data, team_id);
                if !result.teams.iter().any(|t| &t.id == team_id) {
                    result.teams.push(self.data.teams[0].clone());
                }
                // apply_snapshot checks cross-workspace collisions and preserves personal bindings.
                sync::apply_snapshot(&mut result, team_id, snapshot)?;
            }
        }
        result.prune_credential_locations();
        result.bindings.retain(|id, credential| {
            result.nodes.iter().any(|n| &n.id == id)
                && result.credentials.iter().any(|c| &c.id == credential)
        });
        for imported in &self.data.teams {
            let team = result
                .teams
                .iter_mut()
                .find(|t| t.id == imported.id)
                .context("Team fehlt")?;
            if let Some(old) = current.teams.iter().find(|t| t.id == imported.id) {
                team.member_name = old.member_name.clone();
            } else {
                team.member_name.clear();
            }
            if team.member_name.is_empty() {
                team.member_name = member_name.trim().into();
            }
            team_activity::validate_member_name(&team.member_name)?;
            team.dirty = true;
            team.live_sync = false;
            team.sync_base = None;
            let name = team.name.clone();
            let id = team.id.clone();
            let change = result.team_change(
                &id,
                ChangeAction::Imported,
                ChangeObject::Workspace,
                &id,
                &name,
                vec![],
            )?;
            result.record_team_change(&id, change);
        }
        validate(&result)?;
        Ok(result)
    }
}

pub fn encode(backup: &Backup, password: &str) -> Result<Vec<u8>> {
    ensure!(
        password.chars().count() >= 12,
        "Mindestens 12 Zeichen für das Backup-Passwort verwenden"
    );
    backup.validate()?;
    let salt = crypto::random_bytes::<16>();
    let key = crypto::derive_key(password, &salt)?;
    let plaintext = Zeroizing::new(serde_json::to_vec(backup)?);
    ensure!(plaintext.len() <= MAX_BYTES, "Backup ist zu groß");
    let file = BackupFile {
        format: "noryn-backup".into(),
        version: 1,
        salt: STANDARD.encode(salt),
        sealed: crypto::seal(&key, &plaintext, CONTEXT)?,
    };
    let bytes = serde_json::to_vec(&file)?;
    ensure!(bytes.len() <= MAX_BYTES, "Backup ist zu groß");
    Ok(bytes)
}

pub fn decode(bytes: &[u8], password: &str) -> Result<Backup> {
    ensure!(bytes.len() <= MAX_BYTES, "Backup ist zu groß");
    let file: BackupFile =
        serde_json::from_slice(bytes).context("Keine gültige Noryn-Sicherung")?;
    ensure!(
        file.format == "noryn-backup" && file.version == 1,
        "Unbekanntes Backup-Format"
    );
    let salt = STANDARD.decode(&file.salt)?;
    ensure!(salt.len() == 16, "Ungültiges Backup-Salz");
    let key = crypto::derive_key(password, &salt)?;
    let plaintext = crypto::unseal(&key, &file.sealed, CONTEXT)?;
    let backup: Backup = serde_json::from_slice(&plaintext)?;
    backup.validate()?;
    Ok(backup)
}

pub fn read(path: &Path, password: &str) -> Result<Backup> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    decode(&bytes, password)
}

pub fn write_new(path: &Path, backup: &Backup, password: &str) -> Result<()> {
    ensure!(path.is_absolute(), "Absoluten Backup-Pfad auswählen");
    let bytes = encode(backup, password)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("Backup-Pfad ungültig")?)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path).map_err(|e| e.error).context(
        "Backup konnte nicht angelegt werden; vorhandene Dateien werden nicht überschrieben",
    )?;
    Ok(())
}

pub fn fingerprint(data: &VaultData) -> Result<[u8; 32]> {
    let bytes = Zeroizing::new(serde_json::to_vec(data)?);
    Ok(Sha256::digest(&bytes).into())
}
