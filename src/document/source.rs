use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Hsla, SharedString};

/// Where a document came from — the provenance every page labels, icons and
/// colors with. The load menu hands it to the pages that record it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceKind {
    Url,
    File,
    Clipboard,
    Manual,
    Sample,
}

impl SourceKind {
    /// Short tag for history rows and diff side labels.
    pub fn label(self) -> SharedString {
        match self {
            Self::Url => "URL".into(),
            Self::File => tr!("File"),
            Self::Clipboard => tr!("Clipboard"),
            Self::Manual => tr!("Manual input"),
            Self::Sample => tr!("Sample"),
        }
    }

    /// The one glyph for this source, shared by tabs, history and load menu.
    pub fn icon(self) -> IconName {
        match self {
            Self::Url => IconName::Globe,
            Self::File => IconName::FileText,
            Self::Clipboard => IconName::ClipboardList,
            Self::Manual => IconName::PenLine,
            Self::Sample => IconName::Beaker,
        }
    }

    /// Accent color for the source chip, taken from the theme palette so the
    /// same source reads correctly in either appearance.
    pub fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            Self::Url => theme.blue,
            Self::File => theme.green,
            Self::Clipboard => theme.magenta,
            Self::Manual => theme.cyan,
            Self::Sample => theme.yellow,
        }
    }

    /// Title for a document that has no name of its own.
    pub fn default_title(self) -> SharedString {
        match self {
            Self::Clipboard => tr!("Clipboard content"),
            Self::Manual => tr!("Manual input"),
            Self::Sample => tr!("Sample JSON"),
            Self::Url | Self::File => "JSON".into(),
        }
    }
}
