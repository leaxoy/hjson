use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    tab::{Tab, TabBar},
    v_flex,
};
use crate::tr;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyWindowHandle, AppContext as _, Context, Entity, FontWeight, Global,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, px,
};

use std::rc::Rc;
use std::time::Instant;

use crate::source_menu::{
    LoadMethod, LoadTarget, SourceKind, file_title, open_manual_dialog, open_url_dialog, url_title,
};
use crate::viewer::Viewer;
use crate::{json_model::JsonTree, sample};

gpui_kit::actions!(
    hjson,
    [
        PasteJson,
        NewTab,
        CloseTab,
        SelectNextTab,
        SelectPrevTab,
        DiffTab,
        OpenFile,
        ToggleHistory,
        FindInDoc,
        ExpandAll,
        CollapseAll,
        DiffPrev,
        DiffNext
    ]
);

/// The live app view of every open window.
///
/// gpui-kit wraps our root view in its own `Root`, so a window's root view is
/// never `HJsonApp` and cannot be downcast from the handle. Application-wide
/// action handlers have to reach the view through this table instead.
struct AppWindows(Vec<(AnyWindowHandle, WeakEntity<HJsonApp>)>);

impl Global for AppWindows {}

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
/// `cmd-w` closes the active one, `cmd-o` opens a file, `cmd-shift-v` imports
/// the clipboard content, mirroring OK JSON's paste flow, `cmd-shift-h`
/// toggles the history sidebar, `cmd-f` filters the tree, `cmd-shift-e` /
/// `cmd-shift-c` expand and collapse it, and `cmd-[` / `cmd-]` step through a
/// comparison.
pub fn init(cx: &mut gpui_kit::App) {
    // The viewer's own per-pane keys (arrows, tab) live with the panes.
    crate::viewer::init(cx);
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
        KeyBinding::new("cmd-f", FindInDoc, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-f", FindInDoc, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-e", ExpandAll, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-e", ExpandAll, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-c", CollapseAll, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-c", CollapseAll, None),
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
    window_action::<NewTab>(cx, |app, window, cx| app.new_tab(window, cx));
    window_action::<DiffTab>(cx, |app, _window, cx| app.new_diff_tab(None, cx));
    window_action::<CloseTab>(cx, |app, _window, cx| app.close_active_tab(cx));
    window_action::<SelectNextTab>(cx, |app, _window, cx| app.select_next_tab(cx));
    window_action::<SelectPrevTab>(cx, |app, _window, cx| app.select_prev_tab(cx));
    window_action::<ToggleHistory>(cx, |app, _window, cx| app.toggle_history(cx));
    window_action::<FindInDoc>(cx, |app, window, cx| app.focus_filter(window, cx));
    window_action::<ExpandAll>(cx, |app, window, cx| app.set_tree_depth(true, window, cx));
    window_action::<CollapseAll>(cx, |app, window, cx| app.set_tree_depth(false, window, cx));
    window_action::<DiffPrev>(cx, |app, _window, cx| app.step_diff(-1, cx));
    window_action::<DiffNext>(cx, |app, _window, cx| app.step_diff(1, cx));
}

#[derive(Clone)]
pub struct DocEntry {
    pub id: u64,
    pub title: SharedString,
    pub kind: SourceKind,
    /// Original URL (only when the source was fetched over HTTP), kept so a
    /// document can be re-fetched from its toolbar.
    pub url: Option<String>,
    pub text: String,
    /// Number of bytes of the raw JSON text.
    pub size: usize,
    /// One-line preview of the raw JSON for the history list.
    pub preview: String,
    /// When the document was imported this session (for relative timestamps).
    pub opened_at: Instant,
}

/// A status message below the import cards. `ok` selects the color.
#[derive(Clone)]
pub struct Status {
    pub text: String,
    pub ok: bool,
}

/// The two kinds of tab content: the import page, an open document, or a
/// diff comparison.
pub enum TabPage {
    Home(Entity<HomeView>),
    Doc(Entity<Viewer>),
    Diff(Entity<crate::diff_view::DiffView>),
}

pub struct HJsonApp {
    tabs: Vec<TabPage>,
    active: usize,
    /// The history sidebar, docked left of every tab. Off until `⌘⇧H` asks.
    history_open: bool,
    /// Session import history (shared with diff tabs).
    pub(crate) docs: Vec<DocEntry>,
    running_doc_id: u64,
}

impl HJsonApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = cx.weak_entity();
        if !cx.has_global::<AppWindows>() {
            cx.set_global(AppWindows(Vec::new()));
        }
        let table = &mut cx.global_mut::<AppWindows>().0;
        table.retain(|(_, live)| live.upgrade().is_some());
        table.push((window.window_handle(), app.clone()));
        let mut tabs = Vec::new();
        tabs.push(TabPage::Home(cx.new(|_| HomeView::new(app.clone()))));
        Self {
            tabs,
            active: 0,
            history_open: false,
            docs: Vec::new(),
            running_doc_id: 0,
        }
    }

    /// Show or hide the history sidebar (`⌘⇧H`).
    pub fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.history_open = !self.history_open;
        cx.notify();
    }

    // ---------- tab management ----------

    /// Open a fresh import page in a new tab next to the active one.
    pub fn new_tab(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.weak_entity();
        let home = cx.new(|_| HomeView::new(app.clone()));
        self.tabs.insert(self.active + 1, TabPage::Home(home));
        self.active += 1;
        cx.notify();
    }

    /// Open a diff comparison tab; `prefill` seeds one side (from the
    /// viewer's compare button), carrying the doc's kind/title/text/url.
    pub fn new_diff_tab(
        &mut self,
        prefill: Option<(usize, SourceKind, SharedString, String, Option<String>)>,
        cx: &mut Context<Self>,
    ) {
        let app = cx.weak_entity();
        // The diff page reads the session history live from the app when its
        // menus open, so there is nothing to snapshot here.
        let diff = cx.new(|_| crate::diff_view::DiffView::new(app.clone(), prefill));
        self.tabs.insert(self.active + 1, TabPage::Diff(diff));
        self.active += 1;
        cx.notify();
    }

    pub fn close_tab_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs.remove(ix);
        if ix > self.active {
            // Removing a tab to the right keeps the active item unchanged.
        } else if ix < self.active {
            self.active -= 1;
        } else if self.active >= self.tabs.len() {
            // Closed the last tab: fall back to its left neighbour.
            self.active = self.tabs.len().saturating_sub(1);
        } else if self.active > 0 {
            // Prefer the left neighbour, browser-style.
            self.active -= 1;
        }
        cx.notify();
    }

    pub fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            self.close_tab_at(self.active, cx);
        }
    }

    pub fn set_active_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active = ix;
            cx.notify();
        }
    }

    pub fn select_next_tab(&mut self, cx: &mut Context<Self>) {
        if self.active + 1 < self.tabs.len() {
            self.active += 1;
            cx.notify();
        }
    }

    pub fn select_prev_tab(&mut self, cx: &mut Context<Self>) {
        if self.active > 0 {
            self.active -= 1;
            cx.notify();
        }
    }

    // ---------- keyboard actions for the open tab ----------

    /// Run `f` against the document in the active tab; the home and diff tabs
    /// have no document to act on.
    ///
    /// Our own update already holds the window, so gpui cannot re-enter it to
    /// hand the viewer one: the work is scheduled for the end of this effect
    /// cycle, when the window is available again.
    fn with_viewer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Viewer, &mut Window, &mut Context<Viewer>) + 'static,
    ) {
        let Some(TabPage::Doc(viewer)) = self.tabs.get(self.active) else {
            return;
        };
        let viewer = viewer.clone();
        viewer.update(cx, |_, cx| cx.defer_in(window, f));
    }

    /// `cmd-f`: put the cursor in the tree's filter.
    pub fn focus_filter(&self, window: &Window, cx: &mut Context<Self>) {
        self.with_viewer(window, cx, |viewer, window, cx| {
            viewer.focus_filter(window, cx)
        });
    }

    /// `cmd-shift-e` / `cmd-shift-c`: rebuild the tree at every depth, or only
    /// the root's children.
    pub fn set_tree_depth(&self, expand_all: bool, window: &Window, cx: &mut Context<Self>) {
        self.with_viewer(window, cx, move |viewer, _, cx| match expand_all {
            true => viewer.expand_all(cx),
            false => viewer.collapse_all(cx),
        });
    }

    /// `cmd-[` / `cmd-]`: previous / next difference of the open comparison.
    pub fn step_diff(&self, delta: i64, cx: &mut Context<Self>) {
        let Some(TabPage::Diff(view)) = self.tabs.get(self.active) else {
            return;
        };
        let view = view.clone();
        view.update(cx, |view, cx| view.jump(delta, cx));
    }

    // ---------- documents ----------

    /// Validate a text and open it in the active tab (converting a home tab)
    /// or in a brand-new tab when a document is already on screen. Returns
    /// Ok(doc id) on success.
    pub fn open_text(
        &mut self,
        kind: SourceKind,
        title: impl Into<SharedString>,
        text: &str,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(tr!("内容为空").to_string());
        }
        // Validation happens before entering the viewer, so the viewer itself
        // can assume the document parses.
        let parsed = JsonTree::parse(text);
        if let Some(err) = parsed.error {
            return Err(err);
        }

        self.running_doc_id += 1;
        let id = self.running_doc_id;
        // A single-line preview keeps the history list informative at a glance.
        let preview: String = text
            .replace(['\n', '\r', '\t'], " ")
            .chars()
            .take(96)
            .collect();
        let viewer_url = url.clone();
        let entry = DocEntry {
            id,
            title: title.into(),
            kind,
            url,
            text: text.to_string(),
            size: text.len(),
            preview,
            opened_at: Instant::now(),
        };
        let title = entry.title.clone();
        self.docs.insert(0, entry);
        if self.docs.len() > 50 {
            self.docs.pop();
        }

        let weak = cx.weak_entity();
        let viewer = cx.new(|cx| Viewer::new(weak, &title, kind, viewer_url, text, window, cx));

        if matches!(self.tabs.get(self.active), Some(TabPage::Home(_))) {
            // The import page hands itself over to the parsed document.
            self.tabs[self.active] = TabPage::Doc(viewer);
        } else {
            // A document is being viewed: open the new one in a separate tab.
            self.tabs.insert(self.active + 1, TabPage::Doc(viewer));
            self.active += 1;
        }
        cx.notify();
        Ok(id)
    }

    /// Replace the document shown in `viewer` with freshly loaded content —
    /// keeps the tab slot (and position) while registering a new history
    /// entry. Used by the doc toolbar's re-import buttons.
    pub fn replace_doc_of_viewer(
        &mut self,
        viewer: &Entity<Viewer>,
        kind: SourceKind,
        title: impl Into<SharedString>,
        text: &str,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(tr!("内容为空").to_string());
        }
        let parsed = JsonTree::parse(text);
        if let Some(err) = parsed.error {
            return Err(err);
        }
        let Some(ix) = self
            .tabs
            .iter()
            .position(|page| matches!(page, TabPage::Doc(v) if v == viewer))
        else {
            return Err(tr!("标签页已关闭").to_string());
        };

        self.running_doc_id += 1;
        let id = self.running_doc_id;
        let preview: String = text
            .replace(['\n', '\r', '\t'], " ")
            .chars()
            .take(96)
            .collect();
        let new_url = url.clone();
        let entry = DocEntry {
            id,
            title: title.into(),
            kind,
            url,
            text: text.to_string(),
            size: text.len(),
            preview,
            opened_at: Instant::now(),
        };
        let title = entry.title.clone();
        self.docs.insert(0, entry);
        if self.docs.len() > 50 {
            self.docs.pop();
        }

        let weak = cx.weak_entity();
        let new_viewer = cx.new(|cx| Viewer::new(weak, &title, kind, new_url, text, window, cx));
        self.tabs[ix] = TabPage::Doc(new_viewer);
        cx.notify();
        Ok(id)
    }

    pub fn open_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        match text {
            Some(text) if !text.trim().is_empty() => {
                let kind = SourceKind::Clipboard;
                let _ = self.open_text(kind, kind.default_title(), &text, None, window, cx);
            }
            Some(_) => self.note_home(false, tr!("剪贴板内容为空"), cx),
            None => self.note_home(false, tr!("剪贴板里没有文本"), cx),
        }
        cx.notify();
    }

    /// Show a line on the active home tab, where the import errors belong.
    fn note_home(&self, ok: bool, text: impl Into<String>, cx: &mut Context<Self>) {
        let Some(TabPage::Home(home)) = self.tabs.get(self.active) else {
            return;
        };
        let home = home.clone();
        home.update(cx, |home, cx| {
            home.set_status(ok, text);
            cx.notify();
        });
    }

    /// Open a file picked from disk.
    pub fn open_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else { return };
            let read = cx.background_spawn(async move {
                std::fs::read_to_string(&path)
                    .map(|t| (path.clone(), t))
                    .map_err(|e| anyhow::anyhow!("{e}"))
            });
            let result = read.await;
            this.update_in(cx, |app, window, cx| {
                match result {
                    Ok((path, text)) => {
                        let title = file_title(&path);
                        if let Err(err) =
                            app.open_text(SourceKind::File, title, &text, None, window, cx)
                        {
                            app.note_home(false, format!("解析失败：{err}"), cx);
                        }
                    }
                    Err(e) => app.note_home(false, format!("读取文件失败：{e}"), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_doc(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.docs.iter().find(|d| d.id == id) else {
            return;
        };
        let kind = entry.kind;
        let title = entry.title.clone();
        let text = entry.text.clone();
        let url = entry.url.clone();
        let _ = self.open_text(kind, title, &text, url, window, cx);
    }

    /// Drop every document from the session history.
    pub fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.docs.clear();
        cx.notify();
    }

    /// The tab strip: gpui-component's `TabBar` in its folder variant, so the
    /// bar surface, the selected tab and its curves into the content all come
    /// from the same theme tokens as the rest of the chrome. The new-tab
    /// buttons dock at the right end.
    fn render_tab_bar(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity();
        let can_close = self.tabs.len() > 1;

        let mut tabs = Vec::with_capacity(self.tabs.len());
        for (ix, page) in self.tabs.iter().enumerate() {
            let (icon, label) = match page {
                TabPage::Home(_) => (
                    gpui_kit::assets::IconName::House,
                    tr!("首页"),
                ),
                TabPage::Doc(viewer) => {
                    let viewer = viewer.read(cx);
                    (viewer.kind.icon(), viewer.title.clone())
                }
                TabPage::Diff(view) => {
                    let view = view.read(cx);
                    (
                        gpui_kit::assets::IconName::GitCompare,
                        crate::diff_view::tab_title(view),
                    )
                }
            };
            let tab = Tab::new().child(
                h_flex()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .child(Icon::new(icon).small())
                    .child(div().min_w_0().truncate().child(label)),
            );
            tabs.push(if can_close {
                tab.suffix(
                    Button::new(format!("close-tab-{ix}"))
                        .icon(gpui_kit::assets::IconName::X)
                        .ghost()
                        .with_size(gpui_kit::component::Size::XSmall)
                        .mr_2()
                        .tooltip(tr!("关闭标签页 (⌘W)"))
                        .on_click({
                            let app = app.clone();
                            move |_, _, cx| {
                                // Don't let the click also switch tabs.
                                cx.stop_propagation();
                                app.update(cx, |app, cx| app.close_tab_at(ix, cx));
                            }
                        }),
                )
            } else {
                tab
            });
        }

        TabBar::new("tab-bar")
            .flex_shrink_0()
            .folder()
            .max_width(px(240.))
            .selected_index(self.active)
            .children(tabs)
            .prefix(
                h_flex().items_center().pl_2().child(
                    Button::new("toggle-history-button")
                        .icon(gpui_kit::assets::IconName::PanelLeft)
                        .ghost()
                        .small()
                        .toggled(self.history_open)
                        .tooltip(tr!("历史记录 (⌘⇧H)"))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_history(cx))),
                ),
            )
            .on_click({
                let app = app.clone();
                move |ix, _, cx| {
                    let ix = *ix;
                    app.update(cx, |app, cx| app.set_active_tab(ix, cx));
                }
            })
            .suffix(
                h_flex()
                    .items_center()
                    .pr_2()
                    .child(
                        Button::new("new-diff-tab-button")
                            .icon(gpui_kit::assets::IconName::GitCompare)
                            .ghost()
                            .small()
                            .tooltip(tr!("新建对比 (⌘D)"))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.new_diff_tab(None, cx);
                            })),
                    )
                    .child(
                        Button::new("new-tab-button")
                            .icon(gpui_kit::assets::IconName::Plus)
                            .ghost()
                            .small()
                            .tooltip(tr!("新建标签页 (⌘T)"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_tab(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
    /// The history sidebar: this session's imports, newest first, docked to the
    /// left of whichever tab is open. `⌘⇧H` (or the tab bar's panel button)
    /// shows and hides it.
    fn render_history_sidebar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let rows: Vec<gpui_kit::AnyElement> = self
            .docs
            .iter()
            .rev()
            .map(|doc| {
                let color = doc.kind.color(cx);
                let id = doc.id;
                v_flex()
                    .id(doc.id)
                    .w_full()
                    .px_2()
                    .py(px(6.))
                    .gap_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|this| this.bg(theme.list_hover))
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(Icon::new(doc.kind.icon()).small().text_color(color))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(doc.title.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .pl(px(22.))
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(doc.kind.label())
                            .child(human_size(doc.size, cx))
                            .child(h_flex().flex_1())
                            .child(rel_time(doc.opened_at)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_doc(id, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .w(px(280.))
            .flex_shrink_0()
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .text_color(theme.sidebar_foreground)
            // The header stays put while the list scrolls under it.
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .px_3()
                    .py(px(9.))
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(tr!("历史记录")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("{}", self.docs.len())),
                    )
                    .child(h_flex().flex_1())
                    .when(!self.docs.is_empty(), |this| {
                        this.child(
                            Button::new("clear-history")
                                .label(tr!("清空"))
                                .small()
                                .tooltip(tr!("清除本次会话的全部历史"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_history(cx);
                                })),
                        )
                    }),
            )
            .when(rows.is_empty(), |this| {
                this.child(
                    div()
                        .px_3()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(tr!("本次会话还没有导入记录")),
                )
            })
            .when(!rows.is_empty(), |this| {
                this.child(
                    v_flex()
                        .id("history-list")
                        .flex_1()
                        .min_h_0()
                        .px_2()
                        .pb_2()
                        .gap_0p5()
                        .overflow_y_scroll()
                        .children(rows),
                )
            })
            .into_any_element()
    }
}

impl Render for HJsonApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The interface font size is the window's `rem` base, so `text_xs()` /
        // `text_sm()` / … across every widget scales with one knob.
        let ui_font_size = crate::settings::settings(cx).ui_font_size;
        window.set_rem_size(px(ui_font_size));

        let tab_bar = self.render_tab_bar(cx);
        let content = match self.tabs.get(self.active) {
            Some(TabPage::Home(home)) => home.clone().into_any_element(),
            Some(TabPage::Doc(viewer)) => viewer.clone().into_any_element(),
            Some(TabPage::Diff(view)) => view.clone().into_any_element(),
            None => div().into_any_element(),
        };
        let history = self.history_open.then(|| self.render_history_sidebar(cx));
        let theme = cx.theme();

        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(tab_bar)
            .child(
                // `items_stretch`, not `h_flex`'s default centering: the tab
                // content fills its pane, and a cross-axis center would shrink
                // the pane to its intrinsic height and float it mid-window.
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .overflow_hidden()
                    .children(history)
                    .child(div().flex_1().min_w_0().overflow_hidden().child(content)),
            )
    }
}

// MARK: home page (per-tab import view)

/// One home tab: the OK-JSON-style import page. The documents it opens and the
/// session history both belong to the app, so any tab can hand a document to
/// the sidebar.
pub struct HomeView {
    app: WeakEntity<HJsonApp>,
    status: Option<Status>,
}

impl HomeView {
    fn new(app: WeakEntity<HJsonApp>) -> Self {
        Self { app, status: None }
    }

    fn set_status(&mut self, ok: bool, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            ok,
        });
    }

    /// Hand a text over to the app: the active home tab turns into the
    /// document. Errors (empty content / parse failure) land on the status
    /// line and keep the import page intact.
    fn submit(
        &mut self,
        kind: SourceKind,
        title: impl Into<SharedString>,
        text: &str,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let title = title.into();
        // Weak update may fail if the app view is gone; both layers surface
        // as an error string for the status line.
        let outcome = match self.app.update(cx, move |app, cx| {
            app.open_text(kind, title, text, url, window, cx)
        }) {
            Ok(inner) => inner,
            Err(err) => Err(err.to_string()),
        };
        match outcome {
            Ok(id) => {
                self.status = None;
                Ok(id)
            }
            Err(err) => {
                self.set_status(false, format!("解析失败：{err}"));
                Err(err)
            }
        }
    }

    pub fn open_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        match text {
            Some(text) if !text.trim().is_empty() => {
                let kind = SourceKind::Clipboard;
                let _ = self.submit(kind, kind.default_title(), &text, None, window, cx);
            }
            Some(_) => self.set_status(false, tr!("剪贴板内容为空")),
            None => self.set_status(false, tr!("剪贴板里没有文本")),
        }
        cx.notify();
    }

    pub fn open_manual(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let kind = SourceKind::Manual;
        let _ = self.submit(kind, kind.default_title(), text, None, window, cx);
        cx.notify();
    }

    /// Ask for a URL in a modal, then fetch it. The dialog lives in
    /// [`crate::source_menu`] so it is the same one the diff and viewer toolbars
    /// open.
    fn open_url_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = cx.entity().downgrade();
        open_url_dialog(&target, (), None, window, cx);
    }

    /// Ask for raw JSON in a modal, then parse it.
    fn open_manual_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = cx.entity().downgrade();
        open_manual_dialog(&target, (), window, cx);
    }

    pub fn open_sample(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = SourceKind::Sample;
        let _ = self.submit(kind, kind.default_title(), sample::TEXT, None, window, cx);
        cx.notify();
    }

    pub fn open_doc(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.app.update(cx, |app, cx| app.open_doc(id, window, cx));
    }

    pub fn fetch_url(&mut self, url: String, _window: &mut Window, cx: &mut Context<Self>) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.set_status(false, tr!("请输入 URL").to_string());
            cx.notify();
            return;
        }
        self.status = Some(Status {
            text: format!("正在请求 {url} …"),
            ok: true,
        });
        cx.notify();
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
                    match this.submit(SourceKind::Url, title, &text, Some(url.clone()), window, cx)
                    {
                        Ok(_) => this.set_status(true, format!("已加载 {url}")),
                        Err(err) => {
                            this.set_status(false, format!("解析失败：{err}"));
                        }
                    }
                    cx.notify();
                }
                Err(e) => {
                    this.set_status(false, format!("请求失败：{e}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn pick_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let outcome = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(e)) => {
                    this.update(cx, |this, cx| {
                        this.set_status(false, format!("打开文件失败：{e}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Err(_) => None,
            };
            let Some(path) = outcome else { return };
            let read = cx.background_spawn(async move {
                std::fs::read_to_string(&path)
                    .map(|t| (path.clone(), t))
                    .map_err(|e| anyhow::anyhow!("{e}"))
            });
            let read_result = read.await;
            this.update_in(cx, |this, window, cx| {
                match read_result {
                    Ok((path, text)) => {
                        let title = file_title(&path);
                        match this.submit(SourceKind::File, title, &text, None, window, cx) {
                            Ok(_) => this.set_status(true, tr!("已打开文件")),
                            Err(err) => this.set_status(false, format!("解析失败：{err}")),
                        }
                    }
                    Err(e) => this.set_status(false, format!("读取文件失败：{e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl LoadTarget for HomeView {
    type Slot = ();

    /// The home page has one target — itself — so the shared load menu and the
    /// import list both land here.
    fn apply_load(
        &mut self,
        _slot: (),
        method: LoadMethod,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match method {
            LoadMethod::Clipboard => self.open_clipboard(window, cx),
            LoadMethod::PickFile => self.pick_file(window, cx),
            LoadMethod::FetchUrl(url) => self.fetch_url(url, window, cx),
            LoadMethod::Manual(text) => self.open_manual(&text, window, cx),
            LoadMethod::Sample => self.open_sample(window, cx),
            LoadMethod::History(id) => self.open_doc(id, window, cx),
            // Nothing to re-fetch: a home tab has no document of its own.
            LoadMethod::RefetchUrl => {}
        }
    }
}

impl Render for HomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        self.render_home(cx)
    }
}

impl HomeView {
    /// OK-JSON-style home: a compact brand bar and the centered list of import
    /// actions. History is a sidebar of its own now (`⌘⇧H`), so this page stays
    /// about one thing — getting a document in.
    fn render_home(&mut self, cx: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        let theme = cx.theme();

        // ---- brand bar ----
        let header = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                Icon::new(gpui_kit::assets::IconName::Braces)
                    .with_size(gpui_kit::component::Size::Medium)
                    .text_color(theme.primary),
            )
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("HJson"),
            )
            .child(h_flex().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr!("所有数据仅在本机处理，不会上传")),
            );

        // ---- import actions ----
        // The five ways in, as one centered vertical list of button-links. Every
        // row hands off to the same methods the toolbar's load menu uses, so the
        // two entry points stay in step.
        let import_list = v_flex()
            .w_full()
            .max_w(px(440.))
            .self_center()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .child(action_row(
                "paste-clipboard",
                SourceKind::Clipboard,
                tr!("粘贴 JSON"),
                "⌘⇧V",
                cx.listener(|this, _, window, cx| this.open_clipboard(window, cx)),
                cx,
            ))
            .child(row_divider(cx))
            .child(action_row(
                "open-url",
                SourceKind::Url,
                tr!("从 URL 获取"),
                tr!("弹窗输入地址"),
                cx.listener(|this, _, window, cx| this.open_url_dialog(window, cx)),
                cx,
            ))
            .child(row_divider(cx))
            .child(action_row(
                "open-file",
                SourceKind::File,
                tr!("打开文件"),
                "⌘O",
                cx.listener(|this, _, window, cx| this.pick_file(window, cx)),
                cx,
            ))
            .child(row_divider(cx))
            .child(action_row(
                "manual-input",
                SourceKind::Manual,
                tr!("手动输入"),
                tr!("弹窗粘贴 JSON"),
                cx.listener(|this, _, window, cx| this.open_manual_dialog(window, cx)),
                cx,
            ))
            .child(row_divider(cx))
            .child(action_row(
                "open-sample",
                SourceKind::Sample,
                tr!("示例文档"),
                tr!("内置数据"),
                cx.listener(|this, _, window, cx| this.open_sample(window, cx)),
                cx,
            ));

        let status_line = self.status.clone().map(|s| {
            h_flex()
                .gap_2()
                .min_w_0()
                .px_1()
                .text_sm()
                .text_color(if s.ok { theme.success } else { theme.danger })
                .child(
                    Icon::new(if s.ok {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleAlert
                    })
                    .small()
                    .flex_shrink_0(),
                )
                .child(div().min_w_0().truncate().child(s.text))
        });

        // ---- page ----
        // One centered column, so the cards stay aligned with the brand bar
        // instead of hugging the left edge on a wide window.
        let column = v_flex()
            .w_full()
            .max_w(px(880.))
            .gap_4()
            .child(header)
            .child(import_list)
            .children(status_line);

        // Equal spacers above and below center the column in the viewport. They
        // cannot shrink, so once the content is taller than the window they
        // collapse to nothing and the page just scrolls from the top.
        let spacer = || div().w_full().flex_1().flex_shrink_0();

        // `v_flex`, not `div`: a plain div is block layout, where `items_center`
        // is ignored and the capped column would hug the left edge at any width.
        v_flex()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .items_center()
            .px(px(24.))
            .pt(px(18.))
            .pb(px(24.))
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_base()
            .font_family(theme.font_family.clone())
            .child(spacer())
            .child(column)
            .child(spacer())
    }
}

/// Relative time for the history rows (session-local documents only).
fn rel_time(opened_at: Instant) -> String {
    let s = opened_at.elapsed().as_secs();
    match s {
        0..=5 => tr!("刚刚").to_string(),
        6..=59 => format!("{s} 秒前"),
        _ if s < 3600 => format!("{} 分钟前", s / 60),
        _ => format!("{} 小时前", s / 3600),
    }
}

/// One row of the home page's import list: the source's own glyph in its accent
/// color, its label, and a muted hint of what the click does next.
fn action_row(
    id: &'static str,
    kind: SourceKind,
    label: impl Into<gpui_kit::SharedString>,
    hint: impl Into<gpui_kit::SharedString>,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    cx: &Context<HomeView>,
) -> gpui_kit::AnyElement {
    let label = label.into();
    let hint = hint.into();
    let theme = cx.theme();
    div()
        .id(id)
        .w_full()
        .px_4()
        .py(px(11.))
        .cursor_pointer()
        .hover(|this| this.bg(theme.list_hover))
        .on_click(on_click)
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(Icon::new(kind.icon()).small().text_color(kind.color(cx)))
                .child(div().min_w_0().truncate().child(label))
                .child(h_flex().flex_1())
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(hint),
                ),
        )
        .into_any_element()
}

/// Hairline between two import rows, so the list reads as one card.
fn row_divider(cx: &Context<HomeView>) -> gpui_kit::AnyElement {
    div()
        .w_full()
        .h_px()
        .bg(cx.theme().border)
        .into_any_element()
}

pub fn human_size(size: usize, cx: &gpui_kit::App) -> gpui_kit::AnyElement {
    let text = if size >= 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024. * 1024.))
    } else if size >= 1024 {
        format!("{:.1} KB", size as f64 / 1024.)
    } else {
        format!("{size} B")
    };
    div()
        .text_xs()
        .flex_shrink_0()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
