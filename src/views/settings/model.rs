use super::codec;
use crate::i18n::Language;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Global, SharedString};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Appearance {
    Light,
    Dark,
    #[default]
    #[serde(other)]
    System,
}

impl Appearance {
    pub(super) const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub(super) fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|appearance| appearance.key() == key)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    #[serde(deserialize_with = "codec::language")]
    pub(crate) language: Language,
    pub(super) appearance: Appearance,
    #[serde(deserialize_with = "codec::ui_font_size")]
    pub(crate) ui_font_size: f32,
    #[serde(deserialize_with = "codec::zoom")]
    pub(crate) zoom: f32,

    #[serde(deserialize_with = "codec::font_family")]
    pub(crate) font_family: Option<SharedString>,
    #[serde(deserialize_with = "codec::font_size")]
    pub(crate) font_size: f32,
    pub(crate) line_numbers: bool,
    pub(crate) wrap_raw_lines: bool,
    pub(crate) sort_object_keys: bool,

    #[serde(deserialize_with = "codec::syntax_dark")]
    pub(crate) syntax_dark: SharedString,
    #[serde(deserialize_with = "codec::syntax_light")]
    pub(crate) syntax_light: SharedString,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            appearance: Appearance::default(),
            ui_font_size: 16.0,
            zoom: 1.0,

            font_family: None,
            font_size: 13.0,
            line_numbers: true,
            wrap_raw_lines: false,
            sort_object_keys: true,

            syntax_dark: SharedString::from("Tokyo Night"),
            syntax_light: SharedString::from("GitHub"),
        }
    }
}

impl Settings {
    pub(super) const FONT_SIZE_RANGE: (f32, f32) = (10.0, 20.0);
    pub(super) const UI_FONT_SIZE_RANGE: (f32, f32) = (12.0, 22.0);

    const ZOOM_RANGE: (f32, f32) = (0.5, 3.0);
    const ZOOM_STEP: f32 = 1.2;

    pub(super) fn clamp_font_size(value: f32) -> f32 {
        value.clamp(Self::FONT_SIZE_RANGE.0, Self::FONT_SIZE_RANGE.1)
    }

    pub(super) fn clamp_ui_font_size(value: f32) -> f32 {
        value.clamp(Self::UI_FONT_SIZE_RANGE.0, Self::UI_FONT_SIZE_RANGE.1)
    }

    pub(super) fn clamp_zoom(value: f32) -> f32 {
        value.clamp(Self::ZOOM_RANGE.0, Self::ZOOM_RANGE.1)
    }

    pub(super) fn zoom_in(&mut self) {
        self.zoom = Self::clamp_zoom(self.zoom * Self::ZOOM_STEP);
    }

    pub(super) fn zoom_out(&mut self) {
        self.zoom = Self::clamp_zoom(self.zoom / Self::ZOOM_STEP);
    }

    pub(crate) fn mono_family(&self, cx: &App) -> SharedString {
        self.font_family
            .clone()
            .unwrap_or_else(|| cx.theme().mono_font_family.clone())
    }

    pub(crate) fn ui_rem(&self) -> f32 {
        self.ui_font_size * self.zoom
    }

    pub(crate) fn json_font(&self) -> f32 {
        self.font_size * self.zoom
    }

    pub(crate) fn row_height(&self) -> f32 {
        self.json_font() + 8.0
    }
}

impl Global for Settings {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_keys_round_trip() {
        for appearance in Appearance::ALL {
            assert_eq!(Appearance::from_key(appearance.key()), Some(appearance));
        }
        assert_eq!(
            Appearance::ALL.map(Appearance::key),
            ["system", "light", "dark"]
        );
    }
}
