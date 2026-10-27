//! Settings window: a normal application window, separate from the
//! layer-shell link selector.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::browsers::BrowserProfile;
use crate::config::{self, Config, RoutingMode, Rule, RuleAction, Theme};
use crate::{app_path, config_io, launch, registration, routing, urls::PreparedUrl};

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
    config: Config,
    profiles: Vec<BrowserProfile>,
    link: Entity<InputState>,
    status: Option<String>,
    warning: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(
        config: Config,
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
            config,
            profiles,
            link,
            status: None,
            warning,
            _subscriptions: subscriptions,
        }
    }

    fn update_config(&mut self, edit: impl FnOnce(&mut Config), cx: &mut Context<Self>) {
        match config_io::update(edit) {
            Ok(config) => {
                self.config = config;
                self.profiles = crate::browsers::discover(&self.config);
                self.warning = None;
                self.status = Some("Configuration saved.".into());
            }
            Err(error) => self.status = Some(error),
        }
        cx.notify();
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
        if let Err(error) = PreparedUrl::parse(text, self.config.routing.unwrap_redirects) {
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
            let Some(selector) = self.config.routing.default_profile.as_deref() else {
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
        self.update_config(|config| config.routing.mode = mode, cx);
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
        let id = id.to_owned();
        self.update_config(|config| config.routing.default_profile = Some(id), cx);
    }

    fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        crate::ui::apply_theme(theme, cx);
        self.update_config(|config| config.appearance.theme = theme, cx);
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

    fn set_hidden(&mut self, id: &str, hidden: bool, cx: &mut Context<Self>) {
        let id = id.to_owned();
        let browser_id = self
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.browser_id.clone());
        self.update_config(
            |config| {
                if hidden {
                    if !config.browsers.hidden.contains(&id) {
                        config.browsers.hidden.push(id.clone());
                    }
                } else {
                    config
                        .browsers
                        .hidden
                        .retain(|entry| entry != &id && Some(entry) != browser_id.as_ref());
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

    fn open_config(&mut self, cx: &mut Context<Self>) {
        let Some(path) = config::path() else {
            self.set_status("Could not locate the Flipper configuration path.", cx);
            return;
        };
        if !path.exists()
            && let Err(error) = config_io::replace(&self.config)
        {
            self.set_status(error, cx);
            return;
        }
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

    fn general_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let link = self.link.clone();
        let routing = &self.config.routing;

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

        SettingPage::new("General")
            .icon(IconName::Settings)
            .resettable(false)
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
                        SettingItem::new(
                            "When no rule matches",
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
                                        this.update(cx, |this, cx| {
                                            this.set_fallback_mode(mode, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description("Rules always run first."),
                    )
                    .item(
                        SettingItem::new(
                            "Fallback profile",
                            SettingField::scrollable_dropdown(
                                profile_options,
                                move |_| fallback_id.clone(),
                                {
                                    let this = this.clone();
                                    move |value, cx| {
                                        this.update(cx, |this, cx| {
                                            this.set_fallback_profile(&value, cx)
                                        });
                                    }
                                },
                            ),
                        )
                        .description("Used when no rule matches and the picker is off."),
                    )
                    .item(
                        SettingItem::new(
                            "Pause rules",
                            SettingField::switch(move |_| suspended, {
                                let this = this.clone();
                                move |value, cx| {
                                    this.update(cx, |this, cx| {
                                        this.update_config(
                                            |config| config.routing.suspended = value,
                                            cx,
                                        )
                                    });
                                }
                            }),
                        )
                        .description("Show the picker for every link without deleting rules."),
                    )
                    .item(
                        SettingItem::new(
                            "Unwrap redirects",
                            SettingField::switch(move |_| unwrap, {
                                let this = this.clone();
                                move |value, cx| {
                                    this.update(cx, |this, cx| {
                                        this.update_config(
                                            |config| config.routing.unwrap_redirects = value,
                                            cx,
                                        )
                                    });
                                }
                            }),
                        )
                        .description(
                            "Resolve Microsoft SafeLinks and Google redirects before routing.",
                        ),
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
                    .description("Send http and https links from other apps through Flipper."),
                ),
            )
    }

    fn appearance_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let theme: SharedString = match self.config.appearance.theme {
            Theme::System => "system".into(),
            Theme::Light => "light".into(),
            Theme::Dark => "dark".into(),
        };
        let dim = self.config.appearance.dim;
        SettingPage::new("Appearance")
            .icon(IconName::Palette)
            .resettable(false)
            .group(
                SettingGroup::new()
                    .item(SettingItem::new(
                        "Theme",
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
                    ))
                    .item(
                        SettingItem::new(
                            "Dim the screen",
                            SettingField::switch(move |_| dim, {
                                let this = this.clone();
                                move |value, cx| {
                                    this.update(cx, |this, cx| {
                                        this.update_config(
                                            |config| config.appearance.dim = value,
                                            cx,
                                        )
                                    });
                                }
                            }),
                        )
                        .description("Darken everything behind the link selector."),
                    ),
            )
    }

    fn browsers_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let this = cx.entity();
        let fallback = self
            .config
            .routing
            .default_profile
            .as_deref()
            .and_then(|selector| routing::resolve_selector(selector, &self.profiles).ok())
            .map(|profile| profile.id.clone());
        let last = self.profiles.len().saturating_sub(1);
        let mut group = SettingGroup::new()
            .title("Profiles")
            .description("Order and visibility in the link selector.")
            .footer(|_, _| "Add custom browsers under custom_browsers in the YAML configuration.");
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
                                    .disabled(index == 0)
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
                                    .disabled(index == last)
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
        let mut group = SettingGroup::new()
            .title("Rules")
            .description("The first matching rule wins.")
            .footer(|_, _| {
                "Choose Always in the link selector to add a rule, or edit paths, patterns and extensions in YAML."
            });
        if self.config.rules.is_empty() {
            group = group.item(SettingItem::render(|_, _, cx| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No rules yet.")
            }));
        }
        for (index, rule) in self.config.rules.iter().enumerate() {
            let rule_for_remove = rule.clone();
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
                        Button::new(("remove-rule", index))
                            .ghost()
                            .small()
                            .icon(IconName::Delete)
                            .label("Remove")
                            .on_click({
                                let (this, rule) = (this.clone(), rule_for_remove.clone());
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| this.remove_rule(rule.clone(), cx))
                                }
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
        let path = config::path().map_or_else(
            || "~/.config/flipper/config.yaml".into(),
            |path| path.display().to_string(),
        );
        SettingPage::new("Configuration")
            .icon(IconName::FileText)
            .resettable(false)
            .group(
                SettingGroup::new()
                    .item(
                        SettingItem::new(
                            "YAML file",
                            SettingField::render(move |_, _, _| {
                                Button::new("open-config")
                                    .small()
                                    .icon(IconName::ExternalLink)
                                    .label("Open in editor")
                                    .on_click({
                                        let this = this.clone();
                                        move |_, _, cx| {
                                            this.update(cx, |this, cx| this.open_config(cx))
                                        }
                                    })
                            }),
                        )
                        .description(path),
                    )
                    .item(
                        SettingItem::new(
                            "Import and export",
                            SettingField::render(|_, _, _| div()),
                        )
                        .description("flipper --import PATH  ·  flipper --export [PATH|-]"),
                    ),
            )
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
