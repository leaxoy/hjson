//! Load and validate JSON documents from the sources the application supports.
//!
//! UI code owns pickers, dialogs, clipboard access and task scheduling. Once it
//! has a path, URL or text, this module is the single boundary that reads,
//! names, trims and parses the document.

use std::{path::PathBuf, sync::Arc};

use super::SourceKind;
use crate::error::AppError;
use crate::json_model::JsonTree;
use crate::tr;

/// A fully loaded JSON document, ready to be inserted into a tab or diff side.
pub(crate) struct LoadedDocument {
    pub(crate) kind: SourceKind,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) url: Option<String>,
    pub(crate) parsed: Arc<JsonTree>,
}

/// Validate text whose provenance and display metadata are already known.
pub(crate) fn from_text(
    kind: SourceKind,
    title: impl Into<String>,
    text: impl Into<String>,
    url: Option<String>,
) -> Result<LoadedDocument, AppError> {
    from_parts(kind, title.into(), text.into(), url, None)
}

/// Read and validate a document from a filesystem path.
pub(crate) fn from_file(path: PathBuf) -> Result<LoadedDocument, AppError> {
    let text = std::fs::read_to_string(&path).map_err(AppError::read_file)?;
    let title = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file.json")
        .to_string();
    from_parts(SourceKind::File, title, text, None, None)
}

/// Fetch and validate a document from a URL.
pub(crate) fn from_url(
    url: String,
    custom_title: Option<String>,
) -> Result<LoadedDocument, AppError> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::load(tr!("Enter a URL")));
    }
    let response = ureq::get(&url).call().map_err(AppError::request)?;
    let text = response
        .into_body()
        .read_to_string()
        .map_err(AppError::read_response)?;
    let title = import_title(&url, custom_title);
    from_parts(SourceKind::Url, title, text, Some(url), None)
}

/// Validate text read from the clipboard.
pub(crate) fn from_clipboard(text: impl Into<String>) -> Result<LoadedDocument, AppError> {
    let kind = SourceKind::Clipboard;
    from_text(kind, kind.default_title().to_string(), text.into(), None)
}

/// Validate text entered manually.
pub(crate) fn from_manual(text: impl Into<String>) -> Result<LoadedDocument, AppError> {
    let kind = SourceKind::Manual;
    from_text(kind, kind.default_title().to_string(), text.into(), None)
}

/// Load the built-in sample document.
pub(crate) fn from_sample() -> Result<LoadedDocument, AppError> {
    let kind = SourceKind::Sample;
    from_text(
        kind,
        kind.default_title().to_string(),
        super::sample::TEXT.to_string(),
        None,
    )
}

/// Restore a document from session history, reusing its parsed tree when it is
/// still alive.
pub(crate) fn from_history(
    kind: SourceKind,
    title: impl Into<String>,
    text: impl Into<String>,
    url: Option<String>,
    parsed: Option<Arc<JsonTree>>,
) -> Result<LoadedDocument, AppError> {
    from_parts(kind, title.into(), text.into(), url, parsed)
}

fn from_parts(
    kind: SourceKind,
    title: String,
    text: String,
    url: Option<String>,
    parsed: Option<Arc<JsonTree>>,
) -> Result<LoadedDocument, AppError> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::parse(tr!("Nothing to compare")));
    }
    let parsed = parsed.unwrap_or_else(|| Arc::new(JsonTree::parse(&text)));
    if let Some(err) = &parsed.error {
        return Err(AppError::parse(err));
    }
    Ok(LoadedDocument {
        kind,
        title,
        text,
        url,
        parsed,
    })
}

fn import_title(url: &str, custom: Option<String>) -> String {
    custom
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| url.split('/').next_back().unwrap_or("JSON").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_title_prefers_non_empty_custom_title() {
        let url = "https://example.com/default.json";
        assert_eq!(
            import_title(url, Some("  Custom tab  ".to_string())),
            "Custom tab"
        );
        assert_eq!(import_title(url, Some("  ".to_string())), "default.json");
        assert_eq!(import_title(url, None), "default.json");
    }
}
