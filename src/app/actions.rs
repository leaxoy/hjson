use std::rc::Rc;

use gpui_kit::{Action, Context, KeyBinding, WeakEntity, Window};

use super::{AppWindows, HJsonApp, TabPage};

gpui_kit::actions!(
    hjson,
    [
        PasteJson,
        FetchUrl,
        ManualInput,
        OpenSample,
        NewTab,
        CloseTab,
        SelectNextTab,
        SelectPrevTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectTab9,
        DiffTab,
        OpenFile,
        ToggleHistory,
        FindInDoc,
        ExpandAll,
        CollapseAll,
        DiffPrev,
        DiffNext,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ToggleFullscreen
    ]
);

pub(crate) fn refresh_doc_view_key_sort(cx: &mut gpui_kit::App) {
    let Some(windows) = cx.try_global::<AppWindows>() else {
        return;
    };
    let apps = windows
        .0
        .iter()
        .filter_map(|(_, app)| app.upgrade())
        .collect::<Vec<_>>();
    for app in apps {
        app.update(cx, |app, cx| {
            let doc_views = app
                .tabs
                .iter()
                .filter_map(|page| match page {
                    TabPage::Doc(doc_view) => Some(doc_view.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            for doc_view in doc_views {
                doc_view.update(cx, |doc_view, cx| doc_view.refresh_key_sort(cx));
            }
        });
    }
}

/// Run an app-level action against the focused window's HJsonApp view.
fn window_action<A: Action>(
    cx: &mut gpui_kit::App,
    run: impl Fn(&mut HJsonApp, &mut Window, &mut Context<HJsonApp>) + 'static,
) {
    // Handlers can fire repeatedly, so the closure is shared instead of moved.
    let run: Rc<dyn Fn(&mut HJsonApp, &mut Window, &mut Context<HJsonApp>)> = Rc::new(run);
    cx.on_action(move |_: &A, cx| {
        let Some(app) = active_app(cx) else { return };
        let run = run.clone();
        cx.defer(move |cx| {
            let _ = app.update_in(cx, |app, window, cx| run(app, window, cx));
        });
    });
}

/// The app view belonging to the focused window, if it is one of ours.
fn active_app(cx: &gpui_kit::App) -> Option<WeakEntity<HJsonApp>> {
    let window = cx.active_window()?;
    let app = cx
        .try_global::<AppWindows>()?
        .0
        .iter()
        .find(|(handle, _)| *handle == window)
        .map(|(_, app)| app.clone())?;
    app.upgrade().map(|_| app)
}

/// Global key bindings + menu actions. `cmd-t` opens a new tab (import page),
/// `cmd-w` closes the active one, the Home page import shortcuts load JSON
/// from clipboard / URL / file / manual input / sample data, `cmd-shift-h`
/// toggles the history sidebar, `cmd-1` … `cmd-9` select tabs, `cmd-=`, `cmd--`
/// and `cmd-0` control page scale, `cmd-ctrl-f` toggles fullscreen, `cmd-f`
/// filters the tree, and `cmd-[` / `cmd-]` step through a comparison.
/// `cmd-shift-e` / `cmd-shift-c` belong to the document pane key contexts so
/// they only affect the focused raw editor or tree viewer.
pub fn init(cx: &mut gpui_kit::App) {
    // The document view's own per-pane keys (arrows, tab) live with the panes.
    crate::views::doc::init(cx);
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-v", PasteJson, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-v", PasteJson, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-h", ToggleHistory, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-h", ToggleHistory, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-o", OpenFile, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-o", OpenFile, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-l", FetchUrl, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-l", FetchUrl, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-i", ManualInput, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-i", ManualInput, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-s", OpenSample, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-s", OpenSample, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-t", NewTab, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-t", NewTab, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-d", DiffTab, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-d", DiffTab, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-w", CloseTab, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-w", CloseTab, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-]", SelectNextTab, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-[", SelectPrevTab, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-]", SelectNextTab, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-[", SelectPrevTab, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-1", SelectTab1, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-1", SelectTab1, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-2", SelectTab2, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-2", SelectTab2, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-3", SelectTab3, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-3", SelectTab3, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-4", SelectTab4, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-4", SelectTab4, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-5", SelectTab5, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-5", SelectTab5, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-6", SelectTab6, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-6", SelectTab6, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-7", SelectTab7, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-7", SelectTab7, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-8", SelectTab8, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-8", SelectTab8, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-9", SelectTab9, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-9", SelectTab9, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-=", ZoomIn, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-=", ZoomIn, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd--", ZoomOut, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl--", ZoomOut, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-0", ZoomReset, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-0", ZoomReset, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-ctrl-f", ToggleFullscreen, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("f11", ToggleFullscreen, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-f", FindInDoc, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-f", FindInDoc, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-[", DiffPrev, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-[", DiffPrev, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-]", DiffNext, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-]", DiffNext, None),
    ]);

    window_action::<PasteJson>(cx, |app, window, cx| app.open_clipboard(window, cx));
    window_action::<OpenFile>(cx, |app, window, cx| app.open_file(window, cx));
    window_action::<FetchUrl>(cx, |app, window, cx| app.open_url_dialog(window, cx));
    window_action::<ManualInput>(cx, |app, window, cx| app.open_manual_dialog(window, cx));
    window_action::<OpenSample>(cx, |app, window, cx| app.open_sample(window, cx));
    window_action::<NewTab>(cx, |app, window, cx| app.new_tab(window, cx));
    window_action::<DiffTab>(cx, |app, _window, cx| app.new_diff_tab(None, cx));
    window_action::<CloseTab>(cx, |app, _window, cx| app.close_active_tab(cx));
    window_action::<SelectNextTab>(cx, |app, _window, cx| app.select_next_tab(cx));
    window_action::<SelectPrevTab>(cx, |app, _window, cx| app.select_prev_tab(cx));
    window_action::<SelectTab1>(cx, |app, _window, cx| app.set_active_tab(0, cx));
    window_action::<SelectTab2>(cx, |app, _window, cx| app.set_active_tab(1, cx));
    window_action::<SelectTab3>(cx, |app, _window, cx| app.set_active_tab(2, cx));
    window_action::<SelectTab4>(cx, |app, _window, cx| app.set_active_tab(3, cx));
    window_action::<SelectTab5>(cx, |app, _window, cx| app.set_active_tab(4, cx));
    window_action::<SelectTab6>(cx, |app, _window, cx| app.set_active_tab(5, cx));
    window_action::<SelectTab7>(cx, |app, _window, cx| app.set_active_tab(6, cx));
    window_action::<SelectTab8>(cx, |app, _window, cx| app.set_active_tab(7, cx));
    window_action::<SelectTab9>(cx, |app, _window, cx| app.set_active_tab(8, cx));
    window_action::<ToggleHistory>(cx, |app, _window, cx| app.toggle_history(cx));
    window_action::<FindInDoc>(cx, |app, window, cx| app.focus_filter(window, cx));
    window_action::<ExpandAll>(cx, |app, window, cx| app.set_tree_depth(true, window, cx));
    window_action::<CollapseAll>(cx, |app, window, cx| app.set_tree_depth(false, window, cx));
    window_action::<DiffPrev>(cx, |app, _window, cx| app.step_diff(-1, cx));
    window_action::<DiffNext>(cx, |app, _window, cx| app.step_diff(1, cx));
    cx.on_action(|_: &ZoomIn, cx| crate::views::settings::zoom_in(cx));
    cx.on_action(|_: &ZoomOut, cx| crate::views::settings::zoom_out(cx));
    cx.on_action(|_: &ZoomReset, cx| crate::views::settings::reset_zoom(cx));
    window_action::<ToggleFullscreen>(cx, |_app, window, _cx| window.toggle_fullscreen());
}
