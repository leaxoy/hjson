//! Shared JSON source text rendering.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, h_flex,
    scroll::{ScrollableElement as _, ScrollableMask},
    v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, ClickEvent, Context, ElementId, Entity, Hsla, InteractiveElement as _,
    IntoElement, ListState, ParentElement as _, Pixels, Render, RenderOnce, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, TextAlign, Window, div, list, px,
};

use crate::syntax::{Span, SpanKind};

const GUTTER_MIN_WIDTH_PX: f32 = 56.0;
const GUTTER_TEXT_PADDING_PX: f32 = 16.0;

#[derive(Clone, Copy)]
pub enum JsonTextMode {
    Wrapped,
    Scrollable { width: Pixels },
    Clipped,
}

#[derive(Clone, Copy)]
pub enum JsonTextGutter {
    Hidden,
    Dynamic,
    Fixed(Pixels),
}

#[derive(Clone)]
pub struct JsonTextMarker {
    pub glyph: SharedString,
    pub color: Hsla,
}

pub type JsonTextClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub struct JsonTextViewProps {
    pub id: Option<SharedString>,
    pub ix: usize,
    pub lines: Rc<Vec<Vec<Span>>>,
    pub line_h: f32,
    pub mode: JsonTextMode,
    pub gutter: JsonTextGutter,
    pub marker_column: bool,
    pub active: bool,
    pub marker: Option<JsonTextMarker>,
    pub border_color: Option<Hsla>,
    pub on_click: Option<JsonTextClickHandler>,
}

pub struct JsonTextView {
    props: JsonTextViewProps,
}

impl JsonTextView {
    pub fn new(props: JsonTextViewProps) -> Self {
        Self { props }
    }
}

impl RenderOnce for JsonTextView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let JsonTextViewProps {
            id,
            ix,
            lines,
            line_h,
            mode,
            gutter,
            marker_column,
            active,
            marker,
            border_color,
            on_click,
        } = self.props;
        let theme = cx.theme();
        let settings = crate::views::settings::settings(cx);
        let colors = crate::syntax::active(cx);
        let foreground = theme.foreground;
        let font_size = line_h - 8.0;
        let mono = settings.mono_family(cx);
        let gutter_w = match gutter {
            JsonTextGutter::Hidden => None,
            JsonTextGutter::Dynamic => {
                Some(gutter_width(lines.len(), px(font_size), &mono, window))
            }
            JsonTextGutter::Fixed(width) => Some(width),
        };
        let marker_color = marker
            .as_ref()
            .map(|marker| marker.color)
            .unwrap_or(theme.muted_foreground);
        let row_id = id.unwrap_or_else(|| SharedString::from(format!("json-text-row-{ix}")));

        let mut row = h_flex()
            .id(row_id)
            .min_h(px(line_h))
            .flex_shrink_0()
            .text_size(px(font_size))
            .line_height(px(line_h))
            .font_family(mono.clone())
            .items_start()
            .when(active, |this| this.bg(theme.list_active))
            .when_some(border_color, |this, color| {
                this.border_l_2().border_color(color)
            })
            .when_some(on_click, |this, on_click| {
                this.cursor_pointer()
                    .when(!active, |this| this.hover(|this| this.bg(theme.list_hover)))
                    .on_click(on_click)
            });
        row = match mode {
            JsonTextMode::Wrapped => row.w_full(),
            JsonTextMode::Scrollable { width } => row.h(px(line_h)).w(width).min_w(width),
            JsonTextMode::Clipped => row.w_full().h(px(line_h)).items_stretch(),
        };

        if let Some(gutter_w) = gutter_w {
            row = row.child(
                div()
                    .w(gutter_w)
                    .min_h(px(line_h))
                    .flex_shrink_0()
                    .text_align(TextAlign::Right)
                    .pr_2()
                    .mr_2()
                    .border_r_1()
                    .border_color(theme.border)
                    .text_color(marker_color)
                    .child((ix + 1).to_string()),
            );
        }

        if marker_column {
            row = row.child(
                div()
                    .w(px(18.))
                    .flex_shrink_0()
                    .text_color(marker_color)
                    .child(
                        marker
                            .as_ref()
                            .map(|marker| marker.glyph.clone())
                            .unwrap_or_default(),
                    ),
            );
        }

        let mut text = match mode {
            JsonTextMode::Wrapped => h_flex().flex_1().min_w_0().flex_wrap(),
            JsonTextMode::Scrollable { .. } => h_flex().flex_none(),
            JsonTextMode::Clipped => h_flex().flex_1().min_w_0().overflow_hidden(),
        };
        if let Some(line) = lines.get(ix) {
            for span in line {
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
                        .when(
                            matches!(mode, JsonTextMode::Wrapped) && span.kind != SpanKind::Indent,
                            |this| this.min_w_0().whitespace_normal(),
                        )
                        .when(
                            !matches!(mode, JsonTextMode::Wrapped) || span.kind == SpanKind::Indent,
                            |this| this.flex_none().whitespace_nowrap(),
                        )
                        .text_color(color)
                        .child(span.text.clone()),
                );
            }
        }
        row.child(text)
    }
}

pub fn gutter_width(
    line_count: usize,
    font_size: Pixels,
    mono: &SharedString,
    window: &Window,
) -> Pixels {
    let text = line_count.max(1).to_string();
    let run = gpui_kit::TextRun {
        len: text.len(),
        font: gpui_kit::font(mono.clone()),
        color: gpui_kit::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text_width = window
        .text_system()
        .layout_line(&text, font_size, std::slice::from_ref(&run), None)
        .width;
    (text_width + px(GUTTER_TEXT_PADDING_PX)).max(px(GUTTER_MIN_WIDTH_PX))
}

pub fn widest_line(lines: &[Vec<Span>]) -> usize {
    lines
        .iter()
        .enumerate()
        .max_by_key(|(_, spans)| {
            spans
                .iter()
                .map(|span| span.text.chars().count())
                .sum::<usize>()
        })
        .map(|(ix, _)| ix)
        .unwrap_or(0)
}

pub fn nowrap_width(
    lines: &[Vec<Span>],
    widest_line: usize,
    line_h: f32,
    show_gutter: bool,
    mono: &SharedString,
    window: &mut Window,
) -> Pixels {
    let font_size = line_h - 8.0;
    let gutter = if show_gutter {
        (gutter_width(lines.len(), px(font_size), mono, window) / px(1.0)) + 12.0
    } else {
        0.0
    };
    let text = lines
        .get(widest_line)
        .map(|line| {
            let len = line.iter().map(|span| span.text.len()).sum();
            let mut text = String::with_capacity(len);
            for span in line {
                text.push_str(&span.text);
            }
            text
        })
        .unwrap_or_default();
    let run = gpui_kit::TextRun {
        len: text.len(),
        font: gpui_kit::font(mono.clone()),
        color: gpui_kit::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text_width = window
        .text_system()
        .layout_line(&text, px(font_size), std::slice::from_ref(&run), None)
        .width;

    px(((text_width / px(1.0)) + gutter + 24.0).max(640.0))
}

pub fn render_wrapped_list<V, F>(
    id: impl Into<ElementId>,
    list_state: &ListState,
    view: Entity<V>,
    render_row: F,
) -> AnyElement
where
    V: 'static,
    F: Fn(&mut V, usize, &mut Window, &mut Context<V>) -> AnyElement + 'static,
{
    let list_state = list_state.clone();
    let rows = list(list_state.clone(), move |ix, window, cx| {
        view.update(cx, |view, cx| render_row(view, ix, window, cx))
    })
    .size_full()
    .min_h_0();
    div()
        .size_full()
        .min_h_0()
        .child(gpui_kit::base::ScrollBounce::new(id, &list_state, rows))
        .vertical_scrollbar(&list_state)
        .into_any_element()
}

pub fn render_scrollable_list<V, F>(
    view: Entity<V>,
    id: impl Into<ElementId>,
    sizes: Rc<Vec<Size<Pixels>>>,
    widest_line: usize,
    scroll: &gpui_kit::component::VirtualListScrollHandle,
    width: Pixels,
    render_rows: F,
) -> AnyElement
where
    V: Render + 'static,
    F: Fn(&mut V, std::ops::Range<usize>, &mut Window, &mut Context<V>) -> Vec<AnyElement>
        + 'static,
{
    v_virtual_list(view, id, sizes, render_rows)
        .with_item_to_measure_index(widest_line)
        .w(width)
        .min_w_full()
        .track_scroll(scroll)
        .into_any_element()
}

pub fn horizontal_masks(
    id: impl Into<ElementId>,
    scroll: &gpui_kit::component::VirtualListScrollHandle,
) -> [AnyElement; 2] {
    let id = id.into();
    [
        ScrollableMask::new(Axis::Horizontal, scroll)
            .id((id.clone(), "trackpad"))
            .into_any_element(),
        ScrollableMask::vertical_wheel(scroll)
            .id((id, "wheel"))
            .into_any_element(),
    ]
}
