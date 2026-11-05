//! The shared "load a JSON document" menu. Every page reloads from the same
//! sources the home page offers — clipboard, file, URL, manual input, sample,
//! session history — and draws them with the same labels and icons.

use std::{cell::Cell, sync::Arc};

use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::{
    AppContext as _, Context, Focusable as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, WeakEntity, Window, px,
};

use crate::app::HJsonApp;
use crate::document::SourceKind;
use crate::json_model::JsonTree;

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
    FetchUrl { url: String, title: Option<String> },
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
            Self::RefetchUrl | Self::FetchUrl { .. } => Some(SourceKind::Url),
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
/// document view's single document and a side index for the diff pages.
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

pub fn prepared_doc_of(
    app: &WeakEntity<HJsonApp>,
    cx: &gpui_kit::App,
    id: u64,
) -> Option<(
    SourceKind,
    SharedString,
    String,
    Option<String>,
    Option<Arc<JsonTree>>,
)> {
    let app = app.upgrade()?;
    let entry = app.read(cx).docs.iter().find(|d| d.id == id)?;
    Some((
        entry.kind,
        entry.title.clone(),
        entry.text.clone(),
        entry.url.clone(),
        entry.parsed.upgrade(),
    ))
}

/// The one load control a header uses: an icon button whose menu holds every
/// source, wired to one slot of the page. Header bands are narrow, so the
/// button wears no label; every caller gets the same JSON-document glyph and
/// the tooltip carries the page-specific context.
pub fn header_load_button<T: LoadTarget>(
    id: impl Into<gpui_kit::ElementId>,
    tooltip: impl Into<SharedString>,
    slot: T::Slot,
    opts: LoadOptions,
    target: WeakEntity<T>,
) -> gpui_kit::AnyElement {
    Button::new(id)
        .icon(IconName::FileBraces)
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
            tr!("Paste from clipboard"),
            LoadMethod::Clipboard,
        ))
        .item(method_item(
            &target,
            slot,
            tr!("Open file…"),
            LoadMethod::PickFile,
        ));
    if url.is_some() {
        menu = menu.item(method_item(
            &target,
            slot,
            tr!("Re-fetch the URL"),
            LoadMethod::RefetchUrl,
        ));
    }

    let dialog_url = url.clone();
    menu = menu.separator().item(dialog_item(
        &target,
        slot,
        tr!("Load from URL…"),
        SourceKind::Url.icon(),
        move |target, slot, window, cx| {
            open_url_dialog(&target, slot, dialog_url.clone(), window, cx)
        },
    ));
    menu = menu.item(dialog_item(
        &target,
        slot,
        tr!("Type manually…"),
        SourceKind::Manual.icon(),
        |target, slot, window, cx| open_manual_dialog(&target, slot, window, cx),
    ));

    menu = menu.item(method_item(
        &target,
        slot,
        tr!("Load the sample"),
        LoadMethod::Sample,
    ));

    menu.submenu(tr!("Load from history"), window, cx, move |sub, _, _| {
        if history.is_empty() {
            return sub
                .item(PopupMenuItem::new(tr!("No imports in this session yet")).disabled(true));
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
/// Also the home page's "Fetch from URL" entry: pages with no URL of their own
/// pass `initial: None`.
pub fn open_url_dialog<T: LoadTarget>(
    target: &WeakEntity<T>,
    slot: T::Slot,
    initial: Option<String>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let url_input =
        cx.new(|cx| InputState::new(window, cx).placeholder("https://api.example.com/data.json"));
    if let Some(url) = initial {
        url_input.update(cx, |state, cx| state.set_value(url, window, cx));
    }
    let title_input =
        cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Optional: tab title")));

    let focused = Cell::new(true);
    let focus_state = url_input.clone();
    let ok_url = url_input.clone();
    let ok_title = title_input.clone();
    let ok_target = target.clone();
    window.open_alert_dialog(cx, move |alert, window, cx| {
        // Focus on the first build only, so a layer rebuild cannot yank focus
        // away from whatever the user moved to next.
        if focused.replace(false) {
            focus_state.focus_handle(cx).focus(window, cx);
        }
        let url_state = ok_url.clone();
        let title_state = ok_title.clone();
        let submit = ok_target.clone();
        alert
            .title(tr!("Load from URL"))
            .description(tr!("Enter a JSON endpoint. You can also set a tab title."))
            .ok_text(tr!("Load"))
            .confirm()
            .child(
                gpui_kit::component::v_flex()
                    .gap_2()
                    // No shadow focus ring — the blinking caret is enough.
                    .child(Input::new(&url_state).focus_bordered(false))
                    .child(Input::new(&title_state).focus_bordered(false)),
            )
            .on_ok(move |_, window, cx| {
                let url = url_state.read(cx).value().trim().to_string();
                if url.is_empty() {
                    return false;
                }
                let title = title_state.read(cx).value().trim().to_string();
                let title = (!title.is_empty()).then_some(title);
                submit
                    .update(cx, |this, cx| {
                        this.apply_load(slot, LoadMethod::FetchUrl { url, title }, window, cx)
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
            .title(tr!("Type JSON manually"))
            .description(tr!(
                "Paste or type JSON; a successful parse replaces the current document."
            ))
            .ok_text(tr!("Format and load"))
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
