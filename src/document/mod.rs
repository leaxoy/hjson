//! Document loading, provenance and built-in sample data.

mod loader;
pub mod sample;
mod source;

pub(crate) use loader::{
    LoadedDocument, from_clipboard, from_file, from_history, from_manual, from_sample, from_text,
    from_url,
};
pub use source::SourceKind;
