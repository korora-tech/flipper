//! Validated, atomic configuration updates.
//!
//! Edits run as one locked transaction over `config.yaml` and
//! `overrides.yaml`. When `config.yaml` is managed (a symlink), every change
//! goes to `overrides.yaml`; otherwise changes go to `config.yaml` and clear
//! any local override that would hide them.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::Serialize;

use crate::config::{
    self, Config, Field, Layers, Overrides, Paths, RoutingMode, Rule, RuleOrigin, Theme,
};

pub const READ_ONLY: &str = "the configuration is managed by Home Manager and local overrides are disabled (overrides.allowed is false); change it in your Home Manager configuration";
pub const MANAGED_RULE: &str =
    "this rule comes from the managed configuration; change it in your Home Manager configuration";

/// A scalar setting with its new value.
#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    Theme(Theme),
    Dim(bool),
    Mode(RoutingMode),
    DefaultProfile(String),
    Suspended(bool),
    UnwrapRedirects(bool),
}

impl Setting {
    pub fn field(&self) -> Field {
        match self {
            Setting::Theme(_) => Field::Theme,
            Setting::Dim(_) => Field::Dim,
            Setting::Mode(_) => Field::Mode,
            Setting::DefaultProfile(_) => Field::DefaultProfile,
            Setting::Suspended(_) => Field::Suspended,
            Setting::UnwrapRedirects(_) => Field::UnwrapRedirects,
        }
    }

    fn apply_to_config(&self, config: &mut Config) {
        match self.clone() {
            Setting::Theme(value) => config.appearance.theme = value,
            Setting::Dim(value) => config.appearance.dim = value,
            Setting::Mode(value) => config.routing.mode = value,
            Setting::DefaultProfile(value) => config.routing.default_profile = Some(value),
            Setting::Suspended(value) => config.routing.suspended = value,
            Setting::UnwrapRedirects(value) => config.routing.unwrap_redirects = value,
        }
    }

    fn apply_to_overrides(&self, overrides: &mut Overrides) {
        match self.clone() {
            Setting::Theme(value) => overrides.appearance.theme = Some(value),
            Setting::Dim(value) => overrides.appearance.dim = Some(value),
            Setting::Mode(value) => overrides.routing.mode = Some(value),
            Setting::DefaultProfile(value) => overrides.routing.default_profile = Some(value),
            Setting::Suspended(value) => overrides.routing.suspended = Some(value),
            Setting::UnwrapRedirects(value) => overrides.routing.unwrap_redirects = Some(value),
        }
    }
}

/// The writable view of both layers inside a transaction.
pub struct Editor {
    base: Config,
    overrides: Overrides,
    managed: bool,
}

impl Editor {
    fn writable(&self) -> Result<(), String> {
        if self.managed && !self.base.overrides.allowed {
            Err(READ_ONLY.into())
        } else {
            Ok(())
        }
    }

    /// The rules list that the user owns.
    fn local_rules(&mut self) -> &mut Vec<Rule> {
        if self.managed {
            &mut self.overrides.rules
        } else {
            &mut self.base.rules
        }
    }

    pub fn set(&mut self, setting: Setting) -> Result<(), String> {
        self.writable()?;
        let field = setting.field();
        if self.managed {
            let mut base = self.base.clone();
            setting.apply_to_config(&mut base);
            if base == self.base {
                // Choosing the managed value again drops the override.
                self.overrides.clear(field);
            } else {
                setting.apply_to_overrides(&mut self.overrides);
            }
        } else {
            setting.apply_to_config(&mut self.base);
            self.overrides.clear(field);
        }
        Ok(())
    }

    /// Drop a local scalar so the base value applies again.
    pub fn reset(&mut self, field: Field) -> Result<(), String> {
        self.writable()?;
        self.overrides.clear(field);
        Ok(())
    }

    pub fn clear_overrides(&mut self) -> Result<(), String> {
        self.writable()?;
        self.overrides = Overrides::default();
        Ok(())
    }

    /// Replace the display order with a complete list of IDs.
    pub fn set_order(&mut self, order: Vec<String>) -> Result<(), String> {
        self.writable()?;
        if self.managed {
            self.overrides.browsers.order = order;
        } else {
            self.base.browsers.order = order;
            self.overrides.browsers.order.clear();
        }
        Ok(())
    }

    pub fn set_hidden(
        &mut self,
        id: &str,
        browser_id: Option<&str>,
        hidden: bool,
    ) -> Result<(), String> {
        self.writable()?;
        let matches = |entry: &String| entry == id || Some(entry.as_str()) == browser_id;
        if hidden {
            let list = if self.managed {
                &mut self.overrides.browsers.hidden
            } else {
                &mut self.base.browsers.hidden
            };
            if !list.iter().any(|entry| entry == id) {
                list.push(id.to_owned());
            }
            return Ok(());
        }
        self.overrides
            .browsers
            .hidden
            .retain(|entry| !matches(entry));
        if self.managed {
            if self.base.browsers.hidden.iter().any(matches) {
                return Err(format!(
                    "{id} is hidden by the managed configuration; change browsers.hidden in your Home Manager configuration"
                ));
            }
        } else {
            self.base.browsers.hidden.retain(|entry| !matches(entry));
        }
        Ok(())
    }

    /// Save a rule from the picker: replace a rule with identical conditions,
    /// and place it after narrower rules so they keep acting as exceptions.
    pub fn add_rule(&mut self, rule: Rule) -> Result<(), String> {
        self.writable()?;
        let rules = self.local_rules();
        rules.retain(|existing| {
            existing.domains != rule.domains
                || existing.paths != rule.paths
                || existing.patterns != rule.patterns
                || existing.extensions != rule.extensions
        });
        if rule.domains.is_empty() {
            rules.insert(0, rule);
            return Ok(());
        }
        let mut exceptions = Vec::new();
        let mut remaining = Vec::new();
        for existing in std::mem::take(rules) {
            let condition_exception = existing.domains == rule.domains
                && (!existing.paths.is_empty()
                    || !existing.patterns.is_empty()
                    || !existing.extensions.is_empty());
            let narrower_domain = !existing.domains.is_empty()
                && existing.domains != rule.domains
                && existing.domains.iter().all(|narrow| {
                    rule.domains
                        .iter()
                        .any(|broad| domain_pattern_is_subset(narrow, broad))
                });
            if condition_exception || narrower_domain {
                exceptions.push(existing);
            } else {
                remaining.push(existing);
            }
        }
        exceptions.push(rule);
        exceptions.extend(remaining);
        *rules = exceptions;
        Ok(())
    }

    pub fn remove_rule(&mut self, origin: RuleOrigin, rule: &Rule) -> Result<(), String> {
        let rules = match origin {
            RuleOrigin::Local => {
                self.writable()?;
                &mut self.overrides.rules
            }
            RuleOrigin::Base if self.managed => return Err(MANAGED_RULE.into()),
            RuleOrigin::Base => &mut self.base.rules,
        };
        if let Some(index) = rules.iter().position(|candidate| candidate == rule) {
            rules.remove(index);
        }
        Ok(())
    }
}

fn domain_pattern_is_subset(narrow: &str, broad: &str) -> bool {
    let normalize = |pattern: &str| {
        let pattern = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
        match pattern.strip_prefix("*.") {
            Some(domain) => (domain.to_owned(), true),
            None => (pattern, false),
        }
    };
    let (narrow_domain, narrow_wildcard) = normalize(narrow);
    let (broad_domain, broad_wildcard) = normalize(broad);
    let same_or_subdomain =
        narrow_domain == broad_domain || narrow_domain.ends_with(&format!(".{broad_domain}"));
    if !same_or_subdomain {
        return false;
    }
    // A wildcard broad selector excludes its apex, so a narrower selector
    // that includes that apex is not a true subset.
    !(broad_wildcard && !narrow_wildcard && narrow_domain == broad_domain)
}

fn env_paths() -> Result<Paths, String> {
    Paths::from_env().ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())
}

/// Run `edit` in a locked transaction and return the reloaded layers.
pub fn edit(edit: impl FnOnce(&mut Editor) -> Result<(), String>) -> Result<Layers, String> {
    edit_at(&env_paths()?, edit)
}

pub fn edit_at(
    paths: &Paths,
    edit: impl FnOnce(&mut Editor) -> Result<(), String>,
) -> Result<Layers, String> {
    let _lock = lock(paths)?;
    let layers = config::load_layers_at(paths)?;
    let mut editor = Editor {
        base: layers.base.clone(),
        overrides: layers.overrides.clone(),
        managed: layers.managed,
    };
    edit(&mut editor)?;

    let base_changed = editor.base != layers.base;
    let overrides_changed = editor.overrides != layers.overrides;
    if base_changed && layers.managed {
        return Err(format!(
            "{} is managed; refusing to write it",
            paths.config.display()
        ));
    }
    if overrides_changed {
        if !layers.overrides_allowed() {
            return Err(READ_ONLY.into());
        }
        if let Some(error) = &layers.overrides_error {
            return Err(format!("fix or remove the local overrides first: {error}"));
        }
    }
    config::validate(&editor.base)
        .map_err(|error| format!("refusing invalid configuration: {error}"))?;
    config::validate(&config::merge(&editor.base, &editor.overrides))
        .map_err(|error| format!("refusing invalid configuration: {error}"))?;
    if base_changed {
        reject_symlink(&paths.config)?;
        write_yaml(&paths.config, &editor.base)?;
    }
    if overrides_changed {
        reject_symlink(&paths.overrides)?;
        write_yaml(&paths.overrides, &editor.overrides)?;
    }
    config::load_layers_at(paths)
}

/// Replace `config.yaml` wholesale, as `--import` does.
pub fn replace(config: &Config) -> Result<(), String> {
    replace_at(&env_paths()?, config)
}

pub fn replace_at(paths: &Paths, config: &Config) -> Result<(), String> {
    let _lock = lock(paths)?;
    reject_symlink(&paths.config)?;
    config::validate(config).map_err(|error| format!("refusing invalid configuration: {error}"))?;
    write_yaml(&paths.config, config)
}

/// Create an empty overrides file so it can be opened in an editor.
pub fn ensure_overrides(paths: &Paths) -> Result<(), String> {
    let _lock = lock(paths)?;
    reject_symlink(&paths.overrides)?;
    if paths.overrides.exists() {
        return Ok(());
    }
    write_yaml(&paths.overrides, &Overrides::default())
}

struct Lock(fs::File);

/// One lock covers both files so a transaction sees a consistent pair.
fn lock(paths: &Paths) -> Result<Lock, String> {
    let parent = paths
        .config
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let lock_path = paths.config.with_extension("yaml.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|error| format!("{}: {error}", lock_path.display()))?;
    // SAFETY: flock only acts on this open file descriptor.
    if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX) } != 0 {
        return Err(format!("could not lock {}", lock_path.display()));
    }
    Ok(Lock(file))
}

impl Drop for Lock {
    fn drop(&mut self) {
        // SAFETY: the descriptor is still open until this guard drops.
        let _ = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&self.0), libc::LOCK_UN) };
    }
}

fn write_yaml(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = serde_saphyr::to_string(value).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_owned())?;
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file path", path.display()))?
        .to_string_lossy();
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
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
            "{} is a symlink owned by your configuration manager (such as Home Manager); change it there, or keep local changes in {}",
            path.display(),
            config::OVERRIDES_FILE
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuleAction;
    use crate::config::tests::TempDir;

    fn open_rule(domain: &str, profile: &str) -> Rule {
        Rule {
            domains: vec![domain.into()],
            action: RuleAction::Open,
            profile: Some(profile.into()),
            ..Rule::default()
        }
    }

    const MANAGED: &str = "version: 1\nappearance:\n  theme: dark\nrules:\n  - name: managed\n    domains: [example.com]\n    action: open\n    profile: chrome:Default\n";

    #[test]
    fn managed_edits_go_to_overrides() {
        let dir = TempDir::new("io-managed");
        let paths = dir.managed(MANAGED);
        let store = fs::read_to_string(&paths.config).unwrap();

        let layers = edit_at(&paths, |editor| {
            editor.add_rule(open_rule("work.example.com", "firefox:abc.default"))?;
            editor.set(Setting::Suspended(true))?;
            editor.set(Setting::Theme(Theme::Light))
        })
        .unwrap();

        assert!(fs::symlink_metadata(&paths.config).unwrap().is_symlink());
        assert_eq!(fs::read_to_string(&paths.config).unwrap(), store);
        let overrides =
            config::parse_overrides(&fs::read_to_string(&paths.overrides).unwrap()).unwrap();
        assert_eq!(overrides.rules.len(), 1);
        assert_eq!(overrides.routing.suspended, Some(true));
        assert_eq!(overrides.appearance.theme, Some(Theme::Light));
        assert_eq!(layers.effective.rules[0].domains, ["work.example.com"]);
        assert_eq!(layers.effective.rules[1].name.as_deref(), Some("managed"));
        assert_eq!(layers.effective.appearance.theme, Theme::Light);

        // Choosing the managed value again removes the override.
        let layers = edit_at(&paths, |editor| editor.set(Setting::Theme(Theme::Dark))).unwrap();
        assert!(layers.overrides.appearance.theme.is_none());
        assert!(layers.overrides.routing.suspended.is_some());

        let layers = edit_at(&paths, |editor| editor.reset(Field::Suspended)).unwrap();
        assert!(!layers.effective.routing.suspended);

        let managed_rule = layers.effective.rules[1].clone();
        let error = edit_at(&paths, |editor| {
            editor.remove_rule(RuleOrigin::Base, &managed_rule)
        })
        .unwrap_err();
        assert_eq!(error, MANAGED_RULE);
        let local_rule = layers.effective.rules[0].clone();
        let layers = edit_at(&paths, |editor| {
            editor.remove_rule(RuleOrigin::Local, &local_rule)
        })
        .unwrap();
        assert_eq!(layers.effective.rules.len(), 1);
        assert_eq!(layers.overrides, Overrides::default());
    }

    #[test]
    fn managed_hidden_and_order() {
        let dir = TempDir::new("io-hidden");
        let paths = dir.managed("version: 1\nbrowsers:\n  hidden: [opera]\n  order: [chrome]\n");
        let error = edit_at(&paths, |editor| editor.set_hidden("opera", None, false)).unwrap_err();
        assert!(
            error.contains("hidden by the managed configuration"),
            "{error}"
        );
        let layers = edit_at(&paths, |editor| {
            editor.set_hidden("brave:Default", Some("brave"), true)?;
            editor.set_order(vec!["firefox:x".into()])
        })
        .unwrap();
        assert_eq!(layers.effective.browsers.hidden, ["opera", "brave:Default"]);
        assert_eq!(layers.effective.browsers.order, ["firefox:x", "chrome"]);
    }

    #[test]
    fn unmanaged_edits_write_config_and_clear_shadowing_overrides() {
        let dir = TempDir::new("io-unmanaged");
        let paths = dir.paths();
        fs::write(&paths.config, "version: 1\n").unwrap();
        fs::write(&paths.overrides, "routing:\n  suspended: true\n").unwrap();
        let layers = edit_at(&paths, |editor| {
            editor.set(Setting::Suspended(false))?;
            editor.add_rule(open_rule("example.com", "chrome:Default"))
        })
        .unwrap();
        assert!(!layers.managed);
        assert!(!layers.effective.routing.suspended);
        let base = config::parse(&fs::read_to_string(&paths.config).unwrap()).unwrap();
        assert_eq!(base.rules.len(), 1);
        assert!(layers.overrides.is_empty());
    }

    #[test]
    fn unmanaged_without_overrides_creates_no_overrides_file() {
        let dir = TempDir::new("io-plain");
        let paths = dir.paths();
        edit_at(&paths, |editor| editor.set(Setting::Dim(false))).unwrap();
        assert!(paths.config.is_file());
        assert!(!paths.overrides.exists());
    }

    #[test]
    fn disallowed_overrides_are_read_only() {
        let dir = TempDir::new("io-readonly");
        let paths = dir.managed("version: 1\noverrides:\n  allowed: false\n");
        let error = edit_at(&paths, |editor| editor.set(Setting::Suspended(true))).unwrap_err();
        assert_eq!(error, READ_ONLY);
        let error = edit_at(&paths, |editor| {
            editor.add_rule(open_rule("example.com", "chrome:Default"))
        })
        .unwrap_err();
        assert_eq!(error, READ_ONLY);
        assert!(!paths.overrides.exists());
    }

    #[test]
    fn invalid_merged_overrides_are_refused() {
        let dir = TempDir::new("io-invalid");
        let paths = dir.managed("version: 1\n");
        let error = edit_at(&paths, |editor| {
            editor.set(Setting::Mode(RoutingMode::Always))
        })
        .unwrap_err();
        assert!(error.contains("default_profile is required"), "{error}");
        assert!(!paths.overrides.exists());
    }

    #[test]
    fn replace_refuses_a_managed_config() {
        let dir = TempDir::new("io-replace");
        let paths = dir.managed("version: 1\n");
        let error = replace_at(&paths, &Config::default()).unwrap_err();
        assert!(error.contains("is a symlink"), "{error}");
    }

    #[test]
    fn saved_rules_stay_after_narrower_exceptions() {
        let dir = TempDir::new("io-exceptions");
        let paths = dir.managed("version: 1\n");
        let layers = edit_at(&paths, |editor| {
            editor.add_rule(open_rule("mail.example.com", "a:Default"))?;
            editor.add_rule(open_rule("example.com", "b:Default"))
        })
        .unwrap();
        let domains: Vec<_> = layers
            .overrides
            .rules
            .iter()
            .map(|rule| rule.domains[0].as_str())
            .collect();
        assert_eq!(domains, ["mail.example.com", "example.com"]);
    }
}
