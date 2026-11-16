//! Bounded, best-effort parsing of JSON values encoded inside strings.

use serde_json::Value;

const MAX_DEPTH: usize = 128;
const MAX_STRING_LAYERS: usize = 32;
const MAX_DECODES: usize = 65_536;
const MAX_PARSE_BYTES: usize = 64 * 1024 * 1024;

struct Budget {
    bytes: usize,
    decoded: usize,
}

/// Decode values, never keys. Limits bound extra work and combined tree depth
/// even when each individually valid string contains another encoded document.
/// At a limit or parse error, leave the current string intact.
pub(super) fn decode(value: &mut Value, input_len: usize) -> usize {
    let mut budget = Budget {
        bytes: input_len.saturating_mul(4).clamp(1024, MAX_PARSE_BYTES),
        decoded: 0,
    };
    visit(value, 0, 0, &mut budget);
    budget.decoded
}

fn visit(value: &mut Value, depth: usize, layers: usize, budget: &mut Budget) {
    if depth >= MAX_DEPTH {
        return;
    }
    if let Value::String(text) = value {
        if layers >= MAX_STRING_LAYERS || budget.decoded >= MAX_DECODES {
            return;
        }
        let candidate = text.trim();
        if !looks_like_json(candidate) || candidate.len() > budget.bytes {
            return;
        }
        budget.bytes -= candidate.len();
        let Ok(decoded) = serde_json::from_str::<Value>(candidate) else {
            return;
        };
        if !fits_depth(&decoded, MAX_DEPTH - depth) {
            return;
        }
        *value = decoded;
        budget.decoded += 1;
        visit(value, depth, layers + 1, budget);
        return;
    }
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                visit(child, depth + 1, layers, budget);
            }
        }
        Value::Array(items) => {
            for child in items {
                visit(child, depth + 1, layers, budget);
            }
        }
        _ => {}
    }
}

/// Cheap prefilter only; strict serde parsing still decides validity. Ordinary
/// words, URLs and timestamps do not repeatedly allocate a parser error.
fn looks_like_json(text: &str) -> bool {
    matches!(
        text.as_bytes().first(),
        Some(b'{' | b'[' | b'"' | b'-' | b'0'..=b'9')
    ) || matches!(text, "true" | "false" | "null")
}

fn fits_depth(value: &Value, remaining: usize) -> bool {
    if remaining == 0 {
        return !matches!(value, Value::Object(map) if !map.is_empty())
            && !matches!(value, Value::Array(items) if !items.is_empty());
    }
    match value {
        Value::Object(map) => map.values().all(|child| fits_depth(child, remaining - 1)),
        Value::Array(items) => items.iter().all(|child| fits_depth(child, remaining - 1)),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json_model::{JsonTree, JsonType};

    #[test]
    fn string_expansion_is_reversible_and_keeps_source_and_anchors() {
        use std::sync::Arc;
        let text = r#"{"payload":"{\"a\":1}","n":"123"}"#;
        let literal = Arc::new(JsonTree::parse_with_options(text, false).unwrap());
        assert_eq!(literal.root["n"], "123");
        let decoded = literal.with_string_expansion(true);
        assert_eq!(decoded.root["payload"]["a"], 1);
        assert_eq!(decoded.root["n"], 123);
        assert_eq!(decoded.decoded_strings, 2);
        assert_eq!(decoded.source_text, text);
        assert!(Arc::ptr_eq(&decoded, &decoded.with_string_expansion(true)));
        for _ in 0..3 {
            let restored = decoded.with_string_expansion(false);
            assert_eq!(restored.root, literal.root);
            assert_eq!(restored.pretty, literal.pretty);
            assert_eq!(restored.decoded_strings, 0);
            assert_eq!(restored.with_string_expansion(true).root, decoded.root);
        }
        let root = serde_json::to_string(&r#"{"a":1}"#).unwrap();
        let root = Arc::new(JsonTree::parse_with_options(&root, true).unwrap());
        assert!(root.with_string_expansion(false).root.is_string());
    }

    #[test]
    fn root_and_nested_encoded_documents_expand_recursively() {
        let expected = serde_json::json!({"payload":{"a":1,"list":[{"名字":"你好"}]}});
        let nested = serde_json::json!({"payload": serde_json::to_string(&serde_json::json!({"a":1,"list":["{\"名字\":\"你好\"}"]})).unwrap()});
        let text = serde_json::to_string(&serde_json::to_string(&nested).unwrap()).unwrap();
        let tree = JsonTree::parse(&text).unwrap();
        assert_eq!(tree.root, expected);
        assert_eq!(tree.decoded_strings, 3);
        let id = tree
            .nodes
            .iter()
            .position(|n| n.key.as_deref() == Some("名字"))
            .unwrap();
        assert_eq!(tree.json_path(id), "$.payload.list[0].名字");
        assert_eq!(tree.resolve_ref(id), Some(&serde_json::json!("你好")));
        assert!(
            tree.pretty
                .lines()
                .nth(tree.line_of(id).unwrap())
                .unwrap()
                .contains("名字")
        );
    }

    #[test]
    fn invalid_strings_keys_and_non_json_text_stay_unchanged() {
        let value = serde_json::json!({"{\"key\":1}":"{broken", "trailing":"{} garbage", "leading":"001", "text":"hello", "empty":"", "url":"https://example.com"});
        let tree = JsonTree::parse(&value.to_string()).unwrap();
        assert_eq!(tree.root, value);
        assert_eq!(tree.decoded_strings, 0);
        assert!(JsonTree::parse("{broken").is_err());
    }

    #[test]
    fn valid_encoded_scalars_arrays_and_empty_containers_parse() {
        let tree = JsonTree::parse(r#"{"n":"123","b":"true","f":"false","nil":"null","list":"[1,2]","obj":"{}","str":"\"hello\""}"#).unwrap();
        assert_eq!(
            tree.root,
            serde_json::json!({"n":123,"b":true,"f":false,"nil":null,"list":[1,2],"obj":{},"str":"hello"})
        );
        assert_eq!(tree.decoded_strings, 7);
    }

    #[test]
    fn literal_editing_path_preserves_types_and_anchors() {
        let text = r#"{"payload":"{\"a\":1}"}"#;
        let literal = JsonTree::parse_literal(text).unwrap();
        assert_eq!(literal.nodes[1].typ, JsonType::String);
        assert_eq!(literal.decoded_strings, 0);
        let decoded = JsonTree::parse(text).unwrap();
        assert_eq!(decoded.nodes[1].typ, JsonType::Object);
        assert_eq!(decoded.nodes[2].key.as_deref(), Some("a"));
        let round_trip = JsonTree::parse(&decoded.pretty).unwrap();
        assert_eq!(round_trip.root, decoded.root);
        assert_eq!(round_trip.decoded_strings, 0);
    }

    #[test]
    fn depth_limit_leaves_remaining_encoded_value_intact() {
        let mut value = Value::String("{\"final\":1}".into());
        let before = value.clone();
        visit(
            &mut value,
            MAX_DEPTH,
            0,
            &mut Budget {
                bytes: 1024,
                decoded: 0,
            },
        );
        assert_eq!(value, before);
        // Ordinary object nesting grows linearly, unlike repeated string escaping.
        for _ in 0..120 {
            value = serde_json::json!({"payload": value});
        }
        let tree = JsonTree::parse(&value.to_string()).unwrap();
        assert_eq!(tree.decoded_strings, 1);
        assert!(tree.nodes.iter().all(|node| node.depth <= MAX_DEPTH));
    }

    #[test]
    fn combined_depth_and_parse_budget_are_bounded() {
        let deep = serde_json::json!({"value":[[[1]]]});
        assert!(fits_depth(&deep, 4));
        assert!(!fits_depth(&deep, 3));
        let mut value = Value::String("{\"a\":1}".into());
        let before = value.clone();
        visit(
            &mut value,
            0,
            0,
            &mut Budget {
                bytes: 1,
                decoded: 0,
            },
        );
        assert_eq!(value, before);
        visit(
            &mut value,
            0,
            MAX_STRING_LAYERS,
            &mut Budget {
                bytes: 1024,
                decoded: 0,
            },
        );
        assert_eq!(value, before);
    }
}
