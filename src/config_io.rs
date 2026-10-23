//! Validated, atomic configuration updates.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::config::{self, Config};

pub fn update(edit: impl FnOnce(&mut Config)) -> Result<Config, String> {
    let path =
        config::path().ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())?;
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    reject_symlink(&path)?;

    let lock_path = path.with_extension("yaml.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|error| format!("{}: {error}", lock_path.display()))?;
    // SAFETY: flock only acts on this open file descriptor.
    if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } != 0 {
        return Err(format!("could not lock {}", lock_path.display()));
    }
    let _unlock = Unlock(&lock);

    let mut config = config::load()?;
    edit(&mut config);
    write_locked(&path, &config)?;
    Ok(config)
}

pub fn replace(config: &Config) -> Result<(), String> {
    let path =
        config::path().ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())?;
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    reject_symlink(&path)?;

    let lock_path = path.with_extension("yaml.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|error| format!("{}: {error}", lock_path.display()))?;
    // SAFETY: flock only acts on this open file descriptor.
    if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } != 0 {
        return Err(format!("could not lock {}", lock_path.display()));
    }
    let _unlock = Unlock(&lock);

    write_locked(&path, config)
}

fn write_locked(path: &Path, config: &Config) -> Result<(), String> {
    let text = serde_saphyr::to_string(config).map_err(|error| error.to_string())?;
    config::parse(&text).map_err(|error| format!("refusing invalid configuration: {error}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_owned())?;
    let temp = parent.join(format!(".config.yaml.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| format!("{}: {error}", temp.display()))?;
    if let Err(error) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = fs::remove_file(&temp);
        return Err(format!("{}: {error}", temp.display()));
    }
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("{}: {error}", path.display()));
    }
    if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "{} is a symlink; edit the YAML source directly to preserve your configuration manager's ownership",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

struct Unlock<'a>(&'a std::fs::File);

impl Drop for Unlock<'_> {
    fn drop(&mut self) {
        // SAFETY: the borrowed descriptor is still open until this guard drops.
        let _ = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(self.0), libc::LOCK_UN) };
    }
}
