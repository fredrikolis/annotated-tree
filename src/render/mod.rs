// Concern: the renderer extension seam — a `Renderer` trait mapping the canonical map to output, plus the text implementation | Non-concern: building the map | IO: (CodebaseMap) -> String

use crate::cli::Format;
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

/// The one-line overflow summary the text and markdown renderers share when a
/// directory's children were capped by `--max-per-node`. Returns e.g.
/// `+3 folders and 40 files, use --full to expand` (dropping a zero clause), or
/// `None` when nothing was elided. Each renderer wraps it in its own delimiters,
/// so the phrasing lives in ONE place (DRY). Glyph-neutral ASCII so it reads
/// identically in `--ascii` mode.
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

/// Select the renderer for `format`. The `text` view alone needs the `ascii`
/// glyph choice (a display concern from config); json/md carry no such state, so
/// adding a format touches only this match arm (Open/Closed).
pub fn for_format(format: Format, ascii: bool) -> Box<dyn Renderer> {
    match format {
        Format::Text => Box::new(TextRenderer { ascii }),
        Format::Json => Box::new(JsonRenderer),
        Format::Md => Box::new(MdRenderer),
    }
}
