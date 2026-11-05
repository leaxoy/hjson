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
use crate::document::SourceKind;
use crate::document::{self, LoadedDocument};
use crate::error::Status;
use crate::json_model::JsonTree;
use crate::syntax::{Span, highlight};

mod algo;
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
        let text = text.into();
        if ok {
            if let DiffPhase::Config(cfg) = &mut self.phase {
                cfg.sides[side].note = Some(Status::from_parts(true, text));
                cfg.sides[side].busy = false;
            }
            cx.notify();
        } else {
            if let DiffPhase::Config(cfg) = &mut self.phase {
                cfg.sides[side].busy = false;
            }
            chrome::notify_failure(text, cx);
        }
    }

    /// Replace one side with the clipboard contents.
    fn load_clipboard(&mut self, side: SideIx, cx: &mut Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .map(|t| t.trim().to_string());
        match text {
            Some(text) if !text.is_empty() => match document::from_clipboard(text) {
                Ok(document) => self.fill_source(side, document.into(), cx),
                Err(err) => self.note_side(side, false, err.message(), cx),
            },
            Some(_) => self.note_side(side, false, tr!("The clipboard is empty"), cx),
            None => self.note_side(side, false, tr!("No text on the clipboard"), cx),
        }
    }

    /// Replace one side with a file picked from disk.
    fn load_file(&mut self, side: SideIx, cx: &mut Context<Self>) {
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
            this.update_in(cx, |this, _window, cx| match result {
                Ok(document) => this.fill_source(side, document.into(), cx),
                Err(e) => this.note_side(side, false, e.message(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Fetch `url` and put the response into one side.
    fn load_fetch(
        &mut self,
        side: SideIx,
        url: String,
        custom_title: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.note_side(side, false, tr!("Enter a URL"), cx);
            return;
        }
        // Only the config page tracks an in-flight request.
        if let DiffPhase::Config(cfg) = &mut self.phase {
            cfg.sides[side].busy = true;
            cfg.sides[side].note = Some(Status::success(
                tr!("Requesting %{url} …", url = url.clone()).to_string(),
            ));
        }
        cx.notify();

        let task = cx.background_spawn(async move { document::from_url(url, custom_title) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, _window, cx| match result {
                Ok(document) => this.fill_source(side, document.into(), cx),
                Err(e) => this.note_side(side, false, e.message(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Re-fetch the URL one side already came from.
    fn load_refetch(&mut self, side: SideIx, cx: &mut Context<Self>) {
        let source = match &self.phase {
            DiffPhase::Config(cfg) => (
                cfg.sides[side].url.clone(),
                cfg.sides[side].title.as_ref().map(ToString::to_string),
            ),
            DiffPhase::Result(res) => (
                res.sources[side].url.clone(),
                Some(res.sources[side].title.to_string()),
            ),
        };
        if let (Some(url), title) = source {
            self.load_fetch(side, url, title, cx);
        }
    }

    /// Replace one side with a document from the session history.
    fn load_history(&mut self, side: SideIx, id: u64, cx: &mut Context<Self>) {
        let found = crate::components::source_menu::prepared_doc_of(&self.app, cx, id);
        match found {
            Some((kind, title, text, url, parsed)) => {
                match document::from_history(kind, title.to_string(), text, url, parsed) {
                    Ok(document) => self.fill_source(side, document.into(), cx),
                    Err(err) => self.note_side(side, false, err.message(), cx),
                }
            }
            None => self.note_side(side, false, tr!("That entry is no longer in the list"), cx),
        }
    }

    /// Replace one side with text typed into the manual-input dialog.
    fn load_manual(&mut self, side: SideIx, text: String, cx: &mut Context<Self>) {
        match document::from_manual(text) {
            Ok(document) => self.fill_source(side, document.into(), cx),
            Err(err) => self.note_side(side, false, err.message(), cx),
        }
    }

    /// Replace one side with the built-in sample document.
    fn load_sample(&mut self, side: SideIx, cx: &mut Context<Self>) {
        match document::from_sample() {
            Ok(document) => self.fill_source(side, document.into(), cx),
            Err(err) => self.note_side(side, false, err.message(), cx),
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

impl crate::components::source_menu::LoadTarget for DiffView {
    type Slot = SideIx;

    fn apply_load(
        &mut self,
        side: SideIx,
        method: crate::components::source_menu::LoadMethod,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::components::source_menu::LoadMethod;
        match method {
            LoadMethod::Clipboard => self.load_clipboard(side, cx),
            LoadMethod::PickFile => self.load_file(side, cx),
            LoadMethod::RefetchUrl => self.load_refetch(side, cx),
            LoadMethod::FetchUrl { url, title } => self.load_fetch(side, url, title, cx),
            LoadMethod::Manual(text) => self.load_manual(side, text, cx),
            LoadMethod::Sample => self.load_sample(side, cx),
            LoadMethod::History(id) => self.load_history(side, id, cx),
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

    // ---------- config phase ----------

    /// What one side's load menu can offer: its own URL (for re-fetch) plus
    /// the session history.
    fn side_load_options(
        &self,
        side: SideIx,
        cx: &gpui_kit::App,
    ) -> crate::components::source_menu::LoadOptions {
        let url = match &self.phase {
            DiffPhase::Config(cfg) => cfg.sides[side].url.clone(),
            DiffPhase::Result(res) => res.sources[side].url.clone(),
        };
        crate::components::source_menu::LoadOptions {
            url,
            history: crate::components::source_menu::history_of(&self.app, cx),
        }
    }

    /// The side's one load control, docked in that side's titlebar — the same
    /// shape as a document's own header: an icon button whose menu holds every
    /// source. Download when the side is still empty, RefreshCw once it holds
    /// something, because then it replaces what is there.
    fn side_load_button(&self, side: SideIx, cx: &mut Context<Self>) -> AnyElement {
        let (kind, title, filled) = match &self.phase {
            DiffPhase::Config(cfg) => (
                cfg.sides[side]
                    .kind
                    .map(|k| k.label())
                    .unwrap_or(tr!("Nothing yet")),
                cfg.sides[side].title.clone().unwrap_or_default(),
                cfg.sides[side].title.is_some(),
            ),
            DiffPhase::Result(res) => (
                res.sources[side].kind.label(),
                res.sources[side].title.clone(),
                true,
            ),
        };
        let side_label = if side == SIDE_A {
            tr!("Side A")
        } else {
            tr!("Side B")
        };
        let (icon, verb) = if filled {
            (IconName::RefreshCw, tr!("Reload"))
        } else {
            (IconName::Download, tr!("Load"))
        };
        crate::components::source_menu::header_load_button(
            SharedString::from(format!("load-{side}")),
            icon,
            tr!(
                "%{verb} %{side} (current: %{title} · %{kind})",
                verb = verb.to_string(),
                side = side_label.to_string(),
                title = title.to_string(),
                kind = kind.to_string()
            )
            .to_string(),
            side,
            self.side_load_options(side, cx),
            cx.weak_entity(),
        )
    }

    fn render_config(&mut self, cx: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        // Build all child elements first — they borrow cx mutably — then take
        // the (borrowing) theme snapshot for styling the container itself.
        let ready = self.config_ready();
        let notes = self.render_notes(cx);
        let card_a = self.render_side_card(SIDE_A, cx);
        let card_b = self.render_side_card(SIDE_B, cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;

        let page = v_flex()
            .id("diff-config")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(24.))
            .py(px(20.))
            .gap_4()
            .child(div().text_sm().text_color(muted).child(
                tr!("One JSON per side, matched by JSONPath · object keys are order-independent · array items align by index"),
            ))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .child(card_a)
                    .child(card_b),
            )
            .children(notes)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_center()
                    .child(if ready {
                        Button::new("start-diff")
                            .label(tr!("Start comparison"))
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.run_diff(cx)))
                    } else {
                        Button::new("start-diff")
                            .label(tr!("Put content on both sides to start comparing"))
                            .disabled(true)
                    }),
            );

        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .child(page)
    }

    /// One side slot: its titlebar carries the side's name and its single load
    /// control — the same shape as a document's own header — and the body shows
    /// either the empty drop area or what the side holds.
    fn render_side_card(&mut self, side: SideIx, cx: &mut Context<Self>) -> AnyElement {
        let (border, muted, primary) = {
            let theme = cx.theme();
            (theme.border, theme.muted_foreground, theme.primary)
        };
        let side_label = if side == SIDE_A {
            tr!("Side A")
        } else {
            tr!("Side B")
        };
        let (title, kind, busy, preview) = match &self.phase {
            DiffPhase::Config(cfg) => {
                let s = &cfg.sides[side];
                let preview: String = s
                    .text
                    .as_deref()
                    .unwrap_or("")
                    .replace(['\n', '\r', '\t'], " ")
                    .chars()
                    .take(64)
                    .collect();
                (s.title.clone(), s.kind, s.busy, preview)
            }
            _ => unreachable!(),
        };
        let load = self.side_load_button(side, cx);
        // A fetch in flight needs a surface of its own: the load menu closes as
        // soon as it fires, so the side's titlebar says what is happening.
        let busy_chip = busy.then(|| {
            Button::new(SharedString::from(format!("side-busy-{side}")))
                .label(tr!("Requesting…"))
                .small()
                .disabled(true)
                .into_any_element()
        });

        let card = v_flex()
            .id(SharedString::from(format!("side-card-{side}")))
            .w_full()
            .flex_1()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .p_3()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(muted)
                            .child(side_label),
                    )
                    .child(div().flex_1())
                    .children(busy_chip)
                    .child(load),
            );

        if title.is_none() {
            return card
                .child(chrome::empty_state(
                    IconName::Download,
                    tr!("Nothing yet"),
                    Some(tr!(
                        "Use Load in the top-right: paste / open file / fetch URL / pick from history"
                    )),
                ))
                .into_any_element();
        }

        let mono = crate::views::settings::settings(cx).mono_family(cx);
        card.child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(IconName::Braces).small().text_color(primary))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .min_w_0()
                        .truncate()
                        .child(title.unwrap_or_default()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(kind.map(|k| k.label()).unwrap_or("—".into())),
                )
                .child(div().flex_1())
                .child(
                    Button::new(SharedString::from(format!("side-clear-{side}")))
                        .icon(IconName::X)
                        .ghost()
                        .small()
                        .tooltip(tr!("Remove this side"))
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.with_config(|cfg| cfg.sides[side].clear());
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .text_xs()
                .font_family(mono)
                .text_color(muted)
                .truncate()
                .child(preview),
        )
        .into_any_element()
    }

    fn render_notes(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (notes_a, notes_b) = match &self.phase {
            DiffPhase::Config(cfg) => (cfg.sides[0].note.clone(), cfg.sides[1].note.clone()),
            _ => (None, None),
        };
        if notes_a.is_none() && notes_b.is_none() {
            return None;
        }
        let (danger, success) = {
            let theme = cx.theme();
            (theme.danger, theme.success)
        };
        let mk = |n: Option<Status>| {
            n.map(|s| {
                let ok = s.is_success();
                let text = s.message();
                div()
                    .text_xs()
                    .text_color(if ok { success } else { danger })
                    .child(text)
                    .into_any_element()
            })
        };
        let (a, b) = (mk(notes_a), mk(notes_b));
        let joiner = |n: Option<AnyElement>| match n {
            Some(el) => el,
            None => div().into_any_element(),
        };
        Some(
            h_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .child(joiner(a)),
                )
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .child(joiner(b)),
                )
                .into_any_element(),
        )
    }

    // ---------- result phase ----------

    fn render_result(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let bg = cx.theme().background;
        let has_diff = matches!(&self.phase, DiffPhase::Result(res) if !res.entries.is_empty());

        // Neither side gets an initial size: with both on the group's flex
        // defaults they take the width evenly, and once measured the even
        // ratio survives a window resize. The lower bound stops a drag from
        // collapsing one side to nothing; the upper stays open so a wide
        // window still gives each half its half.
        let cols = h_resizable("diff-split")
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_side_panel(SIDE_A, cx)),
            )
            .child(
                resizable_panel()
                    .size_range(px(240.)..Pixels::MAX)
                    .child(self.render_side_panel(SIDE_B, cx)),
            );

        // No shared title bar: each column header reloads its own side and the
        // status bar carries the summary, the error note, and the navigation.
        let status = self.render_status(window, cx);

        // Differences get their own strip under the columns — the whole change
        // set in one place, instead of hunting for the gutter marks line by line.
        let body: AnyElement = if has_diff {
            v_resizable("diff-body")
                .child(
                    resizable_panel()
                        .size(px(420.))
                        .size_range(px(160.)..px(1400.))
                        .child(cols),
                )
                .child(
                    resizable_panel()
                        .size(px(200.))
                        // Room for the title strip, the column head and a few
                        // rows, even when the user drags it down.
                        .size_range(px(120.)..px(600.))
                        .child(self.render_diff_list(window, cx)),
                )
                .into_any_element()
        } else {
            cols.into_any_element()
        };

        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(bg)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .child(status)
    }

    /// The table of every difference, docked below the two columns. A row click
    /// moves both columns to that entry; the current one is filled.
    fn render_diff_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let bg = cx.theme().background;
        let (total, sizes, scroll) = {
            let res = self.result_ref();
            (
                res.entries.len(),
                res.entry_sizes.clone(),
                &res.scroll_entries,
            )
        };
        // Measured once for the whole table so the head and every row agree.
        let path_w = {
            let res = self.result_ref();
            path_col_width(
                &res.entries,
                &crate::views::settings::settings(cx).mono_family(cx),
                window,
            )
        };

        let header = chrome::panel_header(
            tr!("Changes"),
            vec![chrome::stat(
                tr!("%{count} changes", count = total.to_string()),
                cx,
            )],
            vec![chrome::stat(tr!("Click a row to jump to both sides"), cx)],
            cx,
        );

        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(self.render_diff_head(path_w, cx))
            .child(
                v_flex()
                    .id("diff-list-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_virtual_list(view, "diff-list-rows", sizes, move |this, range, _, cx| {
                            this.render_diff_rows(range, path_w, cx)
                        })
                        .track_scroll(scroll),
                    )
                    .vertical_scrollbar(scroll),
            )
            .into_any_element()
    }

    /// The column titles, laid out with the widths the rows use so the two
    /// stay aligned.
    fn render_diff_head(&self, path_w: Pixels, cx: &gpui_kit::App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let title = |label: gpui_kit::SharedString| {
            div()
                .text_xs()
                .min_w_0()
                .truncate()
                .text_color(muted)
                .child(label)
        };
        chrome::strip(cx)
            .child(
                div()
                    .w(px(DIFF_COL_KIND_W))
                    .flex_shrink_0()
                    .child(title(tr!("Change"))),
            )
            .child(
                div()
                    .w(path_w)
                    .flex_shrink_1()
                    .min_w_0()
                    .child(title("JSONPath".into())),
            )
            .child(div().flex_1().min_w_0().child(title(tr!("Detail"))))
            .into_any_element()
    }

    /// One row of the diff table: `diff_row` holds the shared column geometry,
    /// this fills it. `path_w` is the table-wide JSONPath width, measured once
    /// by `render_diff_list`.
    fn render_diff_rows(
        &mut self,
        range: Range<usize>,
        path_w: Pixels,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let (list_active, list_hover, muted, danger, success, warning, primary) = {
            let theme = cx.theme();
            (
                theme.list_active,
                theme.list_hover,
                theme.muted_foreground,
                theme.danger,
                theme.success,
                theme.warning,
                theme.primary,
            )
        };
        let mono = crate::views::settings::settings(cx).mono_family(cx);
        let (entries, current) = {
            let res = self.result_ref();
            (&res.entries, res.current)
        };

        range
            .filter_map(|ix| {
                let entry = entries.get(ix)?;
                let color = match entry.kind {
                    DiffKind::Added => success,
                    DiffKind::Removed => danger,
                    DiffKind::Modified => warning,
                    DiffKind::TypeChanged => primary,
                };
                let value = |text: &str, color: Hsla| {
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(mono.clone())
                        .text_color(if text.is_empty() { muted } else { color })
                        .child(if text.is_empty() {
                            "—".to_string()
                        } else {
                            text.to_string()
                        })
                };
                // Only the values that changed: an added entry has no old side
                // and a removed one no new side.
                let detail = match entry.kind {
                    DiffKind::Added => value(&entry.new, success).into_any_element(),
                    DiffKind::Removed => value(&entry.old, danger).into_any_element(),
                    _ => h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_1()
                        .child(value(&entry.old, danger))
                        .child(Icon::new(IconName::ArrowRight).small().text_color(muted))
                        .child(value(&entry.new, success))
                        .into_any_element(),
                };

                Some(
                    diff_row()
                        .id(SharedString::from(format!("diff-entry-{ix}")))
                        .when(current == Some(ix), |this| this.bg(list_active))
                        .when(current != Some(ix), |this| {
                            this.hover(|this| this.bg(list_hover))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_entry(ix, cx);
                        }))
                        .child(
                            h_flex()
                                .w(px(DIFF_COL_KIND_W))
                                .flex_shrink_0()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(color)
                                        .child(entry.kind.glyph()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(color)
                                        .child(entry.kind.label()),
                                ),
                        )
                        .child(
                            div()
                                .w(path_w)
                                .flex_shrink_1()
                                .min_w_0()
                                .truncate()
                                .font_family(mono.clone())
                                .child(entry.path.clone()),
                        )
                        .child(div().flex_1().min_w_0().child(detail))
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_side_panel(&mut self, side: SideIx, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let (bg, muted) = {
            let theme = cx.theme();
            (theme.background, theme.muted_foreground)
        };
        let wanted_h = crate::views::settings::settings(cx).row_height();
        if let DiffPhase::Result(res) = &mut self.phase
            && res.line_h != wanted_h
        {
            res.line_h = wanted_h;
            let line_h = res.line_h;
            for side_lines in res.sides.iter_mut() {
                side_lines.line_sizes =
                    Rc::new(vec![size(px(640.), px(line_h)); side_lines.lines.len()]);
            }
        }
        let (sizes, scroll, lines_empty, title) = {
            let res = self.result_ref();
            let s = &res.sides[side];
            let scroll = if side == SIDE_A {
                &res.scroll_a
            } else {
                &res.scroll_b
            };
            (
                s.line_sizes.clone(),
                scroll,
                s.lines.is_empty(),
                s.title.clone(),
            )
        };
        // This side's reload entry sits above its own column, next to the
        // source title it belongs to.
        let load = self.side_load_button(side, cx);
        let header = chrome::panel_header(
            if side == SIDE_A { "A" } else { "B" },
            vec![
                div()
                    .text_xs()
                    .text_color(muted)
                    .max_w(px(240.))
                    .min_w_0()
                    .truncate()
                    .child(title)
                    .into_any_element(),
            ],
            vec![load],
            cx,
        );
        v_flex()
            .size_full()
            .bg(bg)
            .child(header)
            .child(
                v_flex()
                    .id(SharedString::from(format!("diff-scroll-{side}")))
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .child(
                        v_virtual_list(
                            view,
                            SharedString::from(format!("diff-rows-{side}")),
                            sizes,
                            move |this, range, window, cx| {
                                this.render_side_rows(side, range, window, cx)
                            },
                        )
                        .track_scroll(scroll),
                    )
                    .vertical_scrollbar(scroll),
            )
            .when(lines_empty, |this| {
                this.child(chrome::empty_state(
                    IconName::FileText,
                    tr!("Empty document"),
                    None,
                ))
            })
            .into_any_element()
    }

    fn render_side_rows(
        &mut self,
        side: SideIx,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // Batch together everything the row builder needs before the loop: the
        // theme borrows cx and the loop wants plain data.
        let row_h = {
            let res = self.result_ref();
            res.line_h
        };
        let settings = crate::views::settings::settings(cx);
        // Line numbers are a document-wide setting, so the diff obeys it too.
        let show_gutter = settings.line_numbers;
        let (border_default, success, danger, warning, primary) = {
            let theme = cx.theme();
            (
                theme.border,
                theme.success,
                theme.danger,
                theme.warning,
                theme.primary,
            )
        };
        let color_of = |kind: DiffKind| -> Hsla {
            match kind {
                DiffKind::Added => success,
                DiffKind::Removed => danger,
                DiffKind::Modified => warning,
                DiffKind::TypeChanged => primary,
            }
        };

        let (lines, marks, entry_of_line, current) = {
            let res = self.result_ref();
            let s = &res.sides[side];
            (s.lines.clone(), &s.marks, &s.entry_of_line, res.current)
        };

        range
            .map(|ix| {
                let mark = marks.get(ix).copied().flatten();
                let entry_ix = entry_of_line.get(&ix).copied();
                let is_current = entry_ix.is_some() && entry_ix == current;

                let marker = mark.map(|kind| JsonTextMarker {
                    glyph: kind.glyph().to_string().into(),
                    color: color_of(kind),
                });
                let on_click: Option<JsonTextClickHandler> = entry_ix.map(|entry| {
                    Box::new(cx.listener(move |this, _: &gpui_kit::ClickEvent, _, cx| {
                        this.select_entry(entry, cx);
                    })) as JsonTextClickHandler
                });
                let row = JsonTextView::new(JsonTextViewProps {
                    id: Some(SharedString::from(format!("diff-line-{side}-{ix}"))),
                    ix,
                    lines: lines.clone(),
                    line_h: row_h,
                    mode: JsonTextMode::Clipped,
                    gutter: if show_gutter {
                        JsonTextGutter::Fixed(px(56.))
                    } else {
                        JsonTextGutter::Hidden
                    },
                    marker_column: true,
                    active: is_current,
                    marker,
                    border_color: Some(mark.map(color_of).unwrap_or(border_default)),
                    on_click,
                })
                .render(window, cx);
                row.into_any_element()
            })
            .collect()
    }

    fn render_status(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, danger, success) = (theme.muted_foreground, theme.danger, theme.success);
        let mono = crate::views::settings::settings(cx).mono_family(cx);
        let (entry, counts, total, current) = {
            let res = self.result_ref();
            let counts = if res.entries.is_empty() {
                String::new()
            } else {
                format!(
                    "+{} −{} ~{} ≈{}",
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::Added)
                        .count(),
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::Removed)
                        .count(),
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::Modified)
                        .count(),
                    res.entries
                        .iter()
                        .filter(|e| e.kind == DiffKind::TypeChanged)
                        .count(),
                )
            };
            (
                res.current.and_then(|c| res.entries.get(c)).cloned(),
                counts,
                res.entries.len(),
                res.current,
            )
        };
        let nav = if total == 0 {
            tr!("No changes").to_string()
        } else {
            match current {
                Some(cur) => tr!(
                    "%{current}/%{total} changes",
                    current = (cur + 1).to_string(),
                    total = total.to_string()
                )
                .to_string(),
                None => tr!("%{count} total changes", count = total.to_string()).to_string(),
            }
        };

        let left = match entry.as_ref() {
            Some(entry) => {
                let kind_color: Hsla = match entry.kind {
                    DiffKind::Added => theme.success,
                    DiffKind::Removed => theme.danger,
                    DiffKind::Modified => theme.warning,
                    DiffKind::TypeChanged => theme.primary,
                };
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .flex_shrink_0()
                            .text_color(kind_color)
                            .child(format!("{} {}", entry.kind.glyph(), entry.kind.label())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(mono.clone())
                            .min_w_0()
                            .truncate()
                            .child(entry.path.clone()),
                    )
                    .into_any_element()
            }
            None => div()
                .text_xs()
                .text_color(muted)
                .child(if counts.is_empty() {
                    tr!("✓ The two documents are identical")
                } else {
                    tr!("Pick a row in the change list below, or step through with ◀ ▶")
                })
                .into_any_element(),
        };

        let right = match entry.as_ref() {
            Some(entry) => h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_family(mono.clone())
                        .max_w(px(240.))
                        .min_w_0()
                        .truncate()
                        .text_color(danger)
                        .child(if entry.old.is_empty() {
                            "—".to_string()
                        } else {
                            entry.old.clone()
                        }),
                )
                .child(Icon::new(IconName::ArrowRight).small().text_color(muted))
                .child(
                    div()
                        .text_xs()
                        .font_family(mono.clone())
                        .max_w(px(240.))
                        .min_w_0()
                        .truncate()
                        .text_color(success)
                        .child(if entry.new.is_empty() {
                            "—".to_string()
                        } else {
                            entry.new.clone()
                        }),
                )
                .into_any_element(),
            None => div().into_any_element(),
        };

        let mut status = chrome::status(cx).left(left);

        status = status
            .right(right)
            .right(
                div()
                    .text_xs()
                    .flex_shrink_0()
                    .text_color(muted)
                    .font_family(mono.clone())
                    .child(counts),
            )
            .right(div().text_xs().flex_shrink_0().text_color(muted).child(nav));

        if total > 0 {
            status = status.right(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("prev-diff")
                            .icon(IconName::ChevronUp)
                            .small()
                            .tooltip(format!(
                                "{}{}",
                                tr!("Previous change"),
                                chrome::shortcut_suffix(&DiffPrev, window)
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.jump(-1, cx))),
                    )
                    .child(
                        Button::new("next-diff")
                            .icon(IconName::ChevronDown)
                            .small()
                            .tooltip(format!(
                                "{}{}",
                                tr!("Next change"),
                                chrome::shortcut_suffix(&DiffNext, window)
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.jump(1, cx))),
                    ),
            );
        }

        status = status.right(
            Button::new("swap-sides")
                .icon(IconName::ArrowLeftRight)
                .small()
                .tooltip(tr!("Swap sides (A ⇆ B)"))
                .on_click(cx.listener(|this, _, _, cx| this.swap_sides(cx))),
        );

        status.into_any_element()
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
