//! Explicitly scoped command library. Nothing in this module executes commands.
use crate::{
    model::VaultData,
    team_activity::{ChangeAction, ChangeObject, ChangedField},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Serialize, Deserialize, PartialEq, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedCommand {
    pub id: String,
    pub workspace: String,
    pub name: String,
    pub description: String,
    pub command: String,
}

pub fn validate(commands: &[SavedCommand]) -> Result<()> {
    ensure!(
        commands.len() <= 2_000,
        "Maximal 2.000 gespeicherte Befehle"
    );
    let mut ids = HashSet::new();
    for command in commands {
        ensure!(
            uuid::Uuid::parse_str(&command.id).is_ok() && ids.insert(&command.id),
            "Ungültige oder doppelte Befehls-ID"
        );
        ensure!(
            !command.name.trim().is_empty()
                && command.name.len() <= 512
                && !command.name.chars().any(char::is_control),
            "Befehlsname: 1–512 Bytes, ohne Steuerzeichen"
        );
        ensure!(
            command.description.len() <= 4_096
                && !command.command.trim().is_empty()
                && command.command.len() <= 65_536,
            "Befehl fehlt oder ist zu lang (maximal 64 KiB)"
        );
        ensure!(
            [&command.command, &command.description].iter().all(|s| !s
                .chars()
                .any(|c| c.is_control() && !['\n', '\t'].contains(&c))),
            "Befehle dürfen keine Terminal-Steuerzeichen enthalten"
        );
        ensure!(
            command.workspace == "private"
                || crate::sync::validate_team_id(&command.workspace).is_ok(),
            "Ungültiger Befehls-Workspace"
        );
    }
    Ok(())
}

pub fn validate_vault(data: &VaultData) -> Result<()> {
    validate(&data.commands)?;
    ensure!(
        data.commands
            .iter()
            .all(|c| c.workspace == "private" || data.teams.iter().any(|t| t.id == c.workspace)),
        "Befehl gehört zu einem unbekannten Workspace"
    );
    Ok(())
}

impl VaultData {
    pub fn save_command(&mut self, command: SavedCommand) -> Result<()> {
        if let Some(old) = self.commands.iter().find(|c| c.id == command.id) {
            ensure!(
                old.workspace == command.workspace,
                "Für einen anderen Tresor bitte eine neue Kopie anlegen"
            );
            if old == &command {
                return Ok(());
            }
        }
        let mut commands = self.commands.clone();
        commands.retain(|c| c.id != command.id);
        commands.push(command.clone());
        validate(&commands)?;
        ensure!(
            command.workspace == "private" || self.teams.iter().any(|t| t.id == command.workspace),
            "Workspace nicht gefunden"
        );
        let old = self.commands.iter().find(|c| c.id == command.id);
        let fields = old
            .map(|old| {
                [
                    (old.name != command.name, ChangedField::Name),
                    (old.description != command.description, ChangedField::Notes),
                    (old.command != command.command, ChangedField::Command),
                ]
                .into_iter()
                .filter_map(|(changed, field)| changed.then_some(field))
                .collect()
            })
            .unwrap_or_default();
        let change = self.team_change(
            &command.workspace,
            if old.is_some() {
                ChangeAction::Updated
            } else {
                ChangeAction::Created
            },
            ChangeObject::Command,
            &command.id,
            &command.name,
            fields,
        )?;
        self.commands = commands;
        self.record_team_change(&command.workspace, change);
        Ok(())
    }

    pub fn remove_command(&mut self, id: &str) -> Result<()> {
        let command = self
            .commands
            .iter()
            .find(|c| c.id == id)
            .context("Befehl wurde bereits entfernt")?;
        let workspace = command.workspace.clone();
        let change = self.team_change(
            &workspace,
            ChangeAction::Deleted,
            ChangeObject::Command,
            id,
            &command.name,
            vec![],
        )?;
        self.commands.retain(|c| c.id != id);
        self.record_team_change(&workspace, change);
        Ok(())
    }
}
