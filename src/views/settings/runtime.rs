use super::model::{Settings, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP, appearance_key, follow_theme_label};
use super::{
    OpenSettings, Quit, SETTINGS_DIALOG_H, SETTINGS_DIALOG_W, SettingsHub, ToggleLineNumbers,
};
use gpui_kit::component::{Theme, ThemeMode, WindowExt as _};
use gpui_kit::{
    App, AppContext as _, Entity, Global, KeyBinding, ParentElement as _, SharedString,
    Styled as _, Window, WindowAppearance, px,
};
use rust_i18n::t;

/// The glyphs the mono probe measures: the widest caps and the narrowest
/// lowercase. Only a monospace family gives them all one advance.
const MONO_PROBE_CHARS: [char; 4] = ['M', 'W', 'X', 'i'];
/// The size they are measured at, in px.
const MONO_PROBE_SIZE: f32 = 16.0;

pub fn settings(cx: &App) -> &Settings {
    cx.global::<Settings>()
}

/// Mutate settings, repaint every window, and persist to the config file.
pub fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    let old_line_numbers = settings(cx).line_numbers;
    let old_sort_object_keys = settings(cx).sort_object_keys;
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
    if settings(cx).sort_object_keys != old_sort_object_keys {
        crate::app::refresh_doc_view_key_sort(cx);
    }
    cx.refresh_windows();
    persist(cx);
    if menu_stale {
        crate::app::menu::install(cx);
    }
}

/// Step the interface zoom, in and out. The two base sizes stay untouched, so
/// the settings dialog keeps showing what was chosen there.
pub fn zoom_in(cx: &mut App) {
    update_settings(cx, |s| {
        s.zoom = (s.zoom * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX)
    });
}

pub fn zoom_out(cx: &mut App) {
    update_settings(cx, |s| {
        s.zoom = (s.zoom / ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX)
    });
}

pub fn reset_zoom(cx: &mut App) {
    update_settings(cx, |s| s.zoom = 1.0);
}

/// Push [`Settings::appearance`] onto the platform and the theme.
///
/// `window` is only used by the follow-system branch, where the OS value has to
/// be read; see [`Theme::sync_system_appearance`].
pub fn apply_appearance(window: Option<&mut Window>, cx: &mut App) {
    match settings(cx).appearance {
        None => {
            cx.set_window_appearance(None);
            Theme::sync_system_appearance(window, cx);
        }
        Some(ThemeMode::Light) => force_appearance(WindowAppearance::Light, cx),
        Some(ThemeMode::Dark) => force_appearance(WindowAppearance::Dark, cx),
    }
}

/// A fixed mode: the native chrome is forced first, then the theme is set to
/// match, because `Theme::sync_system_appearance` would read back whatever the
/// platform reports and the window's own copy can lag behind the override.
fn force_appearance(appearance: WindowAppearance, cx: &mut App) {
    cx.set_window_appearance(Some(appearance));
    Theme::change(appearance, None, cx);
}

/// The monospace families on this machine, or `None` while the probe is still
/// running. The list holds no translated text: the dropdown's "follow the
/// theme" entry is built where it is used, so a language change cannot stale
/// it and the probe never has to run twice.
#[derive(Default)]
struct MonoFamilies(Option<Vec<SharedString>>);

impl Global for MonoFamilies {}

/// Every installed font family that shapes ASCII with equal per-glyph advances
/// (i.e. real monospace fonts), behind the pseudo entry in
/// [`follow_theme_label`].
///
/// The probe is a few hundred milliseconds of text work, so [`init`] runs it in
/// the background and this reads the cache. Opening the dialog before the probe
/// lands pays for it inline instead.
pub fn mono_font_options(cx: &App) -> Vec<SharedString> {
    // The background probe has normally landed long before a dialog can open.
    // Until it has, pay for it here rather than show a half-filled dropdown.
    let families = match cx.try_global::<MonoFamilies>() {
        Some(MonoFamilies(Some(families))) => families.clone(),
        _ => probe_mono_families(cx.text_system()),
    };
    let mut out = vec![follow_theme_label()];
    out.extend(families);
    out
}

/// Start the background probe, so the first settings dialog does not wait for
/// it. Nothing else can invalidate the result: fonts do not come and go while
/// the app runs.
fn warm_mono_families(cx: &App) {
    let text_system = cx.text_system().clone();
    cx.spawn(async move |cx| {
        let families = cx
            .background_spawn(async move { probe_mono_families(&text_system) })
            .await;
        cx.update(|cx| cx.set_global(MonoFamilies(Some(families))));
    })
    .detach();
}

/// The families that pass the probe, in name order.
///
/// Each family is measured on its own and the work is CPU bound, so it is
/// split across the available cores: on one thread the probe costs over a
/// second, which is long enough to be worth the fan-out.
fn probe_mono_families(text_system: &gpui_kit::TextSystem) -> Vec<SharedString> {
    let names = text_system.all_font_names();
    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .min(names.len().max(1));
    let share = names.len().div_ceil(workers).max(1);
    let mut found = Vec::with_capacity(names.len());
    std::thread::scope(|scope| {
        let parts: Vec<_> = names
            .chunks(share)
            .map(|slice| {
                let slice = slice.to_vec();
                scope.spawn(move || {
                    slice
                        .into_iter()
                        .filter(|name| is_monospace_family(text_system, name))
                        .collect::<Vec<String>>()
                })
            })
            .collect();
        for part in parts {
            found.extend(part.join().expect("the mono font probe panicked"));
        }
    });
    found.sort();
    found.into_iter().map(SharedString::from).collect()
}

/// Advance-based mono detection.
///
/// The family passes when every probe glyph has the same advance. Asking the
/// text system for a glyph it does not have is an error, so symbol and icon
/// fonts — which would otherwise pass the test through a fallback — drop out
/// on their missing letters.
fn is_monospace_family(text_system: &gpui_kit::TextSystem, family: &str) -> bool {
    let font_id = text_system.resolve_font(&gpui_kit::font(family));
    let Ok(advances) = MONO_PROBE_CHARS
        .iter()
        .map(|ch| text_system.advance(font_id, px(MONO_PROBE_SIZE), *ch))
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };

    // Half a pixel absorbs the rounding in the metrics, nothing more.
    let width = advances[0].width;
    advances
        .iter()
        .all(|advance| (advance.width - width).abs() < px(0.5))
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
        "zoom": s.zoom,
        "line_numbers": s.line_numbers,
        "wrap_raw_lines": s.wrap_raw_lines,
        "sort_object_keys": s.sort_object_keys,
        "appearance": appearance_key(s.appearance),
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

/// Bind `Cmd-,` / `Ctrl-,` and register the handlers toggling the modal /
/// quitting. The menu bar itself lives in [`crate::app::menu`].
pub fn init(cx: &mut App) {
    load(cx);
    cx.set_global(HubGlobal::default());
    warm_mono_families(cx);

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
                .update(cx, |_, window, cx| HubGlobal::toggle(window, cx))
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
/// frame) can reuse them, and whether the dialog is the one on screen — the
/// key binding has to know, because gpui keeps no handle to a dialog it opened.
#[derive(Default)]
struct HubGlobal {
    hub: Option<Entity<SettingsHub>>,
    open: bool,
}

impl Global for HubGlobal {}

impl HubGlobal {
    /// Open the settings dialog, or close it when it is already up: `Cmd-,`
    /// pressed twice must not stack a second copy of the same panel.
    fn toggle(window: &mut Window, cx: &mut App) {
        if cx.global::<Self>().open {
            // The programmatic close runs no close callback, so clear the flag here.
            cx.global_mut::<Self>().open = false;
            window.close_dialog(cx);
        } else {
            Self::open(window, cx);
        }
    }

    /// Open the settings modal dialog.
    fn open(window: &mut Window, cx: &mut App) {
        if cx.global::<Self>().hub.is_none() {
            let hub = cx.new(|_| SettingsHub);
            cx.global_mut::<Self>().hub = Some(hub);
        }
        let hub = cx.global::<Self>().hub.clone().unwrap();
        cx.global_mut::<Self>().open = true;

        window.open_dialog(cx, move |dialog, _, _| {
            // The builder runs every frame, so it must not move out of its
            // captured state — hence the clone.
            let hub = hub.clone();
            dialog
                .w(px(SETTINGS_DIALOG_W))
                .h(px(SETTINGS_DIALOG_H))
                .overlay(true)
                .overlay_closable(true)
                .title(t!("settings.title"))
                // A child lands in the dialog's body, which scrolls; `.content()`
                // builds a fixed-height content area that clips the rows below it.
                .child(hub)
                // Escape, the backdrop, and any footer all report the close here,
                // which is what lets the key binding tell the panel is gone.
                .on_close(|_, _, cx| cx.global_mut::<Self>().open = false)
        });
    }
}
