use crate::ssh::RemoteEntry;
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    pub path: String,
    pub parent: Option<String>,
    pub separator: String,
    pub entries: Vec<RemoteEntry>,
}

#[derive(Serialize)]
pub struct Location {
    pub name: String,
    pub path: String,
}

fn display(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(share) = value.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{share}");
        }
        if let Some(path) = value.strip_prefix(r"\\?\") {
            return path.into();
        }
    }
    value.into()
}

pub fn locations(home: &Path) -> Vec<Location> {
    let mut locations = vec![Location {
        name: "Persönlicher Ordner".into(),
        path: display(home),
    }];
    for (folder, name) in [
        ("Desktop", "Desktop"),
        ("Documents", "Dokumente"),
        ("Downloads", "Downloads"),
        ("Dokumente", "Dokumente"),
    ] {
        let path = home.join(folder);
        if path.is_dir() {
            locations.push(Location {
                name: name.into(),
                path: display(&path),
            });
        }
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let path = format!("{}:\\", letter as char);
        if Path::new(&path).is_dir() {
            locations.push(Location {
                name: format!("Laufwerk {}:", letter as char),
                path,
            });
        }
    }
    #[cfg(not(windows))]
    locations.push(Location {
        name: "Dateisystem".into(),
        path: "/".into(),
    });
    locations
}

pub fn list(path: &Path) -> Result<Directory> {
    let path = fs::canonicalize(path).context("Lokaler Ordner ist nicht erreichbar")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        let link = entry.file_type()?.is_symlink();
        let metadata = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let is_dir = if link {
            entry.path().is_dir()
        } else {
            metadata.is_dir()
        };
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            format!("{:03o}", metadata.permissions().mode() & 0o7777)
        };
        #[cfg(not(unix))]
        let permissions = if metadata.permissions().readonly() {
            "ro".into()
        } else {
            "rw".into()
        };
        entries.push(RemoteEntry {
            name: entry.file_name().to_string_lossy().into(),
            path: display(&entry.path()),
            is_dir,
            is_link: link,
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|t| t.as_secs())
                .unwrap_or(0),
            permissions,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Directory {
        parent: path.parent().map(display),
        path: display(&path),
        separator: std::path::MAIN_SEPARATOR.to_string(),
        entries,
    })
}

pub fn mkdir(parent: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains(['/', '\\', '\0']) && name != "." && name != "..",
        "Ungültiger Ordnername"
    );
    let parent = fs::canonicalize(parent)?;
    let path = parent.join(name);
    fs::create_dir(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_navigation_and_new_folders_stay_in_the_chosen_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a file.txt"), b"hello").unwrap();
        mkdir(dir.path(), "folder").unwrap();
        assert!(mkdir(dir.path(), "../escape").is_err());
        let view = list(dir.path()).unwrap();
        let canonical = fs::canonicalize(dir.path()).unwrap();
        assert!(view.entries[0].is_dir);
        assert_eq!(view.entries[1].size, 5);
        assert_eq!(view.path, display(&canonical));
        assert_eq!(view.parent, canonical.parent().map(display));
    }

    #[cfg(unix)]
    #[test]
    fn local_navigation_resolves_symlinked_parent_paths() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let created = mkdir(&alias, "child").unwrap();
        let canonical = fs::canonicalize(&real).unwrap();
        assert_eq!(created, canonical.join("child"));
        let view = list(&alias.join("child")).unwrap();
        assert_eq!(view.path, display(&canonical.join("child")));
        assert_eq!(view.parent, Some(display(&canonical)));
    }
}
