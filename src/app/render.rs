use gpui_kit::component::{
    ActiveTheme, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Escape, Input},
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::components::chrome;
use crate::tr;

use super::{CloseTab, DiffTab, HJsonApp, NewTab, TabPage, ToggleHistory};

#[derive(Clone, Copy)]
struct TabDrag {
    from: usize,
}

struct TabDragPreview;

impl Render for TabDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(96.))
            .h(px(24.))
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
    }
}

impl HJsonApp {
    /// The tab strip: gpui-component's `TabBar` in its folder variant, so the
    /// bar surface, the selected tab and its curves into the content all come
    /// from the same theme tokens as the rest of the chrome. The new-tab
    /// buttons dock at the right end.
    fn render_tab_bar(&mut self, window: &Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity();
        let primary = cx.theme().primary;

        let mut tabs = Vec::with_capacity(self.tabs.len());
        for (ix, page) in self.tabs.iter().enumerate() {
            let (icon, label) = match page {
                TabPage::Home(_) => (gpui_kit::assets::IconName::House, tr!("Home")),
                TabPage::Doc(doc_view) => {
                    let doc_view = doc_view.read(cx);
                    (doc_view.kind.icon(), doc_view.title.clone())
                }
                TabPage::Diff(view) => {
                    let view = view.read(cx);
                    (
                        gpui_kit::assets::IconName::GitCompare,
                        crate::views::diff::tab_title(view),
                    )
                }
            };
            let editing_title = self
                .title_editor
                .as_ref()
                .is_some_and(|editor| editor.tab == ix);
            let title_child = if editing_title {
                let input = self
                    .title_editor
                    .as_ref()
                    .map(|editor| editor.input.clone())
                    .expect("checked above");
                let cancel = app.clone();
                div()
                    .w(px(150.))
                    .on_action(move |_: &Escape, _, cx| {
                        cx.stop_propagation();
                        cancel.update(cx, |app, cx| app.cancel_title_edit(cx));
                    })
                    .child(Input::new(&input).small().w_full().focus_bordered(false))
                    .into_any_element()
            } else {
                div().min_w_0().truncate().child(label).into_any_element()
            };
            let edit = app.clone();
            let reorder = app.clone();
            let tab = Tab::new()
                .child(
                    h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_1()
                        .when(!editing_title && self.tab_can_rename(ix), |this| {
                            this.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                if event.click_count >= 2 {
                                    cx.stop_propagation();
                                    edit.update(cx, |app, cx| app.start_title_edit(ix, window, cx));
                                }
                            })
                        })
                        .child(Icon::new(icon).small())
                        .child(title_child),
                )
                .when(!editing_title && self.tabs.len() > 1, |tab| {
                    tab.on_drag(TabDrag { from: ix }, |_, _, _, cx| {
                        cx.new(|_| TabDragPreview)
                    })
                    .on_drop(move |dragged: &TabDrag, _, cx| {
                        reorder.update(cx, |app, cx| app.reorder_tab(dragged.from, ix, cx));
                    })
                });
            // A lone home page has nowhere to go back to, so only it loses the
            // close button; a lone doc or diff tab still closes onto home.
            let can_close = self.tabs.len() > 1 || !matches!(page, TabPage::Home(_));
            tabs.push(if can_close {
                tab.suffix(
                    Button::new(format!("close-tab-{ix}"))
                        .icon(gpui_kit::assets::IconName::X)
                        .ghost()
                        .with_size(gpui_kit::component::Size::XSmall)
                        .mr_2()
                        .tooltip(format!(
                            "{}{}",
                            tr!("Close tab"),
                            chrome::shortcut_suffix(&CloseTab, window)
                        ))
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
                        // `selected` paints the on-state; `toggled` only carries
                        // the pressed metadata, so the button needs both.
                        .selected(self.history_open)
                        .toggled(self.history_open)
                        .when(self.history_open, |b| b.text_color(primary))
                        .tooltip(format!(
                            "{}{}",
                            tr!("History"),
                            chrome::shortcut_suffix(&ToggleHistory, window)
                        ))
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
                            .tooltip(format!(
                                "{}{}",
                                tr!("New comparison"),
                                chrome::shortcut_suffix(&DiffTab, window)
                            ))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.new_diff_tab(None, cx);
                            })),
                    )
                    .child(
                        Button::new("new-tab-button")
                            .icon(gpui_kit::assets::IconName::Plus)
                            .ghost()
                            .small()
                            .tooltip(format!(
                                "{}{}",
                                tr!("New tab"),
                                chrome::shortcut_suffix(&NewTab, window)
                            ))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_tab(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for HJsonApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The interface font size is the window's `rem` base, so `text_xs()` /
        // `text_sm()` / … across every widget scales with one knob.
        let ui_rem = crate::views::settings::settings(cx).ui_rem();
        window.set_rem_size(px(ui_rem));

        let tab_bar = self.render_tab_bar(window, cx);
        let content = match self.tabs.get(self.active) {
            Some(TabPage::Home(home)) => home.clone().into_any_element(),
            Some(TabPage::Doc(doc_view)) => doc_view.clone().into_any_element(),
            Some(TabPage::Diff(view)) => view.clone().into_any_element(),
            None => div().into_any_element(),
        };
        let history = self.history_open.then(|| self.render_history_sidebar(cx));
        let theme = cx.theme();

        // Sidebar first, so it runs the full window height and the tab bar
        // sits to its right (Finder-style), instead of the tab bar spanning
        // across the top of an open sidebar.
        h_flex()
            .size_full()
            // `items_stretch`, not `h_flex`'s default centering: the sidebar
            // and the tab column fill the window height instead of floating
            // at their intrinsic height.
            .items_stretch()
            .overflow_hidden()
            .bg(theme.background)
            .text_color(theme.foreground)
            .children(history)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(tab_bar)
                    .child(div().flex_1().min_h_0().overflow_hidden().child(content)),
            )
    }
}
