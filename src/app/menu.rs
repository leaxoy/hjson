//! The native menu bar, split the way a desktop app normally splits it:
//! app / 文件 / 编辑 / 视图 / 窗口.
//!
//! macOS rewrites the first menu's title as the app name, so its label here is
//! only a placeholder. gpui installs exactly what we hand it — the standard
//! items (Services, Hide, the window list) are not added for free: the window
//! menu only becomes native when its title is exactly `Window`, which would
//! stand out in an otherwise Chinese bar.

use crate::tr;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction};

use crate::app::{
    CloseTab, CollapseAll, DiffNext, DiffPrev, DiffTab, ExpandAll, FindInDoc, NewTab, OpenFile,
    PasteJson, SelectNextTab, SelectPrevTab, ToggleHistory,
};
use crate::views::settings::{OpenSettings, Quit, ToggleLineNumbers, settings};

gpui_kit::actions!(hjson_menu, [About, Minimize]);

/// Bind the menu-only keys and install the bar. `settings::init` has to run
/// first: the check states read the settings global it loads.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-m", Minimize, None),
    ]);

    cx.on_action(|_: &About, cx| {
        let Some(window) = cx.active_window() else { return };
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    window.open_alert_dialog(cx, |alert, _, _| {
                        let version = env!("CARGO_PKG_VERSION");
                        alert
                            .title(tr!("About HJson"))
                            .description(tr!(
                                "Version %{version}\nA JSON viewer and structure comparer. Everything is parsed and compared locally — your data never leaves this machine.",
                                version = version
                            ))
                    });
                })
                .ok();
        });
    });

    cx.on_action(|_: &Minimize, cx| {
        let Some(window) = cx.active_window() else {
            return;
        };
        window
            .update(cx, |_, window, _| window.minimize_window())
            .ok();
    });

    install(cx);
}

/// (Re)install the bar. Settings changes that alter a check mark need this,
/// because gpui bakes the checked state into the menu we hand it.
pub fn install(cx: &mut App) {
    let line_numbers = settings(cx).line_numbers;

    cx.set_menus(vec![
        Menu::new("HJson").items(vec![
            MenuItem::action(tr!("About HJson"), About),
            MenuItem::separator(),
            MenuItem::action(tr!("Settings…"), OpenSettings),
            MenuItem::separator(),
            MenuItem::action(tr!("Quit HJson"), Quit),
        ]),
        Menu::new(tr!("File")).items(vec![
            MenuItem::action(tr!("New tab"), NewTab),
            MenuItem::action(tr!("New comparison"), DiffTab),
            MenuItem::separator(),
            MenuItem::action(tr!("Open file…"), OpenFile),
            MenuItem::action(tr!("Paste JSON from clipboard"), PasteJson),
            MenuItem::separator(),
            MenuItem::action(tr!("Close tab"), CloseTab),
        ]),
        edit_menu(),
        Menu::new(tr!("View")).items(vec![
            MenuItem::action(tr!("Previous tab"), SelectPrevTab),
            MenuItem::action(tr!("Next tab"), SelectNextTab),
            MenuItem::separator(),
            // The native bar is built once, so it cannot carry the sidebar's
            // live open/closed state; the tab-bar button shows that instead.
            MenuItem::action(tr!("History"), ToggleHistory),
            MenuItem::separator(),
            // The document page's own controls, so the keys are discoverable.
            MenuItem::action(tr!("Filter the tree…"), FindInDoc),
            MenuItem::action(tr!("Expand all"), ExpandAll),
            MenuItem::action(tr!("Collapse all"), CollapseAll),
            MenuItem::separator(),
            MenuItem::action(tr!("Previous change"), DiffPrev),
            MenuItem::action(tr!("Next change"), DiffNext),
            MenuItem::separator(),
            MenuItem::action(tr!("Show line numbers"), ToggleLineNumbers).checked(line_numbers),
        ]),
        Menu::new(tr!("Window")).items(vec![MenuItem::action(tr!("Minimize"), Minimize)]),
    ]);
}

/// The clipboard + history edits AppKit owns, so that they also reach the
/// native text fields the dialogs use.
fn edit_menu() -> Menu {
    // Local to this fn: the plain names would shadow the `Copy` trait for any
    // derive in the module.
    use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

    Menu::new(tr!("Edit")).items(vec![
        MenuItem::os_action(tr!("Undo"), Undo, OsAction::Undo),
        MenuItem::os_action(tr!("Redo"), Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action(tr!("Cut"), Cut, OsAction::Cut),
        MenuItem::os_action(tr!("Copy"), Copy, OsAction::Copy),
        MenuItem::os_action(tr!("Paste"), Paste, OsAction::Paste),
        MenuItem::os_action(tr!("Select All"), SelectAll, OsAction::SelectAll),
    ])
}
