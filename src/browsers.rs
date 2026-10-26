//! Browser discovery from local profile metadata. No cookies, history, login
//! databases, sessions, or profile preferences are read. Chromium's Local State,
//! Firefox's profiles.ini and, where the Flipper add-on is installed, a Firefox
//! profile's containers.json are the only files opened by discovery.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::Config;

#[derive(Clone, Debug)]
pub struct BrowserProfile {
    /// Stable identity: browser installation ID plus the profile directory.
    /// Renaming a profile or changing its signed-in email does not change it.
    pub id: String,
    pub browser_id: String,
    pub browser_name: String,
    pub profile_name: String,
    pub email: Option<String>,
    pub icon_name: String,
    /// Shell-free argv template; URL insertion is an entire `{url}` argument.
    pub launch: Vec<String>,
    /// Firefox container (userContextId); the URL is handed to the Flipper
    /// add-on, which opens it in that container.
    pub container: Option<u32>,
    pub hidden: bool,
    pub order: usize,
}

struct Chromium {
    id: &'static str,
    name: &'static str,
    commands: &'static [&'static str],
    directory: &'static str,
    icon: &'static str,
    flatpak: Option<&'static str>,
}

const CHROMIUM: &[Chromium] = &[
    Chromium {
        id: "chrome",
        name: "Google Chrome",
        commands: &["google-chrome-stable", "google-chrome"],
        directory: "google-chrome",
        icon: "google-chrome",
        flatpak: Some("com.google.Chrome"),
    },
    Chromium {
        id: "chrome-beta",
        name: "Google Chrome Beta",
        commands: &["google-chrome-beta"],
        directory: "google-chrome-beta",
        icon: "google-chrome-beta",
        flatpak: None,
    },
    Chromium {
        id: "chrome-dev",
        name: "Google Chrome Dev",
        commands: &["google-chrome-unstable"],
        directory: "google-chrome-unstable",
        icon: "google-chrome-unstable",
        flatpak: None,
    },
    Chromium {
        id: "chromium",
        name: "Chromium",
        commands: &["chromium", "chromium-browser"],
        directory: "chromium",
        icon: "chromium",
        flatpak: Some("org.chromium.Chromium"),
    },
    Chromium {
        id: "brave",
        name: "Brave",
        commands: &["brave", "brave-browser"],
        directory: "BraveSoftware/Brave-Browser",
        icon: "brave-browser",
        flatpak: Some("com.brave.Browser"),
    },
    Chromium {
        id: "edge",
        name: "Microsoft Edge",
        commands: &["microsoft-edge-stable", "microsoft-edge"],
        directory: "microsoft-edge",
        icon: "microsoft-edge",
        flatpak: Some("com.microsoft.Edge"),
    },
    Chromium {
        id: "vivaldi",
        name: "Vivaldi",
        commands: &["vivaldi-stable", "vivaldi"],
        directory: "vivaldi",
        icon: "vivaldi",
        flatpak: Some("com.vivaldi.Vivaldi"),
    },
    Chromium {
        id: "opera",
        name: "Opera",
        commands: &["opera"],
        directory: "opera",
        icon: "opera",
        flatpak: Some("com.opera.Opera"),
    },
];

struct Firefox {
    id: &'static str,
    name: &'static str,
    commands: &'static [&'static str],
    directories: &'static [&'static str],
    /// Roots under `$XDG_CONFIG_HOME`, checked after `directories`: Firefox
    /// uses the XDG root only when the legacy one does not exist.
    xdg_directories: &'static [&'static str],
    icon: &'static str,
    flatpak: Option<&'static str>,
}

const FIREFOX: &[Firefox] = &[
    Firefox {
        id: "firefox",
        name: "Firefox",
        commands: &["firefox"],
        directories: &[".mozilla/firefox"],
        xdg_directories: &["mozilla/firefox"],
        icon: "firefox",
        flatpak: Some("org.mozilla.firefox"),
    },
    Firefox {
        id: "firefox-beta",
        name: "Firefox Beta",
        commands: &["firefox-beta"],
        directories: &[".mozilla/firefox"],
        xdg_directories: &["mozilla/firefox"],
        icon: "firefox-beta",
        flatpak: None,
    },
    Firefox {
        id: "firefox-devedition",
        name: "Firefox Developer Edition",
        commands: &["firefox-devedition", "firefox-developer-edition"],
        directories: &[".mozilla/firefox"],
        xdg_directories: &["mozilla/firefox"],
        icon: "firefox-devedition",
        flatpak: None,
    },
    Firefox {
        id: "firefox-nightly",
        name: "Firefox Nightly",
        commands: &["firefox-nightly"],
        directories: &[".mozilla/firefox"],
        xdg_directories: &["mozilla/firefox"],
        icon: "firefox-nightly",
        flatpak: None,
    },
    Firefox {
        id: "firefox-esr",
        name: "Firefox ESR",
        commands: &["firefox-esr"],
        directories: &[".mozilla/firefox"],
        xdg_directories: &["mozilla/firefox"],
        icon: "firefox-esr",
        flatpak: None,
    },
    Firefox {
        id: "librewolf",
        name: "LibreWolf",
        commands: &["librewolf"],
        directories: &[".librewolf"],
        xdg_directories: &[],
        icon: "librewolf",
        flatpak: Some("io.gitlab.librewolf-community"),
    },
    Firefox {
        id: "floorp",
        name: "Floorp",
        commands: &["floorp"],
        directories: &[".floorp"],
        xdg_directories: &[],
        icon: "floorp",
        flatpak: Some("one.ablaze.floorp"),
    },
    Firefox {
        id: "zen",
        name: "Zen",
        commands: &["zen", "zen-browser"],
        directories: &[".zen"],
        xdg_directories: &[],
        icon: "zen-browser",
        flatpak: Some("app.zen_browser.zen"),
    },
    Firefox {
        id: "waterfox",
        name: "Waterfox",
        commands: &["waterfox", "waterfox-current"],
        directories: &[".waterfox"],
        xdg_directories: &[],
        icon: "waterfox",
        flatpak: Some("net.waterfox.waterfox"),
    },
    Firefox {
        id: "mullvad",
        name: "Mullvad Browser",
        commands: &["mullvad-browser", "mullvadbrowser"],
        directories: &[".mullvad/mullvadbrowser", ".mullvad", ".mullvad-browser"],
        xdg_directories: &[],
        icon: "mullvad-browser",
        flatpak: Some("net.mullvad.MullvadBrowser"),
    },
];

/// Discover native, Flatpak and supported Snap profiles, merge custom commands, then apply
/// configured visibility and ordering. Hidden entries remain in the result so
/// routing can still address their IDs. Missing or malformed metadata is skipped.
pub fn discover(config: &Config) -> Vec<BrowserProfile> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let mut profiles = Vec::new();
    for browser in CHROMIUM {
        let command = executable(browser.commands);
        let default_root = config_home
            .as_ref()
            .map(|root| root.join(browser.directory));
        let supports_chrome_env = browser.id.starts_with("chrome") || browser.id == "chromium";
        let root = if supports_chrome_env {
            chromium_user_data_root(browser.directory)
        } else {
            default_root.clone()
        };
        // Native data roots outlive uninstalled browsers; only list browsers
        // that can actually be launched.
        if let (Some(root), Some(command)) = (root, &command) {
            let metadata = root.join("Local State");
            let Some(user_data_dir) = root.to_str() else {
                continue;
            };
            let launch = vec![command.clone(), format!("--user-data-dir={user_data_dir}")];
            // Preserve ordinary IDs; explicitly configured roots have their
            // own identity so an old routing target cannot select another root.
            let id = if default_root.as_ref() == Some(&root) {
                browser.id.to_owned()
            } else {
                root_identity(browser.id, &root)
            };
            chromium_profiles(browser, &id, &metadata, launch, true, &mut profiles);
        }
        if let (Some(home), Some(app)) = (&home, browser.flatpak) {
            let metadata = home
                .join(".var/app")
                .join(app)
                .join("config")
                .join(browser.directory)
                .join("Local State");
            chromium_profiles(
                browser,
                &format!("{}-flatpak", browser.id),
                &metadata,
                vec!["flatpak".into(), "run".into(), app.into()],
                false,
                &mut profiles,
            );
        }
        if let Some(home) = &home {
            let package = match browser.id {
                "chromium" => Some("chromium"),
                "brave" => Some("brave"),
                "opera" => Some("opera"),
                _ => None,
            };
            if let Some(package) = package {
                let base = home.join("snap").join(package);
                let roots = [
                    base.join("common").join(browser.directory),
                    base.join("current/.config").join(browser.directory),
                    base.join("common/.config").join(browser.directory),
                ];
                if let Some(root) = roots.iter().find(|root| root.join("Local State").is_file()) {
                    chromium_profiles(
                        browser,
                        &format!("{}-snap", browser.id),
                        &root.join("Local State"),
                        vec!["snap".into(), "run".into(), package.into()],
                        false,
                        &mut profiles,
                    );
                }
            }
        }
    }
    for browser in FIREFOX {
        let command = executable(browser.commands);
        if let Some(home) = &home {
            // Firefox editions share Mozilla's profile root, and profile roots
            // outlive uninstalled browsers; only list editions that are installed.
            if let Some(command) = &command {
                let roots: Vec<_> = browser
                    .directories
                    .iter()
                    .map(|directory| home.join(directory))
                    .chain(config_home.iter().flat_map(|config_home| {
                        browser
                            .xdg_directories
                            .iter()
                            .map(|directory| config_home.join(directory))
                    }))
                    .collect();
                if let Some(root) = preferred_firefox_root(&roots) {
                    firefox_profiles(
                        browser,
                        browser.id,
                        root,
                        vec![command.clone()],
                        true,
                        &mut profiles,
                    );
                }
            }
            if let Some(app) = browser.flatpak {
                let app_root = home.join(".var/app").join(app);
                let roots: Vec<_> = browser
                    .directories
                    .iter()
                    .map(|directory| app_root.join(directory))
                    .chain(
                        browser
                            .xdg_directories
                            .iter()
                            .map(|directory| app_root.join("config").join(directory)),
                    )
                    .collect();
                if let Some(root) = preferred_firefox_root(&roots) {
                    firefox_profiles(
                        browser,
                        &format!("{}-flatpak", browser.id),
                        root,
                        vec!["flatpak".into(), "run".into(), app.into()],
                        false,
                        &mut profiles,
                    );
                }
            }
            if browser.id == "firefox" {
                let roots = [
                    home.join("snap/firefox/common/.mozilla/firefox"),
                    home.join("snap/firefox/current/.mozilla/firefox"),
                ];
                if let Some(root) = roots
                    .iter()
                    .find(|root| root.join("profiles.ini").is_file())
                {
                    firefox_profiles(
                        browser,
                        "firefox-snap",
                        root,
                        vec!["snap".into(), "run".into(), "firefox".into()],
                        false,
                        &mut profiles,
                    );
                }
            }
        }
    }
    for browser in &config.custom_browsers {
        let mut launch = browser.command.clone();
        if !launch.iter().any(|arg| arg == "{url}") {
            launch.push("{url}".into());
        }
        profiles.push(BrowserProfile {
            id: format!("custom:{}", browser.id),
            browser_id: format!("custom:{}", browser.id),
            browser_name: browser.name.clone(),
            profile_name: browser
                .profile_name
                .clone()
                .unwrap_or_else(|| "Default".into()),
            email: browser.email.clone(),
            icon_name: browser.icon_name.clone(),
            launch,
            container: None,
            hidden: false,
            order: 0,
        });
    }
    for profile in &mut profiles {
        profile.hidden = config
            .browsers
            .hidden
            .iter()
            .any(|id| id == &profile.id || id == &profile.browser_id);
    }
    let rank = |profile: &BrowserProfile| {
        config
            .browsers
            .order
            .iter()
            .position(|id| id == &profile.id || id == &profile.browser_id)
            .unwrap_or(usize::MAX)
    };
    profiles.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.browser_name.cmp(&b.browser_name))
            .then_with(|| a.profile_name.cmp(&b.profile_name))
            .then_with(|| a.id.cmp(&b.id))
    });
    profiles.dedup_by(|a, b| a.id == b.id);
    for (order, profile) in profiles.iter_mut().enumerate() {
        profile.order = order;
    }
    profiles
}

fn environment_root(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn preferred_firefox_root(roots: &[PathBuf]) -> Option<&PathBuf> {
    roots
        .iter()
        .find(|root| root.join("profiles.ini").is_file())
        .or_else(|| roots.first())
}

/// FNV-1a is specified here rather than using DefaultHasher, whose output is
/// not a stable serialization format. The suffix disambiguates data roots.
fn root_identity(browser: &str, root: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let hash = root
        .as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("{browser}@{hash:016x}")
}

fn chromium_profiles(
    browser: &Chromium,
    id: &str,
    metadata: &Path,
    prefix: Vec<String>,
    installed: bool,
    profiles: &mut Vec<BrowserProfile>,
) {
    let text = read_metadata(metadata);
    let state = text
        .as_deref()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
    let cache = state
        .as_ref()
        .and_then(|state| state.get("profile")?.get("info_cache")?.as_object());
    let start = profiles.len();
    if let Some(cache) = cache {
        for (directory, info) in cache {
            if directory.is_empty()
                || matches!(directory.as_str(), "." | "..")
                || directory.contains(['/', '\\', '\n', '\r', '\0'])
            {
                continue;
            }
            let mut launch = prefix.clone();
            launch.extend([format!("--profile-directory={directory}"), "{url}".into()]);
            profiles.push(BrowserProfile {
                id: format!("{id}:{directory}"),
                browser_id: id.into(),
                browser_name: browser.name.into(),
                profile_name: string_field(info, "name").unwrap_or_else(|| directory.clone()),
                email: string_field(info, "user_name"),
                icon_name: browser.icon.into(),
                launch,
                container: None,
                hidden: false,
                order: 0,
            });
        }
    }
    if profiles.len() == start && installed && text.is_none() {
        let mut launch = prefix;
        launch.extend(["--profile-directory=Default".into(), "{url}".into()]);
        profiles.push(BrowserProfile {
            id: format!("{id}:Default"),
            browser_id: id.into(),
            browser_name: browser.name.into(),
            profile_name: "Default".into(),
            email: None,
            icon_name: browser.icon.into(),
            launch,
            container: None,
            hidden: false,
            order: 0,
        });
    }
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
}

fn firefox_profiles(
    browser: &Firefox,
    id: &str,
    root: &Path,
    prefix: Vec<String>,
    installed: bool,
    profiles: &mut Vec<BrowserProfile>,
) {
    let Some(text) = read_metadata(&root.join("profiles.ini")) else {
        if installed {
            let mut launch = prefix;
            launch.extend(["--new-tab".into(), "{url}".into()]);
            profiles.push(BrowserProfile {
                id: format!("{id}:default"),
                browser_id: id.into(),
                browser_name: browser.name.into(),
                profile_name: "Default".into(),
                email: None,
                icon_name: browser.icon.into(),
                launch,
                container: None,
                hidden: false,
                order: 0,
            });
        }
        return;
    };
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            sections.push((section.into(), Vec::new()));
        } else if let (Some((_, values)), Some((key, value))) =
            (sections.last_mut(), line.split_once('='))
        {
            values.push((key.trim().into(), value.trim().into()));
        }
    }
    for (section, values) in sections {
        if !section.starts_with("Profile") {
            continue;
        }
        let get = |key: &str| {
            values
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
        };
        let Some(path) = get("Path").filter(|path| !path.is_empty() && !path.contains('\0')) else {
            continue;
        };
        let relative = get("IsRelative") != Some("0");
        let profile_path = if relative {
            root.join(path)
        } else {
            PathBuf::from(path)
        };
        if !profile_path.is_absolute() {
            continue;
        }
        let Some(profile_path) = profile_path.to_str() else {
            continue;
        };
        let mut launch = prefix.clone();
        launch.extend([
            "--profile".into(),
            profile_path.into(),
            "--new-tab".into(),
            "{url}".into(),
        ]);
        let profile_name = get("Name").filter(|name| !name.is_empty()).unwrap_or(path);
        profiles.push(BrowserProfile {
            id: format!("{id}:{path}"),
            browser_id: id.into(),
            browser_name: browser.name.into(),
            profile_name: profile_name.into(),
            email: None,
            icon_name: browser.icon.into(),
            launch: launch.clone(),
            container: None,
            hidden: false,
            order: 0,
        });
        // Firefox cannot open a container from the command line; only offer
        // containers where the Flipper add-on can receive the hand-off.
        let profile_dir = Path::new(profile_path);
        if !profile_dir.join("extensions").join(ADDON_XPI).is_file() {
            continue;
        }
        for (container, name) in firefox_containers(profile_dir) {
            profiles.push(BrowserProfile {
                id: format!("{id}:{path}#container-{container}"),
                browser_id: id.into(),
                browser_name: browser.name.into(),
                profile_name: format!("{profile_name} · {name}"),
                email: None,
                icon_name: browser.icon.into(),
                launch: launch.clone(),
                container: Some(container),
                hidden: false,
                order: 0,
            });
        }
    }
}

const ADDON_XPI: &str = "flipper@korora.tech.xpi";

/// Public containers from a profile's containers.json. Firefox's built-in
/// containers may omit `name` and are labelled from `l10nId` or their ID.
fn firefox_containers(profile: &Path) -> Vec<(u32, String)> {
    let Some(state) = read_metadata(&profile.join("containers.json"))
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Vec::new();
    };
    let Some(identities) = state.get("identities").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    identities
        .iter()
        .filter(|identity| identity.get("public").and_then(|value| value.as_bool()) == Some(true))
        .filter_map(|identity| {
            let container = identity
                .get("userContextId")?
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| *id != 0)?;
            let builtin = match identity.get("l10nId").and_then(|value| value.as_str()) {
                Some("user-context-personal") => Some(1),
                Some("user-context-work") => Some(2),
                Some("user-context-banking") => Some(3),
                Some("user-context-shopping") => Some(4),
                _ => None,
            };
            let name = string_field(identity, "name").or_else(|| {
                match builtin.unwrap_or(container) {
                    1 => Some("Personal"),
                    2 => Some("Work"),
                    3 => Some("Banking"),
                    4 => Some("Shopping"),
                    _ => None,
                }
                .map(str::to_owned)
            })?;
            Some((container, name))
        })
        .collect()
}

/// Metadata is small in normal installations; bound reads to avoid loading an
/// accidentally selected large file. No directories inside profiles are scanned.
fn read_metadata(path: &Path) -> Option<String> {
    const LIMIT: u64 = 16 * 1024 * 1024;
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(LIMIT + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= LIMIT).then_some(text)
}

/// Resolve the Chromium user-data root using the same overrides as discovery.
/// Used by native-host installation so Chrome can find its manifest alongside
/// the profiles Flipper discovered.
pub fn chromium_user_data_root(directory: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    environment_root("CHROME_USER_DATA_DIR")
        .or_else(|| environment_root("CHROME_CONFIG_HOME").map(|root| root.join(directory)))
        .or_else(|| config_home.map(|root| root.join(directory)))
}

fn executable(commands: &[&str]) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    commands.iter().find_map(|command| {
        std::env::split_paths(&path)
            .any(|directory| {
                if directory.starts_with("/snap/bin")
                    || directory.starts_with("/var/lib/snapd/snap/bin")
                {
                    return false;
                }
                directory.join(command).metadata().is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
            })
            .then(|| (*command).to_owned())
    })
}
