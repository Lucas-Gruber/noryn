//! Explicit workspace copies. Connection bindings and secrets are never copied implicitly.
use crate::{
    backup,
    model::{validate_nodes, Node, VaultData},
    team_activity::{self, ChangeAction, ChangedField},
};
use anyhow::{ensure, Context, Result};
use std::collections::{HashMap, HashSet};

pub fn validate_destination(data: &VaultData, workspace: &str, parent: Option<&str>) -> Result<()> {
    ensure!(
        workspace == "private" || data.teams.iter().any(|t| t.id == workspace),
        "Ziel-Workspace nicht gefunden"
    );
    if let Some(parent) = parent {
        ensure!(
            data.nodes
                .iter()
                .any(|n| n.id == parent && n.kind == "folder" && n.workspace == workspace),
            "Zielordner gehört nicht zum Ziel-Workspace"
        );
    }
    Ok(())
}

/// Remap a validated forest into the destination. No personal bindings or credentials.
pub fn remap_nodes(nodes: &[Node], workspace: &str, parent: Option<&str>) -> Result<Vec<Node>> {
    validate_nodes(nodes)?;
    let ids: HashMap<_, _> = nodes
        .iter()
        .map(|n| (n.id.as_str(), uuid::Uuid::new_v4().to_string()))
        .collect();
    Ok(nodes
        .iter()
        .map(|n| {
            let mut copy = n.clone();
            copy.id = ids[n.id.as_str()].clone();
            copy.workspace = workspace.into();
            copy.parent_id = n
                .parent_id
                .as_deref()
                .and_then(|id| ids.get(id))
                .cloned()
                .or_else(|| parent.map(str::to_owned));
            copy
        })
        .collect())
}

pub fn copy_nodes(
    data: &mut VaultData,
    source_ids: &[String],
    workspace: &str,
    parent: Option<&str>,
) -> Result<Vec<String>> {
    backup::validate(data)?;
    validate_destination(data, workspace, parent)?;
    ensure!(
        !source_ids.is_empty() && source_ids.len() <= 20_000,
        "Keine Einträge zum Kopieren ausgewählt"
    );
    let mut included: HashSet<String> = source_ids.iter().cloned().collect();
    ensure!(
        included.len() == source_ids.len(),
        "Einträge doppelt ausgewählt"
    );
    for id in source_ids {
        ensure!(
            data.nodes.iter().any(|n| &n.id == id),
            "Quell-Eintrag wurde entfernt; bitte erneut auswählen"
        );
    }
    loop {
        let before = included.len();
        for node in &data.nodes {
            if node
                .parent_id
                .as_ref()
                .is_some_and(|id| included.contains(id))
            {
                included.insert(node.id.clone());
            }
        }
        if before == included.len() {
            break;
        }
    }
    let mut source: Vec<_> = data
        .nodes
        .iter()
        .filter(|n| included.contains(&n.id))
        .cloned()
        .collect();
    // A selected folder's old parent is outside this forest.
    for node in &mut source {
        if node
            .parent_id
            .as_ref()
            .is_some_and(|id| !included.contains(id))
        {
            node.parent_id = None;
        }
    }
    let copies = remap_nodes(&source, workspace, parent)?;
    let copy_ids: HashSet<_> = copies.iter().map(|n| n.id.as_str()).collect();
    let roots: Vec<_> = copies
        .iter()
        .filter(|n| {
            n.parent_id
                .as_deref()
                .is_none_or(|id| !copy_ids.contains(id))
        })
        .collect();
    let mut result = data.clone();
    result.nodes.extend(copies.clone());
    validate_nodes(&result.nodes)?;
    for root in &roots {
        let change = result.team_change(
            workspace,
            ChangeAction::Created,
            team_activity::node_object(root),
            &root.id,
            &root.name,
            if root.kind == "folder" {
                vec![ChangedField::Children]
            } else {
                vec![]
            },
        )?;
        result.record_team_change(workspace, change);
    }
    backup::validate(&result)?;
    let ids = roots.iter().map(|n| n.id.clone()).collect();
    *data = result;
    Ok(ids)
}

/// Leave only this local installation. Never generate a server-side deletion snapshot.
pub fn remove_team(data: &mut VaultData, id: &str) -> Result<()> {
    data.teams
        .iter()
        .find(|t| t.id == id)
        .context("Team nicht gefunden")?;
    let mut result = data.clone();
    result.teams.retain(|t| t.id != id);
    result.nodes.retain(|n| n.workspace != id);
    result.commands.retain(|c| c.workspace != id);
    result
        .credentials
        .retain(|c| c.team_id.as_deref() != Some(id));
    let nodes: HashSet<_> = result.nodes.iter().map(|n| &n.id).collect();
    let credentials: HashSet<_> = result.credentials.iter().map(|c| &c.id).collect();
    result
        .bindings
        .retain(|node, credential| nodes.contains(node) && credentials.contains(credential));
    result.team_activity.remove(id);
    result.prune_credential_locations();
    backup::validate(&result)?;
    *data = result;
    Ok(())
}
