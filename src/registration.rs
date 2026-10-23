//! User-level desktop registration with conservative restore behavior.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app_path;

const DESKTOP_ID: &str = "com.korora.Flipper.desktop";
const MIME_TYPES: [&str; 3] = [
    "x-scheme-handler/http",
    "x-scheme-handler/https",
    "text/html",
];

pub fn register() -> Result<(), String> {
    let backup_path = backup_path()?;
    let previous = match read_backup(&backup_path)? {
        Some(previous) => previous,
        None => {
            let mut previous = BTreeMap::new();
            for mime in MIME_TYPES {
                let current = query_default(mime)?;
                if current == DESKTOP_ID {
                    return Err(format!(
                        "Flipper is already the default for {mime}, but its previous-handler backup is missing; refusing to replace the backup with Flipper itself"
                    ));
                }
                if current.is_empty() {
                    return Err(format!(
                        "cannot safely register Flipper: no previous default is set for {mime}; set a browser as its default first"
                    ));
                }
                previous.insert(mime.to_owned(), current);
            }
            write_backup(&backup_path, &previous)?;
            previous
        }
    };
    for mime in MIME_TYPES {
        if previous
            .get(mime)
            .is_none_or(|handler| handler.is_empty() || handler == DESKTOP_ID)
        {
            return Err(format!(
                "cannot safely register Flipper: the saved previous handler for {mime} is missing, empty, or points to Flipper itself"
            ));
        }
    }

    let data_home = data_home()?;
    let applications = data_home.join("applications");
    std::fs::create_dir_all(&applications)
        .map_err(|error| format!("could not create {}: {error}", applications.display()))?;
    let desktop_path = applications.join(DESKTOP_ID);
    let exec = app_path::desktop_exec_arg(&app_path::invoked()?)?;
    let template = include_str!("../data/com.korora.Flipper.desktop");
    if !template.contains("Exec=flipper -- %u") {
        return Err("desktop entry template has no Flipper Exec line".into());
    }
    let desktop = template.replace("Exec=flipper -- %u", &format!("Exec={exec} -- %u"));
    write_atomic(&desktop_path, desktop.as_bytes(), 0o644)
        .map_err(|error| format!("could not install {}: {error}", desktop_path.display()))?;

    let mut changed = Vec::new();
    for mime in MIME_TYPES {
        if let Err(error) = set_default(mime, DESKTOP_ID) {
            for changed_mime in changed.into_iter().rev() {
                if let Some(old) = previous.get(changed_mime) {
                    let _ = set_default(changed_mime, old);
                }
            }
            return Err(error);
        }
        changed.push(mime);
    }
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let backup_path = backup_path()?;
    let previous = read_backup(&backup_path)?.ok_or_else(|| {
        format!(
            "no previous handler backup at {}; nothing was removed",
            backup_path.display()
        )
    })?;
    for mime in MIME_TYPES {
        let current = query_default(mime)?;
        if current == DESKTOP_ID {
            let old = previous
                .get(mime)
                .filter(|old| !old.is_empty())
                .ok_or_else(|| format!("no restorable previous handler is saved for {mime}"))?;
            set_default(mime, old)?;
        }
    }

    let user_desktop = data_home()?.join("applications").join(DESKTOP_ID);
    if user_desktop.is_file() {
        std::fs::remove_file(&user_desktop)
            .map_err(|error| format!("could not remove {}: {error}", user_desktop.display()))?;
    }
    if backup_path.exists() {
        std::fs::remove_file(&backup_path)
            .map_err(|error| format!("could not remove {}: {error}", backup_path.display()))?;
    }
    Ok(())
}

fn data_home() -> Result<PathBuf, String> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".to_owned())
}

fn backup_path() -> Result<PathBuf, String> {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())?
        .join("flipper");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    Ok(dir.join("previous-defaults.json"))
}

fn read_backup(path: &Path) -> Result<Option<BTreeMap<String, String>>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let backup: BTreeMap<String, String> = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "invalid previous-handler backup {}: {error}",
            path.display()
        )
    })?;
    for mime in MIME_TYPES {
        let Some(previous) = backup.get(mime) else {
            return Err(format!(
                "invalid previous-handler backup {}: missing {mime}",
                path.display()
            ));
        };
        if previous.is_empty() || previous == DESKTOP_ID {
            return Err(format!(
                "invalid previous-handler backup {}: {mime} points to an empty handler or Flipper itself",
                path.display()
            ));
        }
    }
    Ok(Some(backup))
}

fn write_backup(path: &Path, backup: &BTreeMap<String, String>) -> Result<(), String> {
    let contents = serde_json::to_vec_pretty(backup).map_err(|error| error.to_string())?;
    write_atomic(path, &contents, 0o600)
}

fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let temp = path.with_extension(format!(
        "{}.{}.tmp",
        path.extension().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temp)
        .map_err(|error| format!("could not write {}: {error}", temp.display()))?;
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("could not write {}: {error}", temp.display()));
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("could not replace {}: {error}", path.display()));
    }
    Ok(())
}

fn query_default(mime: &str) -> Result<String, String> {
    let output = Command::new("xdg-mime")
        .args(["query", "default", mime])
        .output()
        .map_err(|error| format!("could not run xdg-mime: {error}"))?;
    if !output.status.success() {
        return Err(format!("xdg-mime query failed for {mime}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn set_default(mime: &str, desktop_id: &str) -> Result<(), String> {
    let status = Command::new("xdg-mime")
        .args(["default", desktop_id, mime])
        .status()
        .map_err(|error| format!("could not run xdg-mime: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("xdg-mime could not set {mime} to {desktop_id}"))
    }
}
