use serde_json::Value;
use std::collections::HashSet;

pub(crate) mod hints;
pub(crate) mod pretty;

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
    /// Line in the pretty document where this value starts.
    pub line: Option<usize>,
}

pub struct JsonTree {
    pub root: Value,
    pub nodes: Vec<FlatNode>,
    /// Pretty-printed root JSON, kept for the raw panel.
    pub pretty: String,
}

impl JsonTree {
    /// Parse a text and build the tree arena. Object key order is preserved.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        let root = serde_json::from_str::<Value>(text)?;
        let nodes = vec![FlatNode {
            parent: None,
            children: Vec::new(),
            depth: 0,
            key: None,
            index: None,
            typ: JsonType::Null,
            summary: String::new(),
            line: None,
        }];
        let mut builder = Builder { nodes };
        builder.attach(0, &root);
        let mut nodes = std::mem::take(&mut builder.nodes);
        // One walk gives the raw panel its text and every node its line, so the
        // two panes can never drift apart.
        let doc = pretty::document(&root);
        for id in 0..nodes.len() {
            let path = json_path_for(&nodes, id);
            nodes[id].line = doc.anchors.get(&path).copied();
        }
        Ok(Self {
            root,
            nodes,
            pretty: doc.text,
        })
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
        json_path_for(&self.nodes, id)
    }

    /// The line of [`Self::pretty`] where this node's value starts. `None`
    /// only for a node the pretty walk never reached (a failed parse).
    pub fn line_of(&self, id: usize) -> Option<usize> {
        self.nodes.get(id)?.line
    }

    /// RFC 6901 JSON Pointer, e.g. `/items/2/name`.
    pub fn json_pointer(&self, id: usize) -> String {
        let mut s = String::new();
        for node_id in lineage(&self.nodes, id) {
            let node = &self.nodes[node_id];
            if let Some(key) = &node.key {
                s.push('/');
                s.push_str(&key.replace('~', "~0").replace('/', "~1"));
            } else if let Some(index) = node.index {
                s.push_str(&format!("/{index}"));
            }
        }
        s
    }

    /// The sub-value at `id`.
    pub fn resolve_ref(&self, id: usize) -> Option<&Value> {
        let mut cur = &self.root;
        for node_id in lineage(&self.nodes, id) {
            let node = &self.nodes[node_id];
            cur = if let Some(key) = &node.key {
                cur.as_object()?.get(key)?
            } else {
                let index = node.index?;
                cur.as_array()?.get(index)?
            };
        }
        Some(cur)
    }

    /// The sub-value at `id` cloned from the root.
    pub fn resolve(&self, id: usize) -> Option<Value> {
        self.resolve_ref(id).cloned()
    }

    /// Pretty JSON of the subtree at `id`.
    pub fn subtree_json(&self, id: usize) -> Option<String> {
        serde_json::to_string_pretty(self.resolve_ref(id)?).ok()
    }

    /// Compact JSON of the subtree at `id`.
    pub fn subtree_json_compact(&self, id: usize) -> Option<String> {
        serde_json::to_string(self.resolve_ref(id)?).ok()
    }

    /// jq-style path, e.g. `["items"][2]["name"]` after `.`-able keys.
    pub fn jq_path(&self, id: usize) -> String {
        let mut s = String::new();
        for node_id in lineage(&self.nodes, id) {
            let node = &self.nodes[node_id];
            if let Some(key) = &node.key {
                s.push_str(&format!(
                    "[\"{}\"]",
                    key.replace('\\', "\\\\").replace('"', "\\\"")
                ));
            } else if let Some(index) = node.index {
                s.push_str(&format!("[{index}]"));
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

fn lineage(nodes: &[FlatNode], id: usize) -> Vec<usize> {
    let mut ids = Vec::with_capacity(nodes.get(id).map_or(0, |node| node.depth));
    let mut current = Some(id);
    while let Some(node_id) = current {
        if nodes[node_id].parent.is_none() {
            break;
        }
        ids.push(node_id);
        current = nodes[node_id].parent;
    }
    ids.reverse();
    ids
}

fn json_path_for(nodes: &[FlatNode], id: usize) -> String {
    let mut path = String::from("$");
    for node_id in lineage(nodes, id) {
        let node = &nodes[node_id];
        if let Some(key) = &node.key {
            if is_simple_key(key) {
                path.push('.');
                path.push_str(key);
            } else {
                path.push_str(&format!("['{}']", escape_single(key)));
            }
        } else if let Some(index) = node.index {
            path.push_str(&format!("[{index}]"));
        }
    }
    path
}

/// Incrementally builds the node arena.
struct Builder {
    nodes: Vec<FlatNode>,
}

impl Builder {
    /// Fill the placeholder node at `slot` with `value` and all descendants.
    fn attach(&mut self, slot: usize, value: &Value) {
        match value {
            Value::Object(map) => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::Object;
                node.summary = format!("{{ {} }}", map.len());
                node.children.reserve(map.len());
                for (k, v) in map {
                    let child = self.reserve(slot, Some(k.clone()), None);
                    self.attach(child, v);
                }
            }
            Value::Array(arr) => {
                let node = &mut self.nodes[slot];
                node.typ = JsonType::Array;
                node.summary = format!("[ {} ]", arr.len());
                node.children.reserve(arr.len());
                for (i, v) in arr.iter().enumerate() {
                    let child = self.reserve(slot, None, Some(i));
                    self.attach(child, v);
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
    fn reserve(&mut self, parent: usize, key: Option<String>, index: Option<usize>) -> usize {
        let depth = self.nodes[parent].depth + 1;
        let id = self.nodes.len();
        self.nodes.push(FlatNode {
            parent: Some(parent),
            children: Vec::new(),
            depth,
            key,
            index,
            typ: JsonType::Null,
            summary: String::new(),
            line: None,
        });
        self.nodes[parent].children.push(id);
        id
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

/// The string as the tree shows it: quoted, never truncated, and with control
/// characters spelled out (`\n`, `\t`, …) so a value that holds a real newline
/// stays on its row instead of tearing the layout apart.
fn summarize(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"name":"hi","items":[1,{"deep":null,"weird key/x~":true}]}"#;

    #[test]
    fn parse_types_and_summary() {
        let t = JsonTree::parse(SAMPLE).unwrap();
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
        let t = JsonTree::parse(SAMPLE).unwrap();
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
        let t = JsonTree::parse(SAMPLE).unwrap();
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
        let t = JsonTree::parse(SAMPLE).unwrap();
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
