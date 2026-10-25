//! Private connection-owned logins. These records never belong to a shared Node or snapshot.
use crate::model::{validate_credential, Credential, VaultData};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "mode",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Login {
    Keep,
    Account { credential_id: Option<String> },
    Direct { credential: Credential },
}

pub fn validate(data: &VaultData) -> Result<()> {
    ensure!(
        data.connection_credentials.len() <= 20_000,
        "Zu viele direkte Verbindungszugänge"
    );
    for (id, credential) in &data.connection_credentials {
        ensure!(
            data.nodes
                .iter()
                .any(|n| &n.id == id && n.workspace == "private" && n.kind == "connection"),
            "Direkte Zugangsdaten sind nur für private Verbindungen erlaubt"
        );
        ensure!(
            credential.team_id.is_none() && credential.id == *id,
            "Ungültiger direkter Verbindungszugang"
        );
        validate_credential(credential)?;
    }
    Ok(())
}

impl VaultData {
    pub fn set_connection_login(&mut self, node_id: &str, login: Login) -> Result<()> {
        let node = self
            .nodes
            .iter()
            .find(|n| n.id == node_id)
            .context("Eintrag nicht gefunden")?;
        match login {
            Login::Keep => {
                ensure!(
                    !self.connection_credentials.contains_key(node_id)
                        || (node.workspace == "private" && node.kind == "connection"),
                    "Direkte Zugangsdaten sind nur für private Verbindungen erlaubt"
                );
            }
            Login::Account { credential_id } => {
                if let Some(id) = &credential_id {
                    ensure!(
                        self.credentials.iter().any(|c| &c.id == id),
                        "Zugangsdaten nicht gefunden"
                    );
                }
                self.connection_credentials.remove(node_id);
                if let Some(id) = credential_id {
                    self.bindings.insert(node_id.into(), id);
                } else {
                    self.bindings.remove(node_id);
                }
            }
            Login::Direct { mut credential } => {
                ensure!(
                    node.workspace == "private"
                        && node.kind == "connection"
                        && credential.team_id.is_none(),
                    "Direkte Zugangsdaten sind nur für private Verbindungen erlaubt"
                );
                credential.id = node_id.into();
                credential.name = node.name.clone();
                if let Some(old) = self.connection_credentials.get(node_id) {
                    if credential.kind == old.kind && credential.secret.is_empty() {
                        credential.secret = old.secret.clone();
                        if credential.passphrase.is_empty() {
                            credential.passphrase = old.passphrase.clone();
                        }
                    }
                    if credential.password.is_empty() {
                        credential.password = old.login_password().into();
                    }
                }
                if credential.kind == "password" && !credential.password.is_empty() {
                    credential.secret.clear();
                }
                validate_credential(&credential)?;
                self.connection_credentials
                    .insert(node_id.into(), credential);
                self.bindings.remove(node_id);
            }
        }
        Ok(())
    }
}
