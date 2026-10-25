//! Three-way merge using the last acknowledged snapshot, never device-clock wins.
use crate::{
    model::{Credential, Node},
    sync::{self, TeamSnapshot},
    team_activity::MAX_CHANGES,
};
use anyhow::{ensure, Result};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub object: String,
    pub id: String,
    pub name: String,
    pub fields: Vec<String>,
}

pub enum Merged {
    Ready(TeamSnapshot),
    Conflicts(Vec<Conflict>),
}

fn field<T: Clone + PartialEq>(
    base: &T,
    local: &T,
    remote: &T,
    name: &str,
    conflicts: &mut Vec<String>,
) -> T {
    if local == base {
        remote.clone()
    } else if remote == base || local == remote {
        local.clone()
    } else {
        conflicts.push(name.into());
        local.clone()
    }
}

fn entries<T: Clone + PartialEq>(
    sources: (&[T], &[T], &[T]),
    id: impl Fn(&T) -> &str,
    name: impl Fn(&T) -> &str,
    object: &str,
    merge: impl Fn(&T, &T, &T, &mut Vec<String>) -> T,
    conflicts: &mut Vec<Conflict>,
) -> Vec<T> {
    let (base, local, remote) = sources;
    let base: HashMap<_, _> = base.iter().map(|e| (id(e), e)).collect();
    let local: HashMap<_, _> = local.iter().map(|e| (id(e), e)).collect();
    let remote: HashMap<_, _> = remote.iter().map(|e| (id(e), e)).collect();
    let ids: BTreeSet<_> = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .copied()
        .collect();
    let mut result = Vec::new();
    for id in ids {
        let (b, l, r) = (
            base.get(id).copied(),
            local.get(id).copied(),
            remote.get(id).copied(),
        );
        let mut fields = Vec::new();
        let value = if l == r {
            l.cloned()
        } else if l == b {
            r.cloned()
        } else if r == b {
            l.cloned()
        } else if let (Some(b), Some(l), Some(r)) = (b, l, r) {
            Some(merge(b, l, r, &mut fields))
        } else {
            fields.push("entry".into());
            l.or(r).cloned()
        };
        if !fields.is_empty() {
            conflicts.push(Conflict {
                object: object.into(),
                id: id.into(),
                name: name(l.or(r).or(b).unwrap()).into(),
                fields,
            });
        }
        if let Some(value) = value {
            result.push(value);
        }
    }
    result
}

fn node(b: &Node, l: &Node, r: &Node, conflicts: &mut Vec<String>) -> Node {
    macro_rules! value {
        ($field:ident, $name:literal) => {
            field(&b.$field, &l.$field, &r.$field, $name, conflicts)
        };
    }
    Node {
        id: l.id.clone(),
        workspace: l.workspace.clone(),
        parent_id: value!(parent_id, "parent"),
        kind: value!(kind, "kind"),
        name: value!(name, "name"),
        protocol: value!(protocol, "protocol"),
        host: value!(host, "host"),
        port: value!(port, "port"),
        initial_path: value!(initial_path, "initialPath"),
        notes: value!(notes, "notes"),
        tags: value!(tags, "tags"),
        favorite: value!(favorite, "favorite"),
    }
}

pub fn merge_node_edit(base: &Node, edited: &Node, current: &Node) -> Result<Node> {
    ensure!(
        base.id == edited.id
            && edited.id == current.id
            && base.workspace == edited.workspace
            && edited.workspace == current.workspace,
        "Eintrag wurde verschoben oder ersetzt; bitte erneut öffnen"
    );
    let mut conflicts = Vec::new();
    let merged = node(base, edited, current, &mut conflicts);
    ensure!(conflicts.is_empty(), "Eintrag wurde zwischenzeitlich geändert ({}). Deine Eingaben bleiben im Dialog; bitte prüfen und den Eintrag erneut öffnen.", conflicts.join(", "));
    Ok(merged)
}

fn credential(
    b: &Credential,
    l: &Credential,
    r: &Credential,
    conflicts: &mut Vec<String>,
) -> Credential {
    macro_rules! value {
        ($field:ident, $name:literal) => {
            field(&b.$field, &l.$field, &r.$field, $name, conflicts)
        };
    }
    // A key and its passphrase must remain a matching pair after a merge.
    let base_key = (&b.kind, &b.secret, &b.passphrase);
    let local_key = (&l.kind, &l.secret, &l.passphrase);
    let remote_key = (&r.kind, &r.secret, &r.passphrase);
    let key_conflict = (b.kind == "key" || l.kind == "key" || r.kind == "key")
        && local_key != base_key
        && remote_key != base_key
        && local_key != remote_key;
    if key_conflict {
        conflicts.push("authentication".into());
    }
    Credential {
        id: l.id.clone(),
        team_id: l.team_id.clone(),
        name: value!(name, "name"),
        username: value!(username, "username"),
        kind: value!(kind, "kind"),
        secret: value!(secret, "secret"),
        password: value!(password, "password"),
        passphrase: value!(passphrase, "passphrase"),
    }
}

pub fn merge(base: &TeamSnapshot, local: &TeamSnapshot, remote: &TeamSnapshot) -> Result<Merged> {
    ensure!(
        base.team_id == local.team_id && local.team_id == remote.team_id,
        "Abgleich gehört zu einem anderen Team"
    );
    for snapshot in [base, local, remote] {
        ensure!(
            snapshot
                .commands
                .iter()
                .all(|c| c.workspace == snapshot.team_id),
            "Abgleich enthält private oder fremde Befehle"
        );
        ensure!(
            snapshot.version >= 4 || snapshot.commands.is_empty(),
            "Befehle benötigen Snapshot-Format 4"
        );
        crate::commands::validate(&snapshot.commands)?;
        ensure!(
            snapshot
                .nodes
                .iter()
                .all(|n| n.workspace == snapshot.team_id)
                && snapshot
                    .credentials
                    .iter()
                    .all(|c| c.team_id.as_ref() == Some(&snapshot.team_id)),
            "Abgleich enthält fremde oder private Daten"
        );
        crate::model::validate_nodes(&snapshot.nodes)?;
        let mut ids = std::collections::HashSet::new();
        for credential in &snapshot.credentials {
            ensure!(ids.insert(&credential.id), "Doppelte Zugangsdaten-ID");
            crate::model::validate_credential(credential)?;
        }
        crate::team_activity::validate_changes(&snapshot.activity)?;
    }
    let mut conflicts = Vec::new();
    let commands = entries(
        (
            &base.commands,
            if local.version >= 4 {
                &local.commands
            } else {
                &base.commands
            },
            if remote.version >= 4 {
                &remote.commands
            } else {
                &base.commands
            },
        ),
        |c| &c.id,
        |c| &c.name,
        "command",
        |b, l, r, conflicts| crate::commands::SavedCommand {
            id: l.id.clone(),
            workspace: l.workspace.clone(),
            name: field(&b.name, &l.name, &r.name, "name", conflicts),
            description: field(
                &b.description,
                &l.description,
                &r.description,
                "notes",
                conflicts,
            ),
            command: field(&b.command, &l.command, &r.command, "command", conflicts),
        },
        &mut conflicts,
    );
    let nodes = entries(
        (&base.nodes, &local.nodes, &remote.nodes),
        |n| &n.id,
        |n| &n.name,
        "node",
        node,
        &mut conflicts,
    );
    let credentials = entries(
        (&base.credentials, &local.credentials, &remote.credentials),
        |c| &c.id,
        |c| &c.name,
        "credential",
        credential,
        &mut conflicts,
    );
    let mut activity = Vec::new();
    let mut activity_ids = HashMap::new();
    let base_activity: HashMap<_, _> = base
        .activity
        .iter()
        .map(|event| (&event.id, event))
        .collect();
    let remote_ids: std::collections::HashSet<_> =
        remote.activity.iter().map(|event| &event.id).collect();
    for change in remote.activity.iter().chain(&local.activity) {
        if let Some(old) = base_activity.get(&change.id) {
            ensure!(
                *old == change,
                "Ein bekanntes Änderungsereignis wurde nachträglich verändert"
            );
            // Do not resurrect acknowledged events pruned from the bounded server history.
            if !remote_ids.contains(&change.id) {
                continue;
            }
        }
        if let Some(old) = activity_ids.get(&change.id) {
            ensure!(
                *old == change,
                "Ein Änderungsereignis wurde nachträglich verändert"
            );
        } else {
            activity_ids.insert(change.id.clone(), change);
            activity.push(change.clone());
        }
    }
    if crate::model::validate_nodes(&nodes).is_err() {
        conflicts.push(Conflict {
            object: "tree".into(),
            id: local.team_id.clone(),
            name: "Ordnerstruktur".into(),
            fields: vec!["parent".into()],
        });
    }
    if !conflicts.is_empty() {
        return Ok(Merged::Conflicts(conflicts));
    }
    let mut snapshot = TeamSnapshot {
        version: base.version.max(local.version).max(remote.version).max(3),
        commands,
        team_id: local.team_id.clone(),
        nodes,
        credentials,
        activity,
    };
    sync::canonicalize(&mut snapshot);
    let excess = snapshot.activity.len().saturating_sub(MAX_CHANGES);
    snapshot.activity.drain(..excess);
    Ok(Merged::Ready(snapshot))
}
