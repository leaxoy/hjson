//! Shared chrome: the band that heads a pane, the status band at the bottom,
//! and the two small pieces they carry — the muted inline stat and the hairline
//! between rows. Pages have no band of their own — a document's controls live
//! in the header of the pane they act on. Their paddings and borders live here
//! so the same kind of band reads the same height on every page.

use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme as _, h_flex, toolbar::Toolbar};
use gpui_kit::{
    AnyElement, App, Div, FontWeight, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, div, px,
};

/// The thin band that heads a pane — title, stats, actions.
const STRIP_PY: f32 = 4.0;
/// The band pinned to the bottom of a page.
const STATUS_PY: f32 = 5.0;

/// Everything the bands share except the border: they are all a fixed-height
/// row of left-to-right content that must not be squeezed by scrolling.
fn band(py: f32, cx: &App) -> Div {
    h_flex()
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .px_3()
        .py(px(py))
        .border_color(cx.theme().border)
}

/// A pane header, separated from its content below.
pub fn strip(cx: &App) -> Div {
    band(STRIP_PY, cx).border_b_1()
}

/// The status band, separated from the page above.
pub fn status(_cx: &App) -> StatusBar {
    StatusBar::new()
        .flex_shrink_0()
        .gap_3()
        .px_3()
        .py(px(STATUS_PY))
        .text_sm()
}

/// The commonest strip: a name, what it holds, then the actions scoped to it.
pub fn panel_header(
    title: impl Into<SharedString>,
    stats: Vec<AnyElement>,
    actions: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    strip(cx)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .children(stats)
        .child(div().flex_1())
        .children(actions)
        .into_any_element()
}

/// A pane header with toolbar semantics. Use this when the strip contains
/// commands or inputs, not just static metadata.
pub fn panel_toolbar(
    id: impl Into<gpui_kit::ElementId>,
    title: impl Into<SharedString>,
    stats: Vec<AnyElement>,
    actions: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    Toolbar::new(id)
        .border_b_1()
        .border_color(cx.theme().border)
        .px_3()
        .content(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .contents(stats)
        .content(div().flex_1())
        .contents(actions)
        .into_any_element()
}

/// The muted inline label the bands carry: a count, a size, a hint of what a
/// click does. Used inside [`panel_header`] and by the panes' own headers.
pub fn stat(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .text_xs()
        .flex_shrink_0()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// A hairline between rows of a stacked list, so the list reads as one card.
pub fn divider(cx: &App) -> AnyElement {
    div()
        .w_full()
        .h_px()
        .bg(cx.theme().border)
        .into_any_element()
}
