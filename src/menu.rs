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
use crate::settings::{OpenSettings, Quit, ToggleLineNumbers, settings};

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
                            .title(tr!("关于 HJson"))
                            .description(tr!(
                                "版本 %{version}\nJSON 查看与结构对比。全部解析、对比都在本机完成，数据不会离开这台设备。",
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
            MenuItem::action(tr!("关于 HJson"), About),
            MenuItem::separator(),
            MenuItem::action(tr!("设置…"), OpenSettings),
            MenuItem::separator(),
            MenuItem::action(tr!("退出 HJson"), Quit),
        ]),
        Menu::new(tr!("文件")).items(vec![
            MenuItem::action(tr!("新建标签页"), NewTab),
            MenuItem::action(tr!("新建对比"), DiffTab),
            MenuItem::separator(),
            MenuItem::action(tr!("打开文件…"), OpenFile),
            MenuItem::action(tr!("从剪贴板粘贴 JSON"), PasteJson),
            MenuItem::separator(),
            MenuItem::action(tr!("关闭标签页"), CloseTab),
        ]),
        edit_menu(),
        Menu::new(tr!("视图")).items(vec![
            MenuItem::action(tr!("上一个标签页"), SelectPrevTab),
            MenuItem::action(tr!("下一个标签页"), SelectNextTab),
            MenuItem::separator(),
            // The native bar is built once, so it cannot carry the sidebar's
            // live open/closed state; the tab-bar button shows that instead.
            MenuItem::action(tr!("历史记录"), ToggleHistory),
            MenuItem::separator(),
            // The document page's own controls, so the keys are discoverable.
            MenuItem::action(tr!("过滤结构树…"), FindInDoc),
            MenuItem::action(tr!("展开全部"), ExpandAll),
            MenuItem::action(tr!("收起全部"), CollapseAll),
            MenuItem::separator(),
            MenuItem::action(tr!("上一处差异"), DiffPrev),
            MenuItem::action(tr!("下一处差异"), DiffNext),
            MenuItem::separator(),
            MenuItem::action(tr!("显示行号"), ToggleLineNumbers).checked(line_numbers),
        ]),
        Menu::new(tr!("窗口")).items(vec![MenuItem::action(tr!("最小化"), Minimize)]),
    ]);
}

/// The clipboard + history edits AppKit owns, so that they also reach the
/// native text fields the dialogs use.
fn edit_menu() -> Menu {
    // Local to this fn: the plain names would shadow the `Copy` trait for any
    // derive in the module.
    use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

    Menu::new(tr!("编辑")).items(vec![
        MenuItem::os_action(tr!("撤销"), Undo, OsAction::Undo),
        MenuItem::os_action(tr!("重做"), Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action(tr!("剪切"), Cut, OsAction::Cut),
        MenuItem::os_action(tr!("复制"), Copy, OsAction::Copy),
        MenuItem::os_action(tr!("粘贴"), Paste, OsAction::Paste),
        MenuItem::os_action(tr!("全选"), SelectAll, OsAction::SelectAll),
    ])
}
