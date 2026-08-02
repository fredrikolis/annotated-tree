// Concern: formats the canonical map as a `tree`-style text view | Non-concern: filesystem reads | IO: (CodebaseMap) -> String

use crate::model::{CodebaseMap, DirNode};
use crate::util::format_relative_time;

use super::Renderer;

pub struct TextRenderer {
    pub ascii: bool,
}

impl Renderer for TextRenderer {
    fn render(&self, map: &CodebaseMap) -> String {
        let glyphs = Glyphs::new(self.ascii);
        map.roots
            .iter()
            .map(|root| render_root(root, &glyphs))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

struct Glyphs {
    tee: &'static str,
    elbow: &'static str,
    pipe: &'static str,
    blank: &'static str,
}

impl Glyphs {
    fn new(ascii: bool) -> Self {
        if ascii {
            Glyphs {
                tee: "|-- ",
                elbow: "`-- ",
                pipe: "|   ",
                blank: "    ",
            }
        } else {
            Glyphs {
                tee: "├── ",
                elbow: "└── ",
                pipe: "│   ",
                blank: "    ",
            }
        }
    }
}

/// Render one root's tree. The root's own name is not printed — its contents are
/// shown directly, matching `tree`'s default.
fn render_root(root: &DirNode, glyphs: &Glyphs) -> String {
    let mut out = String::new();
    render_node(root, "", glyphs, &mut out);
    out.truncate(out.trim_end().len());
    out
}

fn render_node(node: &DirNode, prefix: &str, glyphs: &Glyphs, out: &mut String) {
    let marker = super::elision_summary(node.elided_dirs, node.elided_files);
    let child_count = node.dirs.len() + node.files.len() + marker.is_some() as usize;
    let mut index = 0;

    for child in &node.dirs {
        let is_last = index == child_count - 1;
        index += 1;
        let connector = if is_last { glyphs.elbow } else { glyphs.tee };
        let annotation = dir_annotation(child);
        out.push_str(&format!("{prefix}{connector}{}/{annotation}\n", child.name));

        let extension = if is_last { glyphs.blank } else { glyphs.pipe };
        let child_prefix = format!("{prefix}{extension}");
        render_node(child, &child_prefix, glyphs, out);
    }

    for file in &node.files {
        let is_last = index == child_count - 1;
        index += 1;
        let connector = if is_last { glyphs.elbow } else { glyphs.tee };
        let age = age_suffix(file.age_secs);
        let annotation = file_annotation(file.annotation.as_deref());
        out.push_str(&format!(
            "{prefix}{connector}{}{age}{annotation}\n",
            file.name
        ));
    }

    // The per-node overflow marker is always the directory's final child (it was counted into `child_count`), so it takes the elbow connector and folds both elided-dir and elided-file counts into one row.
    if let Some(summary) = marker {
        out.push_str(&format!("{prefix}{}[{summary}]\n", glyphs.elbow));
    }
}

/// The directory row's trailing `# …`: the authored charter first (when resolved), then the
/// observed dep facts folded in behind a `·` separator — authored intent cross-checked against
/// the graph. A charter-less directory is byte-for-byte unchanged (bare deps, or nothing). The
/// separator is glyph-neutral (not ascii-swapped) so `--ascii` stays a pure box-glyph swap.
fn dir_annotation(dir: &DirNode) -> String {
    let charter = dir.charter.as_ref().map(|c| c.line());
    let deps = dir.deps.as_ref().and_then(|d| d.annotation());
    let text = match (charter, deps) {
        (Some(charter), Some(deps)) => format!("{charter}  ·  {deps}"),
        (Some(charter), None) => charter,
        (None, Some(deps)) => deps,
        (None, None) => return String::new(),
    };
    format!("  # {text}")
}

fn file_annotation(annotation: Option<&str>) -> String {
    match annotation {
        Some(text) => format!("  # {text}"),
        None => String::new(),
    }
}

fn age_suffix(age_secs: Option<i64>) -> String {
    match age_secs {
        Some(secs) => format!("  ({})", format_relative_time(secs)),
        None => String::new(),
    }
}
