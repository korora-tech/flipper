mod app_path;
mod browsers;
mod config;
mod config_io;
mod launch;
mod native_host;
mod nix;
mod registration;
mod routing;
mod settings;
mod ui;
mod urls;

use config::{Config, Layers};
use urls::PreparedUrl;

const USAGE: &str = "Flipper — native Linux browser and profile picker

Usage:
  flipper URL                   Choose a browser/profile for a link
  flipper [--settings]          Open Flipper settings
  flipper --register            Make Flipper the default web link handler
  flipper --uninstall           Restore prior web handlers and unregister
  flipper --suspend | --resume  Temporarily bypass or resume routing rules
  flipper --export [PATH|-]     Export the effective YAML (stdout by default)
  flipper --import PATH         Validate and import YAML configuration
  flipper --check-config PATH   Validate a configuration file and exit
  flipper --export-overrides [--format yaml|nix]
                                Print local overrides (yaml by default)
  flipper --native-host         Run the browser extension native host
  flipper --install-native-host chrome|firefox EXTENSION_ID
  flipper --help                Show this help

Configuration: $XDG_CONFIG_HOME/flipper/config.yaml (or ~/.config/flipper/config.yaml).
When config.yaml is a symlink (Home Manager), local changes go to overrides.yaml.";

fn main() {
    if let Err(error) = run() {
        eprintln!("flipper: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let args = args.strip_prefix(&["--".to_owned()]).unwrap_or(&args);
    match args {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("flipper {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        [flag] if flag == "--register" => return registration::register(),
        [flag] if flag == "--uninstall" => return registration::uninstall(),
        [flag] if flag == "--native-host" => return native_host::run(),
        [flag, browser, id] if flag == "--install-native-host" => {
            return native_host::install(browser, id);
        }
        [flag] if flag == "--suspend" || flag == "--resume" => {
            let suspended = flag == "--suspend";
            let layers =
                config_io::edit(|editor| editor.set(config_io::Setting::Suspended(suspended)))?;
            println!(
                "Routing rules {}{}.",
                if layers.effective.routing.suspended {
                    "suspended"
                } else {
                    "resumed"
                },
                if layers.managed {
                    " (saved as a local override)"
                } else {
                    ""
                }
            );
            return Ok(());
        }
        [flag] if flag == "--settings" => return show(None),
        [flag] if flag == "--export" => return export("-"),
        [flag, path] if flag == "--export" => return export(path),
        [flag, path] if flag == "--import" => return import(path),
        [flag, path] if flag == "--check-config" => check_config(path),
        [flag] if flag == "--export-overrides" => export_overrides("yaml"),
        [flag, option, format] if flag == "--export-overrides" && option == "--format" => {
            export_overrides(format)
        }
        [url] if url.starts_with('-') => return Err(format!("unknown option {url:?}\n\n{USAGE}")),
        [url] => return show(Some(url)),
        [] => return show(None),
        _ => return Err(USAGE.into()),
    }
}

fn load_config() -> (Layers, Option<String>) {
    match config::load_layers() {
        Ok(layers) => {
            let warning = layers.overrides_error.clone();
            (layers, warning)
        }
        Err(error) => {
            let mut layers = Layers::unmanaged(Config::default());
            layers.managed = config::is_managed();
            (layers, Some(error))
        }
    }
}

fn show(input: Option<&str>) -> Result<(), String> {
    let (layers, mut warning) = load_config();
    let config = &layers.effective;
    let profiles = browsers::discover(config);
    let prepared = if let Some(input) = input {
        Some(PreparedUrl::parse(input, config.routing.unwrap_redirects)?)
    } else {
        None
    };

    if let Some(url) = prepared.as_ref() {
        match routing::decide(config, &profiles, url) {
            routing::Decision::Open { profile_id, .. } => {
                match profiles.iter().find(|profile| profile.id == profile_id) {
                    Some(profile) => match launch::launch(profile, url.effective.as_str()) {
                        Ok(()) => return Ok(()),
                        Err(error) => {
                            warning = Some(format!(
                                "Could not launch {}: {error}",
                                profile.browser_name
                            ))
                        }
                    },
                    None => warning = Some("The configured profile is no longer available.".into()),
                }
            }
            routing::Decision::Pick { reason, .. } if reason != "choose a browser" => {
                warning = Some(reason);
            }
            routing::Decision::Pick { .. } => {}
        }
    }

    ui::open(layers, profiles, prepared, warning)?;
    Ok(())
}

fn export(path: &str) -> Result<(), String> {
    let (layers, warning) = load_config();
    if let Some(warning) = warning {
        return Err(format!("cannot export invalid configuration: {warning}"));
    }
    let yaml = serde_saphyr::to_string(&layers.effective).map_err(|error| error.to_string())?;
    if path == "-" {
        print!("{yaml}");
        Ok(())
    } else {
        write_atomic(path, yaml.as_bytes())
    }
}

fn write_atomic(path: &str, contents: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    let path = Path::new(path);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file path", path.display()))?
        .to_string_lossy();
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|error| format!("{}: {error}", temp.display()))?;
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{}: {error}", temp.display()));
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{}: {error}", path.display()));
    }
    Ok(())
}

/// Read a configuration file named on the command line.
fn read_config_file(path: &str) -> Result<Config, String> {
    let text = config::read_bounded(std::path::Path::new(path))?
        .ok_or_else(|| format!("{path}: no such file"))?;
    config::parse(&text).map_err(|error| format!("{path}: {error}"))
}

fn import(path: &str) -> Result<(), String> {
    if config::is_managed() {
        return Err(MANAGED_IMPORT.into());
    }
    let config = read_config_file(path)?;
    config_io::replace(&config)?;
    println!("Imported Flipper configuration from {path}.");
    Ok(())
}

const MANAGED_IMPORT: &str = "config.yaml is managed by Home Manager (it is a symlink); set programs.flipper.settings instead of importing, or keep local changes in overrides.yaml";

/// Validate shape only: profile selectors are not resolved, because the
/// browsers they name may not exist where the configuration is built.
fn check_config(path: &str) -> Result<(), String> {
    read_config_file(path)?;
    println!("{path}: configuration is valid.");
    Ok(())
}

fn export_overrides(format: &str) -> Result<(), String> {
    let layers = config::load_layers()?;
    if let Some(error) = layers.overrides_error {
        return Err(format!("cannot export invalid overrides: {error}"));
    }
    match format {
        "yaml" => {
            let yaml =
                serde_saphyr::to_string(&layers.overrides).map_err(|error| error.to_string())?;
            print!("{yaml}");
        }
        "nix" => print!("{}", nix::overrides(&layers.overrides)),
        _ => return Err(format!("unknown format {format:?}; expected yaml or nix")),
    }
    Ok(())
}
