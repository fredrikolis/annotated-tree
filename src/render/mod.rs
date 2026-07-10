// Render: The renderer extension seam — a `Renderer` trait mapping the canonical map to output, plus the text implementation. NOT concerned with building the map. | I/O: (CodebaseMap) -> String

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
