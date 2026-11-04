use self::model::{
    APPEARANCE_OPTIONS, JSON_SIZE_MAX, JSON_SIZE_MIN, UI_SIZE_MAX, UI_SIZE_MIN,
    appearance_from_key, appearance_key, appearance_label,
};
use crate::i18n::Language;
use crate::syntax::{self, SyntaxMode};
use gpui_kit::assets::IconName;
use gpui_kit::component::theme::ThemeColor;
use gpui_kit::component::{
    h_flex,
    setting::{
        AnySettingField, NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage,
        Settings as SettingsView,
    },
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, IntoElement, ParentElement as _, Render, RenderOnce, SharedString,
    Styled as _, Window, div, px,
};
use rust_i18n::t;
use std::borrow::Cow;

gpui_kit::actions!(hjson, [OpenSettings, Quit, ToggleLineNumbers]);

pub(crate) mod model;
pub(crate) mod runtime;

pub use model::{Settings, follow_theme_label};
pub use runtime::{
    apply_appearance, init, mono_font_options, reset_zoom, settings, update_settings, zoom_in,
    zoom_out,
};

pub(crate) const SETTINGS_DIALOG_W: f32 = 620.0;
pub(crate) const SETTINGS_DIALOG_H: f32 = 560.0;
const SETTINGS_SIDEBAR_W: f32 = 180.0;

pub(crate) struct SettingsHub;

impl Render for SettingsHub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Self::view(cx).render(window, cx)
    }
}

impl SettingsHub {
    fn view(cx: &App) -> SettingsView {
        SettingsView::new("hjson-settings")
            .sidebar_width(px(SETTINGS_SIDEBAR_W))
            .page(Self::appearance_page())
            .page(Self::preview_page(cx))
            .page(Self::syntax_page())
    }

    fn item<F>(title: Cow<'static, str>, description: Cow<'static, str>, field: F) -> SettingItem
    where
        F: AnySettingField + 'static,
    {
        SettingItem::new(title.to_string(), field).description(description.to_string())
    }

    fn dropdown(
        title: Cow<'static, str>,
        description: Cow<'static, str>,
        options: Vec<(SharedString, SharedString)>,
        value: impl Fn(&App) -> SharedString + 'static,
        set_value: impl Fn(SharedString, &mut App) + 'static,
        default: SharedString,
    ) -> SettingItem {
        Self::item(
            title,
            description,
            SettingField::dropdown(options, value, set_value).default_value(default),
        )
    }

    fn scrollable_dropdown(
        title: Cow<'static, str>,
        description: Cow<'static, str>,
        options: Vec<(SharedString, SharedString)>,
        value: impl Fn(&App) -> SharedString + 'static,
        set_value: impl Fn(SharedString, &mut App) + 'static,
        default: SharedString,
    ) -> SettingItem {
        Self::item(
            title,
            description,
            SettingField::scrollable_dropdown(options, value, set_value).default_value(default),
        )
    }

    fn number(
        title: Cow<'static, str>,
        description: Cow<'static, str>,
        range: (f32, f32),
        value: impl Fn(&App) -> f64 + 'static,
        set_value: impl Fn(f64, &mut App) + 'static,
        default: f64,
    ) -> SettingItem {
        Self::item(
            title,
            description,
            SettingField::number_input(
                Self::number_options(range.0, range.1, 0.5),
                value,
                set_value,
            )
            .default_value(default),
        )
    }

    fn switch(
        title: Cow<'static, str>,
        description: Cow<'static, str>,
        value: impl Fn(&App) -> bool + 'static,
        set_value: impl Fn(bool, &mut App) + 'static,
        default: bool,
    ) -> SettingItem {
        Self::item(
            title,
            description,
            SettingField::switch(value, set_value).default_value(default),
        )
    }

    fn appearance_page() -> SettingPage {
        SettingPage::new(t!("settings.tab_appearance").to_string())
            .icon(IconName::SunMoon)
            .group(
                SettingGroup::new()
                    .item(Self::dropdown(
                        t!("settings.language"),
                        t!("settings.language_hint"),
                        Self::language_options(),
                        |cx| SharedString::from(settings(cx).language.key()),
                        |value, cx| {
                            if let Some(language) = Language::from_key(value.as_ref()) {
                                update_settings(cx, |s| s.language = language);
                            }
                        },
                        SharedString::from(Language::default().key()),
                    ))
                    .item(Self::dropdown(
                        t!("settings.appearance"),
                        t!("settings.appearance_desc"),
                        Self::appearance_options(),
                        |cx| SharedString::from(appearance_key(settings(cx).appearance)),
                        |value, cx| {
                            if let Some(appearance) = appearance_from_key(value.as_ref()) {
                                update_settings(cx, |s| s.appearance = appearance);
                            }
                        },
                        SharedString::from(appearance_key(Settings::default().appearance)),
                    ))
                    .item(Self::number(
                        t!("settings.ui_font_size"),
                        t!("settings.ui_font_size_desc"),
                        (UI_SIZE_MIN, UI_SIZE_MAX),
                        |cx| settings(cx).ui_font_size as f64,
                        |value, cx| {
                            update_settings(cx, |s| {
                                s.ui_font_size = (value as f32).clamp(UI_SIZE_MIN, UI_SIZE_MAX)
                            });
                        },
                        Settings::default().ui_font_size as f64,
                    )),
            )
    }

    fn preview_page(cx: &App) -> SettingPage {
        SettingPage::new(t!("settings.tab_preview").to_string())
            .icon(IconName::Braces)
            .group(
                SettingGroup::new()
                    .item(Self::scrollable_dropdown(
                        t!("settings.font"),
                        t!("settings.font_desc"),
                        Self::mono_font_dropdown_options(cx),
                        |cx| settings(cx).mono_font.clone().unwrap_or_default(),
                        |value, cx| {
                            let mono_font = (!value.is_empty()).then_some(value);
                            update_settings(cx, |s| s.mono_font = mono_font);
                        },
                        SharedString::default(),
                    ))
                    .item(Self::number(
                        t!("settings.font_size"),
                        t!("settings.font_size_desc"),
                        (JSON_SIZE_MIN, JSON_SIZE_MAX),
                        |cx| settings(cx).font_size as f64,
                        |value, cx| {
                            update_settings(cx, |s| {
                                s.font_size = (value as f32).clamp(JSON_SIZE_MIN, JSON_SIZE_MAX)
                            });
                        },
                        Settings::default().font_size as f64,
                    ))
                    .item(Self::switch(
                        t!("settings.line_numbers"),
                        t!("settings.line_numbers_desc"),
                        |cx| settings(cx).line_numbers,
                        |value, cx| update_settings(cx, |s| s.line_numbers = value),
                        Settings::default().line_numbers,
                    ))
                    .item(Self::switch(
                        t!("settings.wrap_raw_lines"),
                        t!("settings.wrap_raw_lines_desc"),
                        |cx| settings(cx).wrap_raw_lines,
                        |value, cx| update_settings(cx, |s| s.wrap_raw_lines = value),
                        Settings::default().wrap_raw_lines,
                    ))
                    .item(Self::switch(
                        t!("settings.sort_object_keys"),
                        t!("settings.sort_object_keys_desc"),
                        |cx| settings(cx).sort_object_keys,
                        |value, cx| update_settings(cx, |s| s.sort_object_keys = value),
                        Settings::default().sort_object_keys,
                    )),
            )
            .group(SettingGroup::new().item(SettingItem::render(|_, _, cx| {
                Self::preview_card(SyntaxMode::current(cx), true, cx)
            })))
    }

    fn syntax_page() -> SettingPage {
        SettingPage::new(t!("settings.tab_syntax").to_string())
            .icon(IconName::Palette)
            .group(
                SettingGroup::new()
                    .item(Self::scrollable_dropdown(
                        t!("settings.syntax_dark"),
                        t!("settings.syntax_dark_desc"),
                        Self::syntax_theme_options(),
                        |cx| settings(cx).syntax_dark.clone(),
                        |value, cx| update_settings(cx, move |s| s.syntax_dark = value),
                        Settings::default().syntax_dark,
                    ))
                    .item(SettingItem::render(|_, _, cx| {
                        let mode = SyntaxMode::Dark;
                        Self::preview_card(mode, mode.is_current(cx), cx)
                    }))
                    .item(Self::scrollable_dropdown(
                        t!("settings.syntax_light"),
                        t!("settings.syntax_light_desc"),
                        Self::syntax_theme_options(),
                        |cx| settings(cx).syntax_light.clone(),
                        |value, cx| update_settings(cx, move |s| s.syntax_light = value),
                        Settings::default().syntax_light,
                    ))
                    .item(SettingItem::render(|_, _, cx| {
                        let mode = SyntaxMode::Light;
                        Self::preview_card(mode, mode.is_current(cx), cx)
                    })),
            )
    }

    fn language_options() -> Vec<(SharedString, SharedString)> {
        Language::ALL
            .into_iter()
            .map(|language| {
                (
                    SharedString::from(language.key()),
                    SharedString::from(language.label()),
                )
            })
            .collect()
    }

    fn appearance_options() -> Vec<(SharedString, SharedString)> {
        APPEARANCE_OPTIONS
            .into_iter()
            .map(|appearance| {
                (
                    SharedString::from(appearance_key(appearance)),
                    SharedString::from(appearance_label(appearance)),
                )
            })
            .collect()
    }

    fn mono_font_dropdown_options(cx: &App) -> Vec<(SharedString, SharedString)> {
        mono_font_options(cx)
            .into_iter()
            .enumerate()
            .map(|(ix, label)| {
                let value = if ix == 0 {
                    SharedString::default()
                } else {
                    label.clone()
                };
                (value, label)
            })
            .collect()
    }

    fn syntax_theme_options() -> Vec<(SharedString, SharedString)> {
        crate::syntax::theme_names()
            .into_iter()
            .map(|name| (SharedString::from(name), SharedString::from(name)))
            .collect()
    }

    fn number_options(min: f32, max: f32, step: f32) -> NumberFieldOptions {
        NumberFieldOptions {
            min: min as f64,
            max: max as f64,
            step: step as f64,
        }
    }

    /// A sample of JSON in one syntax palette.
    ///
    /// `mode` picks which of the two palettes to show *and* paints the card on
    /// that mode's own surface, so the light palette is legible while the app
    /// itself is dark. The surfaces are the built-in defaults — the same ones
    /// [`Theme::change`] installs — not the live theme, because the whole point
    /// is to preview the mode that is not currently on screen.
    fn preview_card(mode: SyntaxMode, active: bool, cx: &App) -> AnyElement {
        let surface = if mode.is_dark() {
            ThemeColor::dark()
        } else {
            ThemeColor::light()
        };
        let current = settings(cx);
        let colors = syntax::colors_for(mode, cx);
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
            .text_size(px(current.json_font()))
            .font_family(current.mono_family(cx))
            .overflow_hidden()
            .child(v_flex().text_color(surface.foreground).children([
                Self::preview_line(
                    "name",
                    "\"HJson Viewer\"",
                    colors.key.into(),
                    colors.string.into(),
                ),
                Self::preview_line("nodes", "48192", colors.key.into(), colors.number.into()),
                Self::preview_line("active", "true", colors.key.into(), colors.bool_kw.into()),
                Self::preview_line("parent", "null", colors.key.into(), colors.null.into()),
                Self::preview_line(
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
}
