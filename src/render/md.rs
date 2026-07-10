// Markdown renderer: Formats the canonical map as human-facing Markdown — a heading per directory (with its dependency summary) and a bullet per file with its annotation. NOT concerned with filesystem reads or machine contracts. | I/O: (CodebaseMap) -> String

use crate::model::{CodebaseMap, DirNode};

use super::Renderer;

/// Markdown headings cap at level 6; deeper directories reuse `######`.
const MAX_HEADING_LEVEL: usize = 6;

pub struct MdRenderer;

impl Renderer for MdRenderer {
    fn render(&self, map: &CodebaseMap) -> String {
        let mut out = String::new();
        for root in &map.roots {
            // The root's own name is not printed — its contents are shown
            // directly, matching the text renderer's `tree`-style default.
            render_children(root, 0, &mut out);
        }
        out.truncate(out.trim_end().len());
        out
    }
}

/// Render a directory's subdirectories (as sections) then its files (as bullets),
/// matching the text view's dirs-then-files ordering.
fn render_children(node: &DirNode, depth: usize, out: &mut String) {
    for dir in &node.dirs {
        render_dir(dir, depth, out);
    }
    for file in &node.files {
        out.push_str(&file_bullet(&file.name, file.annotation.as_deref()));
    }
    if !node.files.is_empty() {
        out.push('\n');
    }
}

fn render_dir(dir: &DirNode, depth: usize, out: &mut String) {
    let level = (depth + 2).min(MAX_HEADING_LEVEL);
    out.push_str(&format!("{} {}/\n\n", "#".repeat(level), dir.name));
    if let Some(text) = dir.deps.as_ref().and_then(|d| d.annotation()) {
        out.push_str(&format!("_{text}_\n\n"));
    }
    render_children(dir, depth + 1, out);
}

fn file_bullet(name: &str, annotation: Option<&str>) -> String {
    match annotation {
        Some(text) => format!("- `{name}` — {text}\n"),
        None => format!("- `{name}`\n"),
    }
}
