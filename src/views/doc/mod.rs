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
use crate::components::source_menu::{self, LoadMethod, LoadOptions, LoadTarget};
use crate::document::{self, LoadedDocument, SourceKind};
use crate::error::{AppError, Status};
use crate::json_model::{JsonTree, JsonType};

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
    /// Editor and tree row height derived from the current text settings.
    line_h: f32,
    raw_wrap: bool,
    raw_editable: bool,
    /// Raw editor line revealed for the selected tree node. `None` until
    /// something is selected.
    linked: Option<usize>,
    tree_rows: Rc<Vec<TreeRow>>,
    expanded: HashSet<usize>,
    selected_node: Option<usize>,
    tree_list: ListState,
    tree_focus: FocusHandle,
    pane_mode: DocPaneMode,
    search: Entity<InputState>,
    search_locale: &'static str,
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
        let search_locale = settings.language.locale();
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
            line_h,
            raw_wrap,
            raw_editable: false,
            linked: None,
            tree_rows: Rc::new(tree_rows),
            expanded,
            selected_node: None,
            tree_list,
            tree_focus: cx.focus_handle(),
            pane_mode: DocPaneMode::Split,
            search,
            search_locale,
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
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TREE_SEARCH_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| this.apply_filter(query, cx));
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
        self.raw_sync_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RAW_SYNC_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                if this.raw_sync_generation == generation {
                    this.apply_raw_sync(text, cx);
                }
            });
        }));
    }

    fn apply_raw_sync(&mut self, text: String, cx: &mut Context<Self>) {
        let next = JsonTree::parse(&text);
        if let Some(err) = next.error.clone() {
            self.toast = Some(Status::Error(AppError::parse(err)));
            cx.notify();
            return;
        }

        self.replace_tree_document(Arc::new(next), cx);
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
        self.linked = self.selected_node.and_then(|id| doc.line_of(id));
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
        let line = self.doc.line_of(node_id);
        if line != self.linked {
            self.linked = line;
            if let Some(line) = line {
                self.reveal_raw_line(line, window, cx);
            }
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
        let text = text.into();
        if ok {
            self.toast_generation = self.toast_generation.wrapping_add(1);
            let toast_generation = self.toast_generation;
            self.toast = Some(Status::from_parts(true, text));
            cx.notify();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TOAST_DURATION).await;
                let _ = this.update(cx, |this, cx| {
                    if this.toast_generation == toast_generation {
                        this.toast = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        } else {
            chrome::notify_failure(text, cx);
        }
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

    /// Replace this document with the clipboard contents.
    fn load_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .map(|t| t.trim().to_string());
        match text {
            Some(text) if !text.is_empty() => match document::from_clipboard(text) {
                Ok(document) => self.replace_with(document, window, cx),
                Err(err) => self.say(false, err.message(), cx),
            },
            Some(_) => self.say(false, tr!("The clipboard is empty"), cx),
            None => self.say(false, tr!("No text on the clipboard"), cx),
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
            let read = cx.background_spawn(async move { document::from_file(path) });
            let result = read.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(document) => this.replace_with(document, window, cx),
                Err(e) => this.say(false, e.message(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Replace this document with the body of `url`.
    fn load_fetch(&mut self, url: String, custom_title: Option<String>, cx: &mut Context<Self>) {
        self.say(
            true,
            tr!("Requesting %{url} …", url = url.clone()).to_string(),
            cx,
        );
        let task = cx.background_spawn(async move { document::from_url(url, custom_title) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(document) => this.replace_with(document, window, cx),
                Err(e) => this.say(false, e.message(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Replace this document with one from the session history.
    fn load_history(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let found = source_menu::prepared_doc_of(&self.app, cx, id);
        match found {
            Some((kind, title, text, url, parsed)) => {
                match document::from_history(kind, title.to_string(), text, url, parsed) {
                    Ok(document) => self.replace_with(document, window, cx),
                    Err(err) => self.say(false, err.message(), cx),
                }
            }
            None => self.say(false, tr!("That entry is no longer in the list"), cx),
        }
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

impl LoadTarget for DocView {
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
                    self.load_fetch(url, Some(self.title.to_string()), cx);
                }
            }
            LoadMethod::FetchUrl { url, title } => self.load_fetch(url, title, cx),
            LoadMethod::Manual(text) => match document::from_manual(text) {
                Ok(document) => self.replace_with(document, window, cx),
                Err(err) => self.say(false, err.message(), cx),
            },
            LoadMethod::Sample => match document::from_sample() {
                Ok(document) => self.replace_with(document, window, cx),
                Err(err) => self.say(false, err.message(), cx),
            },
            LoadMethod::History(id) => self.load_history(id, window, cx),
        }
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

    // ----- left: raw panel -----

    fn render_raw_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let bg = cx.theme().background;
        let (wrap_raw_lines, line_numbers, wanted_h, mono) = {
            let settings = crate::views::settings::settings(cx);
            (
                settings.wrap_raw_lines,
                settings.line_numbers,
                settings.row_height(),
                settings.mono_family(cx),
            )
        };
        if self.raw_wrap != wrap_raw_lines {
            self.raw_wrap = wrap_raw_lines;
            self.raw_editor.update(cx, |editor, cx| {
                editor.set_soft_wrap(wrap_raw_lines, window, cx);
            });
        }
        self.raw_editor.update(cx, |editor, cx| {
            editor.set_line_number(line_numbers, window, cx);
        });
        // React to font size changes made on the settings page.
        if (self.line_h - wanted_h).abs() > f32::EPSILON {
            self.line_h = wanted_h;
            self.tree_list.remeasure();
        }
        let raw_focus = self.raw_editor.focus_handle(cx);
        let raw_text = self.raw_text(cx);
        let line_count = raw_text.lines().count();
        let byte_size = raw_text.len();
        let edit_tooltip = if self.raw_editable {
            tr!("Disable raw editing")
        } else {
            tr!("Enable raw editing")
        };
        let header = chrome::panel_toolbar(
            "doc-raw-toolbar",
            tr!("Raw"),
            vec![
                chrome::stat(tr!("%{count} lines", count = line_count.to_string()), cx),
                crate::app::human_size(byte_size, cx),
            ],
            vec![
                // The document's own controls, docked in the document's
                // header: reload it (from its source or another one), and
                // start a comparison with it on the A side.
                source_menu::header_load_button(
                    "reload-doc",
                    tr!("Reload this document from its source, or switch to another"),
                    (),
                    LoadOptions {
                        url: self.url.clone(),
                        history: source_menu::history_of(&self.app, cx),
                    },
                    cx.weak_entity(),
                ),
                Button::new("toggle-raw-editable")
                    .icon(if self.raw_editable {
                        IconName::LockOpen
                    } else {
                        IconName::Pencil
                    })
                    .ghost()
                    .small()
                    .selected(self.raw_editable)
                    .accessibility_label(edit_tooltip.clone())
                    .tooltip(edit_tooltip)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_raw_editable(window, cx);
                    }))
                    .into_any_element(),
                Button::new("toggle-raw-line-numbers")
                    .icon(IconName::ListOrdered)
                    .ghost()
                    .small()
                    .selected(line_numbers)
                    .accessibility_label(tr!("Show line numbers"))
                    .tooltip(tr!("Show line numbers"))
                    .on_click(cx.listener(|_, _, _, cx| {
                        crate::views::settings::update_settings(cx, |settings| {
                            settings.line_numbers = !settings.line_numbers;
                        });
                    }))
                    .into_any_element(),
                Button::new("expand-raw-folds")
                    .icon(IconName::ChevronsUpDown)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Expand all"),
                        chrome::shortcut_suffix(&ExpandAll, window)
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_raw_depth(true, window, cx);
                    }))
                    .into_any_element(),
                Button::new("collapse-raw-folds")
                    .icon(IconName::ChevronsDownUp)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Collapse all"),
                        chrome::shortcut_suffix(&CollapseAll, window)
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_raw_depth(false, window, cx);
                    }))
                    .into_any_element(),
                Button::new("compare-doc")
                    .icon(gpui_kit::assets::IconName::GitCompare)
                    .ghost()
                    .small()
                    .tooltip(tr!(
                        "Put this document on side A and compare it with another"
                    ))
                    .on_click(cx.listener(|this, _, _window, cx| {
                        // Re-serializing the root yields semantically
                        // identical JSON; an empty document has nothing to
                        // hand over.
                        let document = match this.diff_snapshot(cx) {
                            Ok(document) => document,
                            Err(err) => {
                                this.say(false, err.message(), cx);
                                return;
                            }
                        };
                        this.app
                            .update(cx, |app, cx| app.new_diff_tab(Some((0, document)), cx))
                            .ok();
                        cx.notify();
                    }))
                    .into_any_element(),
                Button::new("copy-raw")
                    .icon(IconName::Copy)
                    .ghost()
                    .small()
                    .tooltip(tr!("Copy all JSON"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let text = this.raw_text(cx);
                        this.copy_text(text, tr!("All JSON copied").to_string(), cx);
                    }))
                    .into_any_element(),
                Button::new("save-json")
                    .icon(IconName::Download)
                    .ghost()
                    .small()
                    .tooltip(tr!("Save JSON"))
                    .dropdown_menu({
                        let doc_view = cx.weak_entity();
                        move |menu, _, _| {
                            let pretty_doc_view = doc_view.clone();
                            let compact_doc_view = doc_view.clone();
                            menu.item(
                                PopupMenuItem::new(tr!("Save formatted JSON"))
                                    .icon(IconName::FileBraces)
                                    .on_click(move |_, _, cx| {
                                        let _ = pretty_doc_view.update(cx, |doc_view, cx| {
                                            doc_view.save_json(false, cx)
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new(tr!("Save compact JSON"))
                                    .icon(IconName::Braces)
                                    .on_click(move |_, _, cx| {
                                        let _ = compact_doc_view.update(cx, |doc_view, cx| {
                                            doc_view.save_json(true, cx)
                                        });
                                    }),
                            )
                        }
                    })
                    .into_any_element(),
            ],
            cx,
        );
        let font_size = (self.line_h - 8.0).max(1.0);
        let body = Editor::new(&self.raw_editor)
            .readonly(!self.raw_editable)
            .appearance(false)
            .bordered(false)
            .size_full()
            .font_family(mono)
            .text_size(px(font_size))
            .line_height(px(self.line_h))
            .aria_label(tr!("Raw JSON"));
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
                    .track_focus(&raw_focus)
                    // Clicking the raw pane gives the editor keyboard focus.
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus_raw_editor(window, cx);
                    }))
                    .child(body),
            )
            .when(self.doc.pretty.is_empty(), |this| {
                this.child(chrome::empty_state(
                    IconName::FileText,
                    tr!("Empty document"),
                    None,
                ))
            })
            .into_any_element()
    }

    // ----- right: tree panel -----

    fn render_tree_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let bg = cx.theme().background;
        let doc = self.doc.clone();
        let doc_view = cx.weak_entity();
        let query_active = self.query.is_some();
        let match_count = self.match_count;
        let search_locale = crate::views::settings::settings(cx).language.locale();
        if self.search_locale != search_locale {
            self.search_locale = search_locale;
            let search_placeholder = tr!("Search keys or values… (filters the tree)");
            self.search.update(cx, |search, cx| {
                search.set_placeholder(search_placeholder, window, cx);
            });
        }

        let header = chrome::panel_toolbar(
            "doc-tree-toolbar",
            tr!("Tree"),
            vec![if query_active {
                chrome::stat(
                    tr!(
                        "Filtered · %{count} matches",
                        count = match_count.to_string()
                    ),
                    cx,
                )
            } else {
                chrome::stat(
                    tr!("%{count} nodes", count = doc.nodes.len().to_string()),
                    cx,
                )
            }],
            vec![
                // The tree's own filter, in the tree's header: typing narrows
                // this pane, nothing else on the page.
                div()
                    .w(px(220.))
                    .min_w(px(120.))
                    .child(
                        Input::new(&self.search)
                            .small()
                            .cleanable(true)
                            .focus_bordered(false),
                    )
                    .into_any_element(),
                Button::new("expand-all")
                    .icon(IconName::ChevronsUpDown)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Expand all"),
                        chrome::shortcut_suffix(&ExpandAll, window)
                    ))
                    .on_click(cx.listener(|this, _, _, cx| this.expand_tree_all(cx)))
                    .into_any_element(),
                Button::new("collapse-all")
                    .icon(IconName::ChevronsDownUp)
                    .ghost()
                    .small()
                    .tooltip(format!(
                        "{}{}",
                        tr!("Collapse all"),
                        chrome::shortcut_suffix(&CollapseAll, window)
                    ))
                    .on_click(cx.listener(|this, _, _, cx| this.collapse_tree_all(cx)))
                    .into_any_element(),
            ],
            cx,
        );

        let tree_view = JsonTreeView::new(JsonTreeViewProps {
            doc: doc.clone(),
            rows: self.tree_rows.clone(),
            selected_node: self.selected_node,
            list_state: self.tree_list.clone(),
            focus_handle: self.tree_focus.clone(),
            empty_message: tr!("No matching nodes"),
            on_activate: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |node_id, window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.activate_tree_node(node_id, window, cx);
                    });
                }
            })),
            on_context_menu: Some(Rc::new({
                let doc = doc.clone();
                let doc_view = doc_view.clone();
                move |node_id, menu, window, cx| {
                    node_context_menu(node_id, &doc, &doc_view, menu, window, cx)
                }
            })),
            on_select_up: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.move_tree_selection(-1, window, cx);
                    });
                }
            })),
            on_select_down: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.move_tree_selection(1, window, cx);
                    });
                }
            })),
            on_select_left: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |window, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.collapse_tree_selection(window, cx);
                    });
                }
            })),
            on_select_right: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |_, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.expand_tree_selection(cx);
                    });
                }
            })),
            on_confirm: Some(Rc::new({
                let doc_view = doc_view.clone();
                move |_, cx| {
                    let _ = doc_view.update(cx, |doc_view, cx| {
                        doc_view.confirm_tree_selection(cx);
                    });
                }
            })),
        })
        .render(window, cx);

        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(div().flex_1().min_h_0().relative().p_1().child(tree_view))
            .into_any_element()
    }

    // ----- bottom: status bar -----

    fn render_status(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let mono = crate::views::settings::settings(cx).mono_family(cx);
        let syntax_mode = crate::syntax::SyntaxMode::current(cx);

        // Left: the JSONPath (plus type badge) of the currently selected tree
        // node; a hint when nothing is selected.
        let selected = self.selected_node.map(|id| {
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
                        .text_color(type_color(typ, syntax_mode)),
                )
                .child(
                    div()
                        .text_xs()
                        .flex_shrink_0()
                        .text_color(type_color(typ, syntax_mode))
                        .child(typ.label()),
                )
                .child(div().text_xs().font_family(mono).child(path))
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr!(
                    "Select a node to see its JSONPath and locate it in the raw text — ↑↓ nodes, Tab panes, ⌘F filter"
                ))
                .into_any_element(),
        };

        let right = if let Some(toast) = self.toast.clone() {
            let ok = toast.is_success();
            let text = toast.message();
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Icon::new(if ok {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleAlert
                    })
                    .small()
                    .text_color(if ok { theme.success } else { theme.danger }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(text),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        chrome::status(cx)
            .left(left)
            .right(right)
            // One mode toggle for the whole page, docked at the status bar's
            // right edge instead of once per pane header.
            .right(self.pane_mode_toggle(cx))
            .into_any_element()
    }
}

// ---------- tree context menu ----------

/// The right-click menu for a node: copies with paths.
fn node_context_menu(
    id: usize,
    doc: &JsonTree,
    doc_view: &WeakEntity<DocView>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let node = &doc.nodes[id];
    let json_path = doc.json_path(id);

    let open_node = {
        let doc_view = doc_view.clone();
        PopupMenuItem::new(tr!("Open node in new tab"))
            .icon(IconName::ExternalLink)
            .on_click(move |_, window, cx| {
                let _ = doc_view.update(cx, |doc_view, cx| {
                    doc_view.open_node_in_new_tab(id, window, cx);
                });
            })
    };
    let parse_string = {
        let doc_view = doc_view.clone();
        let text = string_value_text(doc, id).unwrap_or_default();
        let can_parse = is_json_text(&text);
        PopupMenuItem::new(tr!("Parse string and open in new tab"))
            .icon(IconName::FileBraces)
            .disabled(!can_parse)
            .on_click(move |_, window, cx| {
                let _ = doc_view.update(cx, |doc_view, cx| {
                    doc_view.open_json_string_node_in_new_tab(id, window, cx);
                });
            })
    };
    let copy_key = {
        let doc_view = doc_view.clone();
        let text = node.key.clone().unwrap_or_default();
        PopupMenuItem::new(tr!("Copy key"))
            .icon(IconName::Quote)
            .disabled(node.key.is_none())
            .on_click(move |_, _, cx| {
                copy_via_doc_view(&doc_view, text.clone(), tr!("Key copied").to_string(), cx);
            })
    };
    let copy_value = copy_item(
        tr!("Copy value"),
        IconName::Copy,
        value_to_copy(doc, id),
        tr!("Value copied"),
        doc_view,
    );
    let copy_string = {
        let doc_view = doc_view.clone();
        let text = scalar_value_text(doc, id);
        PopupMenuItem::new(tr!("Copy string without quotes"))
            .icon(IconName::Quote)
            .disabled(node.typ != JsonType::String)
            .on_click(move |_, _, cx| {
                copy_via_doc_view(
                    &doc_view,
                    text.clone(),
                    tr!("String copied").to_string(),
                    cx,
                );
            })
    };
    let copy_pretty = copy_item(
        tr!("Copy subtree (formatted)"),
        IconName::FileBraces,
        doc.subtree_json(id).unwrap_or_default(),
        tr!("Formatted subtree copied"),
        doc_view,
    );
    let copy_compact = copy_item(
        tr!("Copy subtree (compact)"),
        IconName::Braces,
        doc.subtree_json_compact(id).unwrap_or_default(),
        tr!("Compact subtree copied"),
        doc_view,
    );
    let copy_all = copy_item(
        tr!("Copy all JSON"),
        IconName::Copy,
        doc.pretty.clone(),
        tr!("Everything copied"),
        doc_view,
    );

    let paths_menu = {
        let doc_view = doc_view.clone();
        let entries = [
            ("JSONPath", doc.json_path(id), IconName::Hash),
            ("JSON Pointer", doc.json_pointer(id), IconName::GitBranch),
            ("jq", doc.jq_path(id), IconName::Terminal),
        ];
        PopupMenu::build(window, cx, |sub, _, _| {
            let mut menu = sub;
            for (label, path, icon) in entries {
                let doc_view = doc_view.clone();
                let toast = tr!("%{label} path copied", label = label).to_string();
                menu = menu.item(
                    PopupMenuItem::new(format!("{label} — {path}"))
                        .icon(icon)
                        .on_click(move |_, _, cx| {
                            copy_via_doc_view(&doc_view, path.clone(), toast.clone(), cx);
                        }),
                );
            }
            menu
        })
    };

    menu.max_w(px(560.))
        .label(format!("{} · {}", json_path, node.typ.label()))
        .separator()
        .item(open_node)
        .item(parse_string)
        .separator()
        .item(copy_key)
        .item(copy_value)
        .item(copy_string)
        .item(copy_pretty)
        .item(copy_compact)
        .separator()
        .item(PopupMenuItem::submenu(tr!("Copy path…"), paths_menu))
        .separator()
        .item(copy_all)
}
