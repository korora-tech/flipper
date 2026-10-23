//! Ordered, deterministic routing for prepared URLs.

use crate::browsers::BrowserProfile;
use crate::config::{Config, RoutingMode, Rule, RuleAction};
use crate::urls::PreparedUrl;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Pick {
        reason: String,
        rule: Option<String>,
    },
    Open {
        profile_id: String,
        rule: Option<String>,
    },
}

pub fn decide(config: &Config, profiles: &[BrowserProfile], url: &PreparedUrl) -> Decision {
    if !config.routing.suspended {
        for rule in &config.rules {
            if rule_matches(rule, &url.effective) {
                let rule_name = rule.name.clone();
                return match rule.action {
                    RuleAction::Picker => Decision::Pick {
                        reason: rule_name.clone().unwrap_or_else(|| "picker rule".into()),
                        rule: rule_name,
                    },
                    RuleAction::Open => match resolve_selector(
                        rule.profile.as_deref().unwrap_or_default(),
                        profiles,
                    ) {
                        Ok(profile) if !profile.hidden => Decision::Open {
                            profile_id: profile.id.clone(),
                            rule: rule_name,
                        },
                        Ok(_) => Decision::Pick {
                            reason: "the rule's profile is hidden".into(),
                            rule: rule_name,
                        },
                        Err(reason) => Decision::Pick {
                            reason,
                            rule: rule_name,
                        },
                    },
                };
            }
        }
        if config.routing.mode == RoutingMode::Always {
            if let Some(selector) = &config.routing.default_profile {
                return match resolve_selector(selector, profiles) {
                    Ok(profile) if !profile.hidden => Decision::Open {
                        profile_id: profile.id.clone(),
                        rule: None,
                    },
                    Ok(_) => Decision::Pick {
                        reason: "the fallback profile is hidden".into(),
                        rule: None,
                    },
                    Err(reason) => Decision::Pick { reason, rule: None },
                };
            }
        }
    }
    Decision::Pick {
        reason: if config.routing.suspended {
            "automatic rules are suspended".into()
        } else {
            "choose a browser".into()
        },
        rule: None,
    }
}

pub fn resolve_selector<'a>(
    selector: &str,
    profiles: &'a [BrowserProfile],
) -> Result<&'a BrowserProfile, String> {
    // Explicit stable IDs may contain '@' for alternate data roots; check them
    // before interpreting a bare selector as an email address.
    if let Some(profile) = profiles.iter().find(|profile| profile.id == selector) {
        return Ok(profile);
    }
    if let Some(email) = selector
        .strip_prefix("email:")
        .or_else(|| selector.contains('@').then_some(selector))
    {
        let email = email.trim();
        let matches: Vec<_> = profiles
            .iter()
            .filter(|profile| {
                profile
                    .email
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(email))
            })
            .collect();
        return match matches.as_slice() {
            [profile] => Ok(*profile),
            [] => Err(format!("no browser profile has the email {email:?}")),
            _ => Err(format!(
                "email {email:?} belongs to multiple profiles; use a stable profile ID"
            )),
        };
    }
    Err(format!("browser profile {selector:?} is not available"))
}

fn rule_matches(rule: &Rule, url: &url::Url) -> bool {
    let conditions_present = !rule.domains.is_empty()
        || !rule.paths.is_empty()
        || !rule.patterns.is_empty()
        || !rule.extensions.is_empty();
    if !conditions_present {
        return false;
    }

    let host = url.host_str().unwrap_or_default();
    let domain_match = rule.domains.is_empty()
        || rule
            .domains
            .iter()
            .any(|pattern| domain_matches(pattern, host));
    let path_match = rule.paths.is_empty()
        || rule
            .paths
            .iter()
            .any(|pattern| path_matches(pattern, url.path()));
    let whole_url = url.as_str();
    let pattern_match = rule.patterns.is_empty()
        || rule.patterns.iter().any(|pattern| {
            if let Some(extension) = pattern.strip_prefix("ext:") {
                url.path()
                    .rsplit_once('.')
                    .is_some_and(|(_, path_extension)| {
                        path_extension.eq_ignore_ascii_case(extension)
                    })
            } else {
                glob_matches(pattern, whole_url)
            }
        });
    let extension = url
        .path()
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    let extension_match = rule.extensions.is_empty()
        || extension.is_some_and(|extension| {
            rule.extensions.iter().any(|candidate| {
                candidate
                    .trim()
                    .strip_prefix("ext:")
                    .unwrap_or(candidate.trim())
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
            })
        });
    domain_match && path_match && pattern_match && extension_match
}

fn domain_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Some(domain) = pattern.strip_prefix("*.") {
        return host != domain && host.ends_with(&format!(".{domain}"));
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

fn path_matches(pattern: &str, path: &str) -> bool {
    if pattern.contains('*') {
        return glob_matches(pattern, path);
    }
    path == pattern
        || (path.starts_with(pattern)
            && (pattern.ends_with('/') || path.as_bytes().get(pattern.len()) == Some(&b'/')))
}

/// A small wildcard matcher; `*` spans any bytes and all other bytes match
/// literally. It is sufficient for domain/path patterns without
/// exposing the user configuration to regex execution cost.
fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p, mut v, mut star, mut checkpoint) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && pattern[p] == value[v] {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            checkpoint = v;
        } else if let Some(star_at) = star {
            p = star_at + 1;
            checkpoint += 1;
            v = checkpoint;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}
