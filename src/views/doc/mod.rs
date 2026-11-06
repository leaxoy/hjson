//! The document view page: raw JSON and the tree side by side, each headed by
//! its own strip, and a status bar at the bottom. The document's controls —
//! reload and compare — sit in the raw pane's header; the tree's filter sits in
//! the tree's header, next to what it filters.
//!
//! The two panes are one view of one document: selecting a node moves the raw
//! editor cursor to the line its value starts on and reveals that line. `tab`
//! moves the keyboard between the panes; the tree takes `↑↓←→` and `enter`,
//! while the raw pane starts as a readonly editor for selection, scrolling, and
//! copy. Its header can unlock editing when a user wants to patch the text.
//!
//! The document view only ever receives documents that already parsed successfully;
//! parse errors are caught on the home page.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::{Selectable as _, actions::Confirm};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonGroup, ButtonVariants as _},
    h_flex,
    input::{Editor, EditorState, Input, InputEvent, InputState, Position},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ListAlignment, ListState, ParentElement as _,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Task, WeakEntity, Window, div, px,
};

use crate::app::{CollapseAll, ExpandAll};
use crate::components::chrome;
use crate::components::json_tree::{
    self, JsonTreeView, JsonTreeViewProps, TreeRow, build_tree_rows, tree_list_splice, type_color,
    type_icon,
};
use crate::components::source_menu::{self, LoadOptions};
use crate::document::{self, LoadedDocument, SourceKind};
use crate::error::{AppError, Status};
use crate::json_model::{JsonTree, JsonType};

mod loading;
mod raw_panel;
mod status;
mod tree_panel;

const TREE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);
const RAW_SYNC_DEBOUNCE: Duration = Duration::from_millis(180);
const TOAST_DURATION: Duration = Duration::from_secs(3);

// `tab` in one of the two panes: the keyboard moves to the other one.
gpui_kit::actions!(hjson, [NextPane]);

/// Key context of the raw pane, the only one this page owns.
const RAW_CONTEXT: &str = "hjson-raw";
/// Keys for the two panes, in their own contexts: the arrows mean "walk the
/// tree" in one and "scroll the text" in the other.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-e", ExpandAll, Some(json_tree::TREE_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-e", ExpandAll, Some(json_tree::TREE_CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-c", CollapseAll, Some(json_tree::TREE_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-c", CollapseAll, Some(json_tree::TREE_CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-e", ExpandAll, Some(RAW_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-e", ExpandAll, Some(RAW_CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-c", CollapseAll, Some(RAW_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-c", CollapseAll, Some(RAW_CONTEXT)),
        KeyBinding::new(
            "enter",
            Confirm { secondary: false },
            Some(json_tree::TREE_CONTEXT),
        ),
        KeyBinding::new("tab", NextPane, Some(json_tree::TREE_CONTEXT)),
        KeyBinding::new("tab", NextPane, Some(RAW_CONTEXT)),
    ]);
}

/// How aggressively the rebuilt tree starts out expanded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpandMode {
    /// Only the root is expanded (the default view).
    Root,
    /// Every level expanded (`展开全部`).
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DocPaneMode {
    Split,
    Text,
    Tree,
}

impl DocPaneMode {
    const ALL: [DocPaneMode; 3] = [Self::Split, Self::Text, Self::Tree];

    fn id(self) -> &'static str {
        match self {
            Self::Split => "doc-pane-mode-split",
            Self::Text => "doc-pane-mode-text",
            Self::Tree => "doc-pane-mode-tree",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Split => IconName::Columns2,
            Self::Text => IconName::FileText,
            Self::Tree => IconName::ListTree,
        }
    }

    fn tooltip(self) -> SharedString {
        match self {
            Self::Split => tr!("Show text and tree"),
            Self::Text => tr!("Show text only"),
            Self::Tree => tr!("Show tree only"),
        }
    }
}

pub struct DocView {
    app: WeakEntity<crate::app::HJsonApp>,
    pub title: SharedString,
    pub kind: SourceKind,
    /// Original URL when this document came from HTTP, for toolbar re-fetch.
    url: Option<String>,
    doc: Arc<JsonTree>,
    raw_editor: Entity<EditorState>,
    raw_editable: bool,
    tree_rows: Rc<Vec<TreeRow>>,
    expanded: HashSet<usize>,
    selected_node: Option<usize>,
    tree_list: ListState,
    tree_focus: FocusHandle,
    pane_mode: DocPaneMode,
    search: Entity<InputState>,
    search_task: Option<Task<()>>,
    raw_sync_task: Option<Task<()>>,
    raw_sync_generation: u64,
    query: Option<String>,
    match_count: usize,
    toast: Option<Status>,
    toast_generation: u64,
    _subs: Vec<Subscription>,
}

impl DocView {
    pub fn new(
        app: WeakEntity<crate::app::HJsonApp>,
        title: &str,
        kind: SourceKind,
        url: Option<String>,
        doc: Arc<JsonTree>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = crate::views::settings::settings(cx);
        let line_h = settings.row_height();
        let raw_wrap = settings.wrap_raw_lines;
        let line_numbers = settings.line_numbers;
        let sort_keys = settings.sort_object_keys;
        let raw_text = doc.pretty.clone();
        let raw_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .default_value(raw_text)
                .language("json")
                .line_number(line_numbers)
                .soft_wrap(raw_wrap)
                .scroll_beyond_last_line(Some(0))
                .scroll_bounce(false)
        });
        let expanded = HashSet::from([0]);
        let (tree_rows, _) = build_tree_rows(&doc, None, &expanded, sort_keys);
        let tree_list = ListState::new(
            tree_rows.len(),
            ListAlignment::Top,
            px(json_tree::LIST_OVERDRAW_PX),
        )
        .with_uniform_item_height(px(line_h + json_tree::ROW_PADDING_PX));
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Search keys or values… (filters the tree)"))
        });

        let search_sub = cx.subscribe_in(&search, window, |this, state, event, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let q = state.read(cx).value().trim().to_string();
                this.schedule_filter(q, cx);
            }
        });
        let raw_sub = cx.subscribe_in(&raw_editor, window, |this, state, event, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let text = state.read(cx).value().to_string();
                this.schedule_raw_sync(text, cx);
            }
        });

        Self {
            app,
            title: title.into(),
            kind,
            url,
            raw_editor,
            doc,
            raw_editable: false,
            tree_rows: Rc::new(tree_rows),
            expanded,
            selected_node: None,
            tree_list,
            tree_focus: cx.focus_handle(),
            pane_mode: DocPaneMode::Split,
            search,
            search_task: None,
            raw_sync_task: None,
            raw_sync_generation: 0,
            query: None,
            match_count: 0,
            toast: None,
            toast_generation: 0,
            _subs: vec![search_sub, raw_sub],
        }
    }

    // ---------- actions ----------

    fn schedule_filter(&mut self, query: String, cx: &mut Context<Self>) {
        self.search_task = Some(spawn_after(cx, TREE_SEARCH_DEBOUNCE, move |this, cx| {
            this.apply_filter(query, cx);
        }));
    }

    fn apply_filter(&mut self, query: String, cx: &mut Context<Self>) {
        self.query = (!query.is_empty()).then(|| query.to_lowercase());
        self.toast = None;
        self.rebuild_tree_rows(cx);
    }

    fn schedule_raw_sync(&mut self, text: String, cx: &mut Context<Self>) {
        self.raw_sync_generation = self.raw_sync_generation.wrapping_add(1);
        let generation = self.raw_sync_generation;
        self.raw_sync_task = Some(spawn_after(cx, RAW_SYNC_DEBOUNCE, move |this, cx| {
            if this.raw_sync_generation == generation {
                this.apply_raw_sync(text, cx);
            }
        }));
    }

    fn apply_raw_sync(&mut self, text: String, cx: &mut Context<Self>) {
        match JsonTree::parse(&text) {
            Ok(next) => self.replace_tree_document(Arc::new(next), cx),
            Err(err) => {
                self.toast = Some(Status::Error(AppError::parse(err)));
                cx.notify();
            }
        }
    }

    fn replace_tree_document(&mut self, doc: Arc<JsonTree>, cx: &mut Context<Self>) {
        let selected_path = self
            .selected_node
            .and_then(|id| self.doc.nodes.get(id).map(|_| self.doc.json_path(id)));
        let expanded_paths = self
            .expanded
            .iter()
            .filter_map(|&id| self.doc.nodes.get(id).map(|_| self.doc.json_path(id)))
            .collect::<HashSet<_>>();
        let ids_by_path = node_ids_by_path(&doc);

        self.selected_node = selected_path.and_then(|path| ids_by_path.get(&path).copied());
        self.expanded = expanded_paths
            .into_iter()
            .filter_map(|path| ids_by_path.get(&path).copied())
            .filter(|&id| id == 0 || !doc.nodes[id].children.is_empty())
            .collect();
        if !doc.nodes.is_empty() {
            self.expanded.insert(0);
        }
        if matches!(self.toast, Some(Status::Error(_))) {
            self.toast = None;
        }
        self.doc = doc;
        self.rebuild_tree_rows(cx);
    }

    /// `cmd-shift-e` / `cmd-shift-c`, and the tree header's two buttons.
    pub fn expand_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_focused_viewer_depth(true, window, cx);
    }

    pub fn collapse_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_focused_viewer_depth(false, window, cx);
    }

    fn set_focused_viewer_depth(
        &mut self,
        expand: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw_focused = self.raw_editor.focus_handle(cx).is_focused(window);
        let tree_focused =
            self.tree_focus.is_focused(window) || self.search.focus_handle(cx).is_focused(window);

        if raw_focused || (!tree_focused && self.pane_mode == DocPaneMode::Text) {
            self.set_raw_depth(expand, window, cx);
        } else {
            self.set_expand(
                if expand {
                    ExpandMode::All
                } else {
                    ExpandMode::Root
                },
                cx,
            );
        }
    }

    fn expand_tree_all(&mut self, cx: &mut Context<Self>) {
        self.set_expand(ExpandMode::All, cx);
    }

    fn collapse_tree_all(&mut self, cx: &mut Context<Self>) {
        self.set_expand(ExpandMode::Root, cx);
    }

    fn set_raw_depth(&mut self, expand: bool, window: &mut Window, cx: &mut Context<Self>) {
        if expand {
            self.raw_editor.update(cx, |editor, cx| {
                editor.set_folding(false, window, cx);
                editor.set_folding(true, window, cx);
            });
            cx.notify();
            return;
        }

        self.toast = Some(Status::from_parts(
            false,
            tr!("Raw collapse all requires a public editor folding API").to_string(),
        ));
        cx.notify();
    }

    fn set_expand(&mut self, expand: ExpandMode, cx: &mut Context<Self>) {
        self.expanded.clear();
        if !self.doc.nodes.is_empty() {
            self.expanded.insert(0);
        }
        if expand == ExpandMode::All {
            self.expanded.extend(
                self.doc
                    .nodes
                    .iter()
                    .enumerate()
                    .filter_map(|(id, node)| (!node.children.is_empty()).then_some(id)),
            );
        }
        self.rebuild_tree_rows(cx);
    }

    pub fn refresh_key_sort(&mut self, cx: &mut Context<Self>) {
        self.rebuild_tree_rows(cx);
    }

    fn rebuild_tree_rows(&mut self, cx: &mut Context<Self>) {
        let sort_keys = crate::views::settings::settings(cx).sort_object_keys;
        let (rows, matches) =
            build_tree_rows(&self.doc, self.query.as_deref(), &self.expanded, sort_keys);
        let old_ids = self
            .tree_rows
            .iter()
            .map(|row| row.node_id)
            .collect::<Vec<_>>();
        let new_ids = rows.iter().map(|row| row.node_id).collect::<Vec<_>>();
        let (old_range, new_count) = tree_list_splice(&old_ids, &new_ids);
        self.tree_list.splice(old_range, new_count);
        self.tree_rows = Rc::new(rows);
        self.match_count = matches;
        cx.notify();
    }

    fn move_tree_selection(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tree_rows.len();
        if count == 0 {
            return;
        }
        let selected = self
            .selected_node
            .and_then(|id| self.tree_rows.iter().position(|row| row.node_id == id));
        let next = match (selected, delta.is_negative()) {
            (None, _) => 0,
            (Some(0), true) => count - 1,
            (Some(ix), true) => ix - 1,
            (Some(ix), false) if ix + 1 >= count => 0,
            (Some(ix), false) => ix + 1,
        };
        self.select_tree_node(self.tree_rows[next].node_id, window, cx);
        self.tree_list.scroll_to_reveal_item(next);
    }

    fn toggle_tree_node(&mut self, node_id: usize, cx: &mut Context<Self>) {
        if node_id == 0
            || self.query.is_some()
            || self
                .doc
                .nodes
                .get(node_id)
                .is_none_or(|node| node.children.is_empty())
        {
            return;
        }
        if !self.expanded.remove(&node_id) {
            self.expanded.insert(node_id);
        }
        self.rebuild_tree_rows(cx);
    }

    fn activate_tree_node(&mut self, node_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.tree_focus.focus(window, cx);
        self.select_tree_node(node_id, window, cx);
        self.toggle_tree_node(node_id, cx);
    }

    fn confirm_tree_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(node_id) = self.selected_node {
            self.toggle_tree_node(node_id, cx);
        }
    }

    fn collapse_tree_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node_id) = self.selected_node else {
            return;
        };
        if self.expanded.contains(&node_id) && node_id != 0 {
            self.toggle_tree_node(node_id, cx);
            return;
        }
        if let Some(parent) = self.doc.nodes[node_id].parent {
            self.select_tree_node(parent, window, cx);
            if let Some(ix) = self.tree_rows.iter().position(|row| row.node_id == parent) {
                self.tree_list.scroll_to_reveal_item(ix);
            }
        }
    }

    fn expand_tree_selection(&mut self, cx: &mut Context<Self>) {
        let Some(node_id) = self.selected_node else {
            return;
        };
        if !self.expanded.contains(&node_id) {
            self.toggle_tree_node(node_id, cx);
        }
    }

    // ---------- pane linkage ----------

    fn select_tree_node(&mut self, node_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_node = Some(node_id);
        if let Some(line) = self.doc.line_of(node_id) {
            self.reveal_raw_line(line, window, cx);
        }
        cx.notify();
    }

    fn reveal_raw_line(&self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tree_was_focused = self.tree_focus.is_focused(window);
        let line = line.min(u32::MAX as usize) as u32;
        self.raw_editor.update(cx, |editor, cx| {
            editor.unfold_at(Position::new(line, 0), cx);
            editor.set_cursor_position(Position::new(line, 0), window, cx);
        });
        if tree_was_focused {
            self.tree_focus.focus(window, cx);
        }
    }

    fn focus_raw_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.raw_editor
            .update(cx, |editor, cx| editor.focus(window, cx));
    }

    /// `cmd-f`: put the cursor in the tree's filter.
    pub fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.focus_handle(cx).focus(window, cx);
    }

    fn copy_text(&mut self, text: String, toast: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.say(true, toast, cx);
    }

    fn raw_text(&self, cx: &mut Context<Self>) -> String {
        self.raw_editor.read(cx).value().to_string()
    }

    fn current_document_snapshot(
        &self,
        cx: &mut Context<Self>,
    ) -> Result<LoadedDocument, AppError> {
        let text = self.raw_text(cx);
        let parsed = (text == self.doc.pretty).then(|| self.doc.clone());
        document::from_history(
            self.kind,
            self.title.to_string(),
            text,
            self.url.clone(),
            parsed,
        )
    }

    fn save_json(&mut self, compact: bool, cx: &mut Context<Self>) {
        let text = if compact {
            match self.current_document_snapshot(cx) {
                Ok(document) => serde_json::to_string(&document.parsed.root).unwrap_or_default(),
                Err(err) => {
                    self.say(false, err.message(), cx);
                    return;
                }
            }
        } else {
            self.raw_text(cx)
        };
        let mut name = self.title.replace(['/', '\\', ':'], "_").trim().to_string();
        if !name.to_lowercase().ends_with(".json") {
            name.push_str(".json");
        }
        let directory = std::env::current_dir().unwrap_or_default();
        let receiver = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(path))) => path,
                _ => return,
            };
            let write = cx.background_spawn(async move { std::fs::write(&path, text) });
            let result = write.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.say(true, tr!("JSON saved"), cx),
                Err(err) => this.say(false, AppError::save(err).message(), cx),
            });
        })
        .detach();
    }

    /// Success toasts stay in the statusbar; failures pop up as notifications.
    fn say(&mut self, ok: bool, text: impl Into<String>, cx: &mut Context<Self>) {
        let Some(text) = chrome::inline_success_or_notify_failure(ok, text, cx) else {
            return;
        };
        self.toast_generation = self.toast_generation.wrapping_add(1);
        let toast_generation = self.toast_generation;
        self.toast = Some(Status::from_parts(true, text));
        cx.notify();
        spawn_after(cx, TOAST_DURATION, move |this, cx| {
            if this.toast_generation == toast_generation {
                this.toast = None;
                cx.notify();
            }
        })
        .detach();
    }

    // ---------- reloading this document ----------

    /// Swap this tab to a fully loaded document and register it in history.
    fn replace_with(
        &mut self,
        document: LoadedDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let me = cx.entity();
        let outcome = self.app.update(cx, |app, cx| {
            app.replace_document_in_view(&me, document, window, cx)
        });
        match outcome {
            Ok(Ok(_)) => self.toast = None,
            Ok(Err(err)) => self.say(false, AppError::load(err).message(), cx),
            Err(err) => self.say(false, AppError::load(err).message(), cx),
        }
        cx.notify();
    }

    fn open_node_in_new_tab(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.doc.subtree_json(id) else {
            self.say(false, tr!("Could not open node").to_string(), cx);
            return;
        };
        let path = self.doc.json_path(id);
        let title = format!("{} · {}", self.title, path);
        self.open_text_in_new_tab(title, text, window, cx);
    }

    fn open_json_string_node_in_new_tab(
        &mut self,
        id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = string_value_text(&self.doc, id) else {
            self.say(false, tr!("Could not open node").to_string(), cx);
            return;
        };
        let path = self.doc.json_path(id);
        let title = format!("{} · {} · {}", self.title, path, tr!("Parsed string"));
        self.open_text_in_new_tab(title, text, window, cx);
    }

    fn open_text_in_new_tab(
        &mut self,
        title: String,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match document::from_text(SourceKind::Manual, title, text, None) {
            Ok(document) => {
                let outcome = self
                    .app
                    .update(cx, |app, cx| app.open_document(document, window, cx));
                if let Err(err) = outcome {
                    self.say(false, AppError::load(err).message(), cx);
                }
            }
            Err(err) => self.say(false, err.message(), cx),
        }
    }

    fn toggle_raw_editable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.raw_editable = !self.raw_editable;
        let readonly = !self.raw_editable;
        self.raw_editor
            .update(cx, |editor, cx| editor.set_readonly(readonly, cx));
        if self.raw_editable {
            self.focus_raw_editor(window, cx);
        }
        cx.notify();
    }
}

/// Copy through the document view so it can show a toast. Lives outside `DocView`
/// so context-menu closures (which only capture the weak handle) can use it.
fn copy_via_doc_view(
    doc_view: &WeakEntity<DocView>,
    text: String,
    toast: String,
    cx: &mut gpui_kit::App,
) {
    let _ = doc_view.update(cx, |doc_view, cx| doc_view.copy_text(text, toast, cx));
}

/// Build a `PopupMenuItem` that copies `text` on click.
fn copy_item(
    label: impl Into<SharedString>,
    icon: IconName,
    text: String,
    toast: impl Into<SharedString>,
    doc_view: &WeakEntity<DocView>,
) -> PopupMenuItem {
    let doc_view = doc_view.clone();
    let toast = toast.into().to_string();
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, _, cx| copy_via_doc_view(&doc_view, text.clone(), toast.clone(), cx))
}

/// The scalar text behind a node: strings without quotes, everything else as-is.
fn scalar_value_text(doc: &JsonTree, id: usize) -> String {
    match string_value_text(doc, id) {
        Some(s) => s,
        None => match doc.resolve_ref(id) {
            Some(v) => v.to_string(),
            None => doc.nodes[id].summary.clone(),
        },
    }
}

fn string_value_text(doc: &JsonTree, id: usize) -> Option<String> {
    match doc.resolve_ref(id) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn is_json_text(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).is_ok()
}

fn node_ids_by_path(doc: &JsonTree) -> HashMap<String, usize> {
    doc.nodes
        .iter()
        .enumerate()
        .map(|(id, _)| (doc.json_path(id), id))
        .collect()
}

fn spawn_after<T: 'static>(
    cx: &mut Context<T>,
    delay: Duration,
    run: impl FnOnce(&mut T, &mut Context<T>) + 'static,
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(delay).await;
        let _ = this.update(cx, run);
    })
}

/// What "Copy value" should copy: raw scalar text, or the pretty subtree.
fn value_to_copy(doc: &JsonTree, id: usize) -> String {
    if doc.nodes[id].children.is_empty() {
        scalar_value_text(doc, id)
    } else {
        doc.subtree_json(id).unwrap_or_else(|| "{}".to_string())
    }
}

// ---------- rendering ----------

impl gpui_kit::Render for DocView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .font_family(theme.font_family.clone())
            // `tab` arrives from whichever pane had focus; hand it to the other.
            .on_action(cx.listener(|this, _: &NextPane, window, cx| {
                if this.raw_editor.focus_handle(cx).is_focused(window) {
                    this.tree_focus.focus(window, cx);
                } else {
                    this.focus_raw_editor(window, cx);
                }
            }))
            .child(
                // The resizable group fills the remaining height.
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_body(window, cx)),
            )
            .child(self.render_status(cx))
    }
}

impl DocView {
    /// This document as the A side of a new comparison.
    pub(crate) fn diff_snapshot(&self, cx: &mut Context<Self>) -> Result<LoadedDocument, AppError> {
        self.current_document_snapshot(cx)
    }

    fn render_split(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        // No initial size on either panel: the group's flex defaults split the
        // width evenly, and the measured even ratio then survives a window
        // resize. The lower bound stops a drag from collapsing a pane to
        // nothing; the upper stays open so a wide window keeps the halves even.
        h_resizable("doc-view-split")
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_raw_panel(window, cx)),
            )
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_tree_panel(window, cx)),
            )
            .into_any_element()
    }

    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.pane_mode {
            DocPaneMode::Split => self.render_split(window, cx),
            DocPaneMode::Text => self.render_raw_panel(window, cx),
            DocPaneMode::Tree => self.render_tree_panel(window, cx),
        }
    }

    fn set_pane_mode(&mut self, mode: DocPaneMode, window: &mut Window, cx: &mut Context<Self>) {
        self.pane_mode = mode;
        match mode {
            DocPaneMode::Split => {}
            DocPaneMode::Text => self.focus_raw_editor(window, cx),
            DocPaneMode::Tree => self.tree_focus.focus(window, cx),
        }
        cx.notify();
    }

    fn pane_mode_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.pane_mode;
        ButtonGroup::new("doc-pane-mode")
            .compact()
            .children(DocPaneMode::ALL.into_iter().map(|mode| {
                Button::new(mode.id())
                    .icon(mode.icon())
                    .ghost()
                    .small()
                    .selected(mode == selected)
                    .accessibility_label(mode.tooltip())
                    .tooltip(mode.tooltip())
            }))
            .on_click(cx.listener(|this, selected: &Vec<usize>, window, cx| {
                let Some(ix) = selected.first() else {
                    return;
                };
                if let Some(mode) = DocPaneMode::ALL.get(*ix).copied() {
                    this.set_pane_mode(mode, window, cx);
                }
            }))
            .into_any_element()
    }
}
