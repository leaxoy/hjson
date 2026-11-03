//! HJson — a JSON viewer built with GPUI + gpui-kit.

// The translation bundles. Every string the interface shows comes from a key in
// `locales/<module>.<locale>.toml`; `en` is the fallback, so a key that exists
// in only one locale still renders.
rust_i18n::i18n!("locales", fallback = "en");

pub mod app;
pub mod chrome;
pub mod diff;
pub mod diff_view;
pub mod highlight;
pub mod i18n;
pub mod json_model;
pub mod menu;
pub mod pretty;
pub mod sample;
pub mod settings;
pub mod source_menu;
pub mod syntax_theme;
pub mod viewer;
