use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The JSON type of a value, used for picking colors and icons in the tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsonType {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

impl JsonType {
    /// Short label shown in menus / status bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Object => "Object",
            Self::Array => "Array",
            Self::String => "String",
            Self::Number => "Number",
            Self::Bool => "Boolean",
            Self::Null => "Null",
        }
    }
}

/// A single path segment from the root to a node.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Key(String),
    Index(usize),
}

/// One row of the JSON tree arena. `id` is the index inside `JsonTree::nodes`.
#[derive(Clone, Debug)]
pub struct FlatNode {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub depth: usize,
    /// Key for object members, `None` for the root and array elements.
    pub key: Option<String>,
    /// Array index when the node is an array element; `Some` also marks "in an array".
    pub index: Option<usize>,
    pub typ: JsonType,
    /// Short inline summary for scalars; `"{ N }"` / `"[ N ]"` for collections.
    pub summary: String,
    /// Path segments from the root, used for JSONPath / JSON Pointer.
    pub segments: Vec<Seg>,
}

pub struct JsonTree {
    pub root: Value,
    pub nodes: Vec<FlatNode>,
    /// Pretty-printed root JSON, kept for the raw panel.
    pub pretty: String,
    /// Line of `pretty` where each JSONPath starts, from the same walk that
    /// produced `pretty` — see [`crate::pretty`]. Keys match [`Self::json_path`].
    pub anchors: HashMap<String, usize>,
    pub error: Option<String>,
}

impl JsonTree {
    /// Parse a text and build the tree arena. Object key order is preserved.
    pub fn parse(text: &str) -> Self {
        match serde_json::from_str::<Value>(text) {
            Ok(root) => {
                let nodes = vec![FlatNode {
                    parent: None,
                    children: Vec::new(),
                    depth: 0,
                    key: None,
                    index: None,
                    typ: JsonType::Null,
                    summary: String::new(),
                    segments: Vec::new(),
                }];
                let mut segments = Vec::new();
                let mut pointer = String::new();
                let mut builder = Builder {
                    nodes,
                    seg_stack: &mut segments,
                    pointer: &mut pointer,
                };
                builder.attach(0, &root);
                let nodes = std::mem::take(&mut builder.nodes);
                // One walk gives the raw panel its text and every node its
                // line, so the two panes can never drift apart.
                let doc = crate::pretty::document(&root);
                Self {
                    root,
                    nodes,
                    pretty: doc.text,
                    anchors: doc.anchors,
                    error: None,
                }
            }
            Err(e) => Self {
                root: Value::Null,
                nodes: Vec::new(),
                pretty: String::new(),
                anchors: HashMap::new(),
                error: Some(e.to_string()),
            },
        }
    }

    /// Visible node ids (flattened, preorder) given the expanded set.
    pub fn visible(&self, expanded: &HashSet<usize>) -> Vec<usize> {
        let mut out = Vec::new();
        if self.nodes.is_empty() {
            return out;
        }
        self.collect_visible(&self.nodes[0].children, expanded, &mut out);
        out
    }

    fn collect_visible(&self, ids: &[usize], expanded: &HashSet<usize>, out: &mut Vec<usize>) {
        for &id in ids {
            out.push(id);
            let n = &self.nodes[id];
            if !n.children.is_empty() && expanded.contains(&id) {
                self.collect_visible(&n.children, expanded, out);
            }
        }
    }

    /// Standard JSONPath for the node, e.g. `$.items[2].name`.
    pub fn json_path(&self, id: usize) -> String {
        let mut path = String::new();
        for seg in &self.nodes[id].segments {
            match seg {
                Seg::Key(key) => {
                    if is_simple_key(key) {
                        path.push('.');
                        path.push_str(key);
                    } else {
                        path.push_str(&format!("['{}']", escape_single(key)));
                    }
                }
                Seg::Index(i) => path.push_str(&format!("[{i}]")),
            }
        }
        format!("${path}")
    }

    /// The line of [`Self::pretty`] where this node's value starts. `None`
    /// only for a node the pretty walk never reached (a failed parse).
    pub fn line_of(&self, id: usize) -> Option<usize> {
        self.anchors.get(&self.json_path(id)).copied()
    }

    /// RFC 6901 JSON Pointer, e.g. `/items/2/name`.
    pub fn json_pointer(&self, id: usize) -> String {
        let mut s = String::new();
        for seg in &self.nodes[id].segments {
            match seg {
                Seg::Key(key) => {
                    s.push('/');
                    s.push_str(&key.replace('~', "~0").replace('/', "~1"));
                }
                Seg::Index(i) => s.push_str(&format!("/{i}")),
            }
        }
        s
    }

    /// The sub-Value at `id` (cloned from the root).
    pub fn resolve(&self, id: usize) -> Option<Value> {
        let mut cur = &self.root;
        for seg in &self.nodes[id].segments {
            cur = match seg {
                Seg::Key(key) => cur.as_object()?.get(key)?,
                Seg::Index(i) => cur.as_array()?.get(*i)?,
            };
        }
        Some(cur.clone())
    }

    /// Pretty JSON of the subtree at `id`.
    pub fn subtree_json(&self, id: usize) -> Option<String> {
        serde_json::to_string_pretty(&self.resolve(id)?).ok()
    }

    /// Compact JSON of the subtree at `id`.
    pub fn subtree_json_compact(&self, id: usize) -> Option<String> {
        serde_json::to_string(&self.resolve(id)?).ok()
    }

    /// jq-style path, e.g. `["items"][2]["name"]` after `.`-able keys.
    pub fn jq_path(&self, id: usize) -> String {
        let mut s = String::new();
        for seg in &self.nodes[id].segments {
            match seg {
                Seg::Key(key) => s.push_str(&format!(
                    "[\"{}\"]",
                    key.replace('\\', "\\\\").replace('"', "\\\"")
                )),
                Seg::Index(i) => s.push_str(&format!("[{i}]")),
            }
        }
        if s.is_empty() {
            ".".to_string()
        } else {
            format!(".{s}")
        }
    }

    /// Count of all descendants (0 for scalars, excluding self).
    pub fn descendant_count(&self, id: usize) -> usize {
        let mut count = 0;
        let mut stack = self
            .nodes
            .get(id)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        while let Some(id) = stack.pop() {
            count += 1;
            stack.extend(self.nodes[id].children.iter().copied());
        }
        count
    }

    pub fn type_label(&self, id: usize) -> &'static str {
        self.nodes[id].typ.label()
    }
}

/// Incrementally builds the node arena.
struct Builder<'a> {
    nodes: Vec<FlatNode>,
    seg_stack: &'a mut Vec<Seg>,
    pointer: &'a mut String,
}

impl<'a> Builder<'a> {
    fn push_seg(&mut self, seg: Seg) {
        match &seg {
            Seg::Key(k) => {
                self.pointer.push('/');
                push_key_str(&mut self.pointer, k);
            }
            Seg::Index(i) => self.pointer.push_str(&format!("/{i}")),
        }
        self.seg_stack.push(seg);
    }

    fn pop_seg(&mut self) {
        self.seg_stack.pop();
        if let Some(pos) = self.pointer.rfind('/') {
            self.pointer.truncate(pos);
        }
    }

    /// Fill the placeholder node at `slot` with `value` and all descendants.
    fn attach(&mut self, slot: usize, value: &Value) {
        match value {
            Value::Object(map) => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::Object;
                node.summary = format!("{{ {} }}", map.len());
                node.children.reserve(map.len());
                for (k, v) in map {
                    self.push_seg(Seg::Key(k.clone()));
                    let child = self.reserve(slot);
                    self.attach(child, v);
                    self.pop_seg();
                }
            }
            Value::Array(arr) => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::Array;
                node.summary = format!("[ {} ]", arr.len());
                node.children.reserve(arr.len());
                for (i, v) in arr.iter().enumerate() {
                    self.push_seg(Seg::Index(i));
                    let child = self.reserve(slot);
                    self.attach(child, v);
                    self.pop_seg();
                }
            }
            Value::String(s) => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::String;
                node.summary = summarize(s);
            }
            Value::Number(n) => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::Number;
                node.summary = n.to_string();
            }
            Value::Bool(b) => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::Bool;
                node.summary = b.to_string();
            }
            Value::Null => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::Null;
                node.summary = "null".to_string();
            }
        }
    }

    /// Reserve a slot for a new child of `parent` and set bookkeeping.
    fn reserve(&mut self, parent: usize) -> usize {
        let depth = self.nodes[parent].depth + 1;
        let segments = self.seg_stack.clone();
        let key = match self.seg_stack.last() {
            Some(Seg::Key(k)) => Some(k.clone()),
            _ => None,
        };
        let index = match self.seg_stack.last() {
            Some(Seg::Index(i)) => Some(*i),
            _ => None,
        };
        let id = self.nodes.len();
        self.nodes.push(FlatNode {
            parent: Some(parent),
            children: Vec::new(),
            depth,
            key,
            index,
            typ: JsonType::Null,
            summary: String::new(),
            segments,
        });
        self.nodes[parent].children.push(id);
        id
    }
}

fn push_key_str(pointer: &mut String, key: &str) {
    for c in key.chars() {
        match c {
            '~' => pointer.push_str("~0"),
            '/' => pointer.push_str("~1"),
            _ => pointer.push(c),
        }
    }
}

pub(crate) fn is_simple_key(key: &str) -> bool {
    let mut chars = key.chars();
    let first_ok = chars
        .next()
        .map(|c| c.is_alphabetic() || c == '_')
        .unwrap_or(false);
    first_ok && key.chars().all(|c| c.is_alphanumeric() || c == '_')
}

pub(crate) fn escape_single(key: &str) -> String {
    key.replace('\'', "\\'")
}

fn summarize(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > MAX_SUMMARY {
        let head: String = chars[..MAX_SUMMARY].iter().collect();
        format!("\"{}…\"", head)
    } else {
        format!("\"{s}\"")
    }
}

/// Maximum inline summary length for strings. Exposed for tests.
pub const MAX_SUMMARY: usize = 60;

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"name":"hi","items":[1,{"deep":null,"weird key/x~":true}]}"#;

    #[test]
    fn parse_types_and_summary() {
        let t = JsonTree::parse(SAMPLE);
        assert!(t.error.is_none());
        let root_children = t.nodes[0].children.clone();
        assert_eq!(root_children.len(), 2);
        assert_eq!(t.nodes[root_children[0]].typ, JsonType::String);
        assert_eq!(t.nodes[root_children[0]].summary, "\"hi\"");
        let items = root_children[1];
        assert_eq!(t.nodes[items].typ, JsonType::Array);
        assert_eq!(t.nodes[items].summary, "[ 2 ]");
    }

    #[test]
    fn paths() {
        let t = JsonTree::parse(SAMPLE);
        let items = t.nodes[0].children[1];
        assert_eq!(t.json_path(items), "$.items");
        assert_eq!(t.json_pointer(items), "/items");
        let inner = t.nodes[items].children[1];
        assert_eq!(t.json_path(inner), "$.items[1]");
        let weird = t.nodes[inner].children[1];
        assert_eq!(t.json_path(weird), "$.items[1]['weird key/x~']");
        assert_eq!(t.json_pointer(weird), "/items/1/weird key~1x~0");
        assert_eq!(t.jq_path(weird), ".[\"items\"][1][\"weird key/x~\"]");
    }

    #[test]
    fn visible_expansion() {
        let t = JsonTree::parse(SAMPLE);
        let name = t.nodes[0].children[0];
        let items = t.nodes[0].children[1];
        let mut expanded = HashSet::new();
        // Top level (root's children) is always visible; deeper levels need expansion.
        assert_eq!(t.visible(&expanded), vec![name, items]);
        expanded.insert(items);
        let items_children = &t.nodes[items].children;
        assert_eq!(
            t.visible(&expanded),
            vec![name, items, items_children[0], items_children[1]]
        );
    }

    #[test]
    fn subtree_and_descendants() {
        let t = JsonTree::parse(SAMPLE);
        let items = t.nodes[0].children[1];
        assert_eq!(t.descendant_count(items), 4);
        let sub = t.subtree_json(items).unwrap();
        let parsed: Value = serde_json::from_str(&sub).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!([1, {"deep": null, "weird key/x~": true}])
        );
    }
}
