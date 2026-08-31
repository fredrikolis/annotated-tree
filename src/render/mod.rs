// Concern: the renderer seam — the `Renderer` trait, the format -> renderer factory, and the shared elision summary | Non-concern: building the map | IO: (CodebaseMap) -> String

use crate::config::Format;
use crate::model::CodebaseMap;

pub mod json;
pub mod md;
pub mod text;

pub use json::JsonRenderer;
pub use md::MdRenderer;
pub use text::TextRenderer;

pub trait Renderer {
    fn render(&self, map: &CodebaseMap) -> String;
}

/// Shared by text and markdown; each wraps it in its own delimiters. Glyph-neutral ASCII.
pub(crate) fn elision_summary(elided_dirs: u32, elided_files: u32) -> Option<String> {
    let mut parts = Vec::new();
    if elided_dirs > 0 {
        parts.push(format!("{elided_dirs} folders"));
    }
    if elided_files > 0 {
        parts.push(format!("{elided_files} files"));
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("+{}, use --full to expand", parts.join(" and ")))
}

pub fn for_format(format: Format, ascii: bool) -> Box<dyn Renderer> {
    match format {
        Format::Text => Box::new(TextRenderer { ascii }),
        Format::Json => Box::new(JsonRenderer),
        Format::Md => Box::new(MdRenderer),
    }
}
