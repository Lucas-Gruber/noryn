//! Personal editor preferences, outside the encrypted/shared vault.
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Write, path::Path};
use tauri::Manager;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rule {
    pub extensions: Vec<String>,
    pub program: String,
    pub arguments: Vec<String>,
    pub host: bool,
    pub wait: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    rules: Vec<Rule>,
    flatpak: bool,
}
fn validate(rules: &[Rule]) -> anyhow::Result<()> {
    ensure!(rules.len() <= 32, "Höchstens 32 Programmregeln erlaubt");
    let mut seen = HashSet::new();
    for rule in rules {
        ensure!(
            !rule.extensions.is_empty() && rule.extensions.len() <= 32,
            "Dateiendungen angeben"
        );
        for ext in &rule.extensions {
            ensure!(
                !ext.is_empty()
                    && ext.len() <= 32
                    && ext.bytes().all(|b| b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || b"_+-".contains(&b)),
                "Dateiendungen klein und ohne Punkt angeben"
            );
            ensure!(seen.insert(ext), "Dateiendung ist mehrfach zugeordnet");
        }
        ensure!(
            Path::new(&rule.program).is_absolute()
                && rule.program.len() <= 4096
                && !rule.program.contains(['\0', '\n', '\r']),
            "Absoluten Programmpfad angeben"
        );
        ensure!(
            rule.arguments.len() <= 32
                && rule.arguments.iter().all(|a| a.len() <= 4096
                    && !a.contains(['\0', '\n', '\r'])
                    && (!a.contains(['{', '}']) || a == "{file}")),
            "Ungültige Programmargumente; {{file}} nur als eigenes Argument verwenden"
        );
        ensure!(
            !rule.host || cfg!(target_os = "linux"),
            "Host-Programme sind nur unter Linux verfügbar"
        );
    }
    Ok(())
}
fn read(path: &Path) -> anyhow::Result<Vec<Rule>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    ensure!(
        std::fs::metadata(path)?.len() <= 65536,
        "Programmeinstellungen sind zu groß"
    );
    let rules: Vec<Rule> = serde_json::from_slice(&std::fs::read(path)?)?;
    validate(&rules)?;
    Ok(rules)
}
fn save(path: &Path, rules: &[Rule]) -> anyhow::Result<()> {
    validate(rules)?;
    let parent = path.parent().context("Einstellungspfad fehlt")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    let bytes = serde_json::to_vec(rules)?;
    ensure!(bytes.len() <= 65536, "Programmeinstellungen sind zu groß");
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
pub fn load(app: &tauri::AppHandle) -> crate::Result<Vec<Rule>> {
    read(
        &app.path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join("editor-apps.json"),
    )
    .map_err(|e| format!("Programmeinstellungen: {e:#}"))
}
#[tauri::command]
pub fn file_apps_get(app: tauri::AppHandle) -> crate::Result<Settings> {
    Ok(Settings {
        rules: load(&app)?,
        flatpak: cfg!(target_os = "linux") && Path::new("/.flatpak-info").exists(),
    })
}
#[tauri::command]
pub fn file_apps_save(app: tauri::AppHandle, rules: Vec<Rule>) -> crate::Result<()> {
    save(
        &app.path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join("editor-apps.json"),
        &rules,
    )
    .map_err(|e| format!("{e:#}"))
}
pub fn matching<'a>(rules: &'a [Rule], original: &str) -> Option<&'a Rule> {
    // Use the original extension even when the non-executable working copy is .txt.
    let name = original.rsplit(['/', '\\']).next()?;
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    rules.iter().find(|r| r.extensions.contains(&ext))
}
pub fn launch(
    path: &Path,
    original: &str,
    rules: &[Rule],
    control: &noryn_core::transfer::Control,
) -> anyhow::Result<crate::external_editor::Launch> {
    control.check()?;
    let Some(rule) = matching(rules, original) else {
        return crate::external_editor::launch(path, control);
    };
    validate(rules)?;
    let host = cfg!(target_os = "linux") && Path::new("/.flatpak-info").exists() && rule.host;
    let mut command = if host {
        let mut c = std::process::Command::new("flatpak-spawn");
        c.arg("--host").arg(&rule.program);
        c
    } else if cfg!(target_os = "macos") && rule.program.ends_with(".app") {
        let mut c = std::process::Command::new("/usr/bin/open");
        if rule.wait {
            c.args(["-W", "-n"]);
        }
        c.arg("-a").arg(&rule.program).arg(path);
        if !rule.arguments.is_empty() {
            c.arg("--args");
        }
        c
    } else {
        std::process::Command::new(&rule.program)
    };
    for arg in &rule.arguments {
        if arg == "{file}" {
            command.arg(path);
        } else {
            command.arg(arg);
        }
    }
    if !rule.arguments.iter().any(|a| a == "{file}")
        && !(cfg!(target_os = "macos") && rule.program.ends_with(".app"))
    {
        command.arg(path);
    }
    if rule.wait {
        return crate::external_editor::spawn_waiter(&mut command);
    }
    // A launcher exit is never treated as a close notification for a GUI document.
    crate::external_editor::spawn_detached(&mut command)?;
    Ok(crate::external_editor::Launch::manual())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn configured_program_receives_the_working_path_as_one_literal_argument() {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("test 'quoted';$(no-command).txt");
        let result = directory.path().join("received.txt");
        std::fs::write(&document, b"text").unwrap();
        let rule = Rule {
            extensions: vec!["sh".into()],
            program: "/bin/sh".into(),
            arguments: vec![
                "-c".into(),
                "printf '%s' \"$1\" > \"$2\"".into(),
                "fixture".into(),
                "{file}".into(),
                result.to_str().unwrap().into(),
            ],
            host: false,
            wait: true,
        };
        let launch = launch(&document, "remote-script.sh", &[rule], &Default::default()).unwrap();
        launch.completion.unwrap().await.unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(&result).unwrap(),
            document.to_str().unwrap()
        );
    }
    #[test]
    fn personal_rules_round_trip_match_original_extensions_and_reject_ambiguous_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("editors.json");
        let rule = Rule {
            extensions: vec!["sh".into(), "bashrc".into()],
            program: if cfg!(windows) {
                "C:\\Editor\\editor.exe"
            } else {
                "/usr/bin/editor"
            }
            .into(),
            arguments: vec!["--wait".into(), "{file}".into()],
            host: false,
            wait: true,
        };
        save(&path, std::slice::from_ref(&rule)).unwrap();
        let read = read(&path).unwrap();
        assert!(matching(&read, "/remote/test.SH").is_some());
        assert!(matching(&read, "/remote/.bashrc").is_some());
        assert!(matching(&read, "protected.bash.txt").is_none());
        assert!(validate(&[rule.clone(), rule.clone()]).is_err());
        let mut bad = rule;
        bad.arguments = vec!["--file={file}".into()];
        assert!(validate(&[bad.clone()]).is_err());
        bad.arguments = vec![];
        bad.program = "relative-editor".into();
        assert!(validate(&[bad]).is_err());
    }
}
