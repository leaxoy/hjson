use crate::i18n::Language;
use gpui_kit::component::{ActiveTheme as _, ThemeMode};
use gpui_kit::{App, Global, SharedString};
use rust_i18n::t;
use std::borrow::Cow;

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
pub(crate) const JSON_SIZE_MIN: f32 = 10.0;
pub(crate) const JSON_SIZE_MAX: f32 = 20.0;
/// Allowed range for the interface root font size (the `rem` base).
pub(crate) const UI_SIZE_MIN: f32 = 12.0;
pub(crate) const UI_SIZE_MAX: f32 = 22.0;
/// Whole-interface zoom, a multiplier on both sizes above. It is kept apart
/// from them so `⌘0` can return to 100% without forgetting what the user
/// picked, and the range stops a runaway key from breaking the layout.
pub(super) const ZOOM_MIN: f32 = 0.5;
pub(super) const ZOOM_MAX: f32 = 3.0;
/// One zoom step, applied multiplicatively the way a browser zooms.
pub(super) const ZOOM_STEP: f32 = 1.2;

/// The light / dark mode of the whole interface.
///
/// `None` follows the OS and keeps tracking its changes. A fixed
/// [`ThemeMode`] also forces the native window chrome (on macOS
/// `NSApplication.appearance`), so the title bar matches the theme instead of
/// the desktop.
pub(super) const APPEARANCE_SYSTEM: &str = "system";
pub(crate) const APPEARANCE_OPTIONS: [Option<ThemeMode>; 3] =
    [None, Some(ThemeMode::Light), Some(ThemeMode::Dark)];

pub(crate) fn appearance_label(appearance: Option<ThemeMode>) -> Cow<'static, str> {
    match appearance {
        None => t!("settings.mode_system"),
        Some(ThemeMode::Light) => t!("settings.mode_light"),
        Some(ThemeMode::Dark) => t!("settings.mode_dark"),
    }
}

pub(crate) fn appearance_key(appearance: Option<ThemeMode>) -> &'static str {
    match appearance {
        None => APPEARANCE_SYSTEM,
        Some(mode) => mode.name(),
    }
}

pub(crate) fn appearance_from_key(key: &str) -> Option<Option<ThemeMode>> {
    match key {
        APPEARANCE_SYSTEM => Some(None),
        "light" => Some(Some(ThemeMode::Light)),
        "dark" => Some(Some(ThemeMode::Dark)),
        _ => None,
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
    /// Zoom applied on top of both sizes above, as a multiplier.
    pub zoom: f32,
    pub line_numbers: bool,
    /// Wrap long lines in the raw JSON panel instead of clipping them.
    pub wrap_raw_lines: bool,
    /// Display object members alphabetically in the structure tree.
    pub sort_object_keys: bool,
    /// Light / dark mode, or follow the OS.
    pub appearance: Option<ThemeMode>,
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
            zoom: 1.0,
            line_numbers: true,
            wrap_raw_lines: false,
            sort_object_keys: true,
            appearance: None,
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
            if let Some(zoom) = json.get("zoom").and_then(|v| v.as_f64()) {
                defaults.zoom = (zoom as f32).clamp(ZOOM_MIN, ZOOM_MAX);
            }
            if let Some(ln) = json.get("line_numbers").and_then(|v| v.as_bool()) {
                defaults.line_numbers = ln;
            }
            if let Some(wrap) = json.get("wrap_raw_lines").and_then(|v| v.as_bool()) {
                defaults.wrap_raw_lines = wrap;
            }
            if let Some(sort) = json.get("sort_object_keys").and_then(|v| v.as_bool()) {
                defaults.sort_object_keys = sort;
            }
            if let Some(key) = json.get("appearance").and_then(|v| v.as_str())
                && let Some(a) = appearance_from_key(key)
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
                && crate::syntax::find(name).is_some()
            {
                defaults.syntax_dark = SharedString::from(name);
            }
            if let Some(name) = json.get("syntax_light").and_then(|v| v.as_str())
                && crate::syntax::find(name).is_some()
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

    /// Root font size the window is given: the base the user chose, zoomed.
    /// Everything sized in `rem` — the whole interface — scales with it.
    #[inline]
    pub fn ui_rem(&self) -> f32 {
        self.ui_font_size * self.zoom
    }

    /// Font size of the raw JSON panel, zoomed.
    #[inline]
    pub fn json_font(&self) -> f32 {
        self.font_size * self.zoom
    }

    /// Line height derived from the font size.
    #[inline]
    pub fn row_height(&self) -> f32 {
        self.json_font() + 8.0
    }
}

impl Global for Settings {}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_i18n::t;

    #[test]
    fn parse_file_values() {
        let s = Settings::from_json_str(
            r#"{"mono_font": "Menlo", "font_size": 15.5, "ui_font_size": 18.0, "line_numbers": false, "wrap_raw_lines": true, "sort_object_keys": false, "appearance": "dark"}"#,
        );
        assert_eq!(s.mono_font.as_deref(), Some("Menlo"));
        assert_eq!(s.font_size, 15.5);
        assert_eq!(s.ui_font_size, 18.0);
        assert!(!s.line_numbers);
        assert!(s.wrap_raw_lines);
        assert!(!s.sort_object_keys);
        assert_eq!(s.appearance, Some(ThemeMode::Dark));
    }

    #[test]
    fn parse_clamps_and_defaults() {
        let s = Settings::from_json_str(
            r#"{"font_size": 99, "ui_font_size": 1, "line_numbers": true, "appearance": "sepia"}"#,
        );
        assert_eq!(s.font_size, JSON_SIZE_MAX);
        assert_eq!(s.ui_font_size, UI_SIZE_MIN);
        assert_eq!(s.mono_font, None);
        assert!(s.sort_object_keys);
        // An unknown mode is not a mode.
        assert_eq!(s.appearance, None);

        let s = Settings::from_json_str("");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn appearance_keys_round_trip() {
        for appearance in APPEARANCE_OPTIONS {
            assert_eq!(
                appearance_from_key(appearance_key(appearance)),
                Some(appearance)
            );
        }
        assert_eq!(
            APPEARANCE_OPTIONS.map(appearance_key),
            ["system", "light", "dark"]
        );
    }

    /// The labels the dialog draws come out of the bundle. Tests run in the
    /// fallback locale, `en`, so this also pins the English copy.
    #[test]
    fn labels_are_translated() {
        assert_eq!(appearance_label(None), "Follow system");
        assert_eq!(appearance_label(Some(ThemeMode::Light)), "Light");
        assert_eq!(appearance_label(Some(ThemeMode::Dark)), "Dark");
        assert_eq!(t!("settings.tab_appearance"), "Appearance");
        assert_eq!(t!("settings.tab_preview"), "JSON Preview");
        assert_eq!(t!("settings.tab_syntax"), "Code Colors");
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
