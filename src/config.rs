//! Versioned configuration from `$XDG_CONFIG_HOME/flipper/config.yaml`.
//!
//! Rules run in file order. Within each populated condition group any entry
//! may match; populated groups must all match. An `open` rule selects its
//! stable profile ID, while a `picker` rule overrides automatic routing.

use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub appearance: Appearance,
    pub routing: Routing,
    pub rules: Vec<Rule>,
    pub browsers: BrowserPreferences,
    pub custom_browsers: Vec<CustomBrowser>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            appearance: Appearance::default(),
            routing: Routing::default(),
            rules: Vec::new(),
            browsers: BrowserPreferences::default(),
            custom_browsers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub theme: Theme,
    pub dim: bool,
    /// Picker width in logical pixels.
    pub width: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            dim: true,
            width: 760.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Routing {
    /// Behavior after no explicit rule matches.
    pub mode: RoutingMode,
    /// Stable profile ID used by `always`; an unavailable ID needs the picker.
    pub default_profile: Option<String>,
    /// Temporarily show the picker for every URL without deleting rules.
    pub suspended: bool,
    /// Resolve recognized redirect wrappers before applying routing rules.
    pub unwrap_redirects: bool,
}

impl Default for Routing {
    fn default() -> Self {
        Self {
            mode: RoutingMode::Picker,
            default_profile: None,
            suspended: false,
            unwrap_redirects: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RoutingMode {
    #[default]
    Picker,
    Always,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    pub name: Option<String>,
    /// Hostnames or hostname globs, such as `example.com` or `*.example.com`.
    pub domains: Vec<String>,
    /// URL path globs, such as `/work/*`.
    pub paths: Vec<String>,
    /// Globs matched against the complete URL.
    pub patterns: Vec<String>,
    /// File extensions, such as `pdf`, `.pdf` or `ext:pdf`.
    pub extensions: Vec<String>,
    pub action: RuleAction,
    /// Stable profile ID, required only for an `open` action.
    pub profile: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    #[default]
    Picker,
    Open,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserPreferences {
    /// Profile IDs or browser IDs to omit from the picker.
    pub hidden: Vec<String>,
    /// Profile IDs or browser IDs, in preferred display order.
    pub order: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustomBrowser {
    /// Configuration identity; the resulting stable profile ID is `custom:<id>`.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub profile_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default = "default_icon")]
    pub icon_name: String,
    /// Executable and arguments, without shell interpretation. `{url}` must
    /// occupy an entire argument. Discovery appends it when it is absent.
    pub command: Vec<String>,
}

fn default_icon() -> String {
    "web-browser".into()
}

/// Find the configuration using the XDG convention, with a HOME fallback.
pub fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|root| root.join("flipper/config.yaml"))
}

/// Missing configuration uses defaults. Other errors include the file path so
/// the application can display a warning and choose its fallback behavior.
pub fn load() -> Result<Config, String> {
    let Some(path) = path() else {
        return Ok(Config::default());
    };
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if text.len() > 1024 * 1024 {
        return Err(format!(
            "{}: configuration is larger than 1 MiB",
            path.display()
        ));
    }
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Parse and validate the supported configuration version and command shapes.
pub fn parse(text: &str) -> Result<Config, String> {
    let config: Config = if text.trim().is_empty() {
        Config::default()
    } else {
        serde_saphyr::from_str(text).map_err(|error| error.to_string())?
    };
    if config.version != CONFIG_VERSION {
        return Err(format!(
            "unsupported configuration version {}; expected {CONFIG_VERSION}",
            config.version
        ));
    }
    if !config.appearance.width.is_finite() || !(320.0..=2400.0).contains(&config.appearance.width)
    {
        return Err("appearance.width must be between 320 and 2400".into());
    }
    if config.routing.mode == RoutingMode::Always
        && config
            .routing
            .default_profile
            .as_ref()
            .is_none_or(|id| id.trim().is_empty())
    {
        return Err("routing.default_profile is required when routing.mode is always".into());
    }
    for (index, rule) in config.rules.iter().enumerate() {
        let label = format!("rules[{}]", index);
        if rule.domains.is_empty()
            && rule.paths.is_empty()
            && rule.patterns.is_empty()
            && rule.extensions.is_empty()
        {
            return Err(format!(
                "{label} requires domains, paths, patterns or extensions; use routing for a default"
            ));
        }
        if rule
            .domains
            .iter()
            .chain(&rule.paths)
            .chain(&rule.patterns)
            .chain(&rule.extensions)
            .any(|pattern| pattern.trim().is_empty())
        {
            return Err(format!("{label} contains an empty match pattern"));
        }
        for extension in &rule.extensions {
            let extension = extension
                .trim()
                .strip_prefix("ext:")
                .unwrap_or(extension.trim())
                .trim_start_matches('.');
            if extension.is_empty()
                || !extension
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                return Err(format!(
                    "{label}.extensions must contain file extensions such as pdf or ext:pdf"
                ));
            }
        }
        match rule.action {
            RuleAction::Open if rule.profile.as_ref().is_none_or(|id| id.trim().is_empty()) => {
                return Err(format!("{label}.profile is required for an open action"));
            }
            RuleAction::Picker if rule.profile.is_some() => {
                return Err(format!("{label}.profile is only valid for an open action"));
            }
            _ => {}
        }
    }
    let mut ids = std::collections::HashSet::new();
    for browser in &config.custom_browsers {
        if browser.id.is_empty()
            || !browser
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err("custom browser IDs must contain only letters, digits, '-' or '_'".into());
        }
        if !ids.insert(&browser.id) {
            return Err(format!("duplicate custom browser ID {:?}", browser.id));
        }
        if browser.name.trim().is_empty()
            || browser
                .command
                .first()
                .is_none_or(|exe| exe.trim().is_empty())
        {
            return Err(format!(
                "custom browser {:?} needs a name and a nonempty command",
                browser.id
            ));
        }
        if browser
            .command
            .iter()
            .any(|arg| arg.contains('\0') || (arg.contains("{url}") && arg != "{url}"))
        {
            return Err(format!(
                "custom browser {:?}: {{url}} must be a complete argument, and commands cannot contain NUL",
                browser.id
            ));
        }
        if browser.command[0] == "{url}" {
            return Err(format!(
                "custom browser {:?}: the executable cannot be {{url}}",
                browser.id
            ));
        }
    }
    Ok(config)
}
