//! GPUI picker and settings surface.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::base::Root;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::theme::{Theme as UiTheme, ThemeMode};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::browsers::BrowserProfile;
use crate::config::{self, Config, RoutingMode, Rule, RuleAction, Theme};
use crate::{config_io, launch, registration, urls::PreparedUrl};

actions!(
    flipper,
    [
        MoveDown,
        MoveUp,
        OpenSelected,
        Dismiss,
        SaveAlways,
        SelectOne,
        SelectTwo,
        SelectThree,
        SelectFour,
        SelectFive,
        SelectSix,
        SelectSeven,
        SelectEight,
        SelectNine,
        SelectZero
    ]
);

const ROW_HEIGHT: f32 = 68.;
const MAX_ROWS: usize = 7;

pub fn bind_keys(cx: &mut App) {
    let context = Some("Flipper > Input");
    cx.bind_keys([
        KeyBinding::new("down", MoveDown, context),
        KeyBinding::new("up", MoveUp, context),
        KeyBinding::new("enter", OpenSelected, context),
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("ctrl-enter", SaveAlways, context),
        KeyBinding::new("ctrl-1", SelectOne, context),
        KeyBinding::new("ctrl-2", SelectTwo, context),
        KeyBinding::new("ctrl-3", SelectThree, context),
        KeyBinding::new("ctrl-4", SelectFour, context),
        KeyBinding::new("ctrl-5", SelectFive, context),
        KeyBinding::new("ctrl-6", SelectSix, context),
        KeyBinding::new("ctrl-7", SelectSeven, context),
        KeyBinding::new("ctrl-8", SelectEight, context),
        KeyBinding::new("ctrl-9", SelectNine, context),
        KeyBinding::new("ctrl-0", SelectZero, context),
    ]);
}

pub fn open(
    config: Config,
    profiles: Vec<BrowserProfile>,
    url: Option<PreparedUrl>,
    warning: Option<String>,
    settings: bool,
) -> Result<(), String> {
    let appearance = config.appearance.clone();
    let error = Arc::new(Mutex::new(None));
    let error_from_app = Arc::clone(&error);
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
            match appearance.theme {
                Theme::System => UiTheme::sync_system_appearance(None, cx),
                Theme::Light => UiTheme::change(ThemeMode::Light, None, cx),
                Theme::Dark => UiTheme::change(ThemeMode::Dark, None, cx),
            }
            let link_selector_layer = url.is_some() && !settings && is_wayland_session();
            let options = window_options(&appearance, link_selector_layer);
            let config_layer = config.clone();
            let profiles_layer = profiles.clone();
            let url_layer = url.clone();
            let warning_layer = warning.clone();
            let settings_layer = settings;
            let opened = cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| {
                    PickerView::new(
                        config_layer,
                        profiles_layer,
                        url_layer,
                        warning_layer,
                        settings_layer,
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
            });
            if opened.is_err() && link_selector_layer {
                let fallback = window_options(&appearance, false);
                let fallback_opened = cx.open_window(fallback, move |window, cx| {
                    let view = cx.new(|cx| {
                        PickerView::new(config, profiles, url, warning, settings, window, cx)
                    });
                    cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
                });
                if let Err(fallback_error) = fallback_opened {
                    *error_from_app.lock().expect("UI error lock poisoned") = Some(format!(
                        "could not open layer-shell or fallback window: {fallback_error}"
                    ));
                    cx.quit();
                }
            } else if let Err(open_error) = opened {
                *error_from_app.lock().expect("UI error lock poisoned") =
                    Some(format!("could not open picker window: {open_error}"));
                cx.quit();
            }
        });
    match error.lock().expect("UI error lock poisoned").take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn window_options(appearance: &config::Appearance, layer_shell: bool) -> WindowOptions {
    let mut options = WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(appearance.width + 40.), px(820.)),
        })),
        app_id: Some("com.korora.Flipper".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    if layer_shell {
        options.kind = WindowKind::LayerShell(LayerShellOptions {
            namespace: "flipper".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        });
    }
    options
}

fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session == "wayland")
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

struct PickerView {
    config: Config,
    profiles: Vec<BrowserProfile>,
    prepared: Option<PreparedUrl>,
    input: Entity<InputState>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    query: String,
    selected: usize,
    settings: bool,
    status: Option<String>,
    warning: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl PickerView {
    fn new(
        config: Config,
        profiles: Vec<BrowserProfile>,
        prepared: Option<PreparedUrl>,
        warning: Option<String>,
        settings: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter profiles, or paste a link to open…")
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscriptions = vec![cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                this.selected = 0;
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        })];
        Self {
            config,
            profiles,
            prepared,
            input,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            query: String::new(),
            selected: 0,
            settings,
            status: warning.clone(),
            warning,
            _subscriptions: subscriptions,
        }
    }

    fn visible_profiles(&self) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        self.profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| !profile.hidden)
            .filter(|(_, profile)| {
                query.is_empty()
                    || profile.browser_name.to_lowercase().contains(&query)
                    || profile.profile_name.to_lowercase().contains(&query)
                    || profile
                        .email
                        .as_deref()
                        .is_some_and(|email| email.to_lowercase().contains(&query))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn open_at(&mut self, visible_index: usize, cx: &mut Context<Self>) {
        let profiles = self.visible_profiles();
        let Some(index) = profiles.get(visible_index).copied() else {
            return;
        };
        self.open_profile(index, cx);
    }

    fn open_profile(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(profile) = self.profiles.get(index) else {
            return;
        };
        let Some(url) = self.prepared.as_ref() else {
            self.status = Some("Paste a link first, then press Enter.".into());
            cx.notify();
            return;
        };
        match launch::launch(profile, url.effective.as_str()) {
            Ok(()) => cx.quit(),
            Err(error) => {
                self.status = Some(format!(
                    "Could not launch {}: {error}",
                    profile.browser_name
                ));
                cx.notify();
            }
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.query.trim().to_owned();
        if !text.is_empty() {
            match PreparedUrl::parse(&text, self.config.routing.unwrap_redirects) {
                Ok(prepared) => {
                    self.prepared = Some(prepared);
                    self.query.clear();
                    self.selected = 0;
                    self.status = None;
                    self.input
                        .update(cx, |input, cx| input.set_value(String::new(), window, cx));
                    cx.notify();
                    return;
                }
                Err(error) if self.prepared.is_none() => {
                    self.status = Some(error);
                    cx.notify();
                    return;
                }
                Err(_) => {}
            }
        }
        self.open_at(self.selected, cx);
    }

    fn save_rule(&mut self, index: usize, action: RuleAction, cx: &mut Context<Self>) {
        let Some(prepared) = self.prepared.as_ref() else {
            self.status = Some("Paste a link before creating a routing rule.".into());
            cx.notify();
            return;
        };
        let profile = self.profiles.get(index).cloned();
        let domain = prepared.effective.host_str().map(str::to_owned);
        let pattern_url = prepared.effective.to_string();
        if action == RuleAction::Open && profile.is_none() {
            return;
        }
        let rule = Rule {
            name: Some(match (&domain, &profile, action) {
                (Some(domain), Some(profile), RuleAction::Open) => {
                    format!(
                        "{domain} → {} ({})",
                        profile.browser_name, profile.profile_name
                    )
                }
                (Some(domain), _, _) => format!("Always ask for {domain}"),
                _ => "Open this URL".into(),
            }),
            domains: domain.clone().into_iter().collect(),
            paths: Vec::new(),
            patterns: if domain.is_none() {
                vec![pattern_url]
            } else {
                Vec::new()
            },
            extensions: Vec::new(),
            action,
            profile: if action == RuleAction::Open {
                profile.as_ref().map(|profile| profile.id.clone())
            } else {
                None
            },
        };
        match config_io::update(|config| {
            config.rules.retain(|existing| {
                existing.domains != rule.domains
                    || existing.paths != rule.paths
                    || existing.patterns != rule.patterns
                    || existing.extensions != rule.extensions
            });
            if rule.domains.is_empty() {
                config.rules.insert(0, rule.clone());
                return;
            }
            let mut exceptions = Vec::new();
            let mut remaining = Vec::new();
            for existing in std::mem::take(&mut config.rules) {
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
                let more_specific = condition_exception || narrower_domain;
                if more_specific {
                    exceptions.push(existing);
                } else {
                    remaining.push(existing);
                }
            }
            exceptions.push(rule.clone());
            exceptions.extend(remaining);
            config.rules = exceptions;
        }) {
            Ok(config) => {
                self.config = config;
                self.status = Some(if action == RuleAction::Open {
                    "Rule saved. Opening the link now.".into()
                } else {
                    "Picker exception saved for this domain.".into()
                });
                if action == RuleAction::Open {
                    self.open_profile(index, cx);
                    return;
                }
            }
            Err(error) => self.status = Some(format!("Could not save rule: {error}")),
        }
        cx.notify();
    }

    fn save_always(&mut self, _: &SaveAlways, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.visible_profiles().get(self.selected).copied() {
            self.save_rule(index, RuleAction::Open, cx);
        }
    }

    fn toggle_settings(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings = !self.settings;
        self.status = None;
        cx.notify();
    }

    fn set_suspended(&mut self, cx: &mut Context<Self>) {
        let desired = !self.config.routing.suspended;
        self.update_config(|config| config.routing.suspended = desired, cx);
    }

    fn set_default(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .is_some_and(|profile| profile.hidden)
        {
            self.status = Some("Unhide this profile before using it as the fallback.".into());
            cx.notify();
            return;
        }
        let id = id.to_owned();
        self.update_config(
            |config| {
                config.routing.mode = RoutingMode::Always;
                config.routing.default_profile = Some(id);
            },
            cx,
        );
    }

    fn move_profile(&mut self, id: &str, direction: isize, cx: &mut Context<Self>) {
        let id = id.to_owned();
        let known_profiles = self
            .profiles
            .iter()
            .map(|profile| (profile.id.clone(), profile.browser_id.clone()))
            .collect::<Vec<_>>();
        self.update_config(
            |config| {
                let mut order = Vec::new();
                for entry in &config.browsers.order {
                    let group_profiles = known_profiles
                        .iter()
                        .filter(|(_, browser_id)| browser_id == entry)
                        .map(|(profile_id, _)| profile_id);
                    let mut expanded = false;
                    for profile_id in group_profiles {
                        expanded = true;
                        if !order.contains(profile_id) {
                            order.push(profile_id.clone());
                        }
                    }
                    if !expanded && !order.contains(entry) {
                        order.push(entry.clone());
                    }
                }
                for (profile_id, _) in &known_profiles {
                    if !order.contains(profile_id) {
                        order.push(profile_id.clone());
                    }
                }
                config.browsers.order = order;
                if let Some(index) = config.browsers.order.iter().position(|item| item == &id) {
                    let target = (index as isize + direction)
                        .clamp(0, config.browsers.order.len().saturating_sub(1) as isize)
                        as usize;
                    config.browsers.order.swap(index, target);
                }
            },
            cx,
        );
    }

    fn toggle_hidden(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        let browser_id = self
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.browser_id.clone());
        self.update_config(
            |config| {
                let hidden = config.browsers.hidden.iter().any(|entry| {
                    entry == &id
                        || browser_id
                            .as_ref()
                            .is_some_and(|browser_id| entry == browser_id)
                });
                if hidden {
                    config
                        .browsers
                        .hidden
                        .retain(|entry| entry != &id && Some(entry) != browser_id.as_ref());
                } else {
                    config.browsers.hidden.push(id.clone());
                }
            },
            cx,
        );
    }

    fn remove_rule(&mut self, rule: Rule, cx: &mut Context<Self>) {
        self.update_config(
            |config| {
                if let Some(index) = config.rules.iter().position(|candidate| candidate == &rule) {
                    config.rules.remove(index);
                }
            },
            cx,
        );
    }

    fn update_config(&mut self, edit: impl FnOnce(&mut Config), cx: &mut Context<Self>) {
        match config_io::update(edit) {
            Ok(config) => {
                self.config = config;
                self.profiles = crate::browsers::discover(&self.config);
                self.status = Some("Configuration saved.".into());
            }
            Err(error) => self.status = Some(error),
        }
        cx.notify();
    }

    fn open_config(&mut self, cx: &mut Context<Self>) {
        let Some(path) = config::path() else {
            self.status = Some("Could not locate the Flipper configuration path.".into());
            cx.notify();
            return;
        };
        if !path.exists() {
            if let Err(error) = config_io::replace(&self.config) {
                self.status = Some(error);
                cx.notify();
                return;
            }
        }
        match std::process::Command::new("xdg-open").arg(&path).spawn() {
            Ok(_) => cx.quit(),
            Err(error) => {
                self.status = Some(format!("Could not open {}: {error}", path.display()));
                cx.notify();
            }
        }
    }

    fn register(&mut self, cx: &mut Context<Self>) {
        self.status = Some(match registration::register() {
            Ok(()) => "Flipper is now the default handler for web links.".into(),
            Err(error) => error,
        });
        cx.notify();
    }

    fn unregister(&mut self, cx: &mut Context<Self>) {
        self.status = Some(match registration::uninstall() {
            Ok(()) => "Restored previous web handlers.".into(),
            Err(error) => error,
        });
        cx.notify();
    }

    fn choose_safety_check(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(url) = self.prepared.as_ref() else {
            self.status = Some("Enter a URL before checking it.".into());
            cx.notify();
            return;
        };
        let Some(profile) = self.profiles.get(index) else {
            return;
        };
        let mut check =
            url::Url::parse("https://transparencyreport.google.com/safe-browsing/search")
                .expect("constant URL is valid");
        check
            .query_pairs_mut()
            .append_pair("url", url.effective.as_str());
        match launch::launch(profile, check.as_str()) {
            Ok(()) => cx.quit(),
            Err(error) => self.status = Some(format!("Could not open the report: {error}")),
        }
        cx.notify();
    }

    fn move_selection(&mut self, amount: isize) {
        let count = self.visible_profiles().len();
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = (self.selected as isize + amount).rem_euclid(count as isize) as usize;
            self.scroll
                .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        }
    }

    fn open_selected_action(
        &mut self,
        _: &OpenSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(window, cx);
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1);
        cx.notify();
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1);
        cx.notify();
    }

    fn select_number(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.visible_profiles().len() {
            self.open_at(index, cx);
        }
    }

    fn select_one(&mut self, _: &SelectOne, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(0, cx);
    }
    fn select_two(&mut self, _: &SelectTwo, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(1, cx);
    }
    fn select_three(&mut self, _: &SelectThree, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(2, cx);
    }
    fn select_four(&mut self, _: &SelectFour, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(3, cx);
    }
    fn select_five(&mut self, _: &SelectFive, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(4, cx);
    }
    fn select_six(&mut self, _: &SelectSix, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(5, cx);
    }
    fn select_seven(&mut self, _: &SelectSeven, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(6, cx);
    }
    fn select_eight(&mut self, _: &SelectEight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(7, cx);
    }
    fn select_nine(&mut self, _: &SelectNine, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(8, cx);
    }
    fn select_zero(&mut self, _: &SelectZero, _: &mut Window, cx: &mut Context<Self>) {
        self.select_number(9, cx);
    }

    fn profile_row(
        &self,
        profile_index: usize,
        visible_index: usize,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let profile = &self.profiles[profile_index];
        let selected = self.selected == visible_index;
        let icon = browser_icon(&profile.icon_name)
            .map(|path| img(path).size(px(36.)).into_any_element())
            .unwrap_or_else(|| {
                div()
                    .size(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(11.))
                    .bg(theme_color(dark, 0x26354f, 0xe8edf5))
                    .text_color(theme_color(dark, 0xe7edf9, 0x26354f))
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .child(
                        profile
                            .browser_name
                            .chars()
                            .next()
                            .unwrap_or('W')
                            .to_string(),
                    )
                    .into_any_element()
            });
        let mut row = h_flex()
            .id(format!("profile-{visible_index}"))
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_3()
            .gap_3()
            .items_center()
            .rounded(px(12.))
            .cursor_pointer()
            .when(selected, |row| {
                row.bg(theme_color(dark, 0x24344e, 0xe7efff))
            })
            .when(!selected, |row| {
                row.bg(theme_color(dark, 0x151b28, 0xf6f7f9))
            })
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                this.selected = visible_index;
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.open_profile(profile_index, cx)))
            .child(icon)
            .child(
                v_flex()
                    .flex_1()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_color(theme_color(dark, 0xf2f5fa, 0x202b38))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(profile.browser_name.clone()),
                            )
                            .child(
                                div()
                                    .text_color(theme_color(dark, 0xb6c1d2, 0x546174))
                                    .text_sm()
                                    .child(profile.profile_name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_color(theme_color(dark, 0x93a2b9, 0x667386))
                            .text_xs()
                            .child(profile.email.clone().unwrap_or_else(|| profile.id.clone())),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded(px(7.))
                    .bg(theme_color(dark, 0x202b3d, 0xe8edf4))
                    .text_color(theme_color(dark, 0xb8c7df, 0x48566a))
                    .text_xs()
                    .child(if visible_index < 9 {
                        format!("Ctrl+{}", visible_index + 1)
                    } else if visible_index == 9 {
                        "Ctrl+0".into()
                    } else {
                        "".into()
                    }),
            );

        if selected {
            row = row
                .border_1()
                .border_color(theme_color(dark, 0x7aa2f7, 0x4f7edc));
        }
        row.into_any_element()
    }

    fn settings_panel(&self, dark: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut body = v_flex().gap_3();
        body = body.child(
            h_flex()
                .gap_3()
                .items_center()
                .child(self.button(
                    "suspend-rules",
                    if self.config.routing.suspended {
                        "Resume rules"
                    } else {
                        "Suspend rules"
                    },
                    cx.listener(|this, _, _, cx| this.set_suspended(cx)),
                    dark,
                ))
                .child(self.button(
                    "fallback-mode",
                    if self.config.routing.mode == RoutingMode::Always {
                        "Fallback: Always"
                    } else {
                        "Fallback: Picker"
                    },
                    cx.listener(|this, _, _, cx| {
                        let mode = if this.config.routing.mode == RoutingMode::Picker {
                            RoutingMode::Always
                        } else {
                            RoutingMode::Picker
                        };
                        let fallback = if mode == RoutingMode::Always {
                            let Some(selector) = this.config.routing.default_profile.as_deref() else {
                                this.status = Some(
                                    "Choose a fallback profile before enabling automatic routing."
                                        .into(),
                                );
                                cx.notify();
                                return;
                            };
                            match crate::routing::resolve_selector(selector, &this.profiles) {
                                Ok(profile) if !profile.hidden => Some(selector.to_owned()),
                                Ok(_) => {
                                    this.status = Some(
                                        "The configured fallback profile is hidden; choose a visible fallback first."
                                            .into(),
                                    );
                                    cx.notify();
                                    return;
                                }
                                Err(reason) => {
                                    this.status = Some(format!(
                                        "The configured fallback cannot be resolved: {reason}. Choose a fallback profile first."
                                    ));
                                    cx.notify();
                                    return;
                                }
                            }
                        } else {
                            None
                        };
                        this.update_config(|config| {
                            config.routing.mode = mode;
                            if mode == RoutingMode::Always {
                                config.routing.default_profile = fallback;
                            }
                        }, cx);
                    }),
                    dark,
                ))
                .child(self.button(
                    "open-config",
                    "Open YAML config",
                    cx.listener(|this, _, _, cx| this.open_config(cx)),
                    dark,
                ))
                .child(self.button(
                    "register-default",
                    "Register as default",
                    cx.listener(|this, _, _, cx| this.register(cx)),
                    dark,
                ))
                .child(self.button(
                    "unregister-default",
                    "Restore previous browser",
                    cx.listener(|this, _, _, cx| this.unregister(cx)),
                    dark,
                )),
        );

        body = body.child(
            div()
                .text_color(theme_color(dark, 0x96a6be, 0x657386))
                .text_sm()
                .child(format!(
                    "Config: {}",
                    config::path().map_or_else(
                        || "~/.config/flipper/config.yaml".into(),
                        |path| path.display().to_string()
                    )
                )),
        );
        body = body.child(
            div()
                .text_color(theme_color(dark, 0xf1f5fa, 0x202b38))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Browser profiles"),
        );
        for profile in &self.profiles {
            let id = profile.id.clone();
            let is_hidden = profile.hidden;
            let id_for_button = id.clone();
            let id_for_up = id.clone();
            let id_for_down = id.clone();
            let email = profile.email.as_deref().unwrap_or(&profile.profile_name);
            body = body.child(
                h_flex()
                    .w_full()
                    .py_1()
                    .items_center()
                    .gap_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(format!("{} · {}", profile.browser_name, email))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme_color(dark, 0x8e9db3, 0x68768a))
                                    .child(id.clone()),
                            ),
                    )
                    .child(self.button(
                        format!("move-up-{id_for_up}"),
                        "↑",
                        cx.listener(move |this, _, _, cx| this.move_profile(&id_for_up, -1, cx)),
                        dark,
                    ))
                    .child(self.button(
                        format!("move-down-{id_for_down}"),
                        "↓",
                        cx.listener(move |this, _, _, cx| this.move_profile(&id_for_down, 1, cx)),
                        dark,
                    ))
                    .child(self.button(
                        format!("toggle-hidden-{id_for_button}"),
                        if is_hidden { "Show" } else { "Hide" },
                        cx.listener(move |this, _, _, cx| this.toggle_hidden(&id_for_button, cx)),
                        dark,
                    ))
                    .child(self.button(
                        format!("use-fallback-{id}"),
                        "Use as fallback",
                        cx.listener(move |this, _, _, cx| this.set_default(&id, cx)),
                        dark,
                    )),
            );
        }
        body = body.child(
            div()
                .mt_2()
                .text_color(theme_color(dark, 0xf1f5fa, 0x202b38))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Routing rules"),
        );
        if self.config.rules.is_empty() {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(theme_color(dark, 0x8e9db3, 0x68768a))
                    .child("No rules yet. Use Always from the picker to add one."),
            );
        }
        for (index, rule) in self.config.rules.iter().enumerate() {
            let rule_for_remove = rule.clone();
            let label = rule.name.clone().unwrap_or_else(|| {
                let patterns = rule
                    .domains
                    .iter()
                    .chain(&rule.paths)
                    .chain(&rule.patterns)
                    .chain(&rule.extensions)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{}: {patterns}",
                    if rule.action == RuleAction::Picker {
                        "Picker"
                    } else {
                        "Open"
                    }
                )
            });
            body = body.child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(theme_color(dark, 0xc6d0df, 0x445267))
                            .child(label),
                    )
                    .child(self.button(
                        format!("remove-rule-{index}"),
                        "Remove",
                        cx.listener(move |this, _, _, cx| {
                            this.remove_rule(rule_for_remove.clone(), cx)
                        }),
                        dark,
                    )),
            );
        }
        div()
            .id("settings-scroll")
            .max_h(px(560.))
            .min_h(px(0.))
            .overflow_y_scrollbar()
            .child(body)
            .into_any_element()
    }

    fn button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        dark: bool,
    ) -> Stateful<Div> {
        let label = label.into();
        div()
            .id(id.into())
            .px_3()
            .py_2()
            .rounded(px(8.))
            .bg(theme_color(dark, 0x24334a, 0xe8edf4))
            .text_color(theme_color(dark, 0xdce6f5, 0x273449))
            .text_sm()
            .cursor_pointer()
            .on_click(listener)
            .child(label)
    }
}

impl Focusable for PickerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PickerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let dark = theme.is_dark();
        let visible = self.visible_profiles();
        if self.selected >= visible.len() {
            self.selected = visible.len().saturating_sub(1);
        }
        let title = self
            .prepared
            .as_ref()
            .map(|url| url.destination.clone())
            .unwrap_or_else(|| "Choose where your link opens".into());
        let subtitle = self.prepared.as_ref().map(|url| {
            if url.original != url.effective {
                format!(
                    "{}  ← unwrapped from {}",
                    url.effective,
                    url.original.host_str().unwrap_or("redirect")
                )
            } else {
                url.effective.as_str().to_owned()
            }
        });
        let doc_label = self.prepared.as_ref().and_then(|url| url.document_type);
        let list = if visible.is_empty() {
            div()
                .w_full()
                .py_8()
                .text_center()
                .text_color(theme_color(dark, 0xa1aec2, 0x657386))
                .child("No visible browser profiles. Add a custom browser in YAML or unhide one in Settings.")
                .into_any_element()
        } else {
            let rows = visible.clone();
            uniform_list(
                "profiles",
                visible.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|visible_index| {
                            this.profile_row(rows[visible_index], visible_index, dark, cx)
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.scroll)
            .h(px(ROW_HEIGHT * MAX_ROWS as f32 + 10.))
            .into_any_element()
        };

        let mut panel = v_flex()
            .w(px(self.config.appearance.width))
            .max_h(px(900.))
            .gap_3()
            .p_5()
            .rounded(px(18.))
            .border_1()
            .border_color(theme_color(dark, 0x344156, 0xd8dee9))
            .bg(theme_color(dark, 0x10151f, 0xffffff))
            .shadow_lg();

        panel = panel.child(
            h_flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .size(px(42.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(13.))
                        .bg(theme_color(dark, 0x334c74, 0xdce8fb))
                        .text_color(theme_color(dark, 0xeaf1fd, 0x2a4e83))
                        .text_size(px(20.))
                        .child("⇄"),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap_0p5()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme_color(dark, 0xf1f5fb, 0x202b38))
                                .child("Flipper"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme_color(dark, 0x8e9db3, 0x68768a))
                                .child("BROWSER ROUTER"),
                        ),
                )
                .child(self.button(
                    "settings-toggle",
                    "Settings",
                    cx.listener(Self::toggle_settings),
                    dark,
                ))
                .child(self.button(
                    "close-picker",
                    "Close",
                    cx.listener(|_, _, _, cx| cx.quit()),
                    dark,
                )),
        );
        panel = panel.child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme_color(dark, 0xf3f6fb, 0x182333))
                        .child(title),
                )
                .when_some(subtitle, |this, subtitle| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(theme_color(dark, 0x91a0b5, 0x637185))
                            .child(subtitle),
                    )
                })
                .when_some(doc_label, |this, label| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(theme_color(dark, 0xffc77d, 0x9b5a00))
                            .child(format!("Office file · {label}")),
                    )
                }),
        );
        panel = panel.child(
            h_flex()
                .w_full()
                .h(px(50.))
                .px_3()
                .gap_2()
                .items_center()
                .rounded(px(10.))
                .border_1()
                .border_color(theme_color(dark, 0x303b4d, 0xd7dde6))
                .bg(theme_color(dark, 0x171e2a, 0xf3f5f8))
                .child(
                    div()
                        .text_color(theme_color(dark, 0x8392a9, 0x778396))
                        .child("⌕"),
                )
                .child(
                    div().flex_1().child(
                        Input::new(&self.input)
                            .appearance(false)
                            .h(px(46.))
                            .p_0()
                            .text_size(px(15.)),
                    ),
                ),
        );

        if self.settings {
            panel = panel.child(self.settings_panel(dark, cx));
        } else {
            panel = panel.child(list);
            let selected_profile = visible
                .get(self.selected)
                .and_then(|index| self.profiles.get(*index));
            let mut actions = h_flex().gap_2().items_center();
            if let Some(profile) = selected_profile {
                let index = visible[self.selected];
                let id = profile.id.clone();
                actions = actions
                    .child(self.button(
                        "always-rule",
                        "Always",
                        cx.listener(move |this, _, _, cx| {
                            this.save_rule(index, RuleAction::Open, cx)
                        }),
                        dark,
                    ))
                    .child(self.button(
                        "picker-rule",
                        "Always ask",
                        cx.listener(move |this, _, _, cx| {
                            this.save_rule(index, RuleAction::Picker, cx)
                        }),
                        dark,
                    ))
                    .child(self.button(
                        "safety-check",
                        "Safety check",
                        cx.listener(move |this, _, _, cx| {
                            if let Some(profile_index) =
                                this.profiles.iter().position(|profile| profile.id == id)
                            {
                                this.choose_safety_check(profile_index, cx);
                            }
                        }),
                        dark,
                    ));
            }
            panel = panel.child(actions);
        }

        if let Some(status) = self.status.as_deref() {
            panel = panel.child(
                div()
                    .text_sm()
                    .text_color(theme_color(dark, 0xf0bd83, 0x855300))
                    .child(status.to_owned()),
            );
        }
        if let Some(warning) = self.warning.as_deref() {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(theme_color(dark, 0xc98d74, 0xa54c35))
                    .child(format!("Config warning: {warning}")),
            );
        }
        let footer = if self.prepared.is_some() {
            "↑↓ select   Enter open once   Ctrl+Enter Always   Ctrl+1–0 open directly   Esc close"
        } else {
            "Paste a URL and press Enter, then choose a profile; configure rules and profiles in YAML"
        };
        panel = panel.child(
            div()
                .pt_1()
                .text_xs()
                .text_color(theme_color(dark, 0x8190a6, 0x778396))
                .child(footer),
        );

        let dim = if self.config.appearance.dim {
            if dark { 0x080d16d9 } else { 0x17223588 }
        } else {
            0x00000000
        };
        div()
            .size_full()
            .key_context("Flipper")
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::open_selected_action))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::save_always))
            .on_action(cx.listener(Self::select_one))
            .on_action(cx.listener(Self::select_two))
            .on_action(cx.listener(Self::select_three))
            .on_action(cx.listener(Self::select_four))
            .on_action(cx.listener(Self::select_five))
            .on_action(cx.listener(Self::select_six))
            .on_action(cx.listener(Self::select_seven))
            .on_action(cx.listener(Self::select_eight))
            .on_action(cx.listener(Self::select_nine))
            .on_action(cx.listener(Self::select_zero))
            .track_focus(&self.focus)
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(dim))
            .child(panel)
    }
}

fn browser_icon(name: &str) -> Option<PathBuf> {
    let name = name
        .strip_suffix(".png")
        .or_else(|| name.strip_suffix(".svg"))
        .unwrap_or(name);
    freedesktop_icons::lookup(name)
        .with_size(48)
        .with_scale(1)
        .find()
        .or_else(|| {
            freedesktop_icons::lookup("web-browser")
                .with_size(48)
                .find()
        })
}

fn theme_color(dark: bool, dark_hex: u32, light_hex: u32) -> Rgba {
    rgb(if dark { dark_hex } else { light_hex })
}
