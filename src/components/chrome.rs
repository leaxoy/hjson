//! Shared chrome: the band that heads a pane, the status band at the bottom,
//! and the two small pieces they carry — the muted inline stat and the hairline
//! between rows. Pages have no band of their own — a document's controls live
//! in the header of the pane they act on. Their paddings and borders live here
//! so the same kind of band reads the same height on every page.

use gpui_kit::assets::IconName;
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme as _, Icon, WindowExt as _, h_flex, toolbar::Toolbar};
use gpui_kit::{
    Action, AnyElement, App, AsKeystroke, Div, FontWeight, IntoElement as _, ParentElement as _,
    SharedString, Styled as _, Window, div, px,
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

/// The standard empty state: a muted icon tile over a title (and an optional
/// description), centered in the space it is given.
pub fn empty_state(
    icon: IconName,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
) -> AnyElement {
    let mut header = EmptyHeader::new()
        .media(
            EmptyMedia::new()
                .with_variant(EmptyMediaVariant::Icon)
                .child(Icon::new(icon)),
        )
        .title(EmptyTitle::new().child(title.into()));
    if let Some(description) = description {
        header = header.description(EmptyDescription::new().child(description));
    }
    Empty::new().header(header).into_any_element()
}

/// A keycap showing the action's registered binding, falling back to plain
/// text when the action has none.
pub fn kbd_hint(action: &dyn Action, fallback: &str, window: &Window, cx: &App) -> AnyElement {
    match Kbd::global_binding_for_action(action, window) {
        Some(kbd) => kbd.into_any_element(),
        None => stat(fallback, cx),
    }
}

/// Failure feedback as a floating notification: a statusbar line is easy to
/// miss, an error should not be (successes still go to the statusbar). Stays
/// until dismissed. The app is single-window, so the first window is the one
/// to notify on.
///
/// The push is deferred: several callers run inside `update_in`, i.e. with
/// the window already mutably borrowed, and a nested `handle.update` would
/// fail silently and swallow the notification.
pub fn notify_failure(text: impl Into<String>, cx: &mut App) {
    let text = text.into();
    cx.defer(move |cx| {
        let Some(&handle) = cx.windows().first() else {
            return;
        };
        let _ = handle.update(cx, |_, window, cx| {
            window.push_notification(Notification::error(text).autohide(false), cx);
        });
    });
}

/// The " (⌘[)" tooltip suffix for an action, read from the live keymap so the
/// text tracks the binding instead of hardcoding glyphs.
pub fn shortcut_suffix(action: &dyn Action, window: &Window) -> String {
    window
        .highest_precedence_binding_for_action(action)
        .and_then(|binding| {
            binding
                .keystrokes()
                .first()
                .map(|key| key.as_keystroke().clone())
        })
        .map(|key| format!(" ({})", Kbd::format(&key)))
        .unwrap_or_default()
}
