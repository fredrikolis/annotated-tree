// Model: The single canonical in-memory codebase map — builds the sorted dir/file tree once and performs every filesystem read (annotations, mtime). NOT concerned with output formatting. | I/O: (root, files, graph, Config) -> CodebaseMap

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::annotation;
use crate::config::Config;
use crate::graph::DirDeps;
use crate::symbols::{self, Symbol};
use crate::tokens;

/// One canonical tree per analyzed root. Renderers convert this to text/JSON/etc.
#[derive(Serialize)]
pub struct CodebaseMap {
    pub roots: Vec<DirNode>,
}

#[derive(Serialize)]
pub struct DirNode {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<DirDeps>,
    pub dirs: Vec<DirNode>,
    pub files: Vec<FileNode>,
    /// Sum of every descendant file's estimated tokens; `Some` only when
    /// `show_tokens` and the subtree was expanded (a pruned depth cutoff is `None`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
}

#[derive(Serialize)]
pub struct FileNode {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
    /// Top-level definitions, filled only under `--symbols` on a `symbols`-feature
    /// build. Empty otherwise, and skipped in JSON when empty — so the default
    /// schema is byte-for-byte unchanged whether or not the feature is compiled in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Symbol>,
}

/// Intermediate sorted tree: dirs and files under a directory, keyed by name so
/// iteration is alphabetical (dirs then files, matching `tree`).
#[derive(Default)]
struct RawNode {
    dirs: BTreeMap<String, RawNode>,
    files: BTreeMap<String, PathBuf>,
}

/// Build the canonical model for one root. `files` are absolute paths under
/// `root`; `graph` is keyed by canonicalized directory path. All annotation and
/// mtime reads happen here — renderers are pure over the returned tree.
pub fn build(
    root: &Path,
    files: &[PathBuf],
    graph: &HashMap<PathBuf, DirDeps>,
    config: &Config,
    max_depth: Option<usize>,
) -> DirNode {
    let mut raw = RawNode::default();
    for path in files {
        if let Ok(rel) = path.strip_prefix(root) {
            insert(&mut raw, rel, path);
        }
    }
    // Canonical Representation: `graph` keys directories by canonical path, so
    // canonicalize the root ONCE here and descend by joining child names onto it. The
    // walk never follows symlinks, so `canon_root.join(child…)` is itself canonical —
    // every directory node looks `graph` up directly, with no per-node canonicalize
    // syscall. File paths (used for the annotation/mtime reads) come from `raw` and
    // stay exactly as walked, untouched.
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let now = SystemTime::now();
    convert(&raw, &canon_root, graph, config, now, max_depth, 0)
}

fn insert(node: &mut RawNode, rel: &Path, abs: &Path) {
    let components: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let Some((file, dirs)) = components.split_last() else {
        return;
    };
    let mut cursor = node;
    for dir in dirs {
        cursor = cursor.dirs.entry(dir.clone()).or_default();
    }
    cursor.files.insert(file.clone(), abs.to_path_buf());
}

fn convert(
    node: &RawNode,
    abs_dir: &Path,
    graph: &HashMap<PathBuf, DirDeps>,
    config: &Config,
    now: SystemTime,
    max_depth: Option<usize>,
    depth: usize,
) -> DirNode {
    let deps = dir_deps(abs_dir, graph);
    let name = abs_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    // At the depth cutoff a directory is still listed (name + deps) but its
    // contents are not expanded — mirroring the original render's early return.
    let pruned = max_depth.is_some_and(|limit| depth >= limit) && depth > 0;
    if pruned {
        return DirNode {
            name,
            deps,
            dirs: Vec::new(),
            files: Vec::new(),
            tokens: None,
        };
    }

    let dirs: Vec<DirNode> = node
        .dirs
        .iter()
        .map(|(child, sub)| {
            convert(
                sub,
                &abs_dir.join(child),
                graph,
                config,
                now,
                max_depth,
                depth + 1,
            )
        })
        .collect();

    let files: Vec<FileNode> = node
        .files
        .iter()
        .map(|(name, abs)| {
            let (annotation, symbols) = annotation_and_symbols(abs, config);
            FileNode {
                name: name.clone(),
                annotation,
                age_secs: age_secs(abs, now, config),
                tokens: file_tokens(abs, config),
                symbols,
            }
        })
        .collect();

    let tokens = subtree_tokens(&dirs, &files, config);

    DirNode {
        name,
        deps,
        dirs,
        files,
        tokens,
    }
}

/// Sum this directory's own file tokens with its children's already-computed
/// totals. `None` when tokens are off, so the marker vanishes entirely by default.
fn subtree_tokens(dirs: &[DirNode], files: &[FileNode], config: &Config) -> Option<u32> {
    if !config.display.show_tokens {
        return None;
    }
    let file_sum: u32 = files.iter().filter_map(|f| f.tokens).sum();
    let dir_sum: u32 = dirs.iter().filter_map(|d| d.tokens).sum();
    Some(file_sum + dir_sum)
}

fn dir_deps(abs_dir: &Path, graph: &HashMap<PathBuf, DirDeps>) -> Option<DirDeps> {
    // `abs_dir` is already canonical (descended from the canonicalized root), so this
    // is a direct lookup — no per-node canonicalize syscall.
    graph.get(abs_dir).cloned()
}

/// Resolve a file's annotation and — when `--symbols` is active and the language has
/// a grammar — its definition outline. Efficiency (single read boundary): in the
/// symbol path the file is opened ONCE and the buffer feeds both extractors, instead
/// of a head read for the annotation plus a second full read for symbols. In the
/// default path the annotation stays a bounded head-only read, byte-identical to a
/// symbol-free build; a missing extension/language/unreadable file yields `(None, [])`
/// (graceful, like `annotation: None`).
fn annotation_and_symbols(abs: &Path, config: &Config) -> (Option<String>, Vec<Symbol>) {
    let Some(lang) = config.language_for_path(abs) else {
        return (None, Vec::new());
    };

    if config.display.show_symbols {
        if let Some(extractor) = symbols::for_language(&lang.name) {
            return match read_full_lossy(abs) {
                // The annotation logic reads only the leading comment, so scanning
                // the full buffer is equivalent to the head read for our comment-based
                // languages while avoiding a second open.
                //
                // Inert seam (no shipped language uses it): a language configured with
                // a `pattern` regex could match content BEYOND the 64 KiB annotation
                // head, in which case this full-buffer scan and the non-symbol build's
                // bounded head read (`annotation::extract`) could disagree on the
                // annotation. Acknowledged, not currently reachable.
                Some(src) => (
                    annotation::extract_from(&src, lang),
                    extractor.extract(&src),
                ),
                None => (None, Vec::new()),
            };
        }
    }

    (annotation::extract(abs, lang), Vec::new())
}

/// Read a file's full contents, lossily decoding invalid UTF-8 (a stray binary byte
/// becomes U+FFFD rather than failing the read) — mirroring the annotation head read.
fn read_full_lossy(abs: &Path) -> Option<String> {
    let bytes = std::fs::read(abs).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn file_tokens(abs: &Path, config: &Config) -> Option<u32> {
    if !config.display.show_tokens {
        return None;
    }
    // The heuristic is byte-based, so the size from metadata is all we need — no
    // content read, no per-file buffering (a large blob would otherwise be slurped
    // whole just to count its bytes). Unreadable files yield `None`.
    let bytes = std::fs::metadata(abs).ok()?.len();
    Some(tokens::estimate(bytes))
}

fn age_secs(abs: &Path, now: SystemTime, config: &Config) -> Option<i64> {
    if !config.display.show_age {
        return None;
    }
    let modified = abs.metadata().and_then(|m| m.modified()).ok()?;
    Some(
        now.duration_since(modified)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(-1),
    )
}
