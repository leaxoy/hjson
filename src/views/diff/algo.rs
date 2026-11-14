//! Semantic JSON diff aligned by JSONPath.
//!
//! Differences come from `json_patch::diff` (an RFC 6902 patch between the two
//! documents); this module translates that patch into the entries the diff page
//! renders, keeping its own contract:
//!
//! - Each side is printed by [`crate::json_model::pretty`], the same printer the viewer's
//!   raw panel uses, so the recorded path → line anchors drive the per-side
//!   jump targets (and gutter markers) of the diff page.
//! - Object key order is ignored: JSON Patch compares maps by content.
//! - `1` and `1.0` are the same number: the patch reports that one change, we
//!   drop it — the only correction it needs.
//! - Array removals are reported at their original indices: a patch written
//!   against the mutating document reuses one index for a run of tail removals,
//!   so we count them per array to give each its real position back.

use crate::tr;
use json_patch::PatchOperation;
use json_patch::jsonptr::Pointer;
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
};

use crate::json_model::pretty::{self, key_seg};

/// Kind of a single difference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffKind {
    /// Present in B only.
    Added,
    /// Present in A only.
    Removed,
    /// Same path, same type, different value.
    Modified,
    /// Same path, different type.
    TypeChanged,
}

impl DiffKind {
    /// Gutter glyph drawn at the anchored lines.
    pub(super) fn glyph(self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Removed => "−",
            Self::Modified => "~",
            Self::TypeChanged => "≈",
        }
    }

    pub(super) fn label(self) -> gpui_kit::SharedString {
        match self {
            Self::Added => tr!("Added"),
            Self::Removed => tr!("Removed"),
            Self::Modified => tr!("Modified"),
            Self::TypeChanged => tr!("Type changed"),
        }
    }
}

/// One difference between the two documents. Subtrees count as a single
/// entry, anchored at the first line of the subtree on each side.
#[derive(Clone, Debug)]
pub(super) struct DiffEntry {
    pub(super) kind: DiffKind,
    /// JSONPath of the differing node, e.g. `$.items[3].age`.
    pub(super) path: String,
    /// 0-based line in A's pretty text (`None` for Added).
    pub(super) a_line: Option<usize>,
    /// 0-based line in B's pretty text (`None` for Removed).
    pub(super) b_line: Option<usize>,
    /// Compact JSON of the value on A (`""` for Added).
    pub(super) old: String,
    /// Compact JSON of the value on B (`""` for Removed).
    pub(super) new: String,
}

impl DiffEntry {
    /// Navigation key: smallest line the entry occupies anywhere.
    fn order_key(&self) -> usize {
        self.a_line.or(self.b_line).unwrap_or(usize::MAX)
    }
}

/// Whether both sides hold the same JSON type — the `Value` variant.
fn same_type(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// Our JSONPath for a patch pointer, plus the value it addresses in `root`.
///
/// A numeric token is an index only where `root` holds an array, so an object
/// key like `"0"` stays a key. `shift` restores the real index of the last
/// token: a patch written against the mutating document points at where an
/// element sits *after* the removals that already applied.
fn locate<'a>(root: &'a Value, ptr: &Pointer, shift: usize) -> (String, Option<&'a Value>) {
    let tokens: Vec<_> = ptr.tokens().collect();
    let mut path = String::from("$");
    let mut cur = Some(root);
    for (depth, token) in tokens.iter().enumerate() {
        let key = token.decoded();
        let is_last = depth + 1 == tokens.len();
        cur = match key
            .parse::<usize>()
            .ok()
            .filter(|_| matches!(cur, Some(&Value::Array(_))))
        {
            Some(raw) => {
                let ix = if is_last { raw + shift } else { raw };
                path.push_str(&format!("[{ix}]"));
                cur.and_then(Value::as_array)
                    .and_then(|items| items.get(ix))
            }
            None => {
                path.push_str(&key_seg(&key));
                match cur {
                    Some(Value::Object(map)) => map.get(key.as_ref()),
                    _ => None,
                }
            }
        };
    }
    (path, cur)
}

/// Result of comparing two documents.
pub(super) struct DiffOutput {
    pub(super) pretty_a: String,
    pub(super) pretty_b: String,
    pub(super) entries: Vec<DiffEntry>,
}

/// Compare two JSON texts. Caller handles parse errors first (the config page
/// validates per side), so this assumes both parse.
#[cfg(test)]
fn diff(a_text: &str, b_text: &str) -> DiffOutput {
    let a: Value = serde_json::from_str(a_text.trim()).unwrap_or(Value::Null);
    let b: Value = serde_json::from_str(b_text.trim()).unwrap_or(Value::Null);
    diff_values(&a, &b)
}

/// Compare two values without field exemptions.
#[cfg(test)]
pub(super) fn diff_values(a: &Value, b: &Value) -> DiffOutput {
    diff_values_with_ignored(a, b, &BTreeSet::new())
}

/// Remove matching object properties at every depth, including inside arrays.
/// Keep non-exempt container structure and array indices intact. Only these
/// comparison copies are changed; display text and anchors use the originals.
fn without_ignored_fields(value: &Value, ignored: &BTreeSet<String>) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !ignored.contains(*key))
                .map(|(key, value)| (key.clone(), without_ignored_fields(value, ignored)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|value| without_ignored_fields(value, ignored))
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub(super) fn diff_values_with_ignored(
    a: &Value,
    b: &Value,
    ignored: &BTreeSet<String>,
) -> DiffOutput {
    let side_a = pretty::document(a);
    let side_b = pretty::document(b);
    let a = if ignored.is_empty() {
        Cow::Borrowed(a)
    } else {
        Cow::Owned(without_ignored_fields(a, ignored))
    };
    let b = if ignored.is_empty() {
        Cow::Borrowed(b)
    } else {
        Cow::Owned(without_ignored_fields(b, ignored))
    };
    let (a, b) = (a.as_ref(), b.as_ref());

    let mut entries: Vec<DiffEntry> = Vec::new();
    let mut record = |kind: DiffKind, path: String, old: Option<&Value>, new: Option<&Value>| {
        entries.push(DiffEntry {
            kind,
            a_line: side_a.anchors.get(&path).copied(),
            b_line: side_b.anchors.get(&path).copied(),
            old: old.map(ToString::to_string).unwrap_or_default(),
            new: new.map(ToString::to_string).unwrap_or_default(),
            path,
        });
    };

    // Removals already recorded, per array, in the patch's own pointer space.
    let mut removals: HashMap<String, usize> = HashMap::new();
    for op in json_patch::diff(a, b).0 {
        let ptr = op.path().to_buf();
        let array_key = ptr.parent().unwrap_or(&ptr).to_string();
        let shift = *removals.get(&array_key).unwrap_or(&0);
        match op {
            PatchOperation::Add(add) => {
                let (path, value) = locate(b, &add.path, 0);
                record(DiffKind::Added, path, None, value);
            }
            PatchOperation::Remove(remove) => {
                *removals.entry(array_key).or_default() += 1;
                let (path, value) = locate(a, &remove.path, shift);
                record(DiffKind::Removed, path, value, None);
            }
            PatchOperation::Replace(replace) => {
                let (path, new_value) = locate(b, &replace.path, shift);
                let Some(new_value) = new_value else { continue };
                let old_value = locate(a, &replace.path, shift).1;
                // The one correction the patch needs: it compares `1` and
                // `1.0` strictly, which is not a change for us.
                let same_number = matches!(
                    (old_value, new_value),
                    (Some(Value::Number(x)), Value::Number(y)) if x.as_f64() == y.as_f64()
                );
                if same_number {
                    continue;
                }
                let kind = if old_value.is_some_and(|old| same_type(old, new_value)) {
                    DiffKind::Modified
                } else {
                    DiffKind::TypeChanged
                };
                record(kind, path, old_value, Some(new_value));
            }
            // diff() only ever emits add, remove and replace.
            PatchOperation::Move(_) | PatchOperation::Copy(_) | PatchOperation::Test(_) => {}
        }
    }
    // Navigation order: by the smallest line the entry occupies; the stable
    // sort keeps patch order for equal keys.
    entries.sort_by_key(|e| e.order_key());

    DiffOutput {
        pretty_a: side_a.text,
        pretty_b: side_b.text,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff_ignored(a: Value, b: Value, names: &[&str]) -> DiffOutput {
        let ignored = names.iter().map(|name| name.to_string()).collect();
        diff_values_with_ignored(&a, &b, &ignored)
    }

    #[test]
    fn exemptions_match_fields_at_all_depths() {
        let a = serde_json::json!({"root":{"key1":1,"key2":{"key1":{"child":1},"keep":1}}});
        let b = serde_json::json!({"root":{"key1":999,"key2":{"key1":false,"keep":2}}});
        let out = diff_ignored(a.clone(), b.clone(), &["key1"]);
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, "$.root.key2.keep");
        assert_eq!(out.pretty_a, pretty::document(&a).text);
        assert_eq!(out.pretty_b, pretty::document(&b).text);
        for (text, line) in [
            (&out.pretty_a, out.entries[0].a_line),
            (&out.pretty_b, out.entries[0].b_line),
        ] {
            assert!(
                text.lines()
                    .nth(line.unwrap())
                    .unwrap()
                    .contains("\"keep\"")
            );
        }
    }

    #[test]
    fn exemptions_ignore_add_remove_and_entire_subtree() {
        let out = diff_ignored(
            serde_json::json!({"key1":{"changed":[1,2]},"nested":{"key1":1}}),
            serde_json::json!({"nested":{},"other":{"key1":123}}),
            &["key1"],
        );
        // A non-exempt parent is still structural information even if empty.
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, "$.other");
        assert_eq!(out.entries[0].new, "{}");
        let out = diff_ignored(
            serde_json::json!({"key1":1}),
            serde_json::json!({}),
            &["key1"],
        );
        assert!(out.entries.is_empty());
    }

    #[test]
    fn exemptions_recurse_into_arrays_without_changing_indices() {
        let out = diff_ignored(
            serde_json::json!({"list":[{"key1":1,"keep":0},{"key1":2,"keep":1}]}),
            serde_json::json!({"list":[{"key1":9,"keep":0},{"keep":2}]}),
            &["key1"],
        );
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, "$.list[1].keep");
        let out = diff_ignored(
            serde_json::json!([{"key1":1},{"key1":2}]),
            serde_json::json!([{"key1":3}]),
            &["key1"],
        );
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, "$[1]");
        assert_eq!(out.entries[0].kind, DiffKind::Removed);
    }

    #[test]
    fn exemptions_are_exact_case_sensitive_and_not_paths_or_values() {
        let out = diff_ignored(
            serde_json::json!({"key1":1,"Key1":1,"key10":1,"literal":"key1","0":1,"items":[1]}),
            serde_json::json!({"key1":2,"Key1":2,"key10":2,"literal":"other","0":2,"items":[2]}),
            &["key1", "0", "$.Key1"],
        );
        assert_eq!(out.entries.len(), 4);
        assert!(out.entries.iter().any(|e| e.path == "$.Key1"));
        assert!(out.entries.iter().any(|e| e.path == "$.key10"));
        assert!(out.entries.iter().any(|e| e.path == "$.literal"));
        assert!(out.entries.iter().any(|e| e.path == "$.items[0]"));
    }

    #[test]
    fn exemptions_support_unicode_escaped_and_multiple_names() {
        let a = serde_json::json!({"名字":1,"nested":{"a/b":1,"a~b":1,"key1":1,"keep":1}});
        let b = serde_json::json!({"名字":2,"nested":{"a/b":2,"a~b":2,"key1":2,"keep":1}});
        assert!(
            diff_ignored(a.clone(), b.clone(), &["名字", "a/b", "a~b", "key1"])
                .entries
                .is_empty()
        );
        assert_eq!(diff_ignored(a, b, &[]).entries.len(), 4);
    }

    #[test]
    fn ancestor_changes_never_include_exempt_values_in_details() {
        let out = diff_ignored(
            serde_json::json!({"root":{"key1":"hidden","keep":1}}),
            serde_json::json!({"root":[]}),
            &["key1"],
        );
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].kind, DiffKind::TypeChanged);
        assert_eq!(out.entries[0].old, "{\"keep\":1}");
        assert!(out.pretty_a.contains("hidden"));
    }

    fn kind_of(path: &str, entries: &[DiffEntry]) -> Option<DiffKind> {
        entries.iter().find(|e| e.path == path).map(|e| e.kind)
    }

    #[test]
    fn scalar_changes() {
        let out = diff(r#"{"a":1,"b":"x"}"#, r#"{"a":2,"b":"x"}"#);
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].kind, DiffKind::Modified);
        assert_eq!(out.entries[0].path, "$.a");
        assert_eq!(out.entries[0].old, "1");
        assert_eq!(out.entries[0].new, "2");
        assert!(out.entries[0].a_line.is_some() && out.entries[0].b_line.is_some());
    }

    #[test]
    fn added_and_removed_keys() {
        let out = diff(r#"{"a":1}"#, r#"{"b":2}"#);
        assert_eq!(kind_of("$.a", &out.entries), Some(DiffKind::Removed));
        assert_eq!(kind_of("$.b", &out.entries), Some(DiffKind::Added));
        assert_eq!(out.entries.len(), 2);
    }

    #[test]
    fn numeric_one_equals_one_point_zero() {
        let out = diff(r#"{"n":1}"#, r#"{"n":1.0}"#);
        assert!(out.entries.is_empty());
    }

    #[test]
    fn numeric_equality_holds_deeply() {
        let out = diff(r#"{"n":{"l":[1,2.0]}}"#, r#"{"n":{"l":[1.0,2]}}"#);
        assert!(out.entries.is_empty(), "{:?}", out.entries);
    }

    #[test]
    fn object_key_order_ignored() {
        let out = diff(r#"{"a":1,"b":2}"#, r#"{"b":2,"a":1}"#);
        assert!(out.entries.is_empty());
    }

    #[test]
    fn type_changed() {
        let out = diff(r#"{"m":"7"}"#, r#"{"m":7}"#);
        assert_eq!(kind_of("$.m", &out.entries), Some(DiffKind::TypeChanged));
    }

    #[test]
    fn array_index_alignment() {
        // Inserting at the front shifts everything: [0] differs, the rest
        // line up with different values.
        let out = diff(r#"[1,2,3]"#, r#"[0,1,2,3]"#);
        assert_eq!(kind_of("$[0]", &out.entries), Some(DiffKind::Modified));
        assert_eq!(kind_of("$[1]", &out.entries), Some(DiffKind::Modified));
        assert_eq!(kind_of("$[2]", &out.entries), Some(DiffKind::Modified));
        assert_eq!(kind_of("$[3]", &out.entries), Some(DiffKind::Added));
        assert_eq!(out.entries.len(), 4);
    }

    #[test]
    fn array_tail_removals_keep_their_own_indices() {
        // The patch removes "/1" twice: once the first tail element is gone,
        // the next one has moved into its place.
        let out = diff(r#"["a","b","c"]"#, r#"["a"]"#);
        assert_eq!(out.entries.len(), 2);
        assert_eq!(kind_of("$[1]", &out.entries), Some(DiffKind::Removed));
        assert_eq!(kind_of("$[2]", &out.entries), Some(DiffKind::Removed));
        assert_eq!(
            out.entries.iter().find(|e| e.path == "$[1]").unwrap().old,
            "\"b\""
        );
        assert_eq!(
            out.entries.iter().find(|e| e.path == "$[2]").unwrap().old,
            "\"c\""
        );
    }

    #[test]
    fn key_removals_do_not_shift_array_indices() {
        // Object-key removals share the patch's pointer space with array
        // removals; they must not push an array's indices along.
        let out = diff(r#"{"a":1,"list":[10,11,12]}"#, r#"{"list":[10]}"#);
        assert_eq!(kind_of("$.a", &out.entries), Some(DiffKind::Removed));
        assert_eq!(kind_of("$.list[1]", &out.entries), Some(DiffKind::Removed));
        assert_eq!(kind_of("$.list[2]", &out.entries), Some(DiffKind::Removed));
        assert_eq!(
            out.entries
                .iter()
                .find(|e| e.path == "$.list[2]")
                .unwrap()
                .old,
            "12"
        );
    }

    #[test]
    fn removed_subtree_counts_once() {
        let out = diff(r#"{"u":{"n":1}}"#, r#"{}"#);
        assert_eq!(out.entries.len(), 1);
        assert_eq!(kind_of("$.u", &out.entries), Some(DiffKind::Removed));
        assert!(out.entries[0].old.contains("\"n\""));
        assert!(out.entries[0].b_line.is_none());
    }

    #[test]
    fn anchors_point_at_right_lines() {
        let a = r#"{"top":1,"u":{"n":2}}"#;
        let b = r#"{"top":1,"u":{"n":9}}"#;
        let out = diff(a, b);
        let e = &out.entries[0];
        // Line index inside the pretty text, 0-based; gutter labels are +1.
        let line = &out.pretty_a.lines().collect::<Vec<_>>()[e.a_line.unwrap()];
        assert!(
            line.contains("\"n\""),
            "anchor should hit the n line: {line}"
        );
        assert!(line.ends_with("\"n\": 2"), "{line}");
    }

    #[test]
    fn deep_paths_work() {
        let a = r#"{"o":{"k":[{"u":{"v":1}}]}}"#;
        let b = r#"{"o":{"k":[{"u":{"v":2}}]}}"#;
        let out = diff(a, b);
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, "$.o.k[0].u.v");
        assert_eq!(out.entries[0].kind, DiffKind::Modified);
    }

    #[test]
    fn nested_object_added_inside_array() {
        let out = diff(r#"{"list":[1,2]}"#, r#"{"list":[1,2,3]}"#);
        assert_eq!(kind_of("$.list[2]", &out.entries), Some(DiffKind::Added));
    }

    #[test]
    fn numeric_object_key_stays_a_key() {
        let out = diff(r#"{"0":1}"#, r#"{"0":2}"#);
        assert_eq!(out.entries.len(), 1);
        // Bracketed like the viewer spells non-identifier keys, never `[0]`.
        assert_eq!(out.entries[0].path, "$['0']");
    }

    #[test]
    fn root_type_change() {
        let out = diff(r#"[1]"#, r#"{"a":1}"#);
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, "$");
        assert_eq!(out.entries[0].kind, DiffKind::TypeChanged);
    }

    #[test]
    fn escaped_keys_round_trip() {
        let out = diff(r#"{"/a/b":1}"#, r#"{"/a/b":2}"#);
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, "$['/a/b']");
        assert!(out.entries[0].a_line.is_some());
    }
}
