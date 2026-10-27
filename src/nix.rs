//! Render configuration as Nix in the Home Manager module's option syntax
//! (`programs.flipper.settings`), so local rules can be promoted into Nix.

use crate::config::{Overrides, RoutingMode, Rule, RuleAction, Theme};

/// A Nix string literal.
fn string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn list(values: &[String]) -> String {
    let items: Vec<_> = values.iter().map(|value| string(value)).collect();
    format!("[ {} ]", items.join(" "))
}

/// One element of `programs.flipper.settings.rules`.
pub fn rule(rule: &Rule, indent: usize) -> String {
    let pad = " ".repeat(indent + 2);
    let mut out = String::from("{\n");
    if let Some(name) = &rule.name {
        out.push_str(&format!("{pad}name = {};\n", string(name)));
    }
    for (key, values) in [
        ("domains", &rule.domains),
        ("paths", &rule.paths),
        ("patterns", &rule.patterns),
        ("extensions", &rule.extensions),
    ] {
        if !values.is_empty() {
            out.push_str(&format!("{pad}{key} = {};\n", list(values)));
        }
    }
    let action = match rule.action {
        RuleAction::Open => "open",
        RuleAction::Picker => "picker",
    };
    out.push_str(&format!("{pad}action = {};\n", string(action)));
    if let Some(profile) = &rule.profile {
        out.push_str(&format!("{pad}profile = {};\n", string(profile)));
    }
    out.push_str(&" ".repeat(indent));
    out.push('}');
    out
}

/// Local overrides as a `programs.flipper.settings` fragment. Rules are listed
/// in evaluation order and belong before the existing managed rules.
pub fn overrides(overrides: &Overrides) -> String {
    let mut out = String::from(
        "# Flipper local overrides in programs.flipper.settings syntax.\n# Put these rules before your existing settings.rules to keep their precedence.\n{\n",
    );
    let appearance = &overrides.appearance;
    let mut fields = Vec::new();
    if let Some(theme) = appearance.theme {
        let theme = match theme {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        };
        fields.push(format!("theme = {};", string(theme)));
    }
    if let Some(dim) = appearance.dim {
        fields.push(format!("dim = {dim};"));
    }
    if let Some(width) = appearance.width {
        fields.push(format!("width = {width};"));
    }
    section(&mut out, "appearance", &fields);

    let routing = &overrides.routing;
    let mut fields = Vec::new();
    if let Some(mode) = routing.mode {
        let mode = match mode {
            RoutingMode::Picker => "picker",
            RoutingMode::Always => "always",
        };
        fields.push(format!("mode = {};", string(mode)));
    }
    if let Some(profile) = &routing.default_profile {
        fields.push(format!("defaultProfile = {};", string(profile)));
    }
    if let Some(suspended) = routing.suspended {
        fields.push(format!("suspended = {suspended};"));
    }
    if let Some(unwrap) = routing.unwrap_redirects {
        fields.push(format!("unwrapRedirects = {unwrap};"));
    }
    section(&mut out, "routing", &fields);

    if !overrides.rules.is_empty() {
        out.push_str("  rules = [\n");
        for item in &overrides.rules {
            out.push_str(&format!("    {}\n", rule(item, 4)));
        }
        out.push_str("  ];\n");
    }

    let browsers = &overrides.browsers;
    let mut fields = Vec::new();
    if !browsers.hidden.is_empty() {
        fields.push(format!("hidden = {};", list(&browsers.hidden)));
    }
    if !browsers.order.is_empty() {
        fields.push(format!("order = {};", list(&browsers.order)));
    }
    section(&mut out, "browsers", &fields);

    if !overrides.custom_browsers.is_empty() {
        out.push_str("  customBrowsers = [\n");
        for browser in &overrides.custom_browsers {
            out.push_str("    {\n");
            out.push_str(&format!("      id = {};\n", string(&browser.id)));
            out.push_str(&format!("      name = {};\n", string(&browser.name)));
            if let Some(profile) = &browser.profile_name {
                out.push_str(&format!("      profileName = {};\n", string(profile)));
            }
            if let Some(email) = &browser.email {
                out.push_str(&format!("      email = {};\n", string(email)));
            }
            out.push_str(&format!(
                "      iconName = {};\n",
                string(&browser.icon_name)
            ));
            out.push_str(&format!("      command = {};\n", list(&browser.command)));
            out.push_str("    }\n");
        }
        out.push_str("  ];\n");
    }
    out.push_str("}\n");
    out
}

fn section(out: &mut String, name: &str, fields: &[String]) {
    if fields.is_empty() {
        return;
    }
    out.push_str(&format!("  {name} = {{\n"));
    for field in fields {
        out.push_str(&format!("    {field}\n"));
    }
    out.push_str("  };\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_overrides;

    #[test]
    fn strings_escape_nix_syntax() {
        assert_eq!(string(r#"a"b\c${d}$e"#), r#""a\"b\\c\${d}$e""#);
    }

    #[test]
    fn renders_rules_and_scalars() {
        let overrides = parse_overrides(
            "routing:\n  suspended: true\n  default_profile: chrome:Default\nrules:\n  - name: Work\n    domains: [example.com]\n    action: open\n    profile: \"firefox:abc.default#container-2\"\n",
        )
        .unwrap();
        let nix = super::overrides(&overrides);
        assert!(
            nix.contains("defaultProfile = \"chrome:Default\";"),
            "{nix}"
        );
        assert!(nix.contains("suspended = true;"), "{nix}");
        assert!(nix.contains("domains = [ \"example.com\" ];"), "{nix}");
        assert!(
            nix.contains("profile = \"firefox:abc.default#container-2\";"),
            "{nix}"
        );
        assert!(!nix.contains("appearance"), "{nix}");
    }
}
