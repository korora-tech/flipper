mod app_path;
mod browsers;
mod config;
mod config_io;
mod launch;
mod native_host;
mod registration;
mod routing;
mod settings;
mod ui;
mod urls;

use std::io::Read;

use config::Config;
use urls::PreparedUrl;

const USAGE: &str = "Flipper — native Linux browser and profile picker

Usage:
  flipper URL                   Choose a browser/profile for a link
  flipper [--settings]          Open Flipper settings
  flipper --register            Make Flipper the default web link handler
  flipper --uninstall           Restore prior web handlers and unregister
  flipper --suspend | --resume  Temporarily bypass or resume routing rules
  flipper --export [PATH|-]     Export validated YAML (stdout by default)
  flipper --import PATH         Validate and import YAML configuration
  flipper --native-host         Run the browser extension native host
  flipper --install-native-host chrome|firefox EXTENSION_ID
  flipper --help                Show this help

Configuration: $XDG_CONFIG_HOME/flipper/config.yaml (or ~/.config/flipper/config.yaml)";

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
            let config = config_io::update(|config| config.routing.suspended = suspended)?;
            println!(
                "Routing rules {}.",
                if config.routing.suspended {
                    "suspended"
                } else {
                    "resumed"
                }
            );
            return Ok(());
        }
        [flag] if flag == "--settings" => return show(None),
        [flag] if flag == "--export" => return export("-"),
        [flag, path] if flag == "--export" => return export(path),
        [flag, path] if flag == "--import" => return import(path),
        [url] if url.starts_with('-') => return Err(format!("unknown option {url:?}\n\n{USAGE}")),
        [url] => return show(Some(url)),
        [] => return show(None),
        _ => return Err(USAGE.into()),
    }
}

fn load_config() -> (Config, Option<String>) {
    match config::load() {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error)),
    }
}

fn show(input: Option<&str>) -> Result<(), String> {
    let (config, mut warning) = load_config();
    let profiles = browsers::discover(&config);
    let prepared = if let Some(input) = input {
        Some(PreparedUrl::parse(input, config.routing.unwrap_redirects)?)
    } else {
        None
    };

    if let Some(url) = prepared.as_ref() {
        match routing::decide(&config, &profiles, url) {
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

    ui::open(config, profiles, prepared, warning)?;
    Ok(())
}

fn export(path: &str) -> Result<(), String> {
    let (config, warning) = load_config();
    if let Some(warning) = warning {
        return Err(format!("cannot export invalid configuration: {warning}"));
    }
    let yaml = serde_saphyr::to_string(&config).map_err(|error| error.to_string())?;
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

fn import(path: &str) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path))?;
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("{}: {error}", path))?;
    if text.len() > 1024 * 1024 {
        return Err("configuration file is larger than 1 MiB".into());
    }
    let config = config::parse(&text).map_err(|error| format!("{}: {error}", path))?;
    config_io::replace(&config)?;
    println!("Imported Flipper configuration from {path}.");
    Ok(())
}
