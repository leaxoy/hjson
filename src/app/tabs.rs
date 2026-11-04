use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, Entity, SharedString, Subscription, Window};

use crate::document::LoadedDocument;
use crate::tr;
use crate::views::doc::DocView;

use super::{HJsonApp, HomeView, TabPage};

pub(super) struct TabTitleEditor {
    pub(super) tab: usize,
    pub(super) input: Entity<InputState>,
    pub(super) _sub: Subscription,
}

impl HJsonApp {
    /// Show or hide the history sidebar (`⌘⇧H`).
    pub fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.history_open = !self.history_open;
        cx.notify();
    }

    /// Open a fresh import page in a new tab next to the active one.
    pub fn new_tab(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_title_edit(cx);
        let app = cx.weak_entity();
        let home = cx.new(|_| HomeView::new(app.clone()));
        self.tabs.insert(self.active + 1, TabPage::Home(home));
        self.active += 1;
        cx.notify();
    }

    /// Open a diff comparison tab; `prefill` seeds one side (from the
    /// document view's compare button), carrying the doc's kind/title/text/url.
    pub(crate) fn new_diff_tab(
        &mut self,
        prefill: Option<(usize, LoadedDocument)>,
        cx: &mut Context<Self>,
    ) {
        self.cancel_title_edit(cx);
        let app = cx.weak_entity();
        // The diff page reads the session history live from the app when its
        // menus open, so there is nothing to snapshot here.
        let diff = cx.new(|_| crate::views::diff::DiffView::new(app.clone(), prefill));
        self.tabs.insert(self.active + 1, TabPage::Diff(diff));
        self.active += 1;
        cx.notify();
    }

    pub fn close_tab_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.cancel_title_edit(cx);
        if self.tabs.len() == 1 {
            // The home page is the floor: closing the last tab swaps in a fresh
            // home, so `tabs` is never empty and the active index always has a
            // page to render.
            if matches!(self.tabs[0], TabPage::Home(_)) {
                return;
            }
            let app = cx.weak_entity();
            let home = cx.new(|_| HomeView::new(app.clone()));
            self.tabs[0] = TabPage::Home(home);
            self.active = 0;
            cx.notify();
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
        self.close_tab_at(self.active, cx);
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

    pub(super) fn reorder_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }

        self.cancel_title_edit(cx);
        let page = self.tabs.remove(from);
        let insert_at = to.min(self.tabs.len());
        let active = if self.active == from {
            insert_at
        } else {
            let mut active = self.active;
            if from < active {
                active -= 1;
            }
            if insert_at <= active {
                active += 1;
            }
            active
        };

        self.tabs.insert(insert_at, page);
        self.active = active.min(self.tabs.len().saturating_sub(1));
        cx.notify();
    }

    pub(super) fn tab_title(&self, ix: usize, cx: &gpui_kit::App) -> Option<SharedString> {
        match self.tabs.get(ix)? {
            TabPage::Home(_) => Some(tr!("Home").into()),
            TabPage::Doc(doc_view) => Some(doc_view.read(cx).title.clone()),
            TabPage::Diff(view) => Some(crate::views::diff::tab_title(view.read(cx))),
        }
    }

    pub(super) fn tab_can_rename(&self, ix: usize) -> bool {
        matches!(self.tabs.get(ix), Some(TabPage::Doc(_) | TabPage::Diff(_)))
    }

    pub(super) fn start_title_edit(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tab_can_rename(ix) {
            return;
        }
        let Some(title) = self.tab_title(ix, cx) else {
            return;
        };
        self.cancel_title_edit(cx);
        self.active = ix;

        let input = cx.new(|cx| InputState::new(window, cx).default_value(title.to_string()));
        let sub = cx.subscribe_in(&input, window, move |this, input, event, _window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.finish_title_edit(ix, input, cx);
            }
        });

        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        self.title_editor = Some(TabTitleEditor {
            tab: ix,
            input,
            _sub: sub,
        });
        cx.notify();
    }

    fn finish_title_edit(&mut self, ix: usize, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let Some(editor) = &self.title_editor else {
            return;
        };
        if editor.tab != ix || &editor.input != input {
            return;
        }
        let title = input.read(cx).value().trim().to_string();
        self.title_editor = None;
        if !title.is_empty() {
            self.rename_tab(ix, title.into(), cx);
        } else {
            cx.notify();
        }
    }

    pub(super) fn cancel_title_edit(&mut self, cx: &mut Context<Self>) {
        if self.title_editor.take().is_some() {
            cx.notify();
        }
    }

    fn rename_tab(&mut self, ix: usize, title: SharedString, cx: &mut Context<Self>) {
        match self.tabs.get(ix) {
            Some(TabPage::Doc(doc_view)) => {
                doc_view.update(cx, |doc_view, cx| {
                    doc_view.title = title;
                    cx.notify();
                });
            }
            Some(TabPage::Diff(view)) => {
                view.update(cx, |view, cx| view.set_tab_title(title, cx));
            }
            _ => {}
        }
        cx.notify();
    }

    /// Run `f` against the document in the active tab; the home and diff tabs
    /// have no document to act on.
    ///
    /// Our own update already holds the window, so gpui cannot re-enter it to
    /// hand the document view one: the work is scheduled for the end of this effect
    /// cycle, when the window is available again.
    fn with_doc_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut DocView, &mut Window, &mut Context<DocView>) + 'static,
    ) {
        let Some(TabPage::Doc(doc_view)) = self.tabs.get(self.active) else {
            return;
        };
        let doc_view = doc_view.clone();
        doc_view.update(cx, |_, cx| cx.defer_in(window, f));
    }

    /// `cmd-f`: put the cursor in the tree's filter.
    pub fn focus_filter(&self, window: &Window, cx: &mut Context<Self>) {
        self.with_doc_view(window, cx, |doc_view, window, cx| {
            doc_view.focus_filter(window, cx)
        });
    }

    /// `cmd-shift-e` / `cmd-shift-c`: rebuild the tree at every depth, or only
    /// the root's children.
    pub fn set_tree_depth(&self, expand_all: bool, window: &Window, cx: &mut Context<Self>) {
        self.with_doc_view(window, cx, move |doc_view, _, cx| match expand_all {
            true => doc_view.expand_all(cx),
            false => doc_view.collapse_all(cx),
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
}
