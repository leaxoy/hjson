use super::model::Settings;
use crate::i18n::Language;
use gpui_kit::SharedString;
use serde::{Deserialize, Deserializer};

pub(super) fn font_family<'de, D>(deserializer: D) -> Result<Option<SharedString>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?
        .filter(|font| !font.is_empty())
        .map(SharedString::from))
}

pub(super) fn font_size<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    number_or_default(
        deserializer,
        Settings::default().font_size,
        Settings::clamp_font_size,
    )
}

pub(super) fn ui_font_size<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    number_or_default(
        deserializer,
        Settings::default().ui_font_size,
        Settings::clamp_ui_font_size,
    )
}

pub(super) fn zoom<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    number_or_default(deserializer, Settings::default().zoom, Settings::clamp_zoom)
}

pub(super) fn language<'de, D>(deserializer: D) -> Result<Language, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?
        .as_deref()
        .and_then(Language::from_key)
        .unwrap_or_default())
}

pub(super) fn syntax_dark<'de, D>(deserializer: D) -> Result<SharedString, D::Error>
where
    D: Deserializer<'de>,
{
    syntax_theme_or_default(deserializer, Settings::default().syntax_dark)
}

pub(super) fn syntax_light<'de, D>(deserializer: D) -> Result<SharedString, D::Error>
where
    D: Deserializer<'de>,
{
    syntax_theme_or_default(deserializer, Settings::default().syntax_light)
}

fn number_or_default<'de, D>(
    deserializer: D,
    default: f32,
    clamp: fn(f32) -> f32,
) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<serde_json::Value>::deserialize(deserializer)?
        .and_then(|value| value.as_f64())
        .map(|value| clamp(value as f32))
        .unwrap_or(default))
}

fn syntax_theme_or_default<'de, D>(
    deserializer: D,
    default: SharedString,
) -> Result<SharedString, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?
        .filter(|name| crate::syntax::find(name).is_some())
        .map(SharedString::from)
        .unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::super::model::{Appearance, Settings};
    use super::*;

    #[test]
    fn parses_values_and_preserves_missing_defaults() {
        let settings = serde_json::from_str::<Settings>(
            r#"{"font_family":"Menlo","font_size":15.5,"ui_font_size":18,"line_numbers":false,"wrap_raw_lines":true,"sort_object_keys":false,"appearance":"dark"}"#,
        )
        .unwrap();

        assert_eq!(settings.font_family.as_deref(), Some("Menlo"));
        assert_eq!(settings.font_size, 15.5);
        assert_eq!(settings.ui_font_size, 18.0);
        assert!(!settings.line_numbers);
        assert!(settings.wrap_raw_lines);
        assert!(!settings.sort_object_keys);
        assert_eq!(settings.appearance, Appearance::Dark);
        assert_eq!(settings.language, Language::default());
    }

    #[test]
    fn clamps_values_and_ignores_unknown_options() {
        let settings = serde_json::from_str::<Settings>(
            r#"{"font_size":99,"ui_font_size":1,"line_numbers":true,"appearance":"sepia","language":"fr"}"#,
        )
        .unwrap();

        assert_eq!(settings.font_size, Settings::FONT_SIZE_RANGE.1);
        assert_eq!(settings.ui_font_size, Settings::UI_FONT_SIZE_RANGE.0);
        assert_eq!(settings.appearance, Appearance::System);
        assert_eq!(settings.language, Language::default());
    }

    #[test]
    fn writes_stable_keys() {
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        assert!(defaults["font_family"].is_null());
        assert_eq!(defaults["appearance"], "system");
        assert_eq!(defaults["language"], "en");

        let settings = Settings {
            font_family: Some(SharedString::from("Menlo")),
            appearance: Appearance::Dark,
            language: Language::Chinese,
            ..Settings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();

        assert_eq!(json["font_family"], "Menlo");
        assert_eq!(json["appearance"], "dark");
        assert_eq!(json["language"], "zh");
        assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), settings);
    }
}
