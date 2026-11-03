//! The document viewer page: raw JSON and the tree side by side, each headed by
//! its own strip, and a status bar at the bottom. The document's controls —
//! reload and compare — sit in the raw pane's header; the tree's filter sits in
//! the tree's header, next to what it filters.
//!
//! The two panes are one view of one document: selecting a node highlights the
//! line its value starts on and scrolls the raw pane to it. `tab` moves the
//! keyboard between the panes; the tree then takes `↑↓←→` and `enter`, the raw
//! pane takes `↑↓`, page keys, `home` and `end`.
//!
//! The viewer only ever receives documents that already parsed successfully;
//! parse errors are caught on the home page.

use std::{ops::Range, rc::Rc, sync::Arc};

use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::actions::{
    Confirm, SelectDown, SelectFirst, SelectLast, SelectPageDown, SelectPageUp, SelectUp,
};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, VirtualListScrollHandle,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    menu::{PopupMenu, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
    tree::{TreeEntry, TreeItem, TreeState, tree},
    v_flex, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable,
    FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Pixels,
    ScrollStrategy, SharedString, Size, StatefulInteractiveElement as _, Styled as _, Subscription,
    TextAlign, WeakEntity, Window, div, px, size,
};

use crate::chrome;
use crate::highlight::{Span, SpanKind, highlight};
use crate::json_model::{JsonTree, JsonType};
use crate::source_menu::{
    self, LoadMethod, LoadOptions, LoadTarget, SourceKind, file_title, url_title,
};

const INDENT_PX: f32 = 16.0;

// `tab` in one of the two panes: the keyboard moves to the other one.
gpui_kit::actions!(hjson, [NextPane]);

/// Key context of the raw pane, the only one this page owns.
const RAW_CONTEXT: &str = "hjson-raw";
/// The tree pane's context, as gpui-base's tree names it (it binds `↑↓←→`
/// there itself, and handles `Confirm`).
const TREE_CONTEXT: &str = "Tree";

/// Keys for the two panes, in their own contexts: the arrows mean "walk the
/// tree" in one and "scroll the text" in the other.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm { secondary: false }, Some(TREE_CONTEXT)),
        KeyBinding::new("tab", NextPane, Some(TREE_CONTEXT)),
        KeyBinding::new("up", SelectUp, Some(RAW_CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(RAW_CONTEXT)),
        KeyBinding::new("pageup", SelectPageUp, Some(RAW_CONTEXT)),
        KeyBinding::new("pagedown", SelectPageDown, Some(RAW_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(RAW_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(RAW_CONTEXT)),
        KeyBinding::new("tab", NextPane, Some(RAW_CONTEXT)),
    ]);
}

/// Icon + color for each JSON type. `_dark` is needed for the scalar gray tone.
pub fn type_icon(t: JsonType) -> IconName {
    match t {
        JsonType::Object => IconName::Braces,
        JsonType::Array => IconName::Brackets,
        JsonType::String => IconName::Quote,
        JsonType::Number => IconName::Hash,
        JsonType::Bool => IconName::ToggleRight,
        JsonType::Null => IconName::CircleMinus,
    }
}

pub fn type_color(t: JsonType, dark: bool) -> gpui_kit::Rgba {
    gpui_kit::rgb(match (t, dark) {
        (JsonType::Object, true) => 0x7AA2F7,
        (JsonType::Object, false) => 0x0550AE,
        (JsonType::Array, true) => 0xE0AF68,
        (JsonType::Array, false) => 0x953800,
        (JsonType::String, true) => 0x9ECE6A,
        (JsonType::String, false) => 0x116329,
        (JsonType::Number, true) => 0x7DCFFF,
        (JsonType::Number, false) => 0x0969DA,
        (JsonType::Bool, true) => 0xBB9AF7,
        (JsonType::Bool, false) => 0x8250DF,
        (JsonType::Null, true) => 0x737CA5,
        (JsonType::Null, false) => 0x59636E,
    })
}

/// How aggressively the rebuilt tree starts out expanded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpandMode {
    /// Only the root is expanded (the default view).
    Root,
    /// Every level expanded (`展开全部`).
    All,
}

/// A short-lived message shown in the status bar.
#[derive(Clone)]
struct Toast {
    text: String,
    ok: bool,
}

pub struct Viewer {
    app: WeakEntity<crate::app::HJsonApp>,
    pub title: SharedString,
    pub kind: SourceKind,
    /// Original URL when this document came from HTTP, for toolbar re-fetch.
    url: Option<String>,
    doc: Arc<JsonTree>,
    lines: Vec<Vec<Span>>,
    line_sizes: Rc<Vec<Size<Pixels>>>,
    /// Row height the line sizes were built for, so settings changes rebuild.
    line_h: f32,
    raw_scroll: VirtualListScrollHandle,
    /// Focus for the raw pane; the arrow keys scroll it while this has focus.
    raw_focus: FocusHandle,
    /// Line of `lines` revealed for the selected tree node, so the two panes
    /// show the same node. `None` until something is selected.
    linked: Option<usize>,
    tree_state: Entity<TreeState>,
    search: Entity<InputState>,
    query: Option<String>,
    match_count: usize,
    toast: Option<Toast>,
    _subs: Vec<Subscription>,
}

impl Viewer {
    pub fn new(
        app: WeakEntity<crate::app::HJsonApp>,
        title: &str,
        kind: SourceKind,
        url: Option<String>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc = Arc::new(JsonTree::parse(text));
        let lines = highlight(&doc.pretty);
        let line_h = crate::settings::settings(cx).row_height();
        let line_sizes = Rc::new(vec![size(px(640.), px(line_h)); lines.len()]);
        let (items, _) = build_tree_items(&doc, None, ExpandMode::Root);
        let tree_state = cx.new(|cx| TreeState::new(cx).items(items));
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr!("搜索 key 或值…（输入即过滤树）")));

        let search_sub = cx.subscribe_in(&search, window, |this, state, event, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let q = state.read(cx).value().trim().to_string();
                this.apply_filter(q, cx);
            }
        });

        // gpui-base's tree emits no selection event, but it notifies on every
        // selection change — click or arrow key. Observing that is what lets
        // the raw pane follow the tree and the status bar stay in step.
        let follow_sub = cx.observe(&tree_state, move |this, _, cx| {
            this.follow_selection(cx);
        });

        Self {
            app,
            title: title.into(),
            kind,
            url,
            lines,
            doc,
            line_sizes,
            line_h,
            raw_scroll: VirtualListScrollHandle::new(),
            raw_focus: cx.focus_handle(),
            linked: None,
            tree_state,
            search,
            query: None,
            match_count: 0,
            toast: None,
            _subs: vec![search_sub, follow_sub],
        }
    }

    // ---------- actions ----------

    fn apply_filter(&mut self, query: String, cx: &mut Context<Self>) {
        let q = if query.is_empty() {
            None
        } else {
            Some(query.to_lowercase())
        };
        let (items, matches) = build_tree_items(&self.doc, q.as_deref(), ExpandMode::Root);
        self.query = q.clone();
        self.match_count = matches;
        self.toast = None;
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
        cx.notify();
    }

    /// `cmd-shift-e` / `cmd-shift-c`, and the tree header's two buttons.
    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.set_expand(ExpandMode::All, cx);
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.set_expand(ExpandMode::Root, cx);
    }

    fn set_expand(&mut self, expand: ExpandMode, cx: &mut Context<Self>) {
        let (items, _) = build_tree_items(&self.doc, self.query.as_deref(), expand);
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
        // A rebuilt tree has no selection, so the raw pane loses its mark too.
        self.linked = None;
        cx.notify();
    }

    // ---------- pane linkage ----------

    /// Reveal the tree's selection in the raw pane: highlight the line the node
    /// starts on and scroll it into view. Runs on every tree notification, so
    /// the status bar (which reads the same selection) repaints with it.
    fn follow_selection(&mut self, cx: &mut Context<Self>) {
        let line = self
            .tree_state
            .read(cx)
            .selected_entry()
            .and_then(|entry| entry.item().id.parse::<usize>().ok())
            .and_then(|id| self.doc.line_of(id));
        if line != self.linked {
            self.linked = line;
            if let Some(line) = line {
                self.raw_scroll.scroll_to_item(line, ScrollStrategy::Center);
            }
        }
        cx.notify();
    }

    /// Move the raw pane's viewport so `line` is its first visible row.
    fn scroll_raw_to(&mut self, line: usize, cx: &mut Context<Self>) {
        let last = self.lines.len().saturating_sub(1);
        self.raw_scroll
            .scroll_to_item(line.min(last), ScrollStrategy::Top);
        cx.notify();
    }

    /// Rows that fit in the raw pane at the current row height.
    fn raw_page(&self) -> usize {
        let height = self.raw_scroll.bounds().size.height;
        (((height / px(1.)) / self.line_h).floor() as usize).max(1)
    }

    /// `cmd-f`: put the cursor in the tree's filter.
    pub fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.focus_handle(cx).focus(window, cx);
    }

    fn copy_text(&mut self, text: String, toast: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.say(true, toast, cx);
    }

    /// Status toast under the toolbar. `ok` picks the color.
    fn say(&mut self, ok: bool, text: impl Into<String>, cx: &mut Context<Self>) {
        self.toast = Some(Toast {
            text: text.into(),
            ok,
        });
        cx.notify();
    }

    // ---------- reloading this document ----------

    /// Swap the document in this tab for `text`; the app validates it and
    /// registers a new history entry. A failure lands in the status bar.
    fn replace_with(
        &mut self,
        kind: SourceKind,
        title: impl Into<SharedString>,
        text: &str,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let me = cx.entity();
        let outcome = self.app.update(cx, |app, cx| {
            app.replace_doc_of_viewer(&me, kind, title, text, url, window, cx)
        });
        match outcome {
            Ok(_) => self.toast = None,
            Err(err) => self.say(false, format!("加载失败：{err}"), cx),
        }
        cx.notify();
    }

    /// Replace this document with the clipboard contents.
    fn load_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .map(|t| t.trim().to_string());
        match text {
            Some(text) if !text.is_empty() => {
                let kind = SourceKind::Clipboard;
                self.replace_with(kind, kind.default_title(), &text, None, window, cx)
            }
            Some(_) => self.say(false, tr!("剪贴板内容为空"), cx),
            None => self.say(false, tr!("剪贴板里没有文本"), cx),
        }
    }

    /// Replace this document with a file picked from disk.
    fn load_file(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = outcome else { return };
            let read = cx.background_spawn(async move {
                std::fs::read_to_string(&path)
                    .map(|t| (path.clone(), t))
                    .map_err(|e| anyhow::anyhow!("{e}"))
            });
            let result = read.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok((path, text)) => {
                    let title = file_title(&path);
                    this.replace_with(SourceKind::File, title, &text, None, window, cx);
                }
                Err(e) => this.say(false, format!("读取文件失败：{e}"), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Replace this document with the body of `url`.
    fn load_fetch(&mut self, url: String, cx: &mut Context<Self>) {
        self.say(true, format!("正在请求 {url} …"), cx);
        let task_url = url.clone();
        let task = cx.background_spawn(async move {
            let resp = ureq::get(&task_url).call()?;
            let text = resp.into_body().read_to_string()?;
            Ok::<String, anyhow::Error>(text)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(text) => {
                    let title = url_title(&url);
                    this.replace_with(SourceKind::Url, title, &text, Some(url.clone()), window, cx);
                }
                Err(e) => this.say(false, format!("请求失败：{e}"), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Replace this document with one from the session history.
    fn load_history(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let found = source_menu::doc_of(&self.app, cx, id);
        match found {
            Some((kind, title, text, url)) => {
                self.replace_with(kind, title, &text, url, window, cx)
            }
            None => self.say(false, tr!("该历史记录已不在列表中"), cx),
        }
    }
}

impl LoadTarget for Viewer {
    type Slot = ();

    fn apply_load(
        &mut self,
        _slot: (),
        method: LoadMethod,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match method {
            LoadMethod::Clipboard => self.load_clipboard(window, cx),
            LoadMethod::PickFile => self.load_file(cx),
            LoadMethod::RefetchUrl => {
                if let Some(url) = self.url.clone() {
                    self.load_fetch(url, cx);
                }
            }
            LoadMethod::FetchUrl(url) => self.load_fetch(url, cx),
            LoadMethod::Manual(text) => {
                let kind = SourceKind::Manual;
                self.replace_with(kind, kind.default_title(), &text, None, window, cx)
            }
            LoadMethod::Sample => {
                let kind = SourceKind::Sample;
                self.replace_with(
                    kind,
                    kind.default_title(),
                    crate::sample::TEXT,
                    None,
                    window,
                    cx,
                )
            }
            LoadMethod::History(id) => self.load_history(id, window, cx),
        }
    }
}

/// Copy through the viewer so it can show a toast. Lives outside `Viewer`
/// so context-menu closures (which only capture the weak handle) can use it.
fn copy_via_viewer(
    viewer: &WeakEntity<Viewer>,
    text: String,
    toast: String,
    cx: &mut gpui_kit::App,
) {
    let _ = viewer.update(cx, |viewer, cx| viewer.copy_text(text, toast, cx));
}

/// Build a `PopupMenuItem` that copies `text` on click.
fn copy_item(
    label: impl Into<SharedString>,
    icon: IconName,
    text: String,
    toast: impl Into<SharedString>,
    viewer: &WeakEntity<Viewer>,
) -> PopupMenuItem {
    let viewer = viewer.clone();
    let toast = toast.into().to_string();
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, _, cx| copy_via_viewer(&viewer, text.clone(), toast.clone(), cx))
}

/// The scalar text behind a node: strings without quotes, everything else as-is.
fn scalar_value_text(doc: &JsonTree, id: usize) -> String {
    match doc.resolve(id) {
        Some(serde_json::Value::String(s)) => s,
        Some(v) => v.to_string(),
        None => doc.nodes[id].summary.clone(),
    }
}

/// What "复制 Value" should copy: raw scalar text, or the pretty subtree.
fn value_to_copy(doc: &JsonTree, id: usize) -> String {
    if doc.nodes[id].children.is_empty() {
        scalar_value_text(doc, id)
    } else {
        doc.subtree_json(id).unwrap_or_else(|| "{}".to_string())
    }
}

// ---------- tree construction ----------

/// The muted "· N keys / · N items" suffix on a container row, or `None` when
/// the node has no children or isn't one of the two containers.
///
/// English inflects the noun for one, Chinese does not, so the count picks the
/// key rather than the language picking a plural rule.
fn child_count_unit(typ: JsonType, count: usize) -> Option<SharedString> {
    let count_str = count.to_string();
    match typ {
        JsonType::Object if count == 1 => Some(tr!("1 个键")),
        JsonType::Object => Some(tr!("%{count} 个键", count = count_str)),
        JsonType::Array if count == 1 => Some(tr!("1 项")),
        JsonType::Array => Some(tr!("%{count} 项", count = count_str)),
        _ => None,
    }
}

fn node_label(doc: &JsonTree, id: usize) -> SharedString {
    let n = &doc.nodes[id];
    match (&n.key, &n.summary) {
        (Some(k), _) => format!("{k}: {}", n.summary).into(),
        (None, summary) => summary.clone().into(),
    }
}

fn node_item(doc: &JsonTree, id: usize, expanded: bool) -> TreeItem {
    let node = &doc.nodes[id];
    let mut item = TreeItem::new(id.to_string(), node_label(doc, id)).expanded(expanded);
    if !node.children.is_empty() {
        item = item.children(
            node.children
                .iter()
                .map(|&c| node_item(doc, c, expanded))
                .collect::<Vec<_>>(),
        );
    }
    item
}

/// A node is included when it self-matches or any descendant survives.
fn filter_item(doc: &JsonTree, id: usize, q: &str, hits: &mut usize) -> Option<TreeItem> {
    let node = &doc.nodes[id];
    let children: Vec<TreeItem> = node
        .children
        .iter()
        .filter_map(|&c| filter_item(doc, c, q, hits))
        .collect();
    let self_hit = node_matches(node, &q.to_lowercase());
    if self_hit {
        *hits += 1;
    }
    let include = self_hit || !children.is_empty();
    if !include {
        return None;
    }
    let mut item = TreeItem::new(id.to_string(), node_label(doc, id)).children(children);
    if !node.children.is_empty() {
        item = item.expanded(true);
    }
    Some(item)
}

fn node_matches(node: &crate::json_model::FlatNode, q: &str) -> bool {
    node.key
        .as_deref()
        .map(|k| k.to_lowercase().contains(q))
        .unwrap_or(false)
        || node.summary.to_lowercase().contains(q)
}

/// Rebuild `TreeItem`s. `TreeItem` ids are node indices, so the rendered rows
/// and the context menu always map back to the arena.
fn build_tree_items(
    doc: &JsonTree,
    query: Option<&str>,
    expand: ExpandMode,
) -> (Vec<TreeItem>, usize) {
    let Some(q) = query else {
        let all = expand == ExpandMode::All;
        return (vec![node_item(doc, 0, all)], 0);
    };
    let mut hits = 0;
    match filter_item(doc, 0, &q.to_lowercase(), &mut hits) {
        Some(root) => (vec![root], hits),
        None => (Vec::new(), 0),
    }
}

// ---------- rendering ----------

impl gpui_kit::Render for Viewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .font_family(theme.font_family.clone())
            // `tab` arrives from whichever pane had focus; hand it to the other.
            .on_action(cx.listener(|this, _: &NextPane, window, cx| {
                if this.raw_focus.is_focused(window) {
                    this.tree_state
                        .update(cx, |state, cx| state.focus(window, cx));
                } else {
                    this.raw_focus.focus(window, cx);
                }
            }))
            .child(
                // The resizable group fills the remaining height.
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_split(cx)),
            )
            .child(self.render_status(cx))
    }
}

impl Viewer {
    /// This document as the A side of a new comparison. Re-serializing the
    /// root yields semantically identical JSON; an empty document gives None.
    pub fn diff_snapshot(&self) -> Option<(SourceKind, SharedString, String, Option<String>)> {
        let text = serde_json::to_string(&self.doc.root).ok()?;
        if text.is_empty() {
            return None;
        }
        Some((self.kind, self.title.clone(), text, self.url.clone()))
    }

    fn render_split(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        // No initial size on either panel: the group's flex defaults split the
        // width evenly, and the measured even ratio then survives a window
        // resize. The lower bound stops a drag from collapsing a pane to
        // nothing; the upper stays open so a wide window keeps the halves even.
        h_resizable("viewer-split")
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_raw_panel(cx)),
            )
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_tree_panel(cx)),
            )
            .into_any_element()
    }

    // ----- left: raw panel -----

    fn render_raw_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (bg, muted) = {
            let theme = cx.theme();
            (theme.background, theme.muted_foreground)
        };
        let view = cx.entity();
        // React to font size changes made on the settings page.
        let wanted_h = crate::settings::settings(cx).row_height();
        if (self.line_h - wanted_h).abs() > f32::EPSILON {
            self.line_h = wanted_h;
            self.line_sizes = Rc::new(vec![size(px(640.), px(wanted_h)); self.lines.len()]);
        }
        let sizes = self.line_sizes.clone();
        let line_count = self.lines.len();
        let byte_size = self.doc.pretty.len();
        let header = chrome::panel_header(
            tr!("原文"),
            vec![
                chrome::stat(format!("{line_count} 行"), cx),
                crate::app::human_size(byte_size, cx),
            ],
            vec![
                // The document's own controls, docked in the document's
                // header: reload it (from its source or another one), and
                // start a comparison with it on the A side.
                source_menu::header_load_button(
                    "reload-doc",
                    IconName::RefreshCw,
                    tr!("重新读取当前来源，或换一个来源载入这份文档"),
                    (),
                    LoadOptions {
                        url: self.url.clone(),
                        history: source_menu::history_of(&self.app, cx),
                    },
                    cx.weak_entity(),
                ),
                Button::new("compare-doc")
                    .icon(gpui_kit::assets::IconName::GitCompare)
                    .ghost()
                    .small()
                    .tooltip(tr!("把当前文档放入 A 侧，另选一份 JSON 比较"))
                    .on_click(cx.listener(|this, _, _window, cx| {
                        // Re-serializing the root yields semantically
                        // identical JSON; an empty document has nothing to
                        // hand over.
                        let Some((kind, title, text, url)) = this.diff_snapshot() else {
                            return;
                        };
                        this.app
                            .update(cx, |app, cx| {
                                app.new_diff_tab(Some((0, kind, title, text, url)), cx)
                            })
                            .ok();
                        cx.notify();
                    }))
                    .into_any_element(),
                Button::new("copy-raw")
                    .icon(IconName::Copy)
                    .ghost()
                    .small()
                    .tooltip(tr!("复制全部 JSON"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let text = this.doc.pretty.clone();
                        this.copy_text(text, tr!("已复制全部 JSON").to_string(), cx);
                    }))
                    .into_any_element(),
            ],
            cx,
        );
        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(
                v_flex()
                    .id("raw-scroll")
                    .relative()
                    .flex_1()
                    .size_full()
                    .py_1()
                    .key_context(RAW_CONTEXT)
                    .track_focus(&self.raw_focus)
                    // Clicking the text takes the keyboard, so the keys below
                    // scroll this pane instead of the tree.
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.raw_focus.focus(window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &SelectUp, _, cx| {
                        let top = this.raw_scroll.top_item();
                        this.scroll_raw_to(top.saturating_sub(1), cx);
                    }))
                    .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
                        let top = this.raw_scroll.top_item();
                        this.scroll_raw_to(top + 1, cx);
                    }))
                    .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                        let top = this.raw_scroll.top_item();
                        let page = this.raw_page();
                        this.scroll_raw_to(top.saturating_sub(page), cx);
                    }))
                    .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                        let top = this.raw_scroll.top_item();
                        let page = this.raw_page();
                        this.scroll_raw_to(top + page, cx);
                    }))
                    .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                        this.scroll_raw_to(0, cx);
                    }))
                    .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                        this.scroll_raw_to(usize::MAX, cx);
                    }))
                    .child(
                        v_virtual_list(view, "raw-lines", sizes, |viewer, range, window, cx| {
                            viewer.render_raw_rows(range, window, cx)
                        })
                        .track_scroll(&self.raw_scroll),
                    )
                    .vertical_scrollbar(&self.raw_scroll),
            )
            .when(self.lines.is_empty(), |this| {
                this.child(
                    div()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .text_color(muted)
                        .text_sm()
                        .child(tr!("空文档")),
                )
            })
            .into_any_element()
    }

    fn render_raw_rows(
        &mut self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme();
        let settings = crate::settings::settings(cx);
        let colors = settings.syntax_colors(theme.is_dark());
        let foreground = theme.foreground;
        let row_h = self.line_h;
        let font_size = self.line_h - 8.0;
        let show_gutter = settings.line_numbers;
        let linked = self.linked;
        let mono = crate::settings::settings(cx).mono_family(cx);
        let lines = &self.lines;
        range
            .map(|ix| {
                let mut row = h_flex()
                    .w_full()
                    .h(px(row_h))
                    .flex_shrink_0()
                    .text_size(px(font_size))
                    .font_family(mono.clone())
                    .items_stretch()
                    // The line the selected tree node starts on.
                    .when(linked == Some(ix), |this| this.bg(theme.list_active));

                if show_gutter {
                    row = row.child(
                        div()
                            .w(px(56.))
                            .flex_shrink_0()
                            .text_align(TextAlign::Right)
                            .pr_2()
                            .mr_2()
                            .border_r_1()
                            .border_color(theme.border)
                            .text_color(theme.muted_foreground)
                            .child((ix + 1).to_string()),
                    );
                }

                let mut text = h_flex().flex_1().min_w_0().overflow_hidden();
                for span in &lines[ix] {
                    let color: Hsla = match span.kind {
                        SpanKind::Key => colors.key.into(),
                        SpanKind::Str => colors.string.into(),
                        SpanKind::Num => colors.number.into(),
                        SpanKind::Bool => colors.bool_kw.into(),
                        SpanKind::Null => colors.null.into(),
                        SpanKind::Punct => colors.punct.into(),
                        SpanKind::Indent | SpanKind::Plain => foreground,
                    };
                    text = text.child(
                        div()
                            .whitespace_nowrap()
                            .text_color(color)
                            .child(span.text.clone()),
                    );
                }
                row = row.child(text);
                row.into_any_element()
            })
            .collect()
    }

    // ----- right: tree panel -----

    fn render_tree_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (bg, muted) = {
            let theme = cx.theme();
            (theme.background, theme.muted_foreground)
        };
        let doc = self.doc.clone();
        let viewer = cx.weak_entity();
        let query_active = self.query.is_some();
        let match_count = self.match_count;
        let empty = self.tree_state.read(cx).entry(0).is_none();

        let header = chrome::panel_header(
            tr!("结构树"),
            vec![if query_active {
                chrome::stat(format!("过滤 · {match_count} 处匹配"), cx)
            } else {
                chrome::stat(format!("{} 个节点", doc.nodes.len()), cx)
            }],
            vec![
                // The tree's own filter, in the tree's header: typing narrows
                // this pane, nothing else on the page.
                div()
                    .w(px(220.))
                    .min_w(px(120.))
                    .child(Input::new(&self.search).small().cleanable(true))
                    .into_any_element(),
                Button::new("expand-all")
                    .icon(IconName::ChevronsDownUp)
                    .ghost()
                    .small()
                    .tooltip(tr!("展开全部 (⌘⇧E)"))
                    .on_click(cx.listener(|this, _, _, cx| this.expand_all(cx)))
                    .into_any_element(),
                Button::new("collapse-all")
                    .icon(IconName::ChevronsUpDown)
                    .ghost()
                    .small()
                    .tooltip(tr!("收起全部 (⌘⇧C)"))
                    .on_click(cx.listener(|this, _, _, cx| this.collapse_all(cx)))
                    .into_any_element(),
            ],
            cx,
        );

        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(div().flex_1().min_h_0().relative().p_1().child(if empty {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(muted)
                    .child(tr!("没有匹配的节点"))
                    .into_any_element()
            } else {
                tree(&self.tree_state, {
                    let doc = doc.clone();
                    let state = self.tree_state.clone();
                    move |ix, entry, _selected, _window, cx| node_row(ix, entry, &doc, &state, cx)
                })
                .context_menu({
                    let doc = doc.clone();
                    let viewer = viewer.clone();
                    move |_ix, entry, menu, window, cx| {
                        node_context_menu(entry, &doc, &viewer, menu, window, cx)
                    }
                })
                .size_full()
                .into_any_element()
            }))
            .into_any_element()
    }

    // ----- bottom: status bar -----

    fn render_status(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let dark = theme.is_dark();
        let mono = crate::settings::settings(cx).mono_family(cx);

        // Left: the JSONPath (plus type badge) of the currently selected tree
        // node; a hint when nothing is selected.
        let selected = self
            .tree_state
            .read(cx)
            .selected_entry()
            .and_then(|entry| entry.item().id.parse::<usize>().ok())
            .map(|id| {
                let typ = self.doc.nodes[id].typ;
                (typ, self.doc.json_path(id))
            });

        let left = match selected {
            Some((typ, path)) => h_flex()
                .min_w_0()
                .gap_2()
                .items_center()
                .child(
                    Icon::new(type_icon(typ))
                        .small()
                        .text_color(type_color(typ, dark)),
                )
                .child(
                    div()
                        .text_xs()
                        .flex_shrink_0()
                        .text_color(type_color(typ, dark))
                        .child(typ.label()),
                )
                .child(div().text_xs().font_family(mono).truncate().child(path))
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr!("点选节点查看其 JSONPath 并在原文中定位 — ↑↓ 选节点，Tab 切栏，⌘F 过滤"))
                .into_any_element(),
        };

        let right = if let Some(toast) = self.toast.clone() {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Icon::new(if toast.ok {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleAlert
                    })
                    .small()
                    .text_color(if toast.ok {
                        theme.success
                    } else {
                        theme.danger
                    }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(toast.text),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        chrome::status(cx)
            .child(left)
            .child(div().flex_1())
            .child(right)
            .into_any_element()
    }
}

// ---------- tree row + context menu ----------

/// Render one visible tree row for a JSON node.
fn node_row(
    ix: usize,
    entry: &TreeEntry,
    doc: &JsonTree,
    tree: &Entity<TreeState>,
    cx: &mut gpui_kit::App,
) -> ListItem {
    let theme = cx.theme();
    let Ok(node_id) = entry.item().id.parse::<usize>() else {
        return ListItem::new(ix);
    };
    let node = &doc.nodes[node_id];
    let color = type_color(node.typ, theme.is_dark());
    // The tree shows JSON text too, so it uses the same mono font setting.
    let settings = crate::settings::settings(cx);
    let mono = settings.mono_family(cx);
    let json_font_size = settings.font_size;
    let trunc = |el: gpui_kit::Div| el.max_w(px(280.)).truncate();

    let mut label = h_flex()
        .items_center()
        .gap_1p5()
        .min_w_0()
        .text_size(px(json_font_size))
        .font_family(mono);
    label = label.child(Icon::new(type_icon(node.typ)).small().text_color(color));
    if let Some(key) = &node.key {
        label = label.child(
            trunc(div())
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .child(key.clone()),
        );
        label = label.child(
            div()
                .whitespace_nowrap()
                .text_color(theme.muted_foreground)
                .child(":"),
        );
    } else if let Some(i) = node.index {
        label = label.child(
            div()
                .whitespace_nowrap()
                .text_color(theme.muted_foreground)
                .child(format!("[{i}]")),
        );
    }
    label = label.child(
        trunc(div())
            .whitespace_nowrap()
            .text_color(color)
            .child(node.summary.clone()),
    );
    if let Some(kind) = child_count_unit(node.typ, node.children.len()) {
        label = label.child(
            div()
                .whitespace_nowrap()
                .text_size(px((json_font_size - 2.0).max(9.0)))
                .text_color(theme.muted_foreground)
                .child(format!("· {kind}")),
        );
    }

    ListItem::new(ix)
        .w_full()
        .rounded_sm()
        .pl(px(12. + entry.depth() as f32 * INDENT_PX))
        .pr_2()
        .child(label)
        // A click hands the keyboard to the tree, so `↑↓←→` and `enter`
        // navigate from there; the raw pane follows the selection by itself.
        .on_click({
            let tree = tree.clone();
            move |_, window, cx| {
                tree.update(cx, |state, cx| state.focus(window, cx));
            }
        })
}

/// The right-click menu for a node: copies with paths.
fn node_context_menu(
    entry: &TreeEntry,
    doc: &JsonTree,
    viewer: &WeakEntity<Viewer>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<TreeState>,
) -> PopupMenu {
    let Ok(id) = entry.item().id.parse::<usize>() else {
        return menu;
    };
    let node = &doc.nodes[id];
    let json_path = doc.json_path(id);

    let copy_key = {
        let viewer = viewer.clone();
        let text = node.key.clone().unwrap_or_default();
        PopupMenuItem::new(tr!("复制 Key"))
            .icon(IconName::Quote)
            .disabled(node.key.is_none())
            .on_click(move |_, _, cx| {
                copy_via_viewer(&viewer, text.clone(), tr!("已复制 Key").to_string(), cx);
            })
    };
    let copy_value = copy_item(
        tr!("复制 Value"),
        IconName::Copy,
        value_to_copy(doc, id),
        tr!("已复制 Value"),
        viewer,
    );
    let copy_string = {
        let viewer = viewer.clone();
        let text = scalar_value_text(doc, id);
        PopupMenuItem::new(tr!("复制字符串内容（不含引号）"))
            .icon(IconName::Quote)
            .disabled(node.typ != JsonType::String)
            .on_click(move |_, _, cx| {
                copy_via_viewer(&viewer, text.clone(), tr!("已复制字符串").to_string(), cx);
            })
    };
    let copy_pretty = copy_item(
        tr!("复制子树 JSON（格式化）"),
        IconName::FileBraces,
        doc.subtree_json(id).unwrap_or_default(),
        tr!("已复制格式化子树"),
        viewer,
    );
    let copy_compact = copy_item(
        tr!("复制子树 JSON（压缩）"),
        IconName::Braces,
        doc.subtree_json_compact(id).unwrap_or_default(),
        tr!("已复制压缩子树"),
        viewer,
    );
    let copy_all = copy_item(
        tr!("复制全部 JSON"),
        IconName::Copy,
        doc.pretty.clone(),
        tr!("已复制全部"),
        viewer,
    );

    let paths_menu = {
        let viewer = viewer.clone();
        let entries = [
            ("JSONPath", doc.json_path(id), IconName::Hash),
            ("JSON Pointer", doc.json_pointer(id), IconName::GitBranch),
            ("jq", doc.jq_path(id), IconName::Terminal),
        ];
        PopupMenu::build(window, cx, |sub, _, _| {
            let mut menu = sub;
            for (label, path, icon) in entries {
                let viewer = viewer.clone();
                let toast = format!("已复制 {label} 路径");
                menu = menu.item(
                    PopupMenuItem::new(format!("{label} — {path}"))
                        .icon(icon)
                        .on_click(move |_, _, cx| {
                            copy_via_viewer(&viewer, path.clone(), toast.clone(), cx);
                        }),
                );
            }
            menu
        })
    };

    menu.max_w(px(560.))
        .label(format!("{} · {}", json_path, node.typ.label()))
        .separator()
        .item(copy_key)
        .item(copy_value)
        .item(copy_string)
        .item(copy_pretty)
        .item(copy_compact)
        .separator()
        .item(PopupMenuItem::submenu(tr!("复制路径…"), paths_menu))
        .separator()
        .item(copy_all)
}
