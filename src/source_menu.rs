//! The shared "load a JSON document" menu, and the vocabulary that comes with
//! it: [`SourceKind`] is where a document came from, [`LoadMethod`] is how the
//! user asks for one. Every page reloads from the same sources the home page
//! offers — clipboard, file, URL, manual input, sample, session history — and
//! draws them with the same label / icon / color, from here.

use std::cell::Cell;

use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::{
    App, AppContext as _, Context, Focusable as _, Hsla, IntoElement, ParentElement as _,
    SharedString, WeakEntity, Window, px,
};

use crate::app::HJsonApp;

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
            Self::File => tr!("文件"),
            Self::Clipboard => tr!("剪贴板"),
            Self::Manual => tr!("手动输入"),
            Self::Sample => tr!("示例"),
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

    /// Title for a document that has no name of its own — the URL and the
    /// picked file are titled by [`url_title`] / [`file_title`] instead.
    pub fn default_title(self) -> SharedString {
        match self {
            Self::Clipboard => tr!("剪贴板内容"),
            Self::Manual => tr!("手动输入"),
            Self::Sample => tr!("示例 JSON"),
            Self::Url | Self::File => "JSON".into(),
        }
    }
}

/// Title for a file on disk: its name, without the path.
pub fn file_title(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file.json")
        .to_string()
}

/// Title for a URL: its last path segment.
pub fn url_title(url: &str) -> String {
    url.split('/').next_back().unwrap_or("JSON").to_string()
}

/// One of the import methods, mirroring the home page.
#[derive(Clone)]
pub enum LoadMethod {
    /// Read the clipboard.
    Clipboard,
    /// Ask for a file with the native picker.
    PickFile,
    /// Re-fetch the URL the current document came from.
    RefetchUrl,
    /// Fetch a URL typed into the load dialog.
    FetchUrl(String),
    /// Text typed into the manual-input dialog.
    Manual(String),
    /// The built-in sample document.
    Sample,
    /// A document from this session's import history.
    History(u64),
}

impl LoadMethod {
    /// The source kind a load records. `History` inherits the kind stored with
    /// the entry, so it has none of its own.
    pub fn kind(&self) -> Option<SourceKind> {
        match self {
            Self::Clipboard => Some(SourceKind::Clipboard),
            Self::PickFile => Some(SourceKind::File),
            Self::RefetchUrl | Self::FetchUrl(_) => Some(SourceKind::Url),
            Self::Manual(_) => Some(SourceKind::Manual),
            Self::Sample => Some(SourceKind::Sample),
            Self::History(_) => None,
        }
    }

    /// Menu glyph: re-fetching keeps its own refresh icon, everything else
    /// wears its source kind.
    pub fn icon(&self) -> IconName {
        match self {
            Self::RefetchUrl => IconName::RefreshCw,
            other => other
                .kind()
                .map(SourceKind::icon)
                .unwrap_or(IconName::Download),
        }
    }
}

/// A page that loads documents into one or more slots. `Slot` is `()` for the
/// viewer's single document and a side index for the diff pages.
pub trait LoadTarget: Sized + 'static {
    type Slot: Copy + 'static;

    fn apply_load(
        &mut self,
        slot: Self::Slot,
        method: LoadMethod,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

/// One entry of the session import history.
pub type HistoryItem = (u64, SharedString, SourceKind);

/// What a menu can offer at the moment it opens.
#[derive(Clone)]
pub struct LoadOptions {
    /// The current source URL, when the document came over HTTP.
    pub url: Option<String>,
    /// Session history, newest first.
    pub history: Vec<HistoryItem>,
}

/// Live snapshot of the app-owned import history.
pub fn history_of(app: &WeakEntity<HJsonApp>, cx: &gpui_kit::App) -> Vec<HistoryItem> {
    let Some(app) = app.upgrade() else {
        return Vec::new();
    };
    app.read(cx)
        .docs
        .iter()
        .rev()
        .map(|d| (d.id, d.title.clone(), d.kind))
        .collect()
}

/// The raw document behind a history id: kind, title, text, URL.
pub fn doc_of(
    app: &WeakEntity<HJsonApp>,
    cx: &gpui_kit::App,
    id: u64,
) -> Option<(SourceKind, SharedString, String, Option<String>)> {
    let app = app.upgrade()?;
    let entry = app.read(cx).docs.iter().find(|d| d.id == id)?;
    Some((
        entry.kind,
        entry.title.clone(),
        entry.text.clone(),
        entry.url.clone(),
    ))
}

/// The one load control a header uses: an icon button whose menu holds every
/// source, wired to one slot of the page. Header bands are narrow, so the
/// button wears no label — the glyph says whether it loads (Download) or
/// reloads (RefreshCw), and the tooltip carries the rest.
pub fn header_load_button<T: LoadTarget>(
    id: impl Into<gpui_kit::ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    slot: T::Slot,
    opts: LoadOptions,
    target: WeakEntity<T>,
) -> gpui_kit::AnyElement {
    Button::new(id)
        .icon(icon)
        .ghost()
        .small()
        .tooltip(tooltip)
        .dropdown_menu(move |menu, window, cx| {
            add_load_items(menu, window, cx, target.clone(), slot, opts.clone())
        })
        .into_any_element()
}

/// Append every load method to `menu`, wired to `target`'s `slot`.
pub fn add_load_items<T: LoadTarget>(
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    target: WeakEntity<T>,
    slot: T::Slot,
    opts: LoadOptions,
) -> PopupMenu {
    let LoadOptions { url, history } = opts;

    let mut menu = menu
        .item(method_item(
            &target,
            slot,
            tr!("从剪贴板粘贴"),
            LoadMethod::Clipboard,
        ))
        .item(method_item(
            &target,
            slot,
            tr!("打开文件…"),
            LoadMethod::PickFile,
        ));
    if url.is_some() {
        menu = menu.item(method_item(
            &target,
            slot,
            tr!("重新拉取 URL"),
            LoadMethod::RefetchUrl,
        ));
    }

    let dialog_url = url.clone();
    menu = menu.separator().item(dialog_item(
        &target,
        slot,
        tr!("从 URL 载入…"),
        SourceKind::Url.icon(),
        move |target, slot, window, cx| {
            open_url_dialog(&target, slot, dialog_url.clone(), window, cx)
        },
    ));
    menu = menu.item(dialog_item(
        &target,
        slot,
        tr!("手动输入…"),
        SourceKind::Manual.icon(),
        |target, slot, window, cx| open_manual_dialog(&target, slot, window, cx),
    ));

    menu = menu.item(method_item(&target, slot, tr!("载入示例"), LoadMethod::Sample));

    menu.submenu(tr!("从历史载入"), window, cx, move |sub, _, _| {
        if history.is_empty() {
            return sub.item(PopupMenuItem::new(tr!("本会话还没有导入记录")).disabled(true));
        }
        let mut sub = sub.max_h(px(320.)).scrollable(true);
        for (id, title, kind) in history.iter() {
            let label = format!("{} · {}", title, kind.label());
            sub = sub.item(method_wearing(
                &target,
                slot,
                label,
                kind.icon(),
                LoadMethod::History(*id),
            ));
        }
        sub
    })
}

/// A menu item that hands `method` straight to the target, wearing its glyph.
fn method_item<T: LoadTarget>(
    target: &WeakEntity<T>,
    slot: T::Slot,
    label: impl Into<SharedString>,
    method: LoadMethod,
) -> PopupMenuItem {
    method_wearing(target, slot, label, method.icon(), method)
}

/// [`method_item`] with an explicit glyph: a history row wears its own source's
/// icon rather than the generic load one.
fn method_wearing<T: LoadTarget>(
    target: &WeakEntity<T>,
    slot: T::Slot,
    label: impl Into<SharedString>,
    icon: IconName,
    method: LoadMethod,
) -> PopupMenuItem {
    let target = target.clone();
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, window, cx| {
            target
                .update(cx, |this, cx| {
                    this.apply_load(slot, method.clone(), window, cx)
                })
                .ok();
        })
}

/// A menu item that opens a dialog instead of loading directly.
fn dialog_item<T: LoadTarget>(
    target: &WeakEntity<T>,
    slot: T::Slot,
    label: impl Into<SharedString>,
    icon: IconName,
    open: impl Fn(WeakEntity<T>, T::Slot, &mut Window, &mut gpui_kit::App) + 'static,
) -> PopupMenuItem {
    let target = target.clone();
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, window, cx| open(target.clone(), slot, window, cx))
}

/// Ask for a URL, then load it through [`LoadMethod::FetchUrl`].
///
/// Also the home page's "从 URL 获取" entry: pages with no URL of their own
/// pass `initial: None`.
pub fn open_url_dialog<T: LoadTarget>(
    target: &WeakEntity<T>,
    slot: T::Slot,
    initial: Option<String>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let input =
        cx.new(|cx| InputState::new(window, cx).placeholder("https://api.example.com/data.json"));
    if let Some(url) = initial {
        input.update(cx, |state, cx| state.set_value(url, window, cx));
    }

    let focused = Cell::new(true);
    let focus_state = input.clone();
    let ok_state = input.clone();
    let ok_target = target.clone();
    window.open_alert_dialog(cx, move |alert, window, cx| {
        // Focus on the first build only, so a layer rebuild cannot yank focus
        // away from whatever the user moved to next.
        if focused.replace(false) {
            focus_state.focus_handle(cx).focus(window, cx);
        }
        let state = ok_state.clone();
        let submit = ok_target.clone();
        alert
            .title(tr!("从 URL 载入"))
            .description(tr!("输入 JSON 接口地址，回车或点击「载入」。"))
            .ok_text(tr!("载入"))
            .confirm()
            .child(Input::new(&state))
            .on_ok(move |_, window, cx| {
                let url = state.read(cx).value().trim().to_string();
                if url.is_empty() {
                    return false;
                }
                submit
                    .update(cx, |this, cx| {
                        this.apply_load(slot, LoadMethod::FetchUrl(url), window, cx)
                    })
                    .ok();
                true
            })
    });
}

/// Ask for raw JSON text, then load it through [`LoadMethod::Manual`].
pub fn open_manual_dialog<T: LoadTarget>(
    target: &WeakEntity<T>,
    slot: T::Slot,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let state =
        cx.new(|cx| TextareaState::new(window, cx).placeholder("{\n  \"key\": \"value\"\n}"));

    let focused = Cell::new(true);
    let ok_state = state.clone();
    let ok_target = target.clone();
    window.open_alert_dialog(cx, move |alert, window, cx| {
        if focused.replace(false) {
            ok_state.focus_handle(cx).focus(window, cx);
        }
        let field = Textarea::new(&ok_state).h(px(160.));
        let read = ok_state.clone();
        let submit = ok_target.clone();
        alert
            .title(tr!("手动输入 JSON"))
            .description(tr!("粘贴或输入 JSON 文本，解析成功后替换当前内容。"))
            .ok_text(tr!("格式化并载入"))
            .confirm()
            .child(field)
            .on_ok(move |_, window, cx| {
                let text = read.read(cx).value().trim().to_string();
                if text.is_empty() {
                    return false;
                }
                submit
                    .update(cx, |this, cx| {
                        this.apply_load(slot, LoadMethod::Manual(text), window, cx)
                    })
                    .ok();
                true
            })
    });
}
