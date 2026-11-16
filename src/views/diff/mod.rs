//! The diff page: pick two JSON sources (A/B), each from its own titlebar
//! load button, then show a two-column structural diff. The semantic engine
//! lives in this module's [`algo`] submodule.

use std::{cell::Cell, collections::BTreeSet, ops::Range, rc::Rc, sync::Arc};

use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, VirtualListScrollHandle,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{
        Editor, EditorState, GutterDecoration, GutterDecorationCollection, Indent, Outdent,
        Position, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle,
    },
    resizable::{h_resizable, resizable_panel, v_resizable},
    scroll::ScrollableElement as _,
    v_flex, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, ScrollStrategy, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div, px, size,
};

use crate::app::{DiffNext, DiffPrev};
use crate::components::chrome;
use crate::document::{LoadedDocument, SourceKind};
use crate::error::Status;
use crate::json_model::JsonTree;

mod algo;
mod config;
mod exemptions;
mod loading;
mod result;
pub(crate) use self::algo::DiffKind;
#[cfg(test)]
use self::algo::diff_values;
use self::algo::{DiffEntry, DiffOutput, diff_values_with_ignored};
use gpui_kit::component::WindowExt as _;

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
    string_expansion: Option<bool>,
    /// Exact field names, scoped to this comparison tab; never JSONPath rules.
    ignored_fields: BTreeSet<String>,
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
    sides: [SideEditor; 2],
    /// Apply navigation after both editors have been created in a window.
    reveal_current: bool,
    decoration_state: Option<([Hsla; 4], Option<usize>)>,
    /// Raw inputs (kind / title / url / text) per side — the toolbar's
    /// per-side reload menus swap one of these and re-run the diff.
    sources: [SideSource; 2],
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

/// Each side owns one native editor. Its text is exactly the formatted text
/// whose line anchors the semantic diff returns; no synthetic alignment rows.
struct SideEditor {
    title: SharedString,
    pretty: String,
    line_ranges: Vec<Range<usize>>,
    marks: Vec<(usize, DiffKind)>,
    /// Created in the result's first render, where a Window is available.
    editor: Option<Entity<EditorState>>,
    gutter: Option<GutterDecorationCollection>,
    active_range: Option<RangeDecorationCollection>,
    options: Option<(bool, bool)>,
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
            SideEditor::new(sources[0].title.clone(), out.pretty_a),
            SideEditor::new(sources[1].title.clone(), out.pretty_b),
        ];
        for entry in &entries {
            for (side, line) in [(SIDE_A, entry.a_line), (SIDE_B, entry.b_line)] {
                if let Some(line) = line
                    && line < sides[side].line_ranges.len()
                {
                    sides[side].marks.push((line, entry.kind));
                }
            }
        }

        let entry_count = entries.len();
        Self {
            entries,
            current: None,
            line_h,
            sides,
            sources,
            reveal_current: false,
            decoration_state: None,
            entry_sizes: Rc::new(vec![size(px(640.), px(DIFF_ROW_H)); entry_count]),
            scroll_entries: VirtualListScrollHandle::new(),
        }
    }
}

impl DiffView {
    pub(crate) fn refresh_string_expansion(&mut self, cx: &mut Context<Self>) {
        let expand = crate::views::settings::settings(cx).expand_json_strings;
        if self.string_expansion == Some(expand) {
            return;
        }
        self.string_expansion = Some(expand);
        match &mut self.phase {
            DiffPhase::Config(cfg) => {
                for side in &mut cfg.sides {
                    if let Some(parsed) = &side.parsed {
                        side.parsed = Some(parsed.with_string_expansion(expand));
                    }
                }
            }
            DiffPhase::Result(res) => {
                let sources = res.sources.clone();
                let line_h = res.line_h;
                self.start_diff(sources, line_h, cx);
            }
        }
        cx.notify();
    }

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
        let expand = crate::views::settings::settings(cx).expand_json_strings;
        self.string_expansion = Some(expand);
        self.build_generation += 1;
        let generation = self.build_generation;
        if let DiffPhase::Config(cfg) = &mut self.phase {
            for side in &mut cfg.sides {
                side.busy = true;
                side.note = None;
            }
        }
        cx.notify();

        let ignored = self.ignored_fields.clone();
        let task = cx.background_spawn(async move {
            let sources = sources.map(|mut source| {
                source.parsed = source.parsed.with_string_expansion(expand);
                source
            });
            let output = diff_values_with_ignored(
                &sources[SIDE_A].parsed.root,
                &sources[SIDE_B].parsed.root,
                &ignored,
            );
            (sources, output)
        });
        cx.spawn(async move |this, cx| {
            let (sources, output) = task.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.build_generation {
                    return;
                }
                if crate::views::settings::settings(cx).expand_json_strings != expand {
                    this.start_diff(sources, line_h, cx);
                    return;
                }
                let mut next = ResultPhase::from_output(sources, line_h, output);
                // Changing only exemptions must not reset editor selections,
                // folds or scrolling. Decoration collections will be refreshed.
                if let DiffPhase::Result(previous) = &mut this.phase {
                    for (old, new) in previous.sides.iter_mut().zip(&mut next.sides) {
                        if old.pretty == new.pretty {
                            new.editor = old.editor.take();
                            new.gutter = old.gutter.take();
                            new.active_range = old.active_range.take();
                            new.options = old.options;
                        }
                    }
                }
                this.phase = DiffPhase::Result(Box::new(next));
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- loading a source into one side ----------

    /// Put a raw source into one side, whichever phase the page is in: the
    /// config page fills its slot, the result page re-runs the comparison.
    fn fill_source(&mut self, side: SideIx, source: SideSource, cx: &mut Context<Self>) {
        let expand = crate::views::settings::settings(cx).expand_json_strings;
        let mut source = source;
        source.parsed = source.parsed.with_string_expansion(expand);
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

impl SideEditor {
    fn new(title: SharedString, pretty: String) -> Self {
        let line_ranges = text_line_ranges(&pretty);
        Self {
            title,
            pretty,
            line_ranges,
            marks: Vec::new(),
            editor: None,
            gutter: None,
            active_range: None,
            options: None,
        }
    }
}

/// UTF-8 byte ranges, excluding LF/CRLF terminators. Decorations must use byte
/// offsets, not character counts; in particular JSON keys may contain Unicode.
fn text_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut offset = 0;
    text.split_inclusive('\n')
        .map(|line| {
            let content = line.strip_suffix('\n').unwrap_or(line);
            let content = if line.ends_with('\n') {
                content.strip_suffix('\r').unwrap_or(content)
            } else {
                content
            };
            let range = offset..offset + content.len();
            offset += line.len();
            range
        })
        .collect()
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
            string_expansion: None,
            ignored_fields: BTreeSet::new(),
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
        self.refresh_string_expansion(cx);
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
        if res.entries.get(ix).is_none() {
            return;
        }
        res.current = Some(ix);
        res.reveal_current = true;
        res.scroll_entries
            .scroll_to_item(ix, ScrollStrategy::Center);
        cx.notify();
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;

    #[test]
    fn line_decorations_use_utf8_byte_ranges() {
        let text = "{\n  \"名字\": \"你好\",\n  \"emoji\": \"😀\"\n}";
        let ranges = text_line_ranges(text);
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0], 0..1);
        assert_eq!(&text[ranges[1].clone()], "  \"名字\": \"你好\",");
        assert_eq!(&text[ranges[2].clone()], "  \"emoji\": \"😀\"");
        assert_eq!(&text[ranges[3].clone()], "}");
        assert!(
            ranges
                .iter()
                .all(|r| text.is_char_boundary(r.start) && text.is_char_boundary(r.end))
        );
    }

    #[test]
    fn line_ranges_handle_empty_lines_and_crlf() {
        let text = "a\r\n\r\nb\n";
        assert_eq!(text_line_ranges(text), vec![0..1, 3..3, 5..6]);
        assert!(text_line_ranges("").is_empty());
        assert_eq!(text_line_ranges("null"), vec![0..4]);
    }

    #[test]
    fn result_editor_text_matches_semantic_anchors() {
        let a = Arc::new(JsonTree::parse("{\"名字\":\"旧\",\"gone\":1}").unwrap());
        let b = Arc::new(JsonTree::parse("{\"名字\":\"新\",\"added\":2}").unwrap());
        let out = diff_values(&a.root, &b.root);
        let expected = [out.pretty_a.clone(), out.pretty_b.clone()];
        let source = |title: &str, parsed: Arc<JsonTree>| SideSource {
            kind: SourceKind::Manual,
            title: title.to_string().into(),
            url: None,
            text: parsed.pretty.clone(),
            parsed,
        };
        let result = ResultPhase::from_output([source("A", a), source("B", b)], 24.0, out);
        assert_eq!(result.entries.len(), 3);
        for (ix, side) in result.sides.iter().enumerate() {
            assert_eq!(side.pretty, expected[ix]);
            assert!(side.editor.is_none());
            for entry in &result.entries {
                if let Some(line) = if ix == SIDE_A {
                    entry.a_line
                } else {
                    entry.b_line
                } {
                    assert!(side.marks.contains(&(line, entry.kind)));
                    let range = &side.line_ranges[line];
                    assert!(!side.pretty[range.clone()].is_empty());
                }
            }
        }
        assert!(!result.reveal_current);
    }
}
