//! JSON syntax highlighting and palette resolution.

mod highlight;
mod theme;

pub use highlight::{Span, SpanKind, highlight};
pub use theme::{
    SyntaxColors, SyntaxMode, SyntaxTheme, active, colors_for, fallback, find, theme_names,
};
