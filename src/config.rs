//! Versioned configuration from `$XDG_CONFIG_HOME/flipper/config.yaml`.
//!
//! Rules run in file order. Within each populated condition group any entry
//! may match; populated groups must all match. An `open` rule selects its
//! stable profile ID, while a `picker` rule overrides automatic routing.
//!
//! When `config.yaml` is a symlink it is *managed* (for example by Home
//! Manager, which links it into the Nix store) and Flipper never writes it.
//! Local changes are then kept in `overrides.yaml` beside it. Overrides are
//! layered on top of the base configuration whenever that file exists and the
//! base allows it: override rules run before base rules, override scalar
//! fields win, `browsers.hidden` is a union, `browsers.order` lists override
//! entries first, and custom browsers are merged by ID with the override
//! winning.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;

pub const CONFIG_FILE: &str = "config.yaml";
pub const OVERRIDES_FILE: &str = "overrides.yaml";
const MAX_FILE: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub appearance: Appearance,
    pub routing: Routing,
    pub rules: Vec<Rule>,
    pub browsers: BrowserPreferences,
    pub custom_browsers: Vec<CustomBrowser>,
    /// Whether local overrides in `overrides.yaml` apply. Only meaningful in
    /// the base configuration; omitted when it has its default value.
    #[serde(skip_serializing_if = "OverridePolicy::is_default")]
    pub overrides: OverridePolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct OverridePolicy {
    /// When false, `overrides.yaml` is ignored and a managed configuration is
    /// read-only in the user interface.
    pub allowed: bool,
}

impl Default for OverridePolicy {
    fn default() -> Self {
        Self { allowed: true }
    }
}

impl OverridePolicy {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
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
            overrides: OverridePolicy::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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
            width: 560.0,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserPreferences {
    /// Profile IDs or browser IDs to omit from the picker.
    pub hidden: Vec<String>,
    /// Profile IDs or browser IDs, in preferred display order.
    pub order: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

/// Local changes layered over a managed configuration, from `overrides.yaml`.
///
/// Every scalar is optional: an absent field leaves the base value in place.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Overrides {
    pub version: u32,
    #[serde(skip_serializing_if = "AppearanceOverrides::is_empty")]
    pub appearance: AppearanceOverrides,
    #[serde(skip_serializing_if = "RoutingOverrides::is_empty")]
    pub routing: RoutingOverrides,
    /// Evaluated before every base rule.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    #[serde(skip_serializing_if = "BrowserPreferences::is_empty")]
    pub browsers: BrowserPreferences,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_browsers: Vec<CustomBrowser>,
}

impl Default for Overrides {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            appearance: AppearanceOverrides::default(),
            routing: RoutingOverrides::default(),
            rules: Vec::new(),
            browsers: BrowserPreferences::default(),
            custom_browsers: Vec::new(),
        }
    }
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        self.appearance.is_empty()
            && self.routing.is_empty()
            && self.rules.is_empty()
            && self.browsers.is_empty()
            && self.custom_browsers.is_empty()
    }

    /// Whether a scalar field is set locally.
    pub fn has(&self, field: Field) -> bool {
        match field {
            Field::Theme => self.appearance.theme.is_some(),
            Field::Dim => self.appearance.dim.is_some(),
            Field::Mode => self.routing.mode.is_some(),
            Field::DefaultProfile => self.routing.default_profile.is_some(),
            Field::Suspended => self.routing.suspended.is_some(),
            Field::UnwrapRedirects => self.routing.unwrap_redirects.is_some(),
        }
    }

    /// Remove a local scalar so the base value applies again.
    pub fn clear(&mut self, field: Field) {
        match field {
            Field::Theme => self.appearance.theme = None,
            Field::Dim => self.appearance.dim = None,
            Field::Mode => self.routing.mode = None,
            Field::DefaultProfile => self.routing.default_profile = None,
            Field::Suspended => self.routing.suspended = None,
            Field::UnwrapRedirects => self.routing.unwrap_redirects = None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dim: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
}

impl AppearanceOverrides {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct RoutingOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<RoutingMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suspended: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unwrap_redirects: Option<bool>,
}

impl RoutingOverrides {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl BrowserPreferences {
    fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.order.is_empty()
    }
}

/// Scalar settings that can be overridden one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Theme,
    Dim,
    Mode,
    DefaultProfile,
    Suspended,
    UnwrapRedirects,
}

/// Where a rule in the effective configuration comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleOrigin {
    /// `config.yaml`; read-only when the configuration is managed.
    Base,
    /// `overrides.yaml`.
    Local,
}

/// Locations of the base configuration and its overrides.
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub overrides: PathBuf,
}

impl Paths {
    pub fn in_dir(directory: &Path) -> Self {
        Self {
            config: directory.join(CONFIG_FILE),
            overrides: directory.join(OVERRIDES_FILE),
        }
    }

    /// Find the configuration directory using the XDG convention, with a HOME
    /// fallback.
    pub fn from_env() -> Option<Self> {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .map(|root| Self::in_dir(&root.join("flipper")))
    }

    /// A symlinked `config.yaml` is owned by a configuration manager.
    pub fn is_managed(&self) -> bool {
        std::fs::symlink_metadata(&self.config).is_ok_and(|metadata| metadata.is_symlink())
    }
}

/// The base configuration, its overrides, and the merged result.
#[derive(Clone, Debug)]
pub struct Layers {
    pub base: Config,
    /// Overrides in effect; empty when none exist or they are disallowed.
    pub overrides: Overrides,
    /// `config.yaml` is a symlink and is never written.
    pub managed: bool,
    pub effective: Config,
    /// Why `overrides.yaml` was ignored.
    pub overrides_error: Option<String>,
}

impl Layers {
    pub fn unmanaged(base: Config) -> Self {
        Self {
            effective: base.clone(),
            base,
            overrides: Overrides::default(),
            managed: false,
            overrides_error: None,
        }
    }

    pub fn overrides_allowed(&self) -> bool {
        self.base.overrides.allowed
    }

    /// Managed and without local overrides: nothing can be saved.
    pub fn read_only(&self) -> bool {
        self.managed && !self.overrides_allowed()
    }

    /// Origin of the effective rule at `index`.
    pub fn rule_origin(&self, index: usize) -> RuleOrigin {
        if index < self.overrides.rules.len() {
            RuleOrigin::Local
        } else {
            RuleOrigin::Base
        }
    }

    /// Whether the user interface may change or remove a rule.
    pub fn rule_editable(&self, index: usize) -> bool {
        match self.rule_origin(index) {
            RuleOrigin::Local => self.overrides_allowed(),
            RuleOrigin::Base => !self.managed,
        }
    }
}

/// Whether the configuration in the environment's location is managed.
pub fn is_managed() -> bool {
    Paths::from_env().is_some_and(|paths| paths.is_managed())
}

pub fn load_layers() -> Result<Layers, String> {
    match Paths::from_env() {
        Some(paths) => load_layers_at(&paths),
        None => Ok(Layers::unmanaged(Config::default())),
    }
}

/// Load the base configuration and apply its overrides. An invalid base is an
/// error; invalid overrides are ignored and reported in `overrides_error` so
/// a broken local file cannot hide the managed configuration.
pub fn load_layers_at(paths: &Paths) -> Result<Layers, String> {
    let managed = paths.is_managed();
    let base = match read_bounded(&paths.config)? {
        Some(text) => {
            parse(&text).map_err(|error| format!("{}: {error}", paths.config.display()))?
        }
        None => Config::default(),
    };
    let mut layers = Layers {
        managed,
        ..Layers::unmanaged(base)
    };
    if !layers.overrides_allowed() {
        return Ok(layers);
    }
    let overrides = match read_bounded(&paths.overrides) {
        Ok(Some(text)) => parse_overrides(&text),
        Ok(None) => return Ok(layers),
        Err(error) => Err(error),
    }
    .and_then(|overrides| {
        let effective = merge(&layers.base, &overrides);
        validate(&effective)
            .map(|()| (overrides, effective))
            .map_err(|error| format!("merged configuration is invalid: {error}"))
    });
    match overrides {
        Ok((overrides, effective)) => {
            layers.overrides = overrides;
            layers.effective = effective;
        }
        Err(error) => {
            layers.overrides_error = Some(format!(
                "{}: {error}; local overrides are ignored",
                paths.overrides.display()
            ))
        }
    }
    Ok(layers)
}

/// Read at most 1 MiB; `None` when the file does not exist.
pub fn read_bounded(path: &Path) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut text = String::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if text.len() > MAX_FILE {
        return Err(format!(
            "{}: configuration is larger than 1 MiB",
            path.display()
        ));
    }
    Ok(Some(text))
}

/// Parse and validate the supported configuration version and command shapes.
/// Profile selectors are checked for shape only; the profiles they name may
/// not exist where the configuration is built.
pub fn parse(text: &str) -> Result<Config, String> {
    let config: Config = if text.trim().is_empty() {
        Config::default()
    } else {
        serde_saphyr::from_str(text).map_err(|error| error.to_string())?
    };
    validate(&config)?;
    Ok(config)
}

/// Parse and validate `overrides.yaml` on its own; the merged result is
/// validated separately.
pub fn parse_overrides(text: &str) -> Result<Overrides, String> {
    let overrides: Overrides = if text.trim().is_empty() {
        Overrides::default()
    } else {
        serde_saphyr::from_str(text).map_err(|error| error.to_string())?
    };
    check_version(overrides.version)?;
    if let Some(width) = overrides.appearance.width {
        check_width(width)?;
    }
    validate_rules(&overrides.rules)?;
    validate_custom_browsers(&overrides.custom_browsers)?;
    Ok(overrides)
}

/// Layer overrides on a base configuration.
pub fn merge(base: &Config, overrides: &Overrides) -> Config {
    let mut config = base.clone();
    let appearance = &overrides.appearance;
    if let Some(theme) = appearance.theme {
        config.appearance.theme = theme;
    }
    if let Some(dim) = appearance.dim {
        config.appearance.dim = dim;
    }
    if let Some(width) = appearance.width {
        config.appearance.width = width;
    }
    let routing = &overrides.routing;
    if let Some(mode) = routing.mode {
        config.routing.mode = mode;
    }
    if let Some(profile) = &routing.default_profile {
        config.routing.default_profile = Some(profile.clone());
    }
    if let Some(suspended) = routing.suspended {
        config.routing.suspended = suspended;
    }
    if let Some(unwrap) = routing.unwrap_redirects {
        config.routing.unwrap_redirects = unwrap;
    }
    config.rules = overrides.rules.iter().chain(&base.rules).cloned().collect();
    for id in &overrides.browsers.hidden {
        if !config.browsers.hidden.contains(id) {
            config.browsers.hidden.push(id.clone());
        }
    }
    config.browsers.order = overrides.browsers.order.clone();
    for id in &base.browsers.order {
        if !config.browsers.order.contains(id) {
            config.browsers.order.push(id.clone());
        }
    }
    for browser in &overrides.custom_browsers {
        match config
            .custom_browsers
            .iter_mut()
            .find(|existing| existing.id == browser.id)
        {
            Some(existing) => *existing = browser.clone(),
            None => config.custom_browsers.push(browser.clone()),
        }
    }
    config
}

fn check_version(version: u32) -> Result<(), String> {
    if version != CONFIG_VERSION {
        return Err(format!(
            "unsupported configuration version {version}; expected {CONFIG_VERSION}"
        ));
    }
    Ok(())
}

fn check_width(width: f32) -> Result<(), String> {
    if !width.is_finite() || !(320.0..=2400.0).contains(&width) {
        return Err("appearance.width must be between 320 and 2400".into());
    }
    Ok(())
}

pub fn validate(config: &Config) -> Result<(), String> {
    check_version(config.version)?;
    check_width(config.appearance.width)?;
    if config.routing.mode == RoutingMode::Always
        && config
            .routing
            .default_profile
            .as_ref()
            .is_none_or(|id| id.trim().is_empty())
    {
        return Err("routing.default_profile is required when routing.mode is always".into());
    }
    validate_rules(&config.rules)?;
    validate_custom_browsers(&config.custom_browsers)
}

fn validate_rules(rules: &[Rule]) -> Result<(), String> {
    for (index, rule) in rules.iter().enumerate() {
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
    Ok(())
}

fn validate_custom_browsers(browsers: &[CustomBrowser]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for browser in browsers {
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
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A unique, empty directory removed on drop.
    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "flipper-test-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn paths(&self) -> Paths {
            Paths::in_dir(&self.0)
        }

        /// Write `text` to a separate file and symlink `config.yaml` to it,
        /// as Home Manager does with a store path.
        pub fn managed(&self, text: &str) -> Paths {
            let store = self.0.join("store-config.yaml");
            std::fs::write(&store, text).unwrap();
            std::os::unix::fs::symlink(&store, self.0.join(CONFIG_FILE)).unwrap();
            self.paths()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rule(domain: &str, profile: Option<&str>) -> Rule {
        Rule {
            domains: vec![domain.into()],
            action: if profile.is_some() {
                RuleAction::Open
            } else {
                RuleAction::Picker
            },
            profile: profile.map(str::to_owned),
            ..Rule::default()
        }
    }

    fn custom(id: &str, name: &str) -> CustomBrowser {
        CustomBrowser {
            id: id.into(),
            name: name.into(),
            profile_name: None,
            email: None,
            icon_name: "web-browser".into(),
            command: vec!["browser".into()],
        }
    }

    #[test]
    fn merge_layers_overrides_on_base() {
        let base = Config {
            appearance: Appearance {
                theme: Theme::Dark,
                dim: true,
                width: 560.0,
            },
            rules: vec![rule("example.com", Some("chrome:Default"))],
            browsers: BrowserPreferences {
                hidden: vec!["opera".into()],
                order: vec!["chrome".into(), "firefox".into()],
            },
            custom_browsers: vec![custom("a", "Base A"), custom("b", "Base B")],
            ..Config::default()
        };
        let overrides = Overrides {
            appearance: AppearanceOverrides {
                dim: Some(false),
                ..AppearanceOverrides::default()
            },
            routing: RoutingOverrides {
                suspended: Some(true),
                ..RoutingOverrides::default()
            },
            rules: vec![rule("work.example.com", None)],
            browsers: BrowserPreferences {
                hidden: vec!["vivaldi".into(), "opera".into()],
                order: vec!["firefox".into(), "zen".into()],
            },
            custom_browsers: vec![custom("b", "Local B"), custom("c", "Local C")],
            ..Overrides::default()
        };
        let merged = merge(&base, &overrides);
        assert_eq!(merged.appearance.theme, Theme::Dark);
        assert!(!merged.appearance.dim);
        assert!(merged.routing.suspended);
        assert!(merged.routing.unwrap_redirects);
        assert_eq!(merged.rules[0].domains, ["work.example.com"]);
        assert_eq!(merged.rules[1].domains, ["example.com"]);
        assert_eq!(merged.browsers.hidden, ["opera", "vivaldi"]);
        assert_eq!(merged.browsers.order, ["firefox", "zen", "chrome"]);
        let names: Vec<_> = merged
            .custom_browsers
            .iter()
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(names, ["Base A", "Local B", "Local C"]);
    }

    #[test]
    fn empty_overrides_leave_base_unchanged() {
        let base = parse(include_str!("../config.example.yaml")).unwrap();
        assert_eq!(merge(&base, &Overrides::default()), base);
    }

    #[test]
    fn overrides_reject_unknown_fields_and_policy() {
        assert!(parse_overrides("version: 1\nunknown: true\n").is_err());
        assert!(parse_overrides("version: 1\noverrides:\n  allowed: true\n").is_err());
        assert!(parse_overrides("version: 2\n").is_err());
        assert!(parse_overrides("appearance:\n  width: 10\n").is_err());
        let parsed = parse_overrides("version: 1\nrouting:\n  suspended: true\n").unwrap();
        assert_eq!(parsed.routing.suspended, Some(true));
        assert!(parsed.appearance.theme.is_none());
    }

    #[test]
    fn policy_is_optional_and_parsed() {
        assert!(parse("version: 1\n").unwrap().overrides.allowed);
        assert!(
            !parse("version: 1\noverrides:\n  allowed: false\n")
                .unwrap()
                .overrides
                .allowed
        );
        assert!(parse("version: 1\noverrides:\n  other: false\n").is_err());
        // The default policy is not written back out.
        let text = serde_saphyr::to_string(&Config::default()).unwrap();
        assert!(!text.contains("overrides"), "{text}");
    }

    #[test]
    fn parse_reports_validation_errors() {
        let error =
            parse("version: 1\nrules:\n  - domains: [a.com]\n    action: open\n").unwrap_err();
        assert!(error.contains("rules[0].profile is required"), "{error}");
        let error =
            parse("version: 1\nrules:\n  - domains: [a.com]\n    action: picker\n    profile: x\n")
                .unwrap_err();
        assert!(error.contains("only valid for an open action"), "{error}");
        let error = parse("version: 1\nrouting:\n  mode: always\n").unwrap_err();
        assert!(error.contains("default_profile is required"), "{error}");
        assert!(parse("version: 1\nbogus: 1\n").is_err());
        // Profiles are not resolved: a selector for an absent profile is fine.
        parse("version: 1\nrules:\n  - domains: [a.com]\n    action: open\n    profile: \"firefox:abc.default#container-2\"\n").unwrap();
    }

    #[test]
    fn symlinked_config_is_managed() {
        let dir = TempDir::new("managed");
        let paths = dir.paths();
        assert!(!paths.is_managed());
        std::fs::write(&paths.config, "version: 1\n").unwrap();
        assert!(!paths.is_managed());
        std::fs::remove_file(&paths.config).unwrap();
        let paths = dir.managed("version: 1\nappearance:\n  theme: dark\n");
        assert!(paths.is_managed());
        let layers = load_layers_at(&paths).unwrap();
        assert!(layers.managed);
        assert_eq!(layers.effective.appearance.theme, Theme::Dark);
    }

    #[test]
    fn overrides_apply_unless_disallowed() {
        let dir = TempDir::new("disallowed");
        let paths = dir.managed("version: 1\nrules:\n  - domains: [a.com]\n    action: picker\n");
        std::fs::write(
            &paths.overrides,
            "version: 1\nrules:\n  - domains: [b.com]\n    action: picker\nappearance:\n  dim: false\n",
        )
        .unwrap();
        let layers = load_layers_at(&paths).unwrap();
        assert_eq!(layers.effective.rules.len(), 2);
        assert_eq!(layers.rule_origin(0), RuleOrigin::Local);
        assert_eq!(layers.rule_origin(1), RuleOrigin::Base);
        assert!(layers.rule_editable(0));
        assert!(!layers.rule_editable(1));
        assert!(!layers.effective.appearance.dim);

        let dir = TempDir::new("disallowed-2");
        let paths = dir.managed(
            "version: 1\noverrides:\n  allowed: false\nrules:\n  - domains: [a.com]\n    action: picker\n",
        );
        std::fs::write(
            &paths.overrides,
            "version: 1\nrules:\n  - domains: [b.com]\n    action: picker\n",
        )
        .unwrap();
        let layers = load_layers_at(&paths).unwrap();
        assert!(layers.read_only());
        assert_eq!(layers.effective.rules.len(), 1);
        assert!(layers.overrides.is_empty());
    }

    #[test]
    fn overrides_are_honoured_without_a_managed_base() {
        let dir = TempDir::new("unmanaged-overrides");
        let paths = dir.paths();
        std::fs::write(&paths.config, "version: 1\n").unwrap();
        std::fs::write(&paths.overrides, "routing:\n  suspended: true\n").unwrap();
        let layers = load_layers_at(&paths).unwrap();
        assert!(!layers.managed);
        assert!(layers.effective.routing.suspended);
    }

    #[test]
    fn invalid_overrides_are_ignored_with_a_warning() {
        let dir = TempDir::new("invalid-overrides");
        let paths = dir.managed("version: 1\nappearance:\n  theme: dark\n");
        std::fs::write(&paths.overrides, "routing:\n  mode: always\n").unwrap();
        let layers = load_layers_at(&paths).unwrap();
        let error = layers.overrides_error.as_deref().unwrap();
        assert!(error.contains("default_profile is required"), "{error}");
        assert_eq!(layers.effective, layers.base);
    }
}
