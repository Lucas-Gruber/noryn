pub use crate::credential_folders::CredentialFolder;
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

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
    pub server_url: String,
    pub token: String,
    pub key: String,
    pub revision: Option<String>,
    pub dirty: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultData {
    pub nodes: Vec<Node>,
    pub credentials: Vec<Credential>,
    #[serde(default)]
    pub credential_folders: Vec<CredentialFolder>,
    #[serde(default)]
    pub credential_locations: HashMap<String, String>,
    pub bindings: HashMap<String, String>,
    pub known_hosts: HashMap<String, String>,
    pub teams: Vec<Team>,
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
    pub server_url: String,
    pub revision: Option<String>,
    pub dirty: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultView {
    pub nodes: Vec<Node>,
    pub credentials: Vec<CredentialInfo>,
    pub credential_folders: Vec<CredentialFolder>,
    pub credential_locations: HashMap<String, String>,
    pub bindings: HashMap<String, String>,
    pub teams: Vec<TeamInfo>,
}

impl VaultData {
    pub fn view(&self) -> VaultView {
        VaultView {
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
            bindings: self.bindings.clone(),
            credential_folders: self.credential_folders.clone(),
            credential_locations: self.credential_locations.clone(),
            teams: self
                .teams
                .iter()
                .map(|t| TeamInfo {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    server_url: t.server_url.clone(),
                    revision: t.revision.clone(),
                    dirty: t.dirty,
                })
                .collect(),
        }
    }

    pub fn credential_for(&self, node_id: &str) -> Result<&Credential> {
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
                self.teams.iter().any(|t| &t.id == team),
                "Team-Tresor nicht gefunden"
            );
        }
        let old_team = self
            .credentials
            .iter()
            .find(|c| c.id == credential.id)
            .and_then(|c| c.team_id.clone());
        if let Some(team) = old_team {
            self.mark_dirty(&team);
        }
        if let Some(team) = &credential.team_id {
            self.mark_dirty(team);
        }
        self.credentials.retain(|c| c.id != credential.id);
        self.credentials.push(credential);
        Ok(())
    }

    pub fn remove_credential(&mut self, id: &str) {
        if let Some(team) = self
            .credentials
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.team_id.clone())
        {
            self.mark_dirty(&team);
        }
        self.credentials.retain(|c| c.id != id);
        self.credential_locations.remove(id);
        self.bindings.retain(|_, credential| credential != id);
    }

    pub fn upsert_node(&mut self, node: Node) -> Result<()> {
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
        self.nodes = nodes;
        self.mark_dirty(&node.workspace);
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
        self.nodes.retain(|n| !removed.contains(&n.id));
        self.bindings.retain(|id, _| !removed.contains(id));
        self.mark_dirty(&workspace);
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
