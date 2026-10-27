//! Settings window: a normal application window, separate from the
//! layer-shell link selector.
//!
//! When the configuration is managed by Home Manager, edits are saved as
//! local overrides; managed rules are shown but cannot be changed here.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::browsers::BrowserProfile;
use crate::config::{
    self, Config, Field, Layers, RoutingMode, Rule, RuleAction, RuleOrigin, Theme,
};
use crate::config_io::{self, Editor, Setting};
use crate::{app_path, launch, registration, routing, urls::PreparedUrl};

pub fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Flipper Settings".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::centered(size(px(980.), px(700.)), cx)),
        window_min_size: Some(size(px(640.), px(480.))),
        app_id: Some("com.korora.Flipper".into()),
        ..Default::default()
    }
}

pub struct SettingsView {
    layers: Layers,
    profiles: Vec<BrowserProfile>,
    link: Entity<InputState>,
    status: Option<String>,
    warning: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(
        layers: Layers,
        profiles: Vec<BrowserProfile>,
        warning: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let link = cx.new(|cx| InputState::new(window, cx).placeholder("https://…"));
        let subscriptions = vec![cx.subscribe_in(
            &link,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let text = input.read(cx).value().trim().to_owned();
                    if this.open_link(&text, cx) {
                        input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
                    }
                }
            },
        )];
        Self {
            layers,
            profiles,
            link,
            status: None,
            warning,
            _subscriptions: subscriptions,
        }
    }

    /// The merged configuration that routing uses.
    fn config(&self) -> &Config {
        &self.layers.effective
    }

    fn apply(
        &mut self,
        edit: impl FnOnce(&mut Editor) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        match config_io::edit(edit) {
            Ok(layers) => {
                self.status = Some(if layers.managed {
                    "Saved as a local override.".into()
                } else {
                    "Configuration saved.".into()
                });
                self.warning = layers.overrides_error.clone();
                self.layers = layers;
                self.profiles = crate::browsers::discover(&self.layers.effective);
            }
            Err(error) => self.status = Some(error),
        }
        crate::ui::apply_theme(self.config().appearance.theme, cx);
        cx.notify();
    }

    fn set(&mut self, setting: Setting, cx: &mut Context<Self>) {
        self.apply(|editor| editor.set(setting), cx);
    }

    fn reset(&mut self, field: Field, cx: &mut Context<Self>) {
        self.apply(|editor| editor.reset(field), cx);
    }

    fn clear_overrides(&mut self, cx: &mut Context<Self>) {
        self.apply(|editor| editor.clear_overrides(), cx);
    }

    /// Local overrides shadow a managed value.
    fn overridden(&self, field: Field) -> bool {
        self.layers.managed && self.layers.overrides.has(field)
    }

    fn describe(&self, field: Field, text: &str) -> String {
        if self.overridden(field) {
            format!("{text} Local override; reset to use the Home Manager value.")
        } else {
            text.to_owned()
        }
    }

    fn set_status(&mut self, status: impl Into<String>, cx: &mut Context<Self>) {
        self.status = Some(status.into());
        cx.notify();
    }

    /// Hand the link to a fresh Flipper process so it is routed exactly as a
    /// link from another application would be.
    fn open_link(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        if text.is_empty() {
            return false;
        }
        if let Err(error) = PreparedUrl::parse(text, self.config().routing.unwrap_redirects) {
            self.set_status(error, cx);
            return false;
        }
        let result = app_path::invoked().and_then(|exe| {
            launch::launch_argv(&[exe.to_string_lossy().into_owned(), text.to_owned()])
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(()) => {
                self.set_status(format!("Opening {text}"), cx);
                true
            }
            Err(error) => {
                self.set_status(format!("Could not open the link: {error}"), cx);
                false
            }
        }
    }

    fn set_fallback_mode(&mut self, mode: RoutingMode, cx: &mut Context<Self>) {
        if mode == RoutingMode::Always {
            let Some(selector) = self.config().routing.default_profile.as_deref() else {
                self.set_status(
                    "Choose a fallback profile before enabling automatic routing.",
                    cx,
                );
                return;
            };
            match routing::resolve_selector(selector, &self.profiles) {
                Ok(profile) if !profile.hidden => {}
                Ok(_) => {
                    self.set_status(
                        "The fallback profile is hidden; choose a visible one first.",
                        cx,
                    );
                    return;
                }
                Err(reason) => {
                    self.set_status(
                        format!("The fallback profile cannot be resolved: {reason}."),
                        cx,
                    );
                    return;
                }
            }
        }
        self.set(Setting::Mode(mode), cx);
    }

    fn set_fallback_profile(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .is_some_and(|profile| profile.hidden)
        {
            self.set_status("Show this profile before using it as the fallback.", cx);
            return;
        }
        self.set(Setting::DefaultProfile(id.to_owned()), cx);
    }

    fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.set(Setting::Theme(theme), cx);
    }

    fn move_profile(&mut self, id: &str, direction: isize, cx: &mut Context<Self>) {
        let id = id.to_owned();
        let known_profiles = self
            .profiles
            .iter()
            .map(|profile| (profile.id.clone(), profile.browser_id.clone()))
            .collect::<Vec<_>>();
        let mut order = Vec::new();
        for entry in &self.config().browsers.order {
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
        if let Some(index) = order.iter().position(|item| item == &id) {
            let target = (index as isize + direction)
                .clamp(0, order.len().saturating_sub(1) as isize) as usize;
            order.swap(index, target);
        }
        self.apply(|editor| editor.set_order(order), cx);
    }

    fn set_hidden(&mut self, id: &str, hidden: bool, cx: &mut Context<Self>) {
        let id = id.to_owned();
        let browser_id = self
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.browser_id.clone());
        self.apply(
            |editor| editor.set_hidden(&id, browser_id.as_deref(), hidden),
            cx,
        );
    }

    fn remove_rule(&mut self, origin: RuleOrigin, rule: Rule, cx: &mut Context<Self>) {
        self.apply(|editor| editor.remove_rule(origin, &rule), cx);
    }

    /// Open the file the user owns: `overrides.yaml` when managed, unless
    /// overrides are disabled, in which case the managed file is shown.
    fn open_config(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = config::Paths::from_env() else {
            self.set_status("Could not locate the Flipper configuration path.", cx);
            return;
        };
        let path = if self.layers.managed {
            if self.layers.overrides_allowed() {
                if let Err(error) = config_io::ensure_overrides(&paths) {
                    self.set_status(error, cx);
                    return;
                }
                paths.overrides
            } else {
                paths.config
            }
        } else {
            if !paths.config.exists()
                && let Err(error) = config_io::replace(&self.layers.base)
            {
                self.set_status(error, cx);
                return;
            }
            paths.config
        };
        let argv = ["xdg-open".to_owned(), path.to_string_lossy().into_owned()];
        if let Err(error) = launch::launch_argv(&argv) {
            self.set_status(format!("Could not open {}: {error}", path.display()), cx);
        }
    }

    fn register(&mut self, cx: &mut Context<Self>) {
        let status = match registration::register() {
            Ok(()) => "Flipper is now the default handler for web links.".into(),
            Err(error) => error,
        };
        self.set_status(status, cx);
    }

    fn unregister(&mut self, cx: &mut Context<Self>) {
        let status = match registration::uninstall() {
            Ok(()) => "Restored previous web handlers.".into(),
            Err(error) => error,
        };
        self.set_status(status, cx);
    }

    /// Show a page reset button for overridden fields of a managed config.
    fn resettable(&self) -> bool {
        self.layers.managed && self.layers.overrides_allowed()
    }

    /// Attach "reset to managed" behaviour to a field when it is overridden.
    fn with_reset<T>(
        &self,
        field: SettingField<T>,
        key: Field,
        cx: &mut Context<Self>,
    ) -> SettingField<T> {
        if !self.overridden(key) || !self.layers.overrides_allowed() {
            return field;
        }
        let this = cx.entity();
        field.on_reset(
            |_| true,
            move |_, cx| this.update(cx, |this, cx| this.reset(key, cx)),
        )
    }

    fn general_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let link = self.link.clone();
        let routing = &self.config().routing;
        let read_only = self.layers.read_only();
        let managed = self.layers.managed;

        let profile_options = self
            .profiles
            .iter()
            .filter(|profile| !profile.hidden)
            .map(|profile| (profile.id.clone().into(), profile_label(profile).into()))
            .collect::<Vec<(SharedString, SharedString)>>();
        let fallback_id: SharedString = routing
            .default_profile
            .as_deref()
            .map(|selector| {
                routing::resolve_selector(selector, &self.profiles)
                    .map_or_else(|_| selector.to_owned(), |profile| profile.id.clone())
            })
            .unwrap_or_default()
            .into();
        let mode: SharedString = match routing.mode {
            RoutingMode::Picker => "picker".into(),
            RoutingMode::Always => "always".into(),
        };
        let suspended = routing.suspended;
        let unwrap = routing.unwrap_redirects;

        let mode_field = self.with_reset(
            SettingField::dropdown(
                vec![
                    ("picker".into(), "Show the picker".into()),
                    ("always".into(), "Open the fallback profile".into()),
                ],
                move |_| mode.clone(),
                {
                    let this = this.clone();
                    move |value, cx| {
                        let mode = if value == "always" {
                            RoutingMode::Always
                        } else {
                            RoutingMode::Picker
                        };
                        this.update(cx, |this, cx| this.set_fallback_mode(mode, cx));
                    }
                },
            ),
            Field::Mode,
            cx,
        );
        let fallback_field = self.with_reset(
            SettingField::scrollable_dropdown(profile_options, move |_| fallback_id.clone(), {
                let this = this.clone();
                move |value, cx| {
                    this.update(cx, |this, cx| this.set_fallback_profile(&value, cx));
                }
            }),
            Field::DefaultProfile,
            cx,
        );
        let suspended_field = self.with_reset(
            SettingField::switch(move |_| suspended, {
                let this = this.clone();
                move |value, cx| {
                    this.update(cx, |this, cx| this.set(Setting::Suspended(value), cx));
                }
            }),
            Field::Suspended,
            cx,
        );
        let unwrap_field = self.with_reset(
            SettingField::switch(move |_| unwrap, {
                let this = this.clone();
                move |value, cx| {
                    this.update(cx, |this, cx| this.set(Setting::UnwrapRedirects(value), cx));
                }
            }),
            Field::UnwrapRedirects,
            cx,
        );

        SettingPage::new("General")
            .icon(IconName::Settings)
            .resettable(self.resettable())
            .group(
                SettingGroup::new().title("Open a link").item(
                    SettingItem::new(
                        "Link",
                        SettingField::render(move |_, _, _| {
                            div().w(px(320.)).child(Input::new(&link).cleanable(true))
                        }),
                    )
                    .description("Paste a URL and press Enter to route it through your rules."),
                ),
            )
            .group(
                SettingGroup::new()
                    .title("Routing")
                    .item(
                        SettingItem::new("When no rule matches", mode_field)
                            .description(self.describe(Field::Mode, "Rules always run first."))
                            .disabled(read_only),
                    )
                    .item(
                        SettingItem::new("Fallback profile", fallback_field)
                            .description(self.describe(
                                Field::DefaultProfile,
                                "Used when no rule matches and the picker is off.",
                            ))
                            .disabled(read_only),
                    )
                    .item(
                        SettingItem::new("Pause rules", suspended_field)
                            .description(self.describe(
                                Field::Suspended,
                                "Show the picker for every link without deleting rules.",
                            ))
                            .disabled(read_only),
                    )
                    .item(
                        SettingItem::new("Unwrap redirects", unwrap_field)
                            .description(self.describe(
                                Field::UnwrapRedirects,
                                "Resolve Microsoft SafeLinks and Google redirects before routing.",
                            ))
                            .disabled(read_only),
                    ),
            )
            .group(
                SettingGroup::new().title("Default browser").item(
                    SettingItem::new(
                        "Web links",
                        SettingField::render({
                            let this = this.clone();
                            move |_, _, _| {
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("register-default")
                                            .primary()
                                            .small()
                                            .label("Make Flipper default")
                                            .disabled(managed)
                                            .on_click({
                                                let this = this.clone();
                                                move |_, _, cx| {
                                                    this.update(cx, |this, cx| this.register(cx))
                                                }
                                            }),
                                    )
                                    .child(
                                        Button::new("unregister-default")
                                            .small()
                                            .label("Restore previous")
                                            .disabled(managed)
                                            .on_click({
                                                let this = this.clone();
                                                move |_, _, cx| {
                                                    this.update(cx, |this, cx| this.unregister(cx))
                                                }
                                            }),
                                    )
                            }
                        }),
                    )
                    .description(if managed {
                        "Managed by Home Manager: set programs.flipper.defaultBrowser."
                    } else {
                        "Send http and https links from other apps through Flipper."
                    }),
                ),
            )
    }

    fn appearance_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let theme: SharedString = match self.config().appearance.theme {
            Theme::System => "system".into(),
            Theme::Light => "light".into(),
            Theme::Dark => "dark".into(),
        };
        let dim = self.config().appearance.dim;
        let read_only = self.layers.read_only();
        let theme_field = self.with_reset(
            SettingField::dropdown(
                vec![
                    ("system".into(), "System".into()),
                    ("light".into(), "Light".into()),
                    ("dark".into(), "Dark".into()),
                ],
                move |_| theme.clone(),
                {
                    let this = this.clone();
                    move |value, cx| {
                        let theme = match value.as_ref() {
                            "light" => Theme::Light,
                            "dark" => Theme::Dark,
                            _ => Theme::System,
                        };
                        this.update(cx, |this, cx| this.set_theme(theme, cx));
                    }
                },
            ),
            Field::Theme,
            cx,
        );
        let dim_field = self.with_reset(
            SettingField::switch(move |_| dim, {
                let this = this.clone();
                move |value, cx| {
                    this.update(cx, |this, cx| this.set(Setting::Dim(value), cx));
                }
            }),
            Field::Dim,
            cx,
        );
        SettingPage::new("Appearance")
            .icon(IconName::Palette)
            .resettable(self.resettable())
            .group(
                SettingGroup::new()
                    .item(
                        SettingItem::new("Theme", theme_field)
                            .description(
                                self.describe(Field::Theme, "Light, dark or the system theme."),
                            )
                            .disabled(read_only),
                    )
                    .item(
                        SettingItem::new("Dim the screen", dim_field)
                            .description(self.describe(
                                Field::Dim,
                                "Darken everything behind the link selector.",
                            ))
                            .disabled(read_only),
                    ),
            )
    }

    fn browsers_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let fallback = self
            .config()
            .routing
            .default_profile
            .as_deref()
            .and_then(|selector| routing::resolve_selector(selector, &self.profiles).ok())
            .map(|profile| profile.id.clone());
        let read_only = self.layers.read_only();
        let last = self.profiles.len().saturating_sub(1);
        let mut group = SettingGroup::new()
            .title("Profiles")
            .description("Order and visibility in the link selector.")
            .footer({
                let managed = self.layers.managed;
                move |_, _| {
                    if managed {
                        "Add custom browsers with programs.flipper.settings.customBrowsers in Home Manager."
                    } else {
                        "Add custom browsers under custom_browsers in the YAML configuration."
                    }
                }
            });
        if self.profiles.is_empty() {
            group = group.item(SettingItem::render(|_, _, cx| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No browsers were detected.")
            }));
        }
        for (index, profile) in self.profiles.iter().enumerate() {
            let id = profile.id.clone();
            let visible = !profile.hidden;
            let is_fallback = fallback.as_deref() == Some(id.as_str());
            // Hidden by the managed base: overrides can only add hidden IDs.
            let locked = read_only
                || (self.layers.managed
                    && self
                        .layers
                        .base
                        .browsers
                        .hidden
                        .iter()
                        .any(|entry| entry == &profile.id || entry == &profile.browser_id));
            let this = this.clone();
            group = group.item(
                SettingItem::new(
                    profile_label(profile),
                    SettingField::render(move |_, _, _| {
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when(is_fallback, |row| {
                                row.child(Tag::info().small().child("Fallback"))
                            })
                            .child(
                                Button::new(SharedString::from(format!("up-{id}")))
                                    .ghost()
                                    .small()
                                    .icon(IconName::ArrowUp)
                                    .disabled(index == 0 || read_only)
                                    .on_click({
                                        let (this, id) = (this.clone(), id.clone());
                                        move |_, _, cx| {
                                            this.update(cx, |this, cx| {
                                                this.move_profile(&id, -1, cx)
                                            })
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("down-{id}")))
                                    .ghost()
                                    .small()
                                    .icon(IconName::ArrowDown)
                                    .disabled(index == last || read_only)
                                    .on_click({
                                        let (this, id) = (this.clone(), id.clone());
                                        move |_, _, cx| {
                                            this.update(cx, |this, cx| {
                                                this.move_profile(&id, 1, cx)
                                            })
                                        }
                                    }),
                            )
                            .child(
                                Switch::new(SharedString::from(format!("visible-{id}")))
                                    .checked(visible)
                                    .disabled(locked)
                                    .accessibility_label("Show in the link selector")
                                    .on_change({
                                        let (this, id) = (this.clone(), id.clone());
                                        move |checked, _, cx| {
                                            let hidden = !*checked;
                                            this.update(cx, |this, cx| {
                                                this.set_hidden(&id, hidden, cx)
                                            })
                                        }
                                    }),
                            )
                    }),
                )
                .description(profile.id.clone())
                .keywords([profile.browser_id.clone()]),
            );
        }
        SettingPage::new("Browsers")
            .icon(IconName::Globe)
            .resettable(false)
            .group(group)
    }

    fn rules_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let managed = self.layers.managed;
        let mut group = SettingGroup::new()
            .title("Rules")
            .description(if managed {
                "The first matching rule wins. Local rules run before Home Manager rules."
            } else {
                "The first matching rule wins."
            })
            .footer(move |_, _| {
                if managed {
                    "Choose Always in the link selector to add a local rule; promote local rules into Nix with flipper --export-overrides --format nix."
                } else {
                    "Choose Always in the link selector to add a rule, or edit paths, patterns and extensions in YAML."
                }
            });
        if self.config().rules.is_empty() {
            group = group.item(SettingItem::render(|_, _, cx| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No rules yet.")
            }));
        }
        for (index, rule) in self.config().rules.iter().enumerate() {
            let rule_for_remove = rule.clone();
            let origin = self.layers.rule_origin(index);
            let editable = self.layers.rule_editable(index);
            // Only distinguish origins when there are two layers to tell apart.
            let tag = match origin {
                RuleOrigin::Local if managed || !self.layers.overrides.is_empty() => Some("Local"),
                RuleOrigin::Base if managed => Some("Home Manager"),
                _ => None,
            };
            let this = this.clone();
            let conditions = rule
                .domains
                .iter()
                .chain(&rule.paths)
                .chain(&rule.patterns)
                .chain(&rule.extensions)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let action = match (rule.action, rule.profile.as_deref()) {
                (RuleAction::Open, Some(selector)) => {
                    let target = routing::resolve_selector(selector, &self.profiles)
                        .map_or_else(|_| selector.to_owned(), profile_label);
                    format!("Open in {target}")
                }
                _ => "Show the picker".into(),
            };
            group = group.item(
                SettingItem::new(
                    rule.name.clone().unwrap_or_else(|| conditions.clone()),
                    SettingField::render(move |_, _, _| {
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when_some(tag, |row, tag| {
                                row.child(if origin == RuleOrigin::Base {
                                    Tag::secondary().small().child(
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child(
                                                Icon::new(gpui_kit::assets::IconName::Lock)
                                                    .xsmall(),
                                            )
                                            .child(tag),
                                    )
                                } else {
                                    Tag::info().small().child(tag)
                                })
                            })
                            .when(editable, |row| {
                                row.child(
                                    Button::new(("remove-rule", index))
                                        .ghost()
                                        .small()
                                        .icon(IconName::Delete)
                                        .label("Remove")
                                        .on_click({
                                            let (this, rule) =
                                                (this.clone(), rule_for_remove.clone());
                                            move |_, _, cx| {
                                                this.update(cx, |this, cx| {
                                                    this.remove_rule(origin, rule.clone(), cx)
                                                })
                                            }
                                        }),
                                )
                            })
                    }),
                )
                .description(format!("{conditions} → {action}")),
            );
        }
        SettingPage::new("Rules")
            .icon(IconName::Inbox)
            .resettable(false)
            .group(group)
    }

    fn configuration_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let paths = config::Paths::from_env();
        let config_path = paths.as_ref().map_or_else(
            || "~/.config/flipper/config.yaml".into(),
            |paths| paths.config.display().to_string(),
        );
        let overrides_path = paths.as_ref().map_or_else(
            || "~/.config/flipper/overrides.yaml".into(),
            |paths| paths.overrides.display().to_string(),
        );
        let managed = self.layers.managed;
        let allowed = self.layers.overrides_allowed();
        let has_overrides = !self.layers.overrides.is_empty();
        let (open_title, open_label, open_path) = match (managed, allowed) {
            (true, true) => ("Local overrides", "Open overrides", overrides_path.clone()),
            (true, false) => ("Managed YAML", "View managed file", config_path.clone()),
            _ => ("YAML file", "Open in editor", config_path.clone()),
        };
        let mut group = SettingGroup::new().item(
            SettingItem::new(
                open_title,
                SettingField::render({
                    let this = this.clone();
                    move |_, _, _| {
                        Button::new("open-config")
                            .small()
                            .icon(IconName::ExternalLink)
                            .label(open_label)
                            .on_click({
                                let this = this.clone();
                                move |_, _, cx| this.update(cx, |this, cx| this.open_config(cx))
                            })
                    }
                }),
            )
            .description(open_path),
        );
        if managed {
            group = group.item(
                SettingItem::new(
                    "Managed configuration",
                    SettingField::render(|_, _, _| div()),
                )
                .description(format!("{config_path} (Home Manager; read-only)")),
            );
        }
        if allowed && (managed || has_overrides) {
            group = group.item(
                SettingItem::new(
                    "Clear local overrides",
                    SettingField::render(move |_, _, _| {
                        Button::new("clear-overrides")
                            .small()
                            .icon(IconName::Delete)
                            .label("Clear")
                            .disabled(!has_overrides)
                            .on_click({
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| this.clear_overrides(cx))
                                }
                            })
                    }),
                )
                .description(
                    "Remove every local rule and setting from overrides.yaml. Export them first with flipper --export-overrides --format nix.",
                ),
            );
        }
        group = group.item(
            SettingItem::new("Import and export", SettingField::render(|_, _, _| div()))
                .description(if managed {
                    "flipper --export [PATH|-]  ·  flipper --export-overrides [--format yaml|nix]"
                } else {
                    "flipper --import PATH  ·  flipper --export [PATH|-]"
                }),
        );
        SettingPage::new("Configuration")
            .icon(IconName::FileText)
            .resettable(false)
            .group(group)
    }
}

impl SettingsView {
    fn banner(&self) -> Option<&'static str> {
        match (self.layers.managed, self.layers.overrides_allowed()) {
            (true, true) => Some("Managed by Home Manager — changes are saved as local overrides."),
            (true, false) => Some(
                "Managed by Home Manager — read-only. Local overrides are disabled in your Home Manager configuration.",
            ),
            (false, _) if !self.layers.overrides.is_empty() => {
                Some("Local overrides from overrides.yaml apply on top of config.yaml.")
            }
            _ => None,
        }
    }
}

fn profile_label(profile: &BrowserProfile) -> String {
    match profile.email.as_deref() {
        Some(email) => format!(
            "{} · {} ({email})",
            profile.browser_name, profile.profile_name
        ),
        None => format!("{} · {}", profile.browser_name, profile.profile_name),
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pages = [
            self.general_page(cx),
            self.browsers_page(cx),
            self.rules_page(cx),
            self.appearance_page(cx),
            self.configuration_page(cx),
        ];
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .when_some(self.banner(), |this, banner| {
                this.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_sm()
                        .bg(theme.info.opacity(0.15))
                        .child(banner),
                )
            })
            .when_some(self.warning.clone(), |this, warning| {
                this.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_sm()
                        .bg(theme.warning.opacity(0.15))
                        .child(format!("Config warning: {warning}")),
                )
            })
            .child(
                div().flex_1().min_h_0().child(
                    Settings::new("flipper-settings")
                        .with_group_variant(GroupBoxVariant::Outline)
                        .pages(pages),
                ),
            )
            .when_some(self.status.clone(), |this, status| {
                this.child(
                    div()
                        .px_4()
                        .py_2()
                        .border_t_1()
                        .border_color(theme.border)
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(status),
                )
            })
    }
}
