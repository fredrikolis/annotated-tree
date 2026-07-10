// Text renderer: Formats the canonical map as a `tree`-style view with glyph connectors, per-file `# annotation`/age suffixes, and per-directory dependency edges. NOT concerned with filesystem reads. | I/O: (CodebaseMap) -> String

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
    sym: &'static str,
}

impl Glyphs {
    fn new(ascii: bool) -> Self {
        if ascii {
            Glyphs {
                tee: "|-- ",
                elbow: "`-- ",
                pipe: "|   ",
                blank: "    ",
                sym: "- ",
            }
        } else {
            Glyphs {
                tee: "├── ",
                elbow: "└── ",
                pipe: "│   ",
                blank: "    ",
                sym: "· ",
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
    let child_count = node.dirs.len() + node.files.len();
    let mut index = 0;

    for child in &node.dirs {
        let is_last = index == child_count - 1;
        index += 1;
        let connector = if is_last { glyphs.elbow } else { glyphs.tee };
        let tokens = dir_tokens(child);
        let annotation = dir_annotation(child);
        out.push_str(&format!(
            "{prefix}{connector}{}/{tokens}{annotation}\n",
            child.name
        ));

        let extension = if is_last { glyphs.blank } else { glyphs.pipe };
        let child_prefix = format!("{prefix}{extension}");
        render_node(child, &child_prefix, glyphs, out);
    }

    for file in &node.files {
        let is_last = index == child_count - 1;
        index += 1;
        let connector = if is_last { glyphs.elbow } else { glyphs.tee };
        let age = age_suffix(file.age_secs);
        let tokens = token_suffix(file.tokens);
        let annotation = file_annotation(file.annotation.as_deref());
        out.push_str(&format!(
            "{prefix}{connector}{}{age}{tokens}{annotation}\n",
            file.name
        ));

        // Symbols are leaves under the file: indent them past the file's own row
        // using its continuation column (blank when the file is the last child,
        // else the pipe), so the outline lines up beneath the filename.
        if !file.symbols.is_empty() {
            let extension = if is_last { glyphs.blank } else { glyphs.pipe };
            for symbol in &file.symbols {
                out.push_str(&format!(
                    "{prefix}{extension}{}{}  :{}\n",
                    glyphs.sym, symbol.signature, symbol.line
                ));
            }
        }
    }
}

fn dir_annotation(dir: &DirNode) -> String {
    match dir.deps.as_ref().and_then(|d| d.annotation()) {
        Some(text) => format!("  # {text}"),
        None => String::new(),
    }
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

/// A package directory shows its aggregate subtree token estimate; plain
/// directories carry no dependency identity, so they stay unlabelled.
fn dir_tokens(dir: &DirNode) -> String {
    if dir.deps.is_some() {
        token_suffix(dir.tokens)
    } else {
        String::new()
    }
}

fn token_suffix(tokens: Option<u32>) -> String {
    match tokens {
        Some(count) => format!("  [~{count} tok]"),
        None => String::new(),
    }
}
