//! GPUI link selector surface.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::base::Root;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::theme::{Theme as UiTheme, ThemeMode};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::browsers::BrowserProfile;
use crate::config::{self, Config, Layers, Rule, RuleAction, Theme};
use crate::{app_path, config_io, launch, urls::PreparedUrl};

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

const ROW_HEIGHT: f32 = 40.;
const MAX_ROWS: usize = 8;
const FALLBACK_HEIGHT: f32 = 560.;

/// Set on the picker while the filter is empty, so bare digits open a profile.
/// Once something is typed, digits go to the filter (emails, profile names).
const NUMBER_KEYS: &str = "number_keys";
const NUMBER_KEYS_CONTEXT: &str = "Flipper && number_keys > Input";

pub fn bind_keys(cx: &mut App) {
    let context = Some("Flipper > Input");
    let digits = Some(NUMBER_KEYS_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("down", MoveDown, context),
        KeyBinding::new("up", MoveUp, context),
        KeyBinding::new("enter", OpenSelected, context),
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("ctrl-enter", SaveAlways, context),
        KeyBinding::new("1", SelectOne, digits),
        KeyBinding::new("2", SelectTwo, digits),
        KeyBinding::new("3", SelectThree, digits),
        KeyBinding::new("4", SelectFour, digits),
        KeyBinding::new("5", SelectFive, digits),
        KeyBinding::new("6", SelectSix, digits),
        KeyBinding::new("7", SelectSeven, digits),
        KeyBinding::new("8", SelectEight, digits),
        KeyBinding::new("9", SelectNine, digits),
        KeyBinding::new("0", SelectZero, digits),
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
    layers: Layers,
    profiles: Vec<BrowserProfile>,
    url: Option<PreparedUrl>,
    warning: Option<String>,
) -> Result<(), String> {
    let config = layers.effective.clone();
    let read_only = layers.read_only();
    let appearance = config.appearance.clone();
    let error = Arc::new(Mutex::new(None));
    let error_from_app = Arc::clone(&error);
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
            apply_theme(appearance.theme, cx);
            let Some(url) = url else {
                let opened = cx.open_window(crate::settings::window_options(cx), |window, cx| {
                    let view = cx.new(|cx| {
                        crate::settings::SettingsView::new(layers, profiles, warning, window, cx)
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                });
                if let Err(open_error) = opened {
                    *error_from_app.lock().expect("UI error lock poisoned") =
                        Some(format!("could not open settings window: {open_error}"));
                    cx.quit();
                }
                return;
            };
            let url = Some(url);
            let layer = is_wayland_session();
            let options = window_options(&appearance, layer);
            let config_layer = config.clone();
            let profiles_layer = profiles.clone();
            let url_layer = url.clone();
            let warning_layer = warning.clone();
            let opened = cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| {
                    PickerView::new(
                        config_layer,
                        read_only,
                        profiles_layer,
                        url_layer,
                        warning_layer,
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
            });
            if opened.is_err() && layer {
                let fallback = window_options(&appearance, false);
                let fallback_opened = cx.open_window(fallback, move |window, cx| {
                    let view = cx.new(|cx| {
                        PickerView::new(config, read_only, profiles, url, warning, window, cx)
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

pub fn apply_theme(theme: Theme, cx: &mut App) {
    match theme {
        Theme::System => UiTheme::sync_system_appearance(None, cx),
        Theme::Light => UiTheme::change(ThemeMode::Light, None, cx),
        Theme::Dark => UiTheme::change(ThemeMode::Dark, None, cx),
    }
}

fn window_options(appearance: &config::Appearance, layer_shell: bool) -> WindowOptions {
    let mut options = WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(appearance.width + 40.), px(FALLBACK_HEIGHT)),
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

struct PickerView {
    config: Config,
    /// Managed with overrides disallowed: rules are copied as Nix instead.
    read_only: bool,
    profiles: Vec<BrowserProfile>,
    prepared: Option<PreparedUrl>,
    input: Entity<InputState>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    query: String,
    selected: usize,
    status: Option<String>,
    warning: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl PickerView {
    #[allow(clippy::too_many_arguments)]
    fn new(
        config: Config,
        read_only: bool,
        profiles: Vec<BrowserProfile>,
        prepared: Option<PreparedUrl>,
        warning: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Filter by browser, profile or email, or paste a link…")
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
            read_only,
            profiles,
            prepared,
            input,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            query: String::new(),
            selected: 0,
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

    /// The rule that Always or Always ask would save for this link.
    fn rule_for(&self, index: usize, action: RuleAction) -> Option<Rule> {
        let prepared = self.prepared.as_ref()?;
        let profile = self.profiles.get(index).cloned();
        let domain = prepared.effective.host_str().map(str::to_owned);
        let pattern_url = prepared.effective.to_string();
        if action == RuleAction::Open && profile.is_none() {
            return None;
        }
        Some(Rule {
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
        })
    }

    fn save_rule(&mut self, index: usize, action: RuleAction, cx: &mut Context<Self>) {
        if self.prepared.is_none() {
            self.status = Some("Paste a link before creating a routing rule.".into());
            cx.notify();
            return;
        }
        let Some(rule) = self.rule_for(index, action) else {
            return;
        };
        if self.read_only {
            self.copy_rule(rule, cx);
            return;
        }
        match config_io::edit(|editor| editor.add_rule(rule)) {
            Ok(layers) => {
                let local = if layers.managed {
                    " as a local override"
                } else {
                    ""
                };
                self.config = layers.effective;
                self.status = Some(if action == RuleAction::Open {
                    format!("Rule saved{local}. Opening the link now.")
                } else {
                    format!("Picker exception saved{local} for this domain.")
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

    /// Overrides are disallowed: hand the rule over as a Home Manager snippet.
    fn copy_rule(&mut self, rule: Rule, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(crate::nix::rule(&rule, 0)));
        self.status = Some(
            "Rule copied as Nix. Add it to programs.flipper.settings.rules in Home Manager.".into(),
        );
        cx.notify();
    }

    fn save_always(&mut self, _: &SaveAlways, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.visible_profiles().get(self.selected).copied() {
            self.save_rule(index, RuleAction::Open, cx);
        }
    }

    fn open_settings(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let result = app_path::invoked().and_then(|exe| {
            launch::launch_argv(&[exe.to_string_lossy().into_owned(), "--settings".into()])
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(()) => cx.quit(),
            Err(error) => {
                self.status = Some(format!("Could not open settings: {error}"));
                cx.notify();
            }
        }
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
            .map(|path| img(path).size(px(22.)).flex_none().into_any_element())
            .unwrap_or_else(|| {
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(theme_color(dark, 0x26354f, 0xe8edf5))
                    .text_color(theme_color(dark, 0xe7edf9, 0x26354f))
                    .text_xs()
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
            .px_2()
            .gap_2()
            .items_center()
            .rounded(px(8.))
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
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_baseline()
                    .text_sm()
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme_color(dark, 0xf2f5fa, 0x202b38))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(profile.browser_name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme_color(dark, 0xb6c1d2, 0x546174))
                            .child(profile.profile_name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme_color(dark, 0x93a2b9, 0x667386))
                            .text_xs()
                            .child(profile.email.clone().unwrap_or_else(|| profile.id.clone())),
                    ),
            )
            .when_some(
                shortcut_label(visible_index, self.query.is_empty()),
                |row, label| {
                    row.child(
                        div()
                            .flex_none()
                            .px_1p5()
                            .rounded(px(5.))
                            .bg(theme_color(dark, 0x202b3d, 0xe8edf4))
                            .text_color(theme_color(dark, 0xb8c7df, 0x48566a))
                            .text_xs()
                            .child(label),
                    )
                },
            );

        if selected {
            row = row
                .border_1()
                .border_color(theme_color(dark, 0x7aa2f7, 0x4f7edc));
        }
        row.into_any_element()
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
            .px_2()
            .py_0p5()
            .rounded(px(6.))
            .bg(theme_color(dark, 0x24334a, 0xe8edf4))
            .text_color(theme_color(dark, 0xdce6f5, 0x273449))
            .text_xs()
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
                .py_4()
                .text_sm()
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
            .h(px(ROW_HEIGHT * visible.len().min(MAX_ROWS) as f32))
            .into_any_element()
        };

        let mut panel = v_flex()
            .w(px(self.config.appearance.width))
            .gap_2()
            .p_3()
            .rounded(px(12.))
            .border_1()
            .border_color(theme_color(dark, 0x344156, 0xd8dee9))
            .bg(theme_color(dark, 0x10151f, 0xffffff))
            .shadow_lg();

        panel = panel.child(
            h_flex()
                .items_start()
                .gap_2()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme_color(dark, 0xf3f6fb, 0x182333))
                                .child(title),
                        )
                        .when_some(subtitle, |this, subtitle| {
                            this.child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme_color(dark, 0x91a0b5, 0x637185))
                                    .child(subtitle),
                            )
                        })
                        .when_some(doc_label, |this, label| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(theme_color(dark, 0xffc77d, 0x9b5a00))
                                    .child(format!("Office file · {label}")),
                            )
                        }),
                )
                .child(self.button(
                    "settings-toggle",
                    "Settings",
                    cx.listener(Self::open_settings),
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
            h_flex()
                .w_full()
                .h(px(34.))
                .px_2()
                .gap_2()
                .items_center()
                .rounded(px(8.))
                .border_1()
                .border_color(theme_color(dark, 0x303b4d, 0xd7dde6))
                .bg(theme_color(dark, 0x171e2a, 0xf3f5f8))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme_color(dark, 0x8392a9, 0x778396))
                        .child("⌕"),
                )
                .child(
                    div().flex_1().child(
                        Input::new(&self.input)
                            .appearance(false)
                            .h(px(30.))
                            .p_0()
                            .text_sm(),
                    ),
                ),
        );

        panel = panel.child(list);
        let selected_profile = visible
            .get(self.selected)
            .and_then(|index| self.profiles.get(*index));
        let mut actions = h_flex().flex_none().gap_1().items_center();
        if let Some(profile) = selected_profile {
            let index = visible[self.selected];
            let id = profile.id.clone();
            let (always, ask) = if self.read_only {
                ("Copy rule", "Copy ask rule")
            } else {
                ("Always", "Always ask")
            };
            actions = actions
                .child(self.button(
                    "always-rule",
                    always,
                    cx.listener(move |this, _, _, cx| this.save_rule(index, RuleAction::Open, cx)),
                    dark,
                ))
                .child(self.button(
                    "picker-rule",
                    ask,
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
        let open_keys = if self.query.is_empty() {
            "1–0"
        } else {
            "Ctrl+1–0"
        };
        let hint = if self.prepared.is_some() && self.read_only {
            format!("{open_keys} or Enter open · Ctrl+Enter copy Nix · Esc")
        } else if self.prepared.is_some() {
            format!("{open_keys} or Enter open · Ctrl+Enter always · Esc")
        } else {
            "Paste a URL and press Enter, then choose a profile".into()
        };
        panel = panel.child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(actions)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_right()
                        .text_xs()
                        .text_color(theme_color(dark, 0x8190a6, 0x778396))
                        .child(hint),
                ),
        );

        if let Some(status) = self.status.as_deref() {
            panel = panel.child(
                div()
                    .text_xs()
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
        let dim = if self.config.appearance.dim {
            if dark { 0x080d16d9 } else { 0x17223588 }
        } else {
            0x00000000
        };
        let mut context = KeyContext::default();
        context.add("Flipper");
        if self.query.is_empty() {
            context.add(NUMBER_KEYS);
        }
        div()
            .size_full()
            .key_context(context)
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

/// The key that opens a row: bare digits until the filter has text, then Ctrl.
fn shortcut_label(visible_index: usize, bare_digits: bool) -> Option<String> {
    let digit = match visible_index {
        0..=8 => visible_index + 1,
        9 => 0,
        _ => return None,
    };
    Some(if bare_digits {
        digit.to_string()
    } else {
        format!("Ctrl+{digit}")
    })
}

#[cfg(test)]
mod tests {
    use super::{NUMBER_KEYS_CONTEXT, shortcut_label};
    use gpui_kit::{KeyBindingContextPredicate, KeyContext};

    fn stack(contexts: &[&str]) -> Vec<KeyContext> {
        contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap())
            .collect()
    }

    #[test]
    fn bare_digits_only_apply_while_the_filter_is_empty() {
        let predicate = KeyBindingContextPredicate::parse(NUMBER_KEYS_CONTEXT).unwrap();
        assert!(predicate.eval(&stack(&["Flipper number_keys", "Input"])));
        assert!(!predicate.eval(&stack(&["Flipper", "Input"])));
        assert!(!predicate.eval(&stack(&["Flipper number_keys"])));
    }

    #[test]
    fn shortcut_labels_follow_the_filter() {
        assert_eq!(shortcut_label(0, true).as_deref(), Some("1"));
        assert_eq!(shortcut_label(9, true).as_deref(), Some("0"));
        assert_eq!(shortcut_label(2, false).as_deref(), Some("Ctrl+3"));
        assert_eq!(shortcut_label(10, true), None);
    }
}
