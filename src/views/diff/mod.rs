//! The diff page: pick two JSON sources (A/B), each from its own titlebar
//! load button, then show a two-column structural diff. The semantic engine
//! lives in this module's [`algo`] submodule.

use std::{collections::HashMap, ops::Range, rc::Rc, sync::Arc};

use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, VirtualListScrollHandle,
    button::{Button, ButtonVariants as _},
    h_flex,
    resizable::{h_resizable, resizable_panel, v_resizable},
    scroll::ScrollableElement as _,
    v_flex, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, RenderOnce, ScrollStrategy, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div, px, size,
};

use crate::app::{DiffNext, DiffPrev};
use crate::components::chrome;
use crate::components::json_text::{
    JsonTextClickHandler, JsonTextGutter, JsonTextMarker, JsonTextMode, JsonTextView,
    JsonTextViewProps,
};
use crate::document::{LoadedDocument, SourceKind};
use crate::error::Status;
use crate::json_model::JsonTree;
use crate::syntax::{Span, highlight};

mod algo;
mod config;
mod loading;
mod result;
pub(crate) use self::algo::DiffKind;
use self::algo::{DiffEntry, DiffOutput, diff_values};

/// Which half of the comparison.
pub type SideIx = usize;
pub const SIDE_A: SideIx = 0;
pub const SIDE_B: SideIx = 1;

/// Row height of the difference list under the two columns. It tracks the
/// interface font, not the JSON font, because the list is chrome, not document.
const DIFF_ROW_H: f32 = 28.0;

/// Width of the diff table's first column. The head row and the body rows
/// share it, which is what makes them line up.
const DIFF_COL_KIND_W: f32 = 96.0;

/// Bounds for the JSONPath column, which is otherwise measured from the paths
/// themselves. The floor keeps the head label readable; the ceiling stops one
/// deep path from squeezing every change detail down to an ellipsis.
const DIFF_COL_PATH_MIN_W: f32 = 96.0;
const DIFF_COL_PATH_MAX_W: f32 = 360.0;

/// `text_xs`, in rems — the size the diff rows are drawn at.
const DIFF_TEXT_XS_REM: f32 = 0.75;

/// Width the JSONPath column needs: the longest path in the set, shaped in the
/// font the cells use. Every row is its own flex container, so a content-sized
/// column would only line up by accident — one measured width for the whole
/// table is what keeps the head and the rows in column.
fn path_col_width(entries: &[DiffEntry], mono: &SharedString, window: &Window) -> Pixels {
    let font = gpui_kit::font(mono.clone());
    // `text_xs` is 0.75rem, and the interface root size is a user setting.
    let font_size = window.rem_size() * DIFF_TEXT_XS_REM;
    let text_system = window.text_system();
    let widest = entries.iter().fold(px(0.), |acc, entry| {
        let run = gpui_kit::TextRun {
            len: entry.path.len(),
            font: font.clone(),
            color: gpui_kit::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let width = text_system
            .layout_line(&entry.path, font_size, std::slice::from_ref(&run), None)
            .width;
        acc.max(width)
    });
    // A few px of air so the last glyph is not flush against the next column.
    (widest + px(8.))
        .max(px(DIFF_COL_PATH_MIN_W))
        .min(px(DIFF_COL_PATH_MAX_W))
}

/// The shell of one diff-table row: the geometry every row shares, so the
/// caller only fills in selection state and the three cells.
fn diff_row() -> gpui_kit::Div {
    h_flex()
        .w_full()
        .flex_shrink_0()
        .h(px(DIFF_ROW_H))
        .items_center()
        .gap_2()
        .px_3()
        .cursor_pointer()
        .text_xs()
}

pub struct DiffView {
    app: WeakEntity<crate::app::HJsonApp>,
    phase: DiffPhase,
    custom_tab_title: Option<SharedString>,
    build_generation: u64,
    _subs: Vec<Subscription>,
}

enum DiffPhase {
    Config(Box<ConfigPhase>),
    Result(Box<ResultPhase>),
}

impl DiffView {
    fn result_ref(&self) -> &ResultPhase {
        match &self.phase {
            DiffPhase::Result(res) => res,
            _ => unreachable!("result-only path while not in result phase"),
        }
    }
}

struct ConfigPhase {
    sides: [SideConfig; 2],
}

/// One source slot on the config page.
struct SideConfig {
    title: Option<SharedString>,
    kind: Option<SourceKind>,
    text: Option<String>,
    parsed: Option<Arc<JsonTree>>,
    /// Original URL when this side came from HTTP, for one-click re-fetch.
    url: Option<String>,
    note: Option<Status>,
    busy: bool,
}

struct ResultPhase {
    entries: Vec<DiffEntry>,
    /// Navigation focus (index into entries), moved by ◀/▶ or a list click.
    current: Option<usize>,
    line_h: f32,
    sides: [SideLines; 2],
    /// Raw inputs (kind / title / url / text) per side — the toolbar's
    /// per-side reload menus swap one of these and re-run the diff.
    sources: [SideSource; 2],
    scroll_a: VirtualListScrollHandle,
    scroll_b: VirtualListScrollHandle,
    /// One row per entry in the difference list under the two columns.
    entry_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_entries: VirtualListScrollHandle,
}

/// Raw input of one comparison side.
#[derive(Clone)]
struct SideSource {
    kind: SourceKind,
    title: SharedString,
    url: Option<String>,
    text: String,
    parsed: Arc<JsonTree>,
}

/// Per-side rendered payload: syntax spans, gutter markers, entry lookup.
struct SideLines {
    title: SharedString,
    lines: Rc<Vec<Vec<Span>>>,
    /// Gutter marker per line (`None` = unchanged line).
    marks: Vec<Option<DiffKind>>,
    /// Line → diff-entry index (for nav highlight).
    entry_of_line: HashMap<usize, usize>,
    line_sizes: Rc<Vec<Size<Pixels>>>,
}

/// Diff tab label: A ⇆ B when both sides are loaded, otherwise 对比.
pub(crate) fn tab_title(view: &DiffView) -> SharedString {
    if let Some(title) = &view.custom_tab_title {
        return title.clone();
    }
    match &view.phase {
        DiffPhase::Config(cfg) => {
            let (a, b) = (
                cfg.sides[SIDE_A].title.clone(),
                cfg.sides[SIDE_B].title.clone(),
            );
            match (a, b) {
                (Some(a), Some(b)) => format!("{a} ⇆ {b}").into(),
                (Some(one), None) | (None, Some(one)) => one,
                (None, None) => tr!("Compare"),
            }
        }
        DiffPhase::Result(res) => format!("{} ⇆ {}", res.sides[0].title, res.sides[1].title).into(),
    }
}

impl SideConfig {
    fn new() -> Self {
        Self {
            title: None,
            kind: None,
            text: None,
            parsed: None,
            url: None,
            note: None,
            busy: false,
        }
    }

    fn fill(&mut self, document: LoadedDocument) {
        self.title = Some(document.title.into());
        self.kind = Some(document.kind);
        self.text = Some(document.text);
        self.parsed = Some(document.parsed);
        self.url = document.url;
        self.note = None;
        self.busy = false;
    }

    fn clear(&mut self) {
        self.title = None;
        self.kind = None;
        self.text = None;
        self.parsed = None;
        self.url = None;
        self.note = None;
        self.busy = false;
    }

    /// Promote the config side to a raw source for the result phase.
    fn snapshot(&self) -> SideSource {
        SideSource {
            kind: self.kind.unwrap_or(SourceKind::Clipboard),
            title: self.title.clone().unwrap_or_default(),
            url: self.url.clone(),
            text: self.text.clone().unwrap_or_default(),
            parsed: self
                .parsed
                .clone()
                .expect("a configured diff side is parsed"),
        }
    }
}

impl From<LoadedDocument> for SideSource {
    fn from(document: LoadedDocument) -> Self {
        Self {
            kind: document.kind,
            title: document.title.into(),
            url: document.url,
            text: document.text,
            parsed: document.parsed,
        }
    }
}

impl ResultPhase {
    fn from_output(sources: [SideSource; 2], line_h: f32, out: DiffOutput) -> Self {
        let entries = out.entries;

        let mut sides = [
            build_side_lines(sources[0].title.clone(), &out.pretty_a),
            build_side_lines(sources[1].title.clone(), &out.pretty_b),
        ];
        for (ix, entry) in entries.iter().enumerate() {
            for (side, line) in [(0usize, entry.a_line), (1usize, entry.b_line)] {
                if let Some(line) = line
                    && let Some(mark) = sides[side].marks.get_mut(line)
                {
                    *mark = Some(entry.kind);
                    sides[side].entry_of_line.insert(line, ix);
                }
            }
        }
        for side in &mut sides {
            side.line_sizes = Rc::new(vec![size(px(640.), px(line_h)); side.lines.len()]);
        }

        let entry_count = entries.len();
        Self {
            entries,
            current: None,
            line_h,
            sides,
            sources,
            scroll_a: VirtualListScrollHandle::new(),
            scroll_b: VirtualListScrollHandle::new(),
            entry_sizes: Rc::new(vec![size(px(640.), px(DIFF_ROW_H)); entry_count]),
            scroll_entries: VirtualListScrollHandle::new(),
        }
    }
}

impl DiffView {
    /// Re-run the diff after one side's source was (re)loaded from the
    /// result-phase toolbar. Keeps the untouched side's raw text.
    fn reload_side(&mut self, side: SideIx, source: SideSource, cx: &mut Context<Self>) {
        let (line_h, other) = match &self.phase {
            DiffPhase::Result(res) => (res.line_h, res.sources[1 - side].clone()),
            _ => return,
        };
        let sources = if side == SIDE_A {
            [source, other]
        } else {
            [other, source]
        };
        self.start_diff(sources, line_h, cx);
    }

    /// Swap the two sides and re-run the comparison with A and B exchanged.
    fn swap_sides(&mut self, cx: &mut Context<Self>) {
        let (line_h, sources) = match &self.phase {
            DiffPhase::Result(res) => (res.line_h, res.sources.clone()),
            _ => return,
        };
        let [a, b] = sources;
        self.start_diff([b, a], line_h, cx);
    }

    fn start_diff(&mut self, sources: [SideSource; 2], line_h: f32, cx: &mut Context<Self>) {
        self.build_generation += 1;
        let generation = self.build_generation;
        if let DiffPhase::Config(cfg) = &mut self.phase {
            for side in &mut cfg.sides {
                side.busy = true;
                side.note = None;
            }
        }
        cx.notify();

        let task = cx.background_spawn(async move {
            let output = diff_values(&sources[SIDE_A].parsed.root, &sources[SIDE_B].parsed.root);
            (sources, output)
        });
        cx.spawn(async move |this, cx| {
            let (sources, output) = task.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.build_generation {
                    return;
                }
                this.phase =
                    DiffPhase::Result(Box::new(ResultPhase::from_output(sources, line_h, output)));
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- loading a source into one side ----------

    /// Put a raw source into one side, whichever phase the page is in: the
    /// config page fills its slot, the result page re-runs the comparison.
    fn fill_source(&mut self, side: SideIx, source: SideSource, cx: &mut Context<Self>) {
        if matches!(self.phase, DiffPhase::Config(_)) {
            self.with_config(|cfg| cfg.sides[side].fill(source.into()));
            cx.notify();
        } else {
            self.reload_side(side, source, cx);
        }
    }

    /// Success notes stay inline under the side card; failures pop up as
    /// notifications so they cannot be missed.
    fn note_side(
        &mut self,
        side: SideIx,
        ok: bool,
        text: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(text) = chrome::inline_success_or_notify_failure(ok, text, cx) {
            if let DiffPhase::Config(cfg) = &mut self.phase {
                cfg.sides[side].note = Some(Status::from_parts(true, text));
                cfg.sides[side].busy = false;
            }
            cx.notify();
        } else {
            if let DiffPhase::Config(cfg) = &mut self.phase {
                cfg.sides[side].busy = false;
            }
        }
    }
}

impl From<SideSource> for LoadedDocument {
    fn from(source: SideSource) -> Self {
        Self {
            kind: source.kind,
            title: source.title.to_string(),
            text: source.text,
            url: source.url,
            parsed: source.parsed,
        }
    }
}

fn build_side_lines(title: SharedString, pretty: &str) -> SideLines {
    SideLines {
        title,
        lines: Rc::new(highlight(pretty)),
        marks: Vec::new(),
        entry_of_line: HashMap::new(),
        line_sizes: Rc::new(Vec::new()),
    }
}

impl DiffView {
    /// `prefill` seeds one side directly (used by the document view's compare
    /// button); `None` starts with two empty slots.
    pub(crate) fn new(
        app: WeakEntity<crate::app::HJsonApp>,
        prefill: Option<(SideIx, LoadedDocument)>,
    ) -> Self {
        let mut sides = [SideConfig::new(), SideConfig::new()];

        if let Some((side, document)) = prefill {
            sides[side].fill(document);
        }

        Self {
            app,
            phase: DiffPhase::Config(Box::new(ConfigPhase { sides })),
            custom_tab_title: None,
            build_generation: 0,
            _subs: Vec::new(),
        }
    }

    pub fn set_tab_title(&mut self, title: SharedString, cx: &mut Context<Self>) {
        self.custom_tab_title = Some(title);
        cx.notify();
    }

    fn with_config(&mut self, f: impl FnOnce(&mut ConfigPhase)) {
        if let DiffPhase::Config(cfg) = &mut self.phase {
            f(cfg);
        }
    }
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.phase {
            DiffPhase::Config(_) => self.render_config(cx).into_any_element(),
            DiffPhase::Result(_) => self.render_result(window, cx).into_any_element(),
        }
    }
}

impl DiffView {
    fn config_ready(&self) -> bool {
        match &self.phase {
            DiffPhase::Config(cfg) => {
                cfg.sides[SIDE_A].text.is_some()
                    && cfg.sides[SIDE_B].text.is_some()
                    && !cfg.sides.iter().any(|side| side.busy)
            }
            _ => false,
        }
    }

    // ---------- diff + navigation ----------

    fn run_diff(&mut self, cx: &mut Context<Self>) {
        let line_h = crate::views::settings::settings(cx).row_height();
        let (a_src, b_src) = {
            let cfg = match &self.phase {
                DiffPhase::Config(cfg) => cfg,
                _ => unreachable!(),
            };
            (cfg.sides[0].snapshot(), cfg.sides[1].snapshot())
        };
        if a_src.text.is_empty() || b_src.text.is_empty() {
            return;
        }

        self.start_diff([a_src, b_src], line_h, cx);
    }

    /// Move the selection `delta` entries and scroll both sides in sync.
    pub fn jump(&mut self, delta: i64, cx: &mut Context<Self>) {
        let next = {
            let res = match &self.phase {
                DiffPhase::Result(res) => res,
                _ => return,
            };
            let total = res.entries.len() as i64;
            if total == 0 {
                return;
            }
            let cur =
                res.current
                    .map(|c| c as i64)
                    .unwrap_or(if delta > 0 { total - 1 } else { 0 });
            ((cur + delta) % total + total) % total
        };
        self.select_entry(next as usize, cx);
    }

    /// Focus one difference: mark it, then bring both columns and the list to it.
    fn select_entry(&mut self, ix: usize, cx: &mut Context<Self>) {
        let DiffPhase::Result(res) = &mut self.phase else {
            return;
        };
        let Some(entry) = res.entries.get(ix) else {
            return;
        };
        let (a_line, b_line) = (entry.a_line, entry.b_line);
        res.current = Some(ix);
        if let Some(line) = a_line {
            res.scroll_a.scroll_to_item(line, ScrollStrategy::Center);
        }
        if let Some(line) = b_line {
            res.scroll_b.scroll_to_item(line, ScrollStrategy::Center);
        }
        res.scroll_entries
            .scroll_to_item(ix, ScrollStrategy::Center);
        cx.notify();
    }
}
