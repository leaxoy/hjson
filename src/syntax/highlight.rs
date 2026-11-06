/// Syntax-highlight classification for the raw JSON panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpanKind {
    Key,
    Str,
    Num,
    Bool,
    Null,
    Punct,
    Plain,
    /// Filler for the indent gutter.
    Indent,
}

#[derive(Clone, Debug)]
pub struct Span {
    pub kind: SpanKind,
    pub text: String,
}

impl Span {
    pub fn new(kind: SpanKind, text: impl Into<String>) -> Self {
        Span {
            kind,
            text: text.into(),
        }
    }
}

/// Tokenize one line of pretty-printed JSON into colored spans.
fn line_spans(line: &str) -> Vec<Span> {
    let leading_ws_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let mut out = Vec::new();
    if leading_ws_len > 0 {
        out.push(Span::new(SpanKind::Indent, &line[..leading_ws_len]));
    }
    let rest = &line[leading_ws_len..];
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    let mut plain = String::new();
    while i < chars.len() {
        match chars[i] {
            '"' => {
                if !plain.is_empty() {
                    out.push(Span::new(SpanKind::Plain, plain.clone()));
                    plain.clear();
                }
                // consume the string (with escapes)
                let start = i;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                    } else if chars[i] == '"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                let text: String = chars[start..i].iter().collect();
                // peek next non-space char for key vs value
                let mut j = i;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ':' {
                    out.push(Span::new(SpanKind::Key, text));
                } else {
                    out.push(Span::new(SpanKind::Str, text));
                }
            }
            ':' | ',' => {
                if !plain.is_empty() {
                    out.push(Span::new(SpanKind::Plain, plain.clone()));
                    plain.clear();
                }
                out.push(Span::new(SpanKind::Punct, chars[i].to_string()));
                i += 1;
            }
            c if c.is_whitespace() => {
                if !plain.is_empty() {
                    out.push(Span::new(SpanKind::Plain, plain.clone()));
                    plain.clear();
                }
                out.push(Span::new(SpanKind::Indent, " "));
                i += 1;
            }
            '{' | '}' | '[' | ']' => {
                if !plain.is_empty() {
                    out.push(Span::new(SpanKind::Plain, plain.clone()));
                    plain.clear();
                }
                out.push(Span::new(SpanKind::Punct, chars[i].to_string()));
                i += 1;
            }
            c => {
                plain.push(c);
                i += 1;
                // flush bare words when they end
                let is_word_char = |c: char| {
                    c.is_alphanumeric()
                        || c == '-'
                        || c == '+'
                        || c == '.'
                        || c == '_'
                        || c == 'e'
                        || c == 'E'
                };
                if (i >= chars.len() || !is_word_char(chars[i])) && !plain.is_empty() {
                    let kind = match plain.as_str() {
                        "true" | "false" => SpanKind::Bool,
                        "null" => SpanKind::Null,
                        s if looks_like_number(s) => SpanKind::Num,
                        _ => SpanKind::Plain,
                    };
                    out.push(Span::new(kind, plain.clone()));
                    plain.clear();
                }
            }
        }
    }
    if !plain.is_empty() {
        out.push(Span::new(SpanKind::Plain, plain));
    }
    out
}

fn looks_like_number(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !(first.is_ascii_digit() || first == '-' || first == '+') {
        return false;
    }
    s.parse::<serde_json::Number>().is_ok()
}

/// Tokenize a full pretty document into lines of spans.
pub fn highlight(pretty: &str) -> Vec<Vec<Span>> {
    pretty.lines().map(line_spans).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_vs_values() {
        let lines =
            highlight("{\n  \"a\": \"b\",\n  \"n\": 12,\n  \"ok\": true,\n  \"z\": null\n}");
        let l = lines[0].iter().map(|s| s.kind).collect::<Vec<_>>();
        assert_eq!(l, vec![SpanKind::Punct]);
        let l = lines[1].iter().map(|s| s.kind).collect::<Vec<_>>();
        assert_eq!(
            l,
            vec![
                SpanKind::Indent,
                SpanKind::Key,
                SpanKind::Punct,
                SpanKind::Indent,
                SpanKind::Str,
                SpanKind::Punct
            ]
        );
        let l = lines[2].iter().map(|s| s.kind).collect::<Vec<_>>();
        assert!(l.contains(&SpanKind::Num));
        let l = lines[3].iter().map(|s| s.kind).collect::<Vec<_>>();
        assert!(l.contains(&SpanKind::Bool));
        let l = lines[4].iter().map(|s| s.kind).collect::<Vec<_>>();
        assert!(l.contains(&SpanKind::Null));
    }

    #[test]
    fn numberish() {
        assert!(looks_like_number("-3.25"));
        assert!(looks_like_number("1e10"));
        assert!(!looks_like_number("abc"));
        assert!(!looks_like_number(""));
    }
}
