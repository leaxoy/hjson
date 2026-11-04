use gpui_kit::{AnyWindowHandle, AppContext as _, Context, Entity, Global, WeakEntity, Window};

use crate::views::diff::DiffView;
use crate::views::doc::DocView;

mod actions;
mod documents;
mod history;
mod home;
pub mod menu;
mod render;
mod tabs;

pub(crate) use actions::refresh_doc_view_key_sort;
pub use actions::{
    CloseTab, CollapseAll, DiffNext, DiffPrev, DiffTab, ExpandAll, FindInDoc, NewTab, OpenFile,
    PasteJson, SelectNextTab, SelectPrevTab, ToggleHistory, init,
};
pub use documents::DocEntry;
pub use history::human_size;
pub use home::HomeView;

/// The live app view of every open window.
///
/// gpui-kit wraps our root view in its own `Root`, so a window's root view is
/// never `HJsonApp` and cannot be downcast from the handle. Application-wide
/// action handlers have to reach the view through this table instead.
struct AppWindows(Vec<(AnyWindowHandle, WeakEntity<HJsonApp>)>);

impl Global for AppWindows {}

/// The three kinds of tab content: the import page, an open document, or a
/// diff comparison.
pub enum TabPage {
    Home(Entity<HomeView>),
    Doc(Entity<DocView>),
    Diff(Entity<DiffView>),
}

pub struct HJsonApp {
    tabs: Vec<TabPage>,
    active: usize,
    title_editor: Option<tabs::TabTitleEditor>,
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
            title_editor: None,
            history_open: false,
            docs: Vec::new(),
            running_doc_id: 0,
        }
    }
}
