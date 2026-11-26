use std::path::{Path, PathBuf};

use gpui_kit::http_client::Url;
use gpui_kit::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};

use super::{AppWindows, HJsonApp};

/// Create the main viewer, including when an OS open request arrives after
/// the previous window was closed.
pub fn open_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1280.), px(860.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(900.), px(600.))),
        ..Default::default()
    };

    gpui_kit::open_window(options, cx, |window, cx| {
        window.set_window_title("HJson — JSON Viewer");
        crate::views::settings::apply_appearance(Some(window), cx);
        window
            .observe_window_appearance(|window, cx| {
                crate::views::settings::apply_appearance(Some(window), cx);
            })
            .detach();
        cx.new(|cx| HJsonApp::new(window, cx))
    })
    .expect("failed to open window");
}

/// Handle local file URLs delivered by the OS. URL decoding preserves spaces,
/// Unicode and literal percent signs in filenames; remote URLs are ignored.
pub fn open_external_urls(urls: Vec<String>, cx: &mut App) {
    open_external_paths(
        urls.iter().filter_map(|url| json_file_url(url)).collect(),
        cx,
    );
}

/// Open only JSON paths from launch arguments or native file-open events.
pub fn open_external_paths(paths: Vec<PathBuf>, cx: &mut App) {
    let paths: Vec<_> = paths
        .into_iter()
        .filter(|path| is_json_path(path))
        .collect();
    if paths.is_empty() {
        return;
    }

    let active = cx.active_window();
    let app = cx.try_global::<AppWindows>().and_then(|windows| {
        windows
            .0
            .iter()
            .rev()
            .filter(|(_, app)| app.upgrade().is_some())
            .find(|(window, _)| Some(*window) == active)
            .or_else(|| {
                windows
                    .0
                    .iter()
                    .rev()
                    .find(|(_, app)| app.upgrade().is_some())
            })
            .map(|(_, app)| app.clone())
    });
    let app = app.unwrap_or_else(|| {
        open_window(cx);
        cx.global::<AppWindows>().0.last().unwrap().1.clone()
    });
    let _ = app.update_in(cx, |app, window, cx| {
        window.activate_window();
        app.open_paths(paths, cx);
    });
}

fn is_json_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

fn json_file_url(input: &str) -> Option<PathBuf> {
    let url = Url::parse(input).ok()?;
    if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let path = url.to_file_path().ok()?;
    is_json_path(&path).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_paths_accept_only_json_extensions() {
        for path in ["data.json", "data.JSON", "a folder/中文.Json"] {
            assert!(is_json_path(Path::new(path)), "{path}");
        }
        for path in [
            "data.jsonc",
            "data.json5",
            "data.txt",
            "data.json.bak",
            "json",
        ] {
            assert!(!is_json_path(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn file_urls_decode_special_characters() {
        let path = std::env::temp_dir().join("a folder/中文 # 100%.JSON");
        let url = Url::from_file_path(&path).unwrap();
        assert_eq!(json_file_url(url.as_str()), Some(path));
    }

    #[test]
    fn decoded_file_urls_load_documents_and_report_file_errors() {
        let path = std::env::temp_dir().join(format!(
            "hjson-open-{}-中文 # 100%.json",
            std::process::id()
        ));
        let url = Url::from_file_path(&path).unwrap();
        let text = r#"{"opened":true}"#;
        std::fs::write(&path, text).unwrap();
        let document = crate::document::from_file(json_file_url(url.as_str()).unwrap()).unwrap();
        assert_eq!(document.kind, crate::document::SourceKind::File);
        assert_eq!(document.title, path.file_name().unwrap().to_str().unwrap());
        assert_eq!(document.text, text);
        assert_eq!(document.parsed.root["opened"], true);

        std::fs::write(&path, "invalid JSON").unwrap();
        assert!(matches!(
            crate::document::from_file(path.clone()),
            Err(crate::error::AppError::Parse(_))
        ));
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            crate::document::from_file(path),
            Err(crate::error::AppError::ReadFile(_))
        ));
    }

    #[test]
    fn external_urls_reject_other_types_and_remote_sources() {
        for url in [
            "https://example.com/data.json",
            "file:///tmp/data.txt",
            "file:///tmp/data.jsonc",
            "file:///tmp/data.json5",
            "file:///tmp/data.json?query",
            "file:///tmp/data.json#fragment",
            "invalid URL",
        ] {
            assert_eq!(json_file_url(url), None, "{url}");
        }
    }
}
