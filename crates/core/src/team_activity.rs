use crate::model::{Credential, Node, VaultData};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_CHANGES: usize = 1_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChangeAction {
    Created,
    Updated,
    Deleted,
    Shared,
    Unshared,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChangeObject {
    Folder,
    Connection,
    Credential,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChangedField {
    Name,
    Parent,
    Kind,
    Host,
    Port,
    Protocol,
    InitialPath,
    Notes,
    Tags,
    Favorite,
    Username,
    PrivateKey,
    Passphrase,
    Password,
    Children,
}

/// Describes the change without storing old/new secret values.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamChange {
    pub id: String,
    pub actor: String,
    pub occurred_at: u64,
    pub action: ChangeAction,
    pub object: ChangeObject,
    pub target_id: String,
    pub target_name: String,
    pub fields: Vec<ChangedField>,
}

pub fn validate_member_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty()
            && name.chars().count() <= 80
            && !name.chars().any(char::is_control),
        "Bitte unter Team-Sync deinen Anzeigenamen festlegen (1–80 Zeichen)"
    );
    Ok(())
}

pub fn validate_changes(changes: &[TeamChange]) -> Result<()> {
    ensure!(changes.len() <= MAX_CHANGES, "Zu viele Team-Änderungen");
    let mut ids = HashSet::new();
    for change in changes {
        validate_member_name(&change.actor)?;
        ensure!(
            uuid::Uuid::parse_str(&change.id).is_ok()
                && ids.insert(&change.id)
                && !change.target_id.is_empty()
                && change.target_id.len() <= 128
                && !change.target_name.trim().is_empty()
                && change.target_name.len() <= 512
                && (1..=253_402_300_799_999).contains(&change.occurred_at)
                && change.fields.len() <= 16,
            "Ungültiger Eintrag im Team-Änderungsprotokoll"
        );
    }
    Ok(())
}

impl VaultData {
    pub fn team_change(
        &self,
        team_id: &str,
        action: ChangeAction,
        object: ChangeObject,
        target_id: &str,
        target_name: &str,
        fields: Vec<ChangedField>,
    ) -> Result<Option<TeamChange>> {
        if team_id == "private" {
            return Ok(None);
        }
        let team = self
            .teams
            .iter()
            .find(|team| team.id == team_id)
            .context("Team nicht gefunden")?;
        validate_member_name(&team.member_name)?;
        let occurred_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis()
            .try_into()?;
        Ok(Some(TeamChange {
            id: uuid::Uuid::new_v4().to_string(),
            actor: team.member_name.trim().to_string(),
            occurred_at,
            action,
            object,
            target_id: target_id.into(),
            target_name: target_name.into(),
            fields,
        }))
    }

    pub fn record_team_change(&mut self, team_id: &str, change: Option<TeamChange>) {
        if let Some(change) = change {
            let changes = self.team_activity.entry(team_id.into()).or_default();
            changes.push(change);
            if changes.len() > MAX_CHANGES {
                changes.drain(..changes.len() - MAX_CHANGES);
            }
            self.mark_dirty(team_id);
        }
    }
}

pub fn node_object(node: &Node) -> ChangeObject {
    if node.kind == "folder" {
        ChangeObject::Folder
    } else {
        ChangeObject::Connection
    }
}

pub fn node_fields(old: &Node, new: &Node) -> Vec<ChangedField> {
    use ChangedField::*;
    [
        (old.name != new.name, Name),
        (old.parent_id != new.parent_id, Parent),
        (old.kind != new.kind, Kind),
        (old.host != new.host, Host),
        (old.port != new.port, Port),
        (old.protocol != new.protocol, Protocol),
        (old.initial_path != new.initial_path, InitialPath),
        (old.notes != new.notes, Notes),
        (old.tags != new.tags, Tags),
        (old.favorite != new.favorite, Favorite),
    ]
    .into_iter()
    .filter_map(|(changed, field)| changed.then_some(field))
    .collect()
}

pub fn credential_fields(old: &Credential, new: &Credential) -> Vec<ChangedField> {
    use ChangedField::*;
    [
        (old.name != new.name, Name),
        (old.username != new.username, Username),
        (old.kind != new.kind, Kind),
        (
            (old.kind == "key" || new.kind == "key") && old.secret != new.secret,
            PrivateKey,
        ),
        (old.passphrase != new.passphrase, Passphrase),
        (old.login_password() != new.login_password(), Password),
    ]
    .into_iter()
    .filter_map(|(changed, field)| changed.then_some(field))
    .collect()
}
