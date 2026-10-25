//! Bounded text working copies and optimistic conflict checks, never executable previews.
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;

pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub type DigestBytes = [u8; 32];
pub const CONFLICT: &str = "Die Originaldatei wurde inzwischen geändert. Nicht überschrieben; Arbeitskopie behalten, Original herunterladen und im Editor vergleichen.";
#[derive(Default)]
pub(crate) struct Buffer(pub Zeroizing<Vec<u8>>);
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + bytes.len() > MAX_BYTES {
            return Err(std::io::Error::other(
                "Textdateien dürfen höchstens 8 MiB groß sein",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn digest(bytes: &[u8]) -> DigestBytes {
    Sha256::digest(bytes).into()
}
pub fn digest_hex(value: &DigestBytes) -> String {
    value.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn validate(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_BYTES,
        "Textdateien dürfen höchstens 8 MiB groß sein"
    );
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        ensure!(bytes.len().is_multiple_of(2), "Ungültiger UTF-16-Text");
        let le = bytes[0] == 0xff;
        let units = bytes[2..].as_chunks::<2>().0.iter().map(|b| {
            if le {
                u16::from_le_bytes([b[0], b[1]])
            } else {
                u16::from_be_bytes([b[0], b[1]])
            }
        });
        for character in char::decode_utf16(units) {
            ensure!(
                is_text(character.context("Ungültiger UTF-16-Text")?),
                "Keine Textdatei (Binärdaten erkannt)"
            );
        }
    } else {
        let text = std::str::from_utf8(bytes).context("Textdatei muss UTF-8 oder UTF-16 mit BOM verwenden; andere Dateien bitte herunterladen")?;
        ensure!(
            text.chars().all(is_text),
            "Keine Textdatei (Binärdaten erkannt)"
        );
    }
    Ok(())
}
fn is_text(c: char) -> bool {
    !c.is_control() || ['\t', '\r', '\n', '\u{c}'].contains(&c)
}
pub fn read(reader: &mut impl Read) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    reader
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate(&bytes)?;
    Ok(bytes)
}
pub fn read_local(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let mut file = crate::transfer::open_regular(path)?;
    ensure!(
        file.metadata()?.len() <= MAX_BYTES as u64,
        "Textdateien dürfen höchstens 8 MiB groß sein"
    );
    read(&mut file)
}
pub fn replace_local(path: &Path, expected: &DigestBytes, bytes: &[u8]) -> Result<()> {
    validate(bytes)?;
    let original = crate::transfer::open_regular(path)?;
    let metadata = original.metadata()?;
    let parent = path.parent().context("Ungültiger lokaler Pfad")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let created = temporary.as_file().metadata()?;
        if metadata.uid() != created.uid() || metadata.gid() != created.gid() {
            std::os::unix::fs::chown(temporary.path(), Some(metadata.uid()), Some(metadata.gid()))
                .context(
                    "Besitzer/Gruppe konnten nicht erhalten werden; Original nicht verändert",
                )?;
        }
    }
    temporary
        .as_file()
        .set_permissions(metadata.permissions())?;
    temporary.as_file().sync_all()?;
    ensure!(&digest(&read_local(path)?) == expected, CONFLICT);
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}
// A file association must not turn a downloaded script/launcher into an execution.
// Keep the real basename/extension for ordinary documents; dangerous associations use .txt.
pub fn working_name(name: &str) -> Result<String> {
    crate::transfer::local_component(name)?;
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\', '\0', '\r', '\n', ':'])
            && name.len() <= 200,
        "Dateiname kann nicht als lokale Arbeitskopie geöffnet werden"
    );
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let passive = [
        "txt",
        "text",
        "md",
        "markdown",
        "rst",
        "adoc",
        "log",
        "conf",
        "cfg",
        "config",
        "ini",
        "env",
        "yaml",
        "yml",
        "toml",
        "json",
        "jsonc",
        "xml",
        "csv",
        "tsv",
        "sql",
        "properties",
        "gitignore",
        "gitattributes",
        "editorconfig",
        "lock",
        "service",
        "socket",
        "timer",
        "target",
        "mount",
        "automount",
        "nix",
        "c",
        "h",
        "cpp",
        "hpp",
        "rs",
        "go",
        "ts",
        "tsx",
        "jsx",
        "vue",
        "svelte",
        "css",
        "scss",
        "less",
        "java",
        "kt",
        "swift",
    ]
    .contains(&extension.as_str());
    Ok(if passive {
        name.into()
    } else {
        format!("{name}.txt")
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_limits_encodings_and_conflicts_are_enforced() {
        assert!(validate(b"key: value\n").is_ok());
        assert!(validate(&[0xff, 0xfe, b'a', 0, 10, 0]).is_ok());
        assert!(validate(&[0xfe, 0xff, 0, b'a']).is_ok());
        for invalid in [
            vec![0, 1, 2],
            vec![0xff, 0xfe, 0],
            vec![b'x'; MAX_BYTES + 1],
        ] {
            assert!(validate(&invalid).is_err());
        }
        assert_eq!(working_name("deploy.yml").unwrap(), "deploy.yml");
        assert_eq!(
            working_name("launch.desktop").unwrap(),
            "launch.desktop.txt"
        );
        assert_eq!(working_name("script.Js").unwrap(), "script.Js.txt");
        assert!(working_name("../escape").is_err());
        assert_eq!(
            working_name("unknown.settingcontent-ms").unwrap(),
            "unknown.settingcontent-ms.txt"
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.txt");
        std::fs::write(&file, b"initial").unwrap();
        let expected = digest(&read_local(&file).unwrap());
        replace_local(&file, &expected, b"edited").unwrap();
        assert!(replace_local(&file, &expected, b"lost update").is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"edited");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
