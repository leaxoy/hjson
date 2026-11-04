use std::time::Instant;

use gpui_kit::component::{ActiveTheme, Icon, Sizable as _, button::Button, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use crate::components::chrome;
use crate::tr;

use super::HJsonApp;

impl HJsonApp {
    /// Drop every document from the session history.
    pub fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.docs.clear();
        cx.notify();
    }

    /// The history sidebar: this session's imports, newest first, docked to the
    /// left of whichever tab is open. `⌘⇧H` (or the tab bar's panel button)
    /// shows and hides it.
    pub(super) fn render_history_sidebar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
                            .child(tr!("History")),
                    )
                    .child(chrome::stat(format!("{}", self.docs.len()), cx))
                    .child(h_flex().flex_1())
                    .when(!self.docs.is_empty(), |this| {
                        this.child(
                            Button::new("clear-history")
                                .label(tr!("Clear"))
                                .small()
                                .tooltip(tr!("Clear this session's history"))
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
                        .child(tr!("Nothing imported in this session yet")),
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

/// Relative time for the history rows (session-local documents only).
fn rel_time(opened_at: Instant) -> String {
    let s = opened_at.elapsed().as_secs();
    match s {
        0..=5 => tr!("Just now").to_string(),
        6..=59 => tr!("%{count}s ago", count = s.to_string()).to_string(),
        _ if s < 3600 => tr!("%{count}m ago", count = (s / 60).to_string()).to_string(),
        _ => tr!("%{count}h ago", count = (s / 3600).to_string()).to_string(),
    }
}

/// The document size as a muted stat, in the unit a reader expects.
pub fn human_size(size: usize, cx: &gpui_kit::App) -> gpui_kit::AnyElement {
    let text = if size >= 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024. * 1024.))
    } else if size >= 1024 {
        format!("{:.1} KB", size as f64 / 1024.)
    } else {
        format!("{size} B")
    };
    chrome::stat(text, cx)
}
