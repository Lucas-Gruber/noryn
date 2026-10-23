use crate::model::VaultData;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Personal presentation metadata; never part of credentials or team snapshots.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialFolder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
}

pub fn validate(data: &VaultData) -> Result<()> {
    validate_tree(&data.credential_folders)?;
    let folders: HashSet<_> = data.credential_folders.iter().map(|f| &f.id).collect();
    let credentials: HashSet<_> = data.credentials.iter().map(|c| &c.id).collect();
    ensure!(
        data.credential_locations
            .iter()
            .all(
                |(credential, folder)| credentials.contains(credential) && folders.contains(folder)
            ),
        "Ungültige Ablage für Zugangsdaten"
    );
    Ok(())
}

fn validate_tree(folders: &[CredentialFolder]) -> Result<()> {
    ensure!(folders.len() <= 2_000, "Zu viele Zugangsdaten-Ordner");
    let mut ids = HashMap::new();
    for folder in folders {
        ensure!(
            !folder.id.is_empty() && folder.id.len() <= 128,
            "Ungültige Ordner-ID"
        );
        ensure!(
            !folder.name.trim().is_empty()
                && folder.name.len() <= 512
                && !folder.name.chars().any(char::is_control),
            "Ordnername fehlt oder ist ungültig"
        );
        ensure!(
            ids.insert(&folder.id, folder).is_none(),
            "Doppelte Ordner-ID"
        );
    }
    for folder in folders {
        let mut current = folder;
        let mut seen = HashSet::new();
        while let Some(parent) = &current.parent_id {
            ensure!(seen.insert(&current.id), "Zyklus in Zugangsdaten-Ordnern");
            ensure!(seen.len() <= 32, "Höchstens 32 Ordnerebenen erlaubt");
            current = ids
                .get(parent)
                .ok_or_else(|| anyhow::anyhow!("Übergeordneter Zugangsdaten-Ordner fehlt"))?;
        }
    }
    Ok(())
}

impl VaultData {
    pub fn upsert_credential_folder(&mut self, mut folder: CredentialFolder) -> Result<()> {
        folder.name = folder.name.trim().to_string();
        let mut folders = self.credential_folders.clone();
        folders.retain(|f| f.id != folder.id);
        folders.push(folder);
        validate_tree(&folders)?;
        self.credential_folders = folders;
        Ok(())
    }

    pub fn set_credential_folder(
        &mut self,
        credential: &str,
        folder: Option<String>,
    ) -> Result<()> {
        ensure!(
            self.credentials.iter().any(|c| c.id == credential),
            "Zugangsdaten nicht gefunden"
        );
        if let Some(folder) = folder {
            ensure!(
                self.credential_folders.iter().any(|f| f.id == folder),
                "Zugangsdaten-Ordner nicht gefunden"
            );
            self.credential_locations.insert(credential.into(), folder);
        } else {
            self.credential_locations.remove(credential);
        }
        Ok(())
    }

    pub fn remove_credential_folder(&mut self, id: &str) -> Result<()> {
        let parent = self
            .credential_folders
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| anyhow::anyhow!("Zugangsdaten-Ordner nicht gefunden"))?
            .parent_id
            .clone();
        // Deleting a folder only changes presentation: keep all accounts and children.
        for child in &mut self.credential_folders {
            if child.parent_id.as_deref() == Some(id) {
                child.parent_id = parent.clone();
            }
        }
        self.credential_folders.retain(|f| f.id != id);
        self.credential_locations.retain(|_, folder| {
            if folder != id {
                return true;
            }
            if let Some(parent) = &parent {
                *folder = parent.clone();
                true
            } else {
                false
            }
        });
        Ok(())
    }

    pub fn prune_credential_locations(&mut self) {
        let credentials: HashSet<_> = self.credentials.iter().map(|c| &c.id).collect();
        self.credential_locations
            .retain(|credential, _| credentials.contains(credential));
    }
}
