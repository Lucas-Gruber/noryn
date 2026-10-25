pub use crate::credential_folders::CredentialFolder;
use crate::team_activity::{self, ChangeAction, ChangeObject, TeamChange};
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub workspace: String,
    pub kind: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub initial_path: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub favorite: bool,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: String,
    pub name: String,
    pub username: String,
    pub kind: String,
    pub secret: String,
    pub passphrase: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub team_id: Option<String>,
}

impl Credential {
    pub fn login_password(&self) -> &str {
        if !self.password.is_empty() {
            &self.password
        } else if self.kind == "password" {
            &self.secret
        } else {
            ""
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub member_name: String,
    pub server_url: String,
    pub token: String,
    pub key: String,
    pub revision: Option<String>,
    pub dirty: bool,
    #[serde(default)]
    pub live_sync: bool,
    #[serde(default)]
    pub sync_base: Option<String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultData {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub script_libraries: HashMap<String, crate::scripts::Cache>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub health_logins: HashMap<String, crate::login_health::LoginPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<crate::commands::SavedCommand>,
    #[serde(default)]
    pub local_revision: u64,
    pub nodes: Vec<Node>,
    pub credentials: Vec<Credential>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub connection_credentials: HashMap<String, Credential>,
    #[serde(default)]
    pub credential_folders: Vec<CredentialFolder>,
    #[serde(default)]
    pub credential_locations: HashMap<String, String>,
    pub bindings: HashMap<String, String>,
    pub known_hosts: HashMap<String, String>,
    pub teams: Vec<Team>,
    #[serde(default)]
    pub team_activity: HashMap<String, Vec<TeamChange>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInfo {
    pub id: String,
    pub name: String,
    pub username: String,
    pub kind: String,
    pub has_password: bool,
    pub team_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub member_name: String,
    pub server_url: String,
    pub revision: Option<String>,
    pub dirty: bool,
    pub live_sync: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultView {
    pub script_libraries: HashMap<String, crate::scripts::Summary>,
    pub commands: Vec<crate::commands::SavedCommand>,
    pub local_revision: u64,
    pub nodes: Vec<Node>,
    pub credentials: Vec<CredentialInfo>,
    pub connection_credentials: HashMap<String, CredentialInfo>,
    pub credential_folders: Vec<CredentialFolder>,
    pub credential_locations: HashMap<String, String>,
    pub bindings: HashMap<String, String>,
    pub teams: Vec<TeamInfo>,
    pub team_activity: HashMap<String, Vec<TeamChange>>,
}

impl VaultData {
    pub fn view(&self) -> VaultView {
        VaultView {
            script_libraries: crate::scripts::summaries(self),
            commands: self.commands.clone(),
            local_revision: self.local_revision,
            nodes: self.nodes.clone(),
            credentials: self
                .credentials
                .iter()
                .map(|c| CredentialInfo {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    username: c.username.clone(),
                    kind: c.kind.clone(),
                    has_password: !c.login_password().is_empty(),
                    team_id: c.team_id.clone(),
                })
                .collect(),
            connection_credentials: self
                .connection_credentials
                .iter()
                .map(|(id, c)| {
                    (
                        id.clone(),
                        CredentialInfo {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            username: c.username.clone(),
                            kind: c.kind.clone(),
                            has_password: !c.login_password().is_empty(),
                            team_id: None,
                        },
                    )
                })
                .collect(),
            bindings: self.bindings.clone(),
            credential_folders: self.credential_folders.clone(),
            credential_locations: self.credential_locations.clone(),
            team_activity: self.team_activity.clone(),
            teams: self
                .teams
                .iter()
                .map(|t| TeamInfo {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    member_name: t.member_name.clone(),
                    server_url: t.server_url.clone(),
                    revision: t.revision.clone(),
                    dirty: t.dirty,
                    live_sync: t.live_sync,
                })
                .collect(),
        }
    }

    pub fn credential_for(&self, node_id: &str) -> Result<&Credential> {
        if let Some(credential) = self.connection_credentials.get(node_id) {
            anyhow::ensure!(
                self.nodes
                    .iter()
                    .any(|n| n.id == node_id && n.workspace == "private" && n.kind == "connection"),
                "Direkte Zugangsdaten sind nur für private Verbindungen erlaubt"
            );
            return Ok(credential);
        }
        let mut current = Some(node_id);
        let mut visited = HashSet::new();
        while let Some(id) = current {
            ensure!(visited.insert(id), "Zyklus im Verbindungsbaum");
            if let Some(credential_id) = self.bindings.get(id) {
                return self
                    .credentials
                    .iter()
                    .find(|c| &c.id == credential_id)
                    .ok_or_else(|| anyhow::anyhow!("Zugangsdaten nicht gefunden"));
            }
            current = self
                .nodes
                .iter()
                .find(|n| n.id == id)
                .and_then(|n| n.parent_id.as_deref());
        }
        bail!("Bitte zuerst persönliche Zugangsdaten zuordnen (am Eintrag oder übergeordneten Ordner).")
    }

    pub fn upsert_credential(&mut self, credential: Credential) -> Result<()> {
        validate_credential(&credential)?;
        if let Some(team) = &credential.team_id {
            ensure!(
                team != "private" && self.teams.iter().any(|entry| &entry.id == team),
                "Team-Tresor nicht gefunden"
            );
        }
        let old = self.credentials.iter().find(|c| c.id == credential.id);
        let old_team = old.and_then(|c| c.team_id.clone());
        let fields = old
            .map(|old| team_activity::credential_fields(old, &credential))
            .unwrap_or_default();
        let mut changes = Vec::new();
        if old_team != credential.team_id {
            if let Some(team) = &old_team {
                changes.push((
                    team.clone(),
                    self.team_change(
                        team,
                        ChangeAction::Unshared,
                        ChangeObject::Credential,
                        &credential.id,
                        &old.unwrap().name,
                        vec![],
                    )?,
                ));
            }
            if let Some(team) = &credential.team_id {
                let action = if old.is_none() {
                    ChangeAction::Created
                } else {
                    ChangeAction::Shared
                };
                changes.push((
                    team.clone(),
                    self.team_change(
                        team,
                        action,
                        ChangeObject::Credential,
                        &credential.id,
                        &credential.name,
                        vec![],
                    )?,
                ));
            }
        } else if let Some(team) = &credential.team_id {
            if !fields.is_empty() {
                changes.push((
                    team.clone(),
                    self.team_change(
                        team,
                        ChangeAction::Updated,
                        ChangeObject::Credential,
                        &credential.id,
                        &credential.name,
                        fields,
                    )?,
                ));
            }
        }
        self.credentials.retain(|c| c.id != credential.id);
        self.credentials.push(credential);
        for (team, change) in changes {
            self.record_team_change(&team, change);
        }
        Ok(())
    }

    pub fn remove_credential(&mut self, id: &str) -> Result<()> {
        let change = self
            .credentials
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.team_id.as_ref().map(|team| (team, c)))
            .map(|(team, credential)| {
                Ok::<_, anyhow::Error>((
                    team.clone(),
                    self.team_change(
                        team,
                        ChangeAction::Deleted,
                        ChangeObject::Credential,
                        id,
                        &credential.name,
                        vec![],
                    )?,
                ))
            })
            .transpose()?;
        self.credentials.retain(|c| c.id != id);
        self.credential_locations.remove(id);
        self.bindings.retain(|_, credential| credential != id);
        if let Some((team, event)) = change {
            self.record_team_change(&team, event);
        }
        Ok(())
    }

    pub fn upsert_node(&mut self, node: Node) -> Result<()> {
        ensure!(
            !self.connection_credentials.contains_key(&node.id) || node.kind == "connection",
            "Einträge mit direkten Zugangsdaten müssen Verbindungen bleiben"
        );
        ensure!(
            node.workspace == "private" || self.teams.iter().any(|t| t.id == node.workspace),
            "Unbekannter Arbeitsbereich"
        );
        if let Some(old) = self.nodes.iter().find(|n| n.id == node.id) {
            ensure!(
                old.workspace == node.workspace,
                "Einträge können nicht zwischen Arbeitsbereichen verschoben werden"
            );
        }
        let mut nodes = self.nodes.clone();
        nodes.retain(|n| n.id != node.id);
        nodes.push(node.clone());
        validate_nodes(&nodes)?;
        let old = self.nodes.iter().find(|entry| entry.id == node.id);
        let fields = old
            .map(|old| team_activity::node_fields(old, &node))
            .unwrap_or_default();
        if old.is_some() && fields.is_empty() {
            return Ok(());
        }
        let change = self.team_change(
            &node.workspace,
            if old.is_some() {
                ChangeAction::Updated
            } else {
                ChangeAction::Created
            },
            team_activity::node_object(&node),
            &node.id,
            &node.name,
            fields,
        )?;
        self.nodes = nodes;
        self.record_team_change(&node.workspace, change);
        Ok(())
    }

    pub fn remove_node(&mut self, id: &str) -> Result<()> {
        let workspace = self
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| anyhow::anyhow!("Eintrag nicht gefunden"))?
            .workspace
            .clone();
        let mut removed = HashSet::from([id.to_string()]);
        loop {
            let count = removed.len();
            for node in &self.nodes {
                if node.parent_id.as_ref().is_some_and(|p| removed.contains(p)) {
                    removed.insert(node.id.clone());
                }
            }
            if count == removed.len() {
                break;
            }
        }
        let target = self.nodes.iter().find(|node| node.id == id).unwrap();
        let change = self.team_change(
            &workspace,
            ChangeAction::Deleted,
            team_activity::node_object(target),
            id,
            &target.name,
            if removed.len() > 1 {
                vec![team_activity::ChangedField::Children]
            } else {
                vec![]
            },
        )?;
        self.nodes.retain(|n| !removed.contains(&n.id));
        self.health_logins.retain(|id, _| !removed.contains(id));
        self.bindings.retain(|id, _| !removed.contains(id));
        self.connection_credentials
            .retain(|id, _| !removed.contains(id));
        self.record_team_change(&workspace, change);
        Ok(())
    }

    pub fn mark_dirty(&mut self, workspace: &str) {
        if let Some(team) = self.teams.iter_mut().find(|t| t.id == workspace) {
            team.dirty = true;
        }
    }
}

pub fn validate_credential(credential: &Credential) -> Result<()> {
    ensure!(
        !credential.id.is_empty()
            && credential.id.len() <= 128
            && !credential.name.trim().is_empty()
            && credential.name.len() <= 512
            && !credential.username.trim().is_empty()
            && credential.username.len() <= 512,
        "Ungültige Zugangsdaten"
    );
    ensure!(
        ["password", "key", "agent"].contains(&credential.kind.as_str()),
        "Unbekannter Zugangstyp"
    );
    ensure!(
        credential.secret.len() <= 2 * 1024 * 1024
            && credential.password.len() <= 65_536
            && credential.passphrase.len() <= 65_536,
        "Zugangsdaten sind zu groß"
    );
    ensure!(
        credential.kind != "key" || !credential.secret.is_empty(),
        "Privater SSH-Schlüssel fehlt"
    );
    ensure!(
        credential.team_id.is_none() || credential.kind != "agent",
        "Ein SSH-Agent bleibt lokal und kann nicht in einem Team-Tresor gespeichert werden"
    );
    Ok(())
}

pub fn validate_nodes(nodes: &[Node]) -> Result<()> {
    ensure!(nodes.len() <= 20_000, "Zu viele Einträge");
    let mut ids = HashMap::new();
    for node in nodes {
        ensure!(!node.id.is_empty() && node.id.len() <= 128, "Ungültige ID");
        ensure!(ids.insert(&node.id, node).is_none(), "Doppelte Eintrags-ID");
        ensure!(
            !node.name.trim().is_empty() && node.name.len() <= 512,
            "Name fehlt oder ist zu lang"
        );
        ensure!(
            node.kind == "folder" || node.kind == "connection",
            "Ungültiger Eintragstyp"
        );
        ensure!(
            ["ssh", "sftp", "rdp", "vnc", "ftp", "ftps"].contains(&node.protocol.as_str()),
            "Ungültiges Protokoll"
        );
        ensure!(
            node.initial_path.len() <= 4096
                && !node.initial_path.contains(['\0', '\r', '\n'])
                && (node.initial_path.is_empty()
                    || (node.kind == "connection"
                        && ["sftp", "ftp", "ftps"].contains(&node.protocol.as_str())
                        && node.initial_path.starts_with('/'))),
            "Startordner ist nur als absoluter Pfad bei SFTP, FTP und FTPS erlaubt"
        );
        ensure!(
            node.notes.len() <= 100_000 && node.tags.len() <= 32,
            "Eintrag ist zu groß"
        );
        if node.kind == "connection" {
            ensure!(
                !node.host.trim().is_empty()
                    && node.host.len() <= 253
                    && !node.host.chars().any(char::is_control),
                "Ungültiger Host"
            );
            ensure!(node.port > 0, "Ungültiger Port");
        }
    }
    for node in nodes {
        let mut current = node;
        let mut seen = HashSet::new();
        while let Some(parent_id) = &current.parent_id {
            ensure!(seen.insert(&current.id), "Zyklus im Verbindungsbaum");
            let parent = ids
                .get(parent_id)
                .ok_or_else(|| anyhow::anyhow!("Übergeordneter Ordner fehlt"))?;
            ensure!(
                parent.kind == "folder" && parent.workspace == node.workspace,
                "Ungültiger übergeordneter Ordner"
            );
            current = parent;
        }
    }
    Ok(())
}
