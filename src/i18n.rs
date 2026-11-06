//! The language the interface is drawn in: English, Chinese, or whatever the
//! system reports.
//!
//! Every visible string goes through rust-i18n, so it is looked up by a key in
//! `locales/<module>.<locale>.toml`; `en` is the fallback for a key that only
//! exists in one locale. This module owns only the choice itself — the locale
//! code it resolves to, and the system probe for `Auto`.

use rust_i18n::t;
use serde::{Deserialize, Serialize};

/// `t!` for the places a widget wants a `SharedString` rather than a `&str`,
/// which is most of them. Same lookup, same key.
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::shared(::rust_i18n::t!($key))
    };
    ($key:literal, $($rest:tt)*) => {
        $crate::i18n::shared(::rust_i18n::t!($key, $($rest)*))
    };
}

/// The bundle text as an owned string handle. Accepts whatever `t!` returned:
/// a plain `Cow` for a key without placeholders, an owned `String` when the
/// translation interpolated arguments.
pub fn shared(text: impl std::fmt::Display) -> gpui_kit::SharedString {
    gpui_kit::SharedString::from(text.to_string())
}

/// The locale codes the bundles in `locales/` are named after.
pub const EN: &str = "en";
pub const ZH: &str = "zh";

/// The language choice in the settings dialog.
///
/// `Auto` reads the system locale; the two fixed values never consult it, so
/// picking a language keeps working on a system that reports something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh")]
    Chinese,
}

impl Default for Language {
    /// English is the default; `Auto` has to be asked for.
    fn default() -> Self {
        Self::English
    }
}

impl Language {
    /// Every choice, in the order the picker shows them.
    pub const ALL: [Self; 3] = [Self::Auto, Self::English, Self::Chinese];

    /// The name of the language in that language, the way every system
    /// language picker lists them. `Auto` is the one entry that is a sentence,
    /// so it is translated.
    pub fn label(self) -> String {
        match self {
            Self::Auto => t!("settings.lang_auto").to_string(),
            Self::English => "English".to_string(),
            Self::Chinese => "中文".to_string(),
        }
    }

    /// The key written to the settings file, and the bundle locale for a fixed
    /// choice.
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::English => EN,
            Self::Chinese => ZH,
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }

    /// Position inside [`Language::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.into_iter().position(|l| l == self).unwrap_or(0)
    }

    /// The bundle locale this choice selects right now.
    pub fn locale(self) -> &'static str {
        match self {
            Self::Auto => {
                if system_is_chinese() {
                    ZH
                } else {
                    EN
                }
            }
            Self::English => EN,
            Self::Chinese => ZH,
        }
    }

    /// Make the choice take effect for every `t!` from now on.
    pub fn apply(self) {
        rust_i18n::set_locale(self.locale());
    }
}

/// The system's preferred language, as a locale tag such as `zh-Hans-CN`.
fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

/// Whether that tag is Chinese. Anything the probe does not answer is not
/// Chinese, which is also the fallback locale.
fn system_is_chinese() -> bool {
    system_locale()
        .map(|tag| tag.to_lowercase().replace('_', "-").starts_with("zh"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_english() {
        assert_eq!(Language::default(), Language::English);
        assert_eq!(Language::default().locale(), EN);
    }

    #[test]
    fn keys_round_trip() {
        for lang in Language::ALL {
            assert_eq!(Language::from_key(lang.key()), Some(lang));
        }
        assert_eq!(Language::ALL.map(Language::index), [0, 1, 2]);
        assert_eq!(Language::from_key("fr"), None);
    }

    #[test]
    fn fixed_choices_ignore_the_system() {
        assert_eq!(Language::English.locale(), EN);
        assert_eq!(Language::Chinese.locale(), ZH);
    }

    #[test]
    fn auto_resolves_to_a_bundled_locale() {
        assert!(matches!(Language::Auto.locale(), EN | ZH));
    }

    #[test]
    fn a_tag_is_read_as_chinese_only_for_chinese() {
        for tag in ["zh-CN", "zh_CN.UTF-8", "zh-Hans-US", "en-US", "fr-FR"] {
            assert_eq!(
                tag.to_lowercase().replace('_', "-").starts_with("zh"),
                matches!(tag, "zh-CN" | "zh_CN.UTF-8" | "zh-Hans-US"),
                "{tag}"
            );
        }
    }

    /// `key = value` pairs of one locale file, with `[table]` headers folded
    /// into the key the way `t!` looks them up.
    fn catalog_pairs(file: &str) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("locales")
                .join(file),
        )
        .unwrap();
        let mut table = String::new();
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[') {
                table = format!("{}.", name.trim_end_matches(']'));
                continue;
            }
            let (key, value) = line.split_once('=').unwrap();
            let key = key.trim().trim_matches('"').replace(r"\n", "\n");
            let value = value.trim().trim_matches('"').replace(r"\n", "\n");
            out.push((format!("{table}{key}"), value));
        }
        out
    }

    fn source_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                source_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    fn unescape_key(raw: &str) -> String {
        let mut out = String::new();
        let mut chars = raw.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        out
    }

    fn tr_keys_in_source(text: &str) -> Vec<String> {
        let mut keys = Vec::new();
        let mut rest = text;
        while let Some(ix) = rest.find("tr!(") {
            rest = &rest[ix + 4..];
            rest = rest.trim_start();
            let Some(after_quote) = rest.strip_prefix('"') else {
                continue;
            };

            let mut escaped = false;
            let mut end = None;
            for (ix, ch) in after_quote.char_indices() {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    end = Some(ix);
                    break;
                }
            }

            if let Some(end) = end {
                keys.push(unescape_key(&after_quote[..end]));
                rest = &after_quote[end + 1..];
            } else {
                break;
            }
        }
        keys
    }

    fn contains_chinese(text: &str) -> bool {
        text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }

    /// The two locales are one catalog each: a key added for one language must
    /// be added for the other. Keys are English source strings, and English
    /// values must not fall back to Chinese copy.
    #[test]
    fn both_locales_cover_the_same_keys() {
        for stem in ["auto", "settings"] {
            let en = catalog_pairs(&format!("{stem}.en.toml"));
            let zh = catalog_pairs(&format!("{stem}.zh.toml"));
            let mut en_keys: Vec<&str> = en.iter().map(|(k, _)| k.as_str()).collect();
            let mut zh_keys: Vec<&str> = zh.iter().map(|(k, _)| k.as_str()).collect();
            en_keys.sort_unstable();
            zh_keys.sort_unstable();
            assert_eq!(en_keys, zh_keys, "{stem}: the two locales diverged");
            for key in &en_keys {
                assert!(
                    !contains_chinese(key),
                    "{stem}: key is still Chinese: {key}"
                );
            }
            for (key, value) in &en {
                assert!(!value.is_empty(), "{stem}: {key} has no English text");
                assert!(!contains_chinese(value), "{stem}: {key} is still Chinese");
            }
        }
    }

    #[test]
    fn auto_catalog_covers_all_tr_keys() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        source_files(&root.join("src"), &mut files);

        let mut keys = Vec::new();
        for file in files {
            if file.file_name().is_some_and(|name| name == "i18n.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            keys.extend(tr_keys_in_source(&text));
        }
        keys.sort();
        keys.dedup();

        let en: std::collections::HashSet<_> = catalog_pairs("auto.en.toml")
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        let zh: std::collections::HashSet<_> = catalog_pairs("auto.zh.toml")
            .into_iter()
            .map(|(key, _)| key)
            .collect();

        let missing_en: Vec<_> = keys.iter().filter(|key| !en.contains(*key)).collect();
        let missing_zh: Vec<_> = keys.iter().filter(|key| !zh.contains(*key)).collect();
        assert!(missing_en.is_empty(), "auto.en.toml misses {missing_en:?}");
        assert!(missing_zh.is_empty(), "auto.zh.toml misses {missing_zh:?}");
    }
}
