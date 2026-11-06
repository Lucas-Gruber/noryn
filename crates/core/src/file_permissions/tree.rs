//! Bounded, reviewed permission changes. Directories are changed after children
//! so removing directory search/write access cannot strand unprocessed entries.
use super::{Change, Snapshot, STALE};
use crate::{ssh::RemoteEntry, transfer::Control};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path};

const MAX_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 64;
const MAX_PATH_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub path: String,
    pub local: bool,
    pub change: Change,
    pub root_fingerprint: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub fingerprint: String,
    pub files: usize,
    pub directories: usize,
    pub skipped_links: usize,
    pub skipped_special: usize,
}
pub trait Backend {
    fn read(&mut self, path: &str) -> Result<Snapshot>;
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>>;
    fn apply(
        &mut self,
        path: &str,
        expected: &str,
        change: &Change,
        control: &Control,
    ) -> Result<()>;
}
pub struct Local;
impl Backend for Local {
    fn read(&mut self, path: &str) -> Result<Snapshot> {
        super::local_read(path)
    }
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
        use crate::file_actions::Files;
        std::fs::read_dir(path)?
            .take(MAX_ENTRIES + 1)
            .map(|entry| {
                let path = entry?.path();
                crate::file_actions::Local.stat(path.to_str().context("Ungültiger Dateipfad")?)
            })
            .collect()
    }
    fn apply(
        &mut self,
        path: &str,
        expected: &str,
        change: &Change,
        control: &Control,
    ) -> Result<()> {
        super::local_apply(path, expected, change, control).map(|_| ())
    }
}
impl Backend for crate::remote::Remote {
    fn read(&mut self, path: &str) -> Result<Snapshot> {
        self.permissions_read(path)
    }
    fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
        self.list(path)
    }
    fn apply(
        &mut self,
        path: &str,
        expected: &str,
        change: &Change,
        control: &Control,
    ) -> Result<()> {
        self.permissions_apply(path, expected, change, control)
            .map(|_| ())
    }
}
#[derive(Serialize)]
struct Item {
    metadata: Snapshot,
    parent: Option<usize>,
    depth: usize,
}
struct Plan {
    items: Vec<Item>,
    preview: Preview,
}

fn plan(
    backend: &mut (impl Backend + ?Sized),
    request: &Request,
    control: &Control,
) -> Result<Plan> {
    control.check()?;
    request.change.validate()?;
    super::validate_path(&request.path)?;
    if request.local {
        let path = Path::new(&request.path);
        ensure!(
            path.is_absolute()
                && !path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "Absoluten Pfad ohne Punktsegmente verwenden"
        );
    } else {
        ensure!(
            request.path.starts_with('/')
                && request
                    .path
                    .split('/')
                    .skip(1)
                    .all(|p| !p.is_empty() && p != "." && p != ".."),
            "Absoluten Remote-Pfad ohne Punktsegmente verwenden; die Wurzel ist kein Aktionsziel"
        );
    }
    let root = backend.read(&request.path)?;
    ensure!(
        root.is_dir,
        "Rekursive Berechtigungen sind nur für Ordner verfügbar"
    );
    root.check(&request.root_fingerprint, &request.change)?;
    let mut items = vec![Item {
        metadata: root,
        parent: None,
        depth: 0,
    }];
    let mut skipped = Vec::new();
    let mut path_bytes = request.path.len();
    let mut index = 0;
    while index < items.len() {
        control.check()?;
        let current = index;
        index += 1;
        if !items[current].metadata.is_dir {
            continue;
        }
        verify_ancestors(backend, &items, Some(current), control)?;
        let entries = backend.list(&items[current].metadata.path)?;
        ensure!(
            items.len() + skipped.len() + entries.len() <= MAX_ENTRIES,
            "Rekursive Berechtigungen: höchstens 10000 Einträge erlaubt"
        );
        let mut names = HashSet::new();
        for entry in entries {
            control.check()?;
            ensure!(
                !entry.name.is_empty()
                    && ![".", ".."].contains(&entry.name.as_str())
                    && !entry.name.contains(['/', '\0'])
                    && (!request.local || !cfg!(windows) || !entry.name.contains('\\')),
                "Ungültiger Dateiname im Berechtigungsbaum"
            );
            ensure!(
                names.insert(entry.name.clone()),
                "Doppelter Name im Dateibaum"
            );
            let path = if request.local {
                Path::new(&items[current].metadata.path)
                    .join(&entry.name)
                    .to_str()
                    .context("Ungültiger Dateipfad")?
                    .to_owned()
            } else {
                format!("{}/{}", items[current].metadata.path, entry.name)
            };
            path_bytes = path_bytes
                .checked_add(path.len())
                .context("Dateibaum ist zu groß")?;
            ensure!(path_bytes <= MAX_PATH_BYTES, "Dateibaum ist zu groß");
            let depth = items[current].depth + 1;
            ensure!(
                depth <= MAX_DEPTH,
                "Dateibaum ist zu tief (höchstens 64 Ebenen)"
            );
            if entry.is_link || (!entry.is_file && !entry.is_dir) {
                skipped.push((path, entry.is_link));
                continue;
            }
            let metadata = backend.read(&path)?;
            ensure!(
                metadata.path == path && metadata.is_dir == entry.is_dir,
                STALE
            );
            metadata.check(&metadata.fingerprint, &request.change)?;
            items.push(Item {
                metadata,
                parent: Some(current),
                depth,
            });
        }
    }
    // Canonical order makes listing order irrelevant; bind changes and scope too.
    let mut identities: Vec<_> = items
        .iter()
        .map(|i| (&i.metadata.path, &i.metadata.fingerprint))
        .collect();
    identities.sort();
    skipped.sort();
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(request, identities, &skipped))?)
    );
    let preview = Preview {
        fingerprint,
        files: items.iter().filter(|i| !i.metadata.is_dir).count(),
        directories: items.iter().filter(|i| i.metadata.is_dir).count(),
        skipped_links: skipped.iter().filter(|(_, link)| *link).count(),
        skipped_special: skipped.iter().filter(|(_, link)| !*link).count(),
    };
    Ok(Plan { items, preview })
}
fn verify_ancestors(
    backend: &mut (impl Backend + ?Sized),
    items: &[Item],
    mut parent: Option<usize>,
    control: &Control,
) -> Result<()> {
    while let Some(index) = parent {
        control.check()?;
        let old = &items[index];
        let actual = backend.read(&old.metadata.path)?;
        ensure!(
            actual.is_dir && actual.fingerprint == old.metadata.fingerprint,
            STALE
        );
        parent = old.parent;
    }
    Ok(())
}
pub fn preview(
    backend: &mut (impl Backend + ?Sized),
    request: &Request,
    control: &Control,
) -> Result<Preview> {
    Ok(plan(backend, request, control)?.preview)
}
pub fn execute(
    backend: &mut (impl Backend + ?Sized),
    request: &Request,
    expected: &str,
    control: &Control,
) -> Result<Preview> {
    let plan = plan(backend, request, control)?;
    ensure!(
        plan.preview.fingerprint == expected,
        "Dateibaum wurde seit der Vorschau geändert; bitte erneut prüfen"
    );
    // Parents come before children in the plan, so reverse order keeps all
    // unmodified ancestors accessible and permits validating their fingerprints.
    for item in plan.items.iter().rev() {
        control.check()?;
        verify_ancestors(backend, &plan.items, item.parent, control)?;
        backend
            .apply(
                &item.metadata.path,
                &item.metadata.fingerprint,
                &request.change,
                control,
            )
            .with_context(|| {
                format!(
                    "Berechtigungen konnten nicht geändert werden: {}",
                    item.metadata.path
                )
            })?;
    }
    Ok(plan.preview)
}

#[cfg(test)]
mod tests;
