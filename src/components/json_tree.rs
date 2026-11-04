use crate::json_model::{FlatNode, JsonTree, JsonType};
use crate::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::{
    ScrollBounce,
    actions::{Confirm, SelectDown, SelectLeft, SelectRight, SelectUp},
};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, h_flex,
    list::ListItem,
    menu::{ContextMenuExt as _, PopupMenu},
    scroll::ScrollableElement as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, FocusHandle, FontWeight, InteractiveElement as _, IntoElement, ListState,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, list, px,
};
use std::{collections::HashSet, ops::Range, rc::Rc, sync::Arc};

const INDENT_PX: f32 = 16.0;

pub const TREE_CONTEXT: &str = "Tree";
pub const LIST_OVERDRAW_PX: f32 = 400.0;
pub const ROW_PADDING_PX: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub node_id: usize,
    pub depth: usize,
    pub expanded: bool,
}

pub fn tree_list_splice<T: Eq>(old: &[T], new: &[T]) -> (Range<usize>, usize) {
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(old, new)| old == new)
        .count();
    let max_suffix = old.len().min(new.len()).saturating_sub(prefix);
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(old, new)| old == new)
        .count();
    (prefix..old.len() - suffix, new.len() - prefix - suffix)
}

pub fn node_label(doc: &JsonTree, id: usize) -> SharedString {
    let n = &doc.nodes[id];
    match (&n.key, &n.summary) {
        (Some(k), _) => format!("{k}: {}", n.summary).into(),
        (None, summary) => summary.clone().into(),
    }
}

fn tree_child_ids(doc: &JsonTree, id: usize, sort_keys: bool) -> Vec<usize> {
    let node = &doc.nodes[id];
    let mut children = node.children.clone();
    if sort_keys && node.typ == JsonType::Object {
        children.sort_by(|a, b| doc.nodes[*a].key.cmp(&doc.nodes[*b].key));
    }
    children
}

fn node_matches(node: &FlatNode, q: &str) -> bool {
    node.key
        .as_deref()
        .is_some_and(|key| contains_folded(key, q))
        || contains_folded(&node.summary, q)
}

fn contains_folded(text: &str, lowercase_query: &str) -> bool {
    if text.is_ascii() && lowercase_query.is_ascii() {
        return text
            .as_bytes()
            .windows(lowercase_query.len())
            .any(|window| window.eq_ignore_ascii_case(lowercase_query.as_bytes()));
    }
    text.to_lowercase().contains(lowercase_query)
}

pub fn build_tree_rows(
    doc: &JsonTree,
    query: Option<&str>,
    expanded: &HashSet<usize>,
    sort_keys: bool,
) -> (Vec<TreeRow>, usize) {
    if doc.nodes.is_empty() {
        return (Vec::new(), 0);
    }

    let (included, hits) = if let Some(query) = query {
        let matched = doc
            .nodes
            .iter()
            .map(|node| node_matches(node, query))
            .collect::<Vec<_>>();
        let hits = matched.iter().filter(|&&matched| matched).count();
        let mut included = matched;
        for id in (1..doc.nodes.len()).rev() {
            if included[id]
                && let Some(parent) = doc.nodes[id].parent
            {
                included[parent] = true;
            }
        }
        (Some(included), hits)
    } else {
        (None, 0)
    };

    if included.as_ref().is_some_and(|included| !included[0]) {
        return (Vec::new(), 0);
    }

    let filtered = included.is_some();
    let mut rows = Vec::new();
    let mut stack = vec![(0, 0)];
    while let Some((node_id, depth)) = stack.pop() {
        if included.as_ref().is_some_and(|included| !included[node_id]) {
            continue;
        }
        let node = &doc.nodes[node_id];
        let is_expanded = !node.children.is_empty() && (filtered || expanded.contains(&node_id));
        rows.push(TreeRow {
            node_id,
            depth,
            expanded: is_expanded,
        });
        if is_expanded {
            let children = tree_child_ids(doc, node_id, sort_keys);
            stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        }
    }
    (rows, hits)
}

/// Icon + color for each JSON type. The mode keeps the light/dark choice named
/// instead of passing raw booleans through render code.
pub fn type_icon(t: JsonType) -> IconName {
    match t {
        JsonType::Object => IconName::Braces,
        JsonType::Array => IconName::Brackets,
        JsonType::String => IconName::Quote,
        JsonType::Number => IconName::Hash,
        JsonType::Bool => IconName::ToggleRight,
        JsonType::Null => IconName::CircleMinus,
    }
}

pub fn type_color(t: JsonType, mode: crate::syntax::SyntaxMode) -> gpui_kit::Rgba {
    use crate::syntax::SyntaxMode::{Dark, Light};
    gpui_kit::rgb(match (t, mode) {
        (JsonType::Object, Dark) => 0x7AA2F7,
        (JsonType::Object, Light) => 0x0550AE,
        (JsonType::Array, Dark) => 0xE0AF68,
        (JsonType::Array, Light) => 0x953800,
        (JsonType::String, Dark) => 0x9ECE6A,
        (JsonType::String, Light) => 0x116329,
        (JsonType::Number, Dark) => 0x7DCFFF,
        (JsonType::Number, Light) => 0x0969DA,
        (JsonType::Bool, Dark) => 0xBB9AF7,
        (JsonType::Bool, Light) => 0x8250DF,
        (JsonType::Null, Dark) => 0x737CA5,
        (JsonType::Null, Light) => 0x59636E,
    })
}

pub type TreeNodeActivateHandler = Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>;
pub type TreeActionHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;
pub type TreeContextMenuHandler =
    Rc<dyn Fn(usize, PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static>;

pub struct JsonTreeViewProps {
    pub doc: Arc<JsonTree>,
    pub rows: Rc<Vec<TreeRow>>,
    pub selected_node: Option<usize>,
    pub list_state: ListState,
    pub focus_handle: FocusHandle,
    pub empty_message: SharedString,
    pub on_activate: Option<TreeNodeActivateHandler>,
    pub on_context_menu: Option<TreeContextMenuHandler>,
    pub on_select_up: Option<TreeActionHandler>,
    pub on_select_down: Option<TreeActionHandler>,
    pub on_select_left: Option<TreeActionHandler>,
    pub on_select_right: Option<TreeActionHandler>,
    pub on_confirm: Option<TreeActionHandler>,
}

pub struct JsonTreeView {
    props: JsonTreeViewProps,
}

impl JsonTreeView {
    pub fn new(props: JsonTreeViewProps) -> Self {
        Self { props }
    }
}

impl RenderOnce for JsonTreeView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let props = self.props;
        if props.rows.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(props.empty_message)
                .into_any_element();
        }

        let list_state = props.list_state.clone();
        let rows = props.rows.clone();
        let doc = props.doc.clone();
        let selected_node = props.selected_node;
        let on_activate = props.on_activate.clone();
        let on_context_menu = props.on_context_menu.clone();
        let row_list = list(list_state.clone(), move |ix, window, cx| {
            let Some(row) = rows.get(ix).copied() else {
                return div().into_any_element();
            };
            let label = node_label(&doc, row.node_id);
            let folder = !doc.nodes[row.node_id].children.is_empty();

            let mut item = div()
                .id(("tree-row", ix))
                .role(Role::TreeItem)
                .aria_label(label)
                .aria_selected(selected_node == Some(row.node_id))
                .when(folder, |this| this.aria_expanded(row.expanded))
                .child(
                    JsonTreeRowView::new(JsonTreeRowViewProps {
                        ix,
                        row,
                        doc: doc.clone(),
                        selected_node,
                    })
                    .render(window, cx),
                );
            if let Some(on_activate) = on_activate.clone() {
                item = item.on_click(move |_, window, cx| {
                    on_activate(row.node_id, window, cx);
                });
            }
            if let Some(on_context_menu) = on_context_menu.clone() {
                return item
                    .context_menu(move |menu, window, cx| {
                        on_context_menu(row.node_id, menu, window, cx)
                    })
                    .into_any_element();
            }
            item.into_any_element()
        })
        .size_full()
        .min_h_0();

        let mut tree = div()
            .id("tree-list")
            .role(Role::Tree)
            .size_full()
            .min_h_0()
            .key_context(TREE_CONTEXT)
            .track_focus(&props.focus_handle);
        if let Some(on_select_up) = props.on_select_up {
            tree = tree.on_action(move |_: &SelectUp, window, cx| on_select_up(window, cx));
        }
        if let Some(on_select_down) = props.on_select_down {
            tree = tree.on_action(move |_: &SelectDown, window, cx| on_select_down(window, cx));
        }
        if let Some(on_select_left) = props.on_select_left {
            tree = tree.on_action(move |_: &SelectLeft, window, cx| on_select_left(window, cx));
        }
        if let Some(on_select_right) = props.on_select_right {
            tree = tree.on_action(move |_: &SelectRight, window, cx| on_select_right(window, cx));
        }
        if let Some(on_confirm) = props.on_confirm {
            tree = tree.on_action(move |_: &Confirm, window, cx| on_confirm(window, cx));
        }

        tree.child(ScrollBounce::new(
            "tree-scroll-bounce",
            &list_state,
            row_list,
        ))
        .vertical_scrollbar(&list_state)
        .into_any_element()
    }
}

struct JsonTreeRowViewProps {
    pub ix: usize,
    pub row: TreeRow,
    pub doc: Arc<JsonTree>,
    pub selected_node: Option<usize>,
}

struct JsonTreeRowView {
    props: JsonTreeRowViewProps,
}

impl JsonTreeRowView {
    fn new(props: JsonTreeRowViewProps) -> Self {
        Self { props }
    }
}

impl RenderOnce for JsonTreeRowView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let props = self.props;
        let theme = cx.theme();
        let node_id = props.row.node_id;
        let node = &props.doc.nodes[node_id];
        let color = type_color(node.typ, crate::syntax::SyntaxMode::current(cx));
        // The tree shows JSON text too, so it uses the same mono font setting.
        let settings = crate::views::settings::settings(cx);
        let mono = settings.mono_family(cx);
        let json_font_size = settings.json_font();
        let row_height = settings.row_height();

        let mut label = h_flex()
            .w_full()
            .items_start()
            .gap_1p5()
            .min_w_0()
            .text_size(px(json_font_size))
            .line_height(px(row_height))
            .font_family(mono);
        label = label.child(
            div()
                .h(px(row_height))
                .flex_shrink_0()
                .flex()
                .items_center()
                .child(Icon::new(type_icon(node.typ)).small().text_color(color)),
        );
        if let Some(key) = &node.key {
            label = label.child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .child(key.clone()),
            );
            label = label.child(
                div()
                    .whitespace_nowrap()
                    .text_color(theme.muted_foreground)
                    .child(":"),
            );
        } else if let Some(i) = node.index {
            label = label.child(
                div()
                    .whitespace_nowrap()
                    .text_color(theme.muted_foreground)
                    .child(format!("[{i}]")),
            );
        }
        label = label.child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_normal()
                .text_color(color)
                .child(node.summary.clone()),
        );
        if let Some(hint) = timestamp_hint(&props.doc, node_id) {
            label = label.child(
                div()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_size(px((json_font_size - 2.0).max(9.0)))
                    .text_color(theme.muted_foreground)
                    .child(format!("· {hint}")),
            );
        }
        if let Some(kind) = child_count_unit(node.typ, node.children.len()) {
            label = label.child(
                div()
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_size(px((json_font_size - 2.0).max(9.0)))
                    .text_color(theme.muted_foreground)
                    .child(format!("· {kind}")),
            );
        }

        ListItem::new(props.ix)
            .selected(props.selected_node == Some(node_id))
            .w_full()
            .rounded_sm()
            .pl(px(12. + props.row.depth as f32 * INDENT_PX))
            .pr_2()
            .items_start()
            .child(label)
    }
}

/// The muted "· N keys / · N items" suffix on a container row, or `None` when
/// the node has no children or isn't one of the two containers.
///
/// English inflects the noun for one, Chinese does not, so the count picks the
/// key rather than the language picking a plural rule.
fn child_count_unit(typ: JsonType, count: usize) -> Option<SharedString> {
    let count_str = count.to_string();
    match typ {
        JsonType::Object if count == 1 => Some(tr!("1 key")),
        JsonType::Object => Some(tr!("%{count} keys", count = count_str)),
        JsonType::Array if count == 1 => Some(tr!("1 item")),
        JsonType::Array => Some(tr!("%{count} items", count = count_str)),
        _ => None,
    }
}

fn timestamp_hint(doc: &JsonTree, id: usize) -> Option<String> {
    let node = doc.nodes.get(id)?;
    if !matches!(node.typ, JsonType::Number | JsonType::String)
        || !key_suggests_time(node.key.as_deref()?)
    {
        return None;
    }

    let (seconds, nanos) = epoch_parts_from_value(doc.resolve_ref(id)?)?;
    format_unix_utc(seconds, nanos)
}

fn key_suggests_time(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("time")
        || lower.contains("epoch")
        || lower.contains("date")
        || lower.ends_with("_at")
        || lower.ends_with("-at")
        || lower.ends_with("at")
            && [
                "created", "updated", "deleted", "expired", "expires", "started", "ended",
            ]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
}

fn epoch_parts_from_value(value: &serde_json::Value) -> Option<(i64, u32)> {
    match value {
        serde_json::Value::Number(number) => epoch_parts_from_number(number),
        serde_json::Value::String(text) => epoch_parts_from_str(text),
        _ => None,
    }
}

fn epoch_parts_from_number(number: &serde_json::Number) -> Option<(i64, u32)> {
    if let Some(value) = number.as_i64() {
        return epoch_parts_from_int(value as i128);
    }
    if let Some(value) = number.as_u64() {
        return epoch_parts_from_int(i128::from(value));
    }
    let value = number.as_f64()?;
    if !value.is_finite() {
        return None;
    }
    epoch_parts_from_f64(value)
}

fn epoch_parts_from_str(text: &str) -> Option<(i64, u32)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse::<i128>().ok().and_then(epoch_parts_from_int);
    }
    if let Some(parts) = epoch_parts_from_decimal_str(text) {
        return Some(parts);
    }
    if text.contains('.') {
        return text.parse::<f64>().ok().and_then(epoch_parts_from_f64);
    }
    None
}

fn epoch_parts_from_decimal_str(text: &str) -> Option<(i64, u32)> {
    let (seconds, fraction) = text.split_once('.')?;
    let unsigned_seconds = seconds.strip_prefix(['+', '-']).unwrap_or(seconds);
    if unsigned_seconds.is_empty()
        || fraction.is_empty()
        || !unsigned_seconds.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }

    let seconds = seconds.parse::<i128>().ok()?;
    if !plausible_epoch_seconds(seconds) {
        return None;
    }
    let mut nanos = 0_u32;
    let mut scale = 100_000_000_u32;
    for byte in fraction.bytes().take(9) {
        nanos += u32::from(byte - b'0') * scale;
        scale /= 10;
    }
    Some((seconds as i64, nanos))
}

fn epoch_parts_from_int(value: i128) -> Option<(i64, u32)> {
    const UNITS: [i128; 4] = [1, 1_000, 1_000_000, 1_000_000_000];
    for unit in UNITS {
        let seconds = value / unit;
        let fraction = value.rem_euclid(unit);
        if plausible_epoch_seconds(seconds) {
            let nanos = ((fraction * 1_000_000_000) / unit) as u32;
            return Some((seconds as i64, nanos));
        }
    }
    None
}

fn epoch_parts_from_f64(value: f64) -> Option<(i64, u32)> {
    if !plausible_epoch_seconds(value as i128) {
        return None;
    }
    let seconds = value.trunc() as i64;
    let nanos = ((value.fract().abs() * 1_000_000_000.0).round() as u32).min(999_999_999);
    Some((seconds, nanos))
}

fn plausible_epoch_seconds(seconds: i128) -> bool {
    // 2000-01-01 through 2100-01-01 keeps ordinary durations like
    // `timeout: 3000` from rendering as timestamps while covering app data.
    (946_684_800..=4_102_444_800).contains(&seconds)
}

fn format_unix_utc(seconds: i64, nanos: u32) -> Option<String> {
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days)?;
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    if nanos == 0 {
        Some(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC"
        ))
    } else {
        let millis = nanos / 1_000_000;
        Some(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{millis:03} UTC"
        ))
    }
}

fn civil_from_days(days_since_epoch: i64) -> Option<(i32, u32, u32)> {
    let z = days_since_epoch.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    Some((i32::try_from(year).ok()?, month as u32, day as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_containers(doc: &JsonTree) -> HashSet<usize> {
        doc.nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| (!node.children.is_empty()).then_some(id))
            .collect()
    }

    #[test]
    fn tree_sorts_object_keys_and_preserves_array_indices() {
        let doc = JsonTree::parse(r#"{"z":0,"items":[{"b":2,"a":1},3],"a":1}"#);
        let (rows, _) = build_tree_rows(&doc, None, &all_containers(&doc), true);

        let root_keys = rows
            .iter()
            .map(|row| &doc.nodes[row.node_id])
            .filter(|node| node.parent == Some(0))
            .map(|node| node.key.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(root_keys, ["a", "items", "z"]);

        let array_id = doc
            .nodes
            .iter()
            .position(|node| node.key.as_deref() == Some("items"))
            .unwrap();
        let indices = rows
            .iter()
            .map(|row| &doc.nodes[row.node_id])
            .filter(|node| node.parent == Some(array_id))
            .map(|node| node.index.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(indices, [0, 1]);

        let nested_id = doc.nodes[array_id].children[0];
        let nested_keys = rows
            .iter()
            .map(|row| &doc.nodes[row.node_id])
            .filter(|node| node.parent == Some(nested_id))
            .map(|node| node.key.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(nested_keys, ["a", "b"]);
    }

    #[test]
    fn tree_preserves_object_order_when_sorting_is_disabled() {
        let doc = JsonTree::parse(r#"{"z":0,"a":1}"#);
        let (rows, _) = build_tree_rows(&doc, None, &all_containers(&doc), false);
        let root_keys = rows
            .iter()
            .map(|row| &doc.nodes[row.node_id])
            .filter(|node| node.parent == Some(0))
            .map(|node| node.key.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(root_keys, ["z", "a"]);
    }

    #[test]
    fn collapsed_tree_keeps_root_and_first_level_visible() {
        let doc = JsonTree::parse(r#"{"a":1,"nested":{"b":2}}"#);
        let expanded = HashSet::from([0]);
        let (rows, _) = build_tree_rows(&doc, None, &expanded, true);
        let ids = rows.iter().map(|row| row.node_id).collect::<Vec<_>>();
        assert_eq!(ids, [0, 1, 2]);
    }

    #[test]
    fn tree_filter_keeps_matching_ancestors() {
        let doc = JsonTree::parse(r#"{"a":1,"nested":{"target":"value","other":2}}"#);
        let expanded = HashSet::from([0]);
        let (rows, hits) = build_tree_rows(&doc, Some("target"), &expanded, true);
        let keys = rows
            .iter()
            .filter_map(|row| doc.nodes[row.node_id].key.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(hits, 1);
        assert_eq!(keys, ["nested", "target"]);
    }

    #[test]
    fn timestamp_hint_formats_epoch_units() {
        let doc = JsonTree::parse(
            r#"{
                "created_time": 1700000000,
                "updated_epoch": 1700000000000,
                "sent_timestamp": 1700000000000000,
                "received_date": 1700000000000000000
            }"#,
        );

        let hint_for = |key: &str| {
            let id = doc
                .nodes
                .iter()
                .position(|node| node.key.as_deref() == Some(key))
                .unwrap();
            timestamp_hint(&doc, id)
        };

        assert_eq!(
            hint_for("created_time").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for("updated_epoch").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for("sent_timestamp").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for("received_date").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
    }

    #[test]
    fn timestamp_hint_formats_string_epoch_values() {
        let doc = JsonTree::parse(
            r#"{
                "created_time": "1700000000",
                "updated_epoch": "1700000000000",
                "sent_timestamp": "1700000000.123",
                "bad_time": "not a timestamp"
            }"#,
        );

        let hint_for = |key: &str| {
            let id = doc
                .nodes
                .iter()
                .position(|node| node.key.as_deref() == Some(key))
                .unwrap();
            timestamp_hint(&doc, id)
        };

        assert_eq!(
            hint_for("created_time").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for("updated_epoch").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for("sent_timestamp").as_deref(),
            Some("2023-11-14 22:13:20.123 UTC")
        );
        assert_eq!(hint_for("bad_time"), None);
    }

    #[test]
    fn timestamp_hint_ignores_non_epoch_durations() {
        let doc = JsonTree::parse(r#"{"timeout": 3000, "count": 1700000000}"#);
        let hint_for = |key: &str| {
            let id = doc
                .nodes
                .iter()
                .position(|node| node.key.as_deref() == Some(key))
                .unwrap();
            timestamp_hint(&doc, id)
        };

        assert_eq!(hint_for("timeout"), None);
        assert_eq!(hint_for("count"), None);
    }

    #[test]
    fn tree_list_splice_only_replaces_changed_rows() {
        assert_eq!(tree_list_splice(&[0, 1, 2], &[0, 1, 3, 4, 2]), (2..2, 2));
        assert_eq!(tree_list_splice(&[0, 1, 3, 4, 2], &[0, 1, 2]), (2..4, 0));
        assert_eq!(tree_list_splice(&[0, 1, 2], &[0, 2, 1]), (1..3, 2));
    }
}
