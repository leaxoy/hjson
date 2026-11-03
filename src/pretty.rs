//! Pretty-printed JSON that knows where every node lives.
//!
//! The layout matches `serde_json`'s `to_string_pretty` (2-space indent), but
//! the walk also records, for each node, the line its value starts on. Because
//! the text and the map come from the same pass, a path found in
//! [`PrettyDoc::anchors`] always addresses a real line of [`PrettyDoc::text`].
//!
//! Paths are spelled the way `JsonTree::json_path` spells them — `$`, `.key`
//! for identifier-like keys, `['odd key']` otherwise, `[0]` for array items —
//! so the viewer's tree ids and the diff engine's patch pointers land in one
//! coordinate system.

use std::collections::HashMap;

use serde_json::Value;

use crate::json_model::{escape_single, is_simple_key};

pub struct PrettyDoc {
    pub text: String,
    /// JSONPath → 0-based line in `text`.
    pub anchors: HashMap<String, usize>,
}

/// One path segment for an object key: `.key` when simple, `['key']` otherwise.
pub fn key_seg(key: &str) -> String {
    if is_simple_key(key) {
        format!(".{key}")
    } else {
        format!("['{}']", escape_single(key))
    }
}

/// The JSON-quoted form of a key, e.g. `"age"` with escapes.
fn quoted_key(key: &str) -> String {
    Value::String(key.to_string()).to_string()
}

fn pad(indent: usize) -> String {
    "  ".repeat(indent)
}

/// Print `value` and map every node path to its line.
pub fn document(value: &Value) -> PrettyDoc {
    let mut lines: Vec<String> = Vec::new();
    let mut anchors: HashMap<String, usize> = HashMap::new();
    emit(String::new(), value, 0, "$", &mut lines, &mut anchors);
    PrettyDoc {
        text: lines.join("\n"),
        anchors,
    }
}

/// Print `v` into `lines`, recording its line in `anchors`. `head` is the
/// prefix already written on the value's line (`"  \"key\": "` for members).
fn emit(
    head: String,
    v: &Value,
    indent: usize,
    path: &str,
    lines: &mut Vec<String>,
    anchors: &mut HashMap<String, usize>,
) {
    anchors.insert(path.to_string(), lines.len());
    match v {
        Value::Object(map) if map.is_empty() => lines.push(format!("{head}{{}}")),
        Value::Array(items) if items.is_empty() => lines.push(format!("{head}[]")),
        Value::Object(map) => {
            lines.push(format!("{head}{{"));
            let ind = pad(indent);
            for (ix, (key, child)) in map.iter().enumerate() {
                let child_head = format!("{}{}: ", pad(indent + 1), quoted_key(key));
                let child_path = format!("{path}{}", key_seg(key));
                emit(child_head, child, indent + 1, &child_path, lines, anchors);
                comma(lines, ix + 1 < map.len());
            }
            lines.push(format!("{ind}}}"));
        }
        Value::Array(items) => {
            lines.push(format!("{head}["));
            let ind = pad(indent);
            for (ix, child) in items.iter().enumerate() {
                let child_head = pad(indent + 1);
                let child_path = format!("{path}[{ix}]");
                emit(child_head, child, indent + 1, &child_path, lines, anchors);
                comma(lines, ix + 1 < items.len());
            }
            lines.push(format!("{ind}]"));
        }
        scalar => lines.push(format!("{head}{scalar}")),
    }
}

/// Close a member or item with a comma when another one follows it.
fn comma(lines: &mut [String], more: bool) {
    if more && let Some(last) = lines.last_mut() {
        last.push(',');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole contract: what the viewer shows is byte-for-byte what
    /// `serde_json` would have shown, so switching printers is invisible.
    #[test]
    fn matches_serde_pretty() {
        for text in [
            r#"{"a":1,"b":[true,null,"x","/y~z"],"c":{},"d":[]}"#,
            r#"{"0":1,"":{"":2},"n":{"l":[{"e":[]},[{"deep":"→"}]]}}"#,
            r#"[1,2.5,{"a":"b"},[],"tail"]"#,
            "null",
        ] {
            let value: Value = serde_json::from_str(text).unwrap();
            assert_eq!(
                document(&value).text,
                serde_json::to_string_pretty(&value).unwrap()
            );
        }
    }

    #[test]
    fn anchors_point_at_the_value_line() {
        let value: Value = serde_json::from_str(r#"{"top":1,"u":{"n":2},"list":[10,11]}"#).unwrap();
        let doc = document(&value);
        let lines: Vec<&str> = doc.text.lines().collect();
        assert_eq!(lines[doc.anchors["$"]], "{");
        assert_eq!(lines[doc.anchors["$.top"]], r#"  "top": 1,"#);
        assert_eq!(lines[doc.anchors["$.u.n"]], r#"    "n": 2"#);
        assert_eq!(lines[doc.anchors["$.list[0]"]], "    10,");
        assert_eq!(lines[doc.anchors["$.list[1]"]], "    11");
    }

    #[test]
    fn odd_keys_use_the_viewer_path_spelling() {
        let value: Value = serde_json::from_str(r#"{"0":1,"a b":2}"#).unwrap();
        let doc = document(&value);
        assert!(doc.anchors.contains_key("$['0']"));
        assert!(doc.anchors.contains_key("$['a b']"));
    }
}
