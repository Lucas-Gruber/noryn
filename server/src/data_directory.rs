use anyhow::{ensure, Context, Result};
use std::{fs, io::Write, path::Path};

/// Existing volumes keep their administrator-managed ownership, permissions and ACLs.
/// Request private permissions only when creating directories, without chmod on a PVC root.
pub fn prepare(directory: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory).with_context(|| {
        format!(
            "Sync-Datenverzeichnis '{}' konnte nicht angelegt werden. NORYN_SYNC_DATA und Volume-Zugriff prüfen",
            directory.display()
        )
    })?;

    // Exercise the same write/sync/rename pattern as uploads before accepting clients.
    // Both names are exclusively reserved probe files; no existing snapshot is touched.
    let temporary = || {
        tempfile::Builder::new()
            .prefix(".noryn-storage-check-")
            .tempfile_in(directory)
            .with_context(|| {
                format!(
                    "Testdatei in '{}' konnte nicht angelegt werden. Das Volume muss für den Server-Benutzer schreibbar sein (Container-Standard: UID/GID 10001)",
                    directory.display()
                )
            })
    };
    let mut source = temporary()?;
    let expected = b"Noryn storage check\n";
    source.write_all(expected).with_context(|| {
        format!(
            "Schreiben im Sync-Datenverzeichnis '{}' fehlgeschlagen",
            directory.display()
        )
    })?;
    source.as_file().sync_all().with_context(|| {
        format!(
            "Synchronisieren der Testdatei in '{}' fehlgeschlagen",
            directory.display()
        )
    })?;
    let destination = temporary()?.into_temp_path();
    let published = source.persist(&destination).with_context(|| {
        format!("Atomisches Ersetzen der Testdatei in '{}' fehlgeschlagen. Das Volume muss Dateiumbenennungen unterstützen", directory.display())
    })?;
    drop(published);
    let actual = fs::read(&destination).with_context(|| {
        format!(
            "Lesen der Testdatei in '{}' fehlgeschlagen",
            directory.display()
        )
    })?;
    ensure!(
        actual == expected,
        "Speicherprüfung in '{}' lieferte unerwartete Daten",
        directory.display()
    );
    destination.close().with_context(|| {
        format!(
            "Entfernen der Testdatei in '{}' fehlgeschlagen",
            directory.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_directory_supports_snapshot_operations_without_leaving_probe_files() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("new").join("snapshots");
        prepare(&directory).unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn existing_volume_permissions_and_snapshot_bytes_are_preserved() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path();
        let snapshot = directory.join("existing-snapshot.json");
        fs::write(&snapshot, b"existing encrypted snapshot").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o2770)).unwrap();
        }
        let before = fs::metadata(directory).unwrap().permissions();
        prepare(directory).unwrap();
        assert_eq!(fs::metadata(directory).unwrap().permissions(), before);
        assert_eq!(fs::read(&snapshot).unwrap(), b"existing encrypted snapshot");
        assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
    }

    #[test]
    fn invalid_directory_reports_the_failing_operation_and_preserves_the_file() {
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("not-a-directory");
        fs::write(&file, b"keep").unwrap();
        let error = format!("{:#}", prepare(&file).unwrap_err());
        assert!(error.contains("konnte nicht angelegt werden"));
        assert!(error.contains(&file.display().to_string()));
        assert_eq!(fs::read(&file).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn read_only_volume_reports_missing_write_access_without_changing_permissions() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let temporary = tempfile::tempdir().unwrap();
        if fs::metadata(temporary.path()).unwrap().uid() == 0 {
            return; // Root bypasses this permission check; run as an unprivileged user.
        }
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = prepare(temporary.path());
        let mode = fs::metadata(temporary.path()).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("Testdatei") && error.contains("schreibbar"));
        assert_eq!(mode, 0o500);
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 0);
    }
}
