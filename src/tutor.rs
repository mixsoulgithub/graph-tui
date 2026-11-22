//! Built-in tutorial: an editable `.gtui` document bundled with the program, worked
//! through like `vimtutor`.
//!
//! The tutorial has no special privileges: it is an ordinary document; break it and rerun
//! the command to restore it (every run parses from the embedded text).

use crate::document::Document;

const EN: &str = include_str!("../assets/tutor.en.gtui");

/// Raw text of the tutorial document.
pub fn source() -> &'static str {
    EN
}

/// Parse into a document: each call yields a fresh copy the caller may freely edit.
pub fn document() -> Result<Document, String> {
    Document::load(source()).map_err(|e| format!("tutor: {e}"))
}

/// Status line on entry: states that this is a copy, how to leave, and how to reset.
pub fn hint() -> &'static str {
    "tutorial copy; :q to leave; run graph-tui tutor again to reset"
}
