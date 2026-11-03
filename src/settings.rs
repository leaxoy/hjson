//! Settings dialog: opened (macOS `Cmd-,`, Windows/Linux `Ctrl-,`) as a
//! modal overlay. The dialog content is a small view whose control states
//! live in a lazily-created global so the dialog can be rebuilt every frame.

use crate::i18n::Language;
use rust_i18n::t;
use std::borrow::Cow;
use gpui_kit::assets::IconName;
use gpui_kit::component::theme::ThemeColor;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IndexPath, Sizable as _, Theme, WindowExt as _, h_flex,
    input::{InputEvent, InputState, NumberInput},
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, Global, IntoElement, KeyBinding,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, WindowAppearance,
    div, px,
};

gpui_kit::actions!(hjson, [OpenSettings, Quit, ToggleLineNumbers]);

/// The pseudo item in the font dropdown meaning "the theme's default mono font"
/// (which resolves to the system default monospace font).
///
/// Translated, so it is rebuilt from the current locale at both uses: the
/// dropdown item and the comparison in its subscription. The settings dialog's
/// cached entity is dropped when the language changes, so the two always agree.
pub fn follow_theme_label() -> SharedString {
    SharedString::from(t!("settings.font_follow_system").as_ref())
}

/// Allowed range for the JSON panel font size.
const JSON_SIZE_MIN: f32 = 10.0;
const JSON_SIZE_MAX: f32 = 20.0;
/// Allowed range for the interface root font size (the `rem` base).
const UI_SIZE_MIN: f32 = 12.0;
const UI_SIZE_MAX: f32 = 22.0;

/// The text-system probe strings for mono detection: two 4-char strings whose
/// widths only agree in a monospace font.
const MONO_PROBE_WIDE: &str = "MWX@";
const MONO_PROBE_NARROW: &str = "iii,";

/// The light / dark mode of the whole interface.
///
/// `System` follows the OS and keeps tracking its changes. The two fixed modes
/// also force the native window chrome (on macOS `NSApplication.appearance`),
/// so the title bar matches the theme instead of the desktop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    /// Every choice, in the order the picker shows them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The name of the mode in the current language. [`Self::key`] is the
    /// persisted value, which is never localized.
    pub fn label(self) -> Cow<'static, str> {
        match self {
            Self::System => t!("settings.mode_system"),
            Self::Light => t!("settings.mode_light"),
            Self::Dark => t!("settings.mode_dark"),
        }
    }

    /// The key written to the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }

    /// Position inside [`Appearance::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.into_iter().position(|a| a == self).unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// `None` means follow the system default mono font (via the theme).
    pub mono_font: Option<SharedString>,
    /// Font size of the raw JSON panel, in px.
    pub font_size: f32,
    /// Root font size of the interface, in px. This is the `rem` base every
    /// `text_xs()` / `text_sm()` / `text_base()` … resolves against, so it
    /// scales the whole UI proportionally.
    pub ui_font_size: f32,
    pub line_numbers: bool,
    /// Light / dark mode, or follow the OS.
    pub appearance: Appearance,
    /// The interface language, or follow the OS.
    pub language: Language,
    /// Syntax palette used while the app is in dark mode.
    pub syntax_dark: SharedString,
    /// Syntax palette used while the app is in light mode.
    pub syntax_light: SharedString,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mono_font: None,
            font_size: 13.0,
            ui_font_size: 16.0,
            line_numbers: true,
            appearance: Appearance::default(),
            language: Language::default(),
            syntax_dark: SharedString::from("Tokyo Night"),
            syntax_light: SharedString::from("GitHub"),
        }
    }
}

impl Settings {
    /// Parse a settings JSON file, falling back to defaults. Values are
    /// clamped to the same ranges the settings dialog enforces.
    pub fn from_json_str(text: &str) -> Self {
        let mut defaults = Self::default();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
            match json.get("mono_font").and_then(|v| v.as_str()) {
                Some(f) if !f.is_empty() => defaults.mono_font = Some(SharedString::from(f)),
                _ => defaults.mono_font = None,
            }
            if let Some(size) = json.get("font_size").and_then(|v| v.as_f64()) {
                defaults.font_size = (size as f32).clamp(JSON_SIZE_MIN, JSON_SIZE_MAX);
            }
            if let Some(size) = json.get("ui_font_size").and_then(|v| v.as_f64()) {
                defaults.ui_font_size = (size as f32).clamp(UI_SIZE_MIN, UI_SIZE_MAX);
            }
            if let Some(ln) = json.get("line_numbers").and_then(|v| v.as_bool()) {
                defaults.line_numbers = ln;
            }
            if let Some(key) = json.get("appearance").and_then(|v| v.as_str())
                && let Some(a) = Appearance::from_key(key)
            {
                defaults.appearance = a;
            }
            if let Some(key) = json.get("language").and_then(|v| v.as_str())
                && let Some(l) = Language::from_key(key)
            {
                defaults.language = l;
            }
            // Unknown theme names fall back to their defaults.
            if let Some(name) = json.get("syntax_dark").and_then(|v| v.as_str())
                && crate::syntax_theme::find(name).is_some()
            {
                defaults.syntax_dark = SharedString::from(name);
            }
            if let Some(name) = json.get("syntax_light").and_then(|v| v.as_str())
                && crate::syntax_theme::find(name).is_some()
            {
                defaults.syntax_light = SharedString::from(name);
            }
        }
        defaults
    }

    /// The effective mono font family for the raw panel.
    pub fn mono_family(&self, cx: &App) -> SharedString {
        match &self.mono_font {
            Some(f) => f.clone(),
            None => cx.theme().mono_font_family.clone(),
        }
    }

    /// The syntax palette resolved for the given app mode. Unknown names in
    /// the persisted file fall back to the default theme.
    pub fn syntax_colors(&self, dark: bool) -> &'static crate::syntax_theme::SyntaxColors {
        let name = if dark {
            &self.syntax_dark
        } else {
            &self.syntax_light
        };
        crate::syntax_theme::find(name)
            .map(|theme| if dark { &theme.dark } else { &theme.light })
            .unwrap_or_else(|| {
                let theme = crate::syntax_theme::fallback();
                if dark { &theme.dark } else { &theme.light }
            })
    }

    /// Line height derived from the font size.
    #[inline]
    pub fn row_height(&self) -> f32 {
        self.font_size + 8.0
    }
}

impl Global for Settings {}

pub fn settings(cx: &App) -> &Settings {
    cx.global::<Settings>()
}

/// Mutate settings, repaint every window, and persist to the config file.
pub fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    let old_line_numbers = settings(cx).line_numbers;
    let old_appearance = settings(cx).appearance;
    let old_language = settings(cx).language;
    f(cx.global_mut::<Settings>());
    let language = settings(cx).language;
    // The native menu bakes its labels and check marks in, so anything that
    // moves one is worth rebuilding the whole bar for.
    let menu_stale = settings(cx).line_numbers != old_line_numbers || language != old_language;
    if language != old_language {
        language.apply();
        // Its dropdowns hold the labels built for the old language.
        if cx.has_global::<HubGlobal>() {
            cx.global_mut::<HubGlobal>().hub = None;
        }
    }
    // No window here: the fixed modes never read the platform, and clearing an
    // override has to re-read the live app appearance, not a window's cache.
    if settings(cx).appearance != old_appearance {
        apply_appearance(None, cx);
    }
    cx.refresh_windows();
    persist(cx);
    if menu_stale {
        crate::menu::install(cx);
    }
}

/// Push [`Settings::appearance`] onto the platform and the theme.
///
/// `window` is only used by the follow-system branch, where the OS value has to
/// be read; see [`Theme::sync_system_appearance`].
pub fn apply_appearance(window: Option<&mut Window>, cx: &mut App) {
    match settings(cx).appearance {
        Appearance::System => {
            cx.set_window_appearance(None);
            Theme::sync_system_appearance(window, cx);
        }
        Appearance::Light => force_appearance(WindowAppearance::Light, cx),
        Appearance::Dark => force_appearance(WindowAppearance::Dark, cx),
    }
}

/// A fixed mode: the native chrome is forced first, then the theme is set to
/// match, because `Theme::sync_system_appearance` would read back whatever the
/// platform reports and the window's own copy can lag behind the override.
fn force_appearance(appearance: WindowAppearance, cx: &mut App) {
    cx.set_window_appearance(Some(appearance));
    Theme::change(appearance, None, cx);
}

/// Every installed font family, filtered down to those that actually shape
/// ASCII with equal per-glyph advances (i.e. real monospace fonts).
///
/// Probing runs on the window text system because only the window shapes text.
pub fn mono_font_options(window: &mut Window, cx: &App) -> Vec<SharedString> {
    let mut families: Vec<SharedString> = cx
        .text_system()
        .all_font_names()
        .into_iter()
        .filter(|name| is_monospace_family(window, cx, name))
        .map(SharedString::from)
        .collect();
    families.sort();
    let mut out = vec![follow_theme_label()];
    out.extend(families);
    out
}

/// Measure-based mono detection, run on the window text system.
///
/// The family passes when "MWX@" and "iii," layout to the same width. A guard
/// also rejects fonts whose glyphs came from a fallback: icon / symbol fonts
/// have no `W` or `i`, so they would "pass" the width test through the
/// fallback font without it.
fn is_monospace_family(window: &Window, cx: &App, family: &str) -> bool {
    let font = gpui_kit::font(family);
    let resolved = cx.text_system().resolve_font(&font);
    let text_system = window.text_system();

    let width_of = |text: &str| -> Option<gpui_kit::Pixels> {
        let run = gpui_kit::TextRun {
            len: text.len(),
            font: font.clone(),
            color: gpui_kit::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = text_system.layout_line(text, px(16.), std::slice::from_ref(&run), None);
        // Reject fallback shaping: the first run must be the family itself.
        (line.runs.first()?.font_id == resolved).then_some(line.width)
    };

    match (width_of(MONO_PROBE_WIDE), width_of(MONO_PROBE_NARROW)) {
        (Some(wide), Some(narrow)) => (wide - narrow).abs() < px(0.5),
        _ => false,
    }
}

// MARK: persistence

/// The on-disk settings file, e.g.
/// macOS: `~/Library/Application Support/hjson/settings.json`,
/// Windows: `%APPDATA%\hjson\settings.json`,
/// Linux: `~/.config/hjson/settings.json`.
fn config_path() -> std::path::PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("hjson");
    path.push("settings.json");
    path
}

/// Load persisted settings at startup (falls back to defaults) and switch the
/// interface to the language they ask for.
fn load(cx: &mut App) {
    let text = std::fs::read_to_string(config_path()).unwrap_or_default();
    cx.set_global(Settings::from_json_str(&text));
    settings(cx).language.apply();
}

/// Write the current settings back to disk. Errors are ignored: missing
/// config directories or read-only disks must not crash the app.
fn persist(cx: &App) {
    let s = cx.global::<Settings>();
    let json = serde_json::json!({
        "mono_font": s.mono_font.as_deref().unwrap_or(""),
        "font_size": s.font_size,
        "ui_font_size": s.ui_font_size,
        "line_numbers": s.line_numbers,
        "appearance": s.appearance.key(),
        "language": s.language.key(),
        "syntax_dark": s.syntax_dark.as_ref(),
        "syntax_light": s.syntax_light.as_ref(),
    });
    let path = config_path();
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&json).unwrap_or_default(),
        );
    }
}

/// Bind `Cmd-,` / `Ctrl-,` and register the handlers opening the modal /
/// quitting. The menu bar itself lives in [`crate::menu`].
pub fn init(cx: &mut App) {
    load(cx);

    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-,", OpenSettings, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-,", OpenSettings, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-q", Quit, None),
    ]);

    cx.on_action(|_: &OpenSettings, cx| {
        let Some(window) = cx.active_window() else {
            return;
        };
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| open_dialog(window, cx))
                .ok();
        });
    });

    cx.on_action(|_: &ToggleLineNumbers, cx| {
        update_settings(cx, |s| s.line_numbers = !s.line_numbers);
    });

    cx.on_action(|_: &Quit, cx| {
        cx.quit();
    });
}

/// Holds the dialog control-state entities so the dialog (rebuilt every
/// frame) can reuse them.
struct HubGlobal {
    hub: Option<Entity<SettingsHub>>,
}

impl Global for HubGlobal {}

/// Open the settings modal dialog.
pub fn open_dialog(window: &mut Window, cx: &mut App) {
    if !cx.has_global::<HubGlobal>() {
        cx.set_global(HubGlobal { hub: None });
    }
    let missing = cx.global::<HubGlobal>().hub.is_none();
    if missing {
        let hub = cx.new(|cx| SettingsHub::new(window, cx));
        cx.global_mut::<HubGlobal>().hub = Some(hub);
    }
    let hub = cx.global::<HubGlobal>().hub.clone().unwrap();

    window.open_dialog(cx, move |dialog, _, _| {
        // The builder runs every frame, so it must not move out of its
        // captured state — hence the clone.
        let hub = hub.clone();
        dialog
            .w(px(620.))
            .h(px(500.))
            .overlay(true)
            .overlay_closable(true)
            .title(t!("settings.title"))
            // A child lands in the dialog's body, which scrolls; `.content()`
            // builds a fixed-height content area that clips the rows below it.
            .child(hub)
    });
}

// MARK: dialog content

/// The dialog's tabs. Each one owns a slice of the surface, so the window stays
/// short instead of scrolling every control at once. The icons are static; the
/// titles come from [`tab_label`], because `t!` needs a literal key.
const TABS: [IconName; 3] = [IconName::SunMoon, IconName::Braces, IconName::Palette];

/// The title of the tab at `ix`, in the current language.
fn tab_label(ix: usize) -> Cow<'static, str> {
    match ix {
        1 => t!("settings.tab_preview"),
        2 => t!("settings.tab_syntax"),
        _ => t!("settings.tab_appearance"),
    }
}

pub struct SettingsHub {
    /// Index into [`TABS`].
    tab: usize,
    font_select: Entity<SelectState<Vec<SharedString>>>,
    size_input: Entity<InputState>,
    ui_size_input: Entity<InputState>,
    syntax_dark_select: Entity<SelectState<Vec<&'static str>>>,
    syntax_light_select: Entity<SelectState<Vec<&'static str>>>,
    _subs: Vec<Subscription>,
}

impl SettingsHub {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Syntax palette picks are a static catalog.
        let theme_names = crate::syntax_theme::theme_names();
        let theme_row = |current: &SharedString| {
            theme_names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(current))
                .unwrap_or(0)
        };
        let (theme_row_dark, theme_row_light) = (
            theme_row(&settings(cx).syntax_dark),
            theme_row(&settings(cx).syntax_light),
        );
        let syntax_dark_select = cx.new(|cx| {
            SelectState::new(
                theme_names.clone(),
                Some(IndexPath::default().row(theme_row_dark)),
                window,
                cx,
            )
        });
        let syntax_light_select = cx.new(|cx| {
            SelectState::new(
                theme_names,
                Some(IndexPath::default().row(theme_row_light)),
                window,
                cx,
            )
        });

        let font_options = mono_font_options(window, cx);
        let selected_row = {
            let current = settings(cx).mono_font.as_deref().map(str::to_string);
            match current {
                None => 0,
                Some(current) => font_options
                    .iter()
                    .position(|o| o.eq_ignore_ascii_case(&current))
                    .unwrap_or(0),
            }
        };
        let font_select = cx.new(|cx| {
            SelectState::new(
                font_options,
                Some(IndexPath::default().row(selected_row)),
                window,
                cx,
            )
        });
        let size = settings(cx).font_size;
        let size_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(size.to_string())
                .step(0.5)
                .min(JSON_SIZE_MIN as f64)
                .max(JSON_SIZE_MAX as f64)
        });
        let ui_size = settings(cx).ui_font_size;
        let ui_size_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(ui_size.to_string())
                .step(0.5)
                .min(UI_SIZE_MIN as f64)
                .max(UI_SIZE_MAX as f64)
        });

        let font_sub = cx.subscribe(
            &font_select,
            |_, _, event: &SelectEvent<Vec<SharedString>>, cx| match event {
                SelectEvent::Confirm(v) => {
                    let mono_font = match v {
                        None => None,
                        Some(label) if label == &follow_theme_label() => None,
                        Some(font) => Some(font.clone()),
                    };
                    update_settings(cx, |s| s.mono_font = mono_font);
                }
            },
        );
        let size_sub = size_subscription(&size_input, JSON_SIZE_MIN, JSON_SIZE_MAX, cx, |s, v| {
            s.font_size = v;
        });
        let ui_size_sub =
            size_subscription(&ui_size_input, UI_SIZE_MIN, UI_SIZE_MAX, cx, |s, v| {
                s.ui_font_size = v;
            });
        let dark_syntax_sub = cx.subscribe(
            &syntax_dark_select,
            |_, _, event: &SelectEvent<Vec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(name)) = event {
                    let name = *name;
                    update_settings(cx, move |s| s.syntax_dark = SharedString::from(name));
                }
            },
        );
        let light_syntax_sub = cx.subscribe(
            &syntax_light_select,
            |_, _, event: &SelectEvent<Vec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(name)) = event {
                    let name = *name;
                    update_settings(cx, move |s| s.syntax_light = SharedString::from(name));
                }
            },
        );

        Self {
            tab: 0,
            font_select,
            size_input,
            ui_size_input,
            syntax_dark_select,
            syntax_light_select,
            _subs: vec![
                font_sub,
                size_sub,
                ui_size_sub,
                dark_syntax_sub,
                light_syntax_sub,
            ],
        }
    }
}

impl Render for SettingsHub {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = match self.tab {
            1 => self.render_preview(cx),
            2 => self.render_syntax(cx),
            _ => self.render_appearance(cx),
        };
        v_flex().w_full().child(self.render_tabs(cx)).child(pane)
    }
}

impl SettingsHub {
    /// The tab strip. `Underline` because these are sections of one window
    /// rather than open documents: the bar stays transparent and only the
    /// active label carries a rule.
    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        TabBar::new("settings-tabs")
            .flex_shrink_0()
            .underline()
            .small()
            .pl_4()
            .selected_index(self.tab)
            .children(TABS.into_iter().enumerate().map(|(ix, icon)| {
                Tab::new().child(
                    h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_1p5()
                        .child(Icon::new(icon).small())
                        .child(div().min_w_0().truncate().child(tab_label(ix))),
                )
            }))
            .on_click({
                let hub = cx.entity();
                move |ix, _, cx| {
                    let ix = *ix;
                    hub.update(cx, |this, _| this.tab = ix);
                }
            })
            .into_any_element()
    }

    /// The rows of one tab, in a single padded pane.
    fn pane(rows: Vec<AnyElement>) -> AnyElement {
        v_flex()
            .w_full()
            .p_4()
            .gap_3()
            .children(rows)
            .into_any_element()
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let appearance = settings(cx).appearance;
        let language = settings(cx).language;

        let language_row = row(
            cx,
            &t!("settings.language"),
            Some(&t!("settings.language_hint")),
            TabBar::new("language-picker")
                .segmented()
                .small()
                .selected_index(language.index())
                .children(Language::ALL.map(|l| Tab::new().child(l.label())))
                .on_click(move |ix, _, cx| {
                    let ix = *ix;
                    update_settings(cx, |s| s.language = Language::ALL[ix]);
                }),
        );

        let appearance_row = row(
            cx,
            &t!("settings.appearance"),
            Some(&t!("settings.appearance_desc")),
            TabBar::new("appearance-picker")
                .segmented()
                .small()
                .selected_index(appearance.index())
                .children(Appearance::ALL.map(|a| Tab::new().child(a.label())))
                .on_click(move |ix, _, cx| {
                    let ix = *ix;
                    update_settings(cx, |s| s.appearance = Appearance::ALL[ix]);
                }),
        );

        let ui_font_row = row(
            cx,
            &t!("settings.ui_font_size"),
            Some(&t!("settings.ui_font_size_desc")),
            size_input(&self.ui_size_input, cx),
        );

        Self::pane(vec![language_row, appearance_row, ui_font_row])
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let current = settings(cx);
        let line_numbers = current.line_numbers;

        let font_row = row(
            cx,
            &t!("settings.font"),
            Some(&t!("settings.font_desc")),
            h_flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_xs()
                        .flex_shrink_0()
                        .text_color(theme.muted_foreground)
                        .child(t!("settings.font_hint")),
                )
                .child(div().w(px(240.)).child(Select::new(&self.font_select))),
        );

        let font_size_row = row(
            cx,
            &t!("settings.font_size"),
            Some(&t!("settings.font_size_desc")),
            size_input(&self.size_input, cx),
        );

        let line_numbers_row = row(
            cx,
            &t!("settings.line_numbers"),
            Some(&t!("settings.line_numbers_desc")),
            // A controlled switch: it paints from the setting, and the click
            // asks for the value it should become.
            Switch::new("toggle-line-numbers")
                .checked(line_numbers)
                .on_change(|&checked, _, cx| {
                    update_settings(cx, |s| s.line_numbers = checked);
                }),
        );

        Self::pane(vec![
            font_row,
            font_size_row,
            line_numbers_row,
            // The card shows the appearance the app is in, since that is the
            // one whose font this page is about.
            preview_card(cx.theme().is_dark(), true, cx),
        ])
    }

    fn render_syntax(&self, cx: &mut Context<Self>) -> AnyElement {
        let dark_active = cx.theme().is_dark();

        let dark_row = row(
            cx,
            &t!("settings.syntax_dark"),
            Some(&t!("settings.syntax_dark_desc")),
            div()
                .w(px(240.))
                .child(Select::new(&self.syntax_dark_select)),
        );

        let light_row = row(
            cx,
            &t!("settings.syntax_light"),
            Some(&t!("settings.syntax_light_desc")),
            div()
                .w(px(240.))
                .child(Select::new(&self.syntax_light_select)),
        );

        Self::pane(vec![
            Self::swatch(dark_row, preview_card(true, dark_active, cx)),
            Self::swatch(light_row, preview_card(false, !dark_active, cx)),
        ])
    }

    /// A picker with its own preview underneath it. Each palette is judged on
    /// its own surface, so one card no longer hides the other choice.
    fn swatch(control: AnyElement, card: AnyElement) -> AnyElement {
        v_flex()
            .w_full()
            .gap_2()
            .child(control)
            .child(card)
            .into_any_element()
    }
}

/// A sample of JSON in one syntax palette.
///
/// `dark` picks which of the two palettes to show *and* paints the card on that
/// mode's own surface, so the light palette is legible while the app itself is
/// dark. The surfaces are the built-in defaults — the same ones [`Theme::change`]
/// installs — not the live theme, because the whole point is to preview the mode
/// that is not currently on screen.
fn preview_card(dark: bool, active: bool, cx: &Context<SettingsHub>) -> AnyElement {
    let surface = if dark {
        ThemeColor::dark()
    } else {
        ThemeColor::light()
    };
    let current = settings(cx);
    let colors = current.syntax_colors(dark);
    div()
        .w_full()
        .rounded_md()
        .border_1()
        // Mark the card whose mode the app is actually using.
        .border_color(if active {
            surface.primary
        } else {
            surface.border
        })
        .bg(surface.background)
        .p_2()
        .text_size(px(current.font_size))
        .font_family(current.mono_family(cx))
        .overflow_hidden()
        .child(v_flex().text_color(surface.foreground).children([
            preview_line(
                "name",
                "\"HJson Viewer\"",
                colors.key.into(),
                colors.string.into(),
            ),
            preview_line("nodes", "48192", colors.key.into(), colors.number.into()),
            preview_line("active", "true", colors.key.into(), colors.bool_kw.into()),
            preview_line("parent", "null", colors.key.into(), colors.null.into()),
            preview_line(
                "path",
                "$.config.nested.deep.path[0]",
                colors.punct.into(),
                colors.string.into(),
            ),
        ]))
        .into_any_element()
}

fn preview_line(
    key: &str,
    value: &str,
    key_color: gpui_kit::Hsla,
    value_color: gpui_kit::Hsla,
) -> AnyElement {
    h_flex()
        .gap_2()
        .whitespace_nowrap()
        .child(
            div()
                .w(px(48.))
                .text_color(key_color)
                .child(key.to_string()),
        )
        .child(div().text_color(value_color).child(value.to_string()))
        .into_any_element()
}

fn row(
    cx: &Context<SettingsHub>,
    title: &str,
    description: Option<&str>,
    control: impl IntoElement,
) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_4()
        .py_1()
        .child(
            v_flex()
                .min_w_0()
                .child(div().text_sm().child(title.to_string()))
                .children(description.map(|d| {
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(d.to_string())
                })),
        )
        .child(control_slot(control))
        .into_any_element()
}

/// The column every row's control sits in. One width for the whole page is what
/// lines the controls up; they hug its right edge, like macOS settings.
const CONTROL_W: f32 = 340.0;

fn control_slot(control: impl IntoElement) -> AnyElement {
    h_flex()
        .w(px(CONTROL_W))
        .flex_shrink_0()
        .items_center()
        .justify_end()
        .gap_2()
        .child(control)
        .into_any_element()
}

/// The box of a size row: the value in the middle, minus on its left and plus
/// on its right. The unit goes inside the field so no second label has to
/// repeat the number.
const SIZE_INPUT_W: f32 = 140.0;

fn size_input(input: &Entity<InputState>, cx: &App) -> AnyElement {
    NumberInput::new(input)
        .w(px(SIZE_INPUT_W))
        .suffix(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("px"),
        )
        .into_any_element()
}

/// Feed one size box into the setting it edits.
///
/// The box reports text — typed or stepped — so the value is parsed. A number
/// outside the range is ignored rather than clamped: "1" on the way to "15"
/// must not shrink the interface to its minimum, and the field clamps itself
/// on blur anyway.
fn size_subscription(
    input: &Entity<InputState>,
    min: f32,
    max: f32,
    cx: &mut Context<SettingsHub>,
    set: impl Fn(&mut Settings, f32) + 'static,
) -> Subscription {
    cx.subscribe(input, move |_, state, event: &InputEvent, cx| {
        if !matches!(event, InputEvent::Change | InputEvent::Blur) {
            return;
        }
        let Ok(value) = state.read(cx).value().parse::<f32>() else {
            return;
        };
        if !(min..=max).contains(&value) {
            return;
        }
        update_settings(cx, |s| set(s, value));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_values() {
        let s = Settings::from_json_str(
            r#"{"mono_font": "Menlo", "font_size": 15.5, "ui_font_size": 18.0, "line_numbers": false, "appearance": "dark"}"#,
        );
        assert_eq!(s.mono_font.as_deref(), Some("Menlo"));
        assert_eq!(s.font_size, 15.5);
        assert_eq!(s.ui_font_size, 18.0);
        assert!(!s.line_numbers);
        assert_eq!(s.appearance, Appearance::Dark);
    }

    #[test]
    fn parse_clamps_and_defaults() {
        let s = Settings::from_json_str(
            r#"{"font_size": 99, "ui_font_size": 1, "line_numbers": true, "appearance": "sepia"}"#,
        );
        assert_eq!(s.font_size, JSON_SIZE_MAX);
        assert_eq!(s.ui_font_size, UI_SIZE_MIN);
        assert_eq!(s.mono_font, None);
        // An unknown mode is not a mode.
        assert_eq!(s.appearance, Appearance::System);

        let s = Settings::from_json_str("");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn appearance_keys_round_trip() {
        for a in Appearance::ALL {
            assert_eq!(Appearance::from_key(a.key()), Some(a));
        }
        assert_eq!(Appearance::ALL.map(Appearance::index), [0, 1, 2]);
    }

    /// The labels the dialog draws come out of the bundle. Tests run in the
    /// fallback locale, `en`, so this also pins the English copy.
    #[test]
    fn labels_are_translated() {
        assert_eq!(Appearance::System.label(), "Follow system");
        assert_eq!(Appearance::Light.label(), "Light");
        assert_eq!(Appearance::Dark.label(), "Dark");
        assert_eq!(tab_label(0), "Appearance");
        assert_eq!(tab_label(1), "JSON Preview");
        assert_eq!(tab_label(2), "Code Colors");
        assert_eq!(follow_theme_label().as_ref(), "Follow system (monospace)");
    }

    /// A key missing from the Chinese bundle falls back to the English text
    /// instead of failing, so the two languages are asserted apart.
    #[test]
    fn labels_are_translated_into_chinese_too() {
        assert_ne!(
            t!("settings.mode_dark", locale = "en"),
            t!("settings.mode_dark", locale = "zh")
        );
        assert_ne!(
            t!("settings.tab_preview", locale = "en"),
            t!("settings.tab_preview", locale = "zh")
        );
        assert_ne!(
            t!("settings.font_follow_system", locale = "en"),
            t!("settings.font_follow_system", locale = "zh")
        );
    }
}
