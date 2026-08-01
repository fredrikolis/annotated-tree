// Concern: the canonical in-memory map and every filesystem read behind it — a sorted tree of each file's annotation and mtime | Non-concern: rendering | IO: (paths, graph, Config) -> CodebaseMap

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::annotation;
use crate::charter::{self, Charter};
use crate::config::Config;
use crate::graph::DirDeps;
use crate::sidecar;

/// One canonical tree per analyzed root. Renderers convert this to text/JSON/etc.
#[derive(Serialize)]
pub struct CodebaseMap {
    pub roots: Vec<DirNode>,
    /// Non-fatal manifest-parse warnings from the graph walk. NOT part of the tree — the
    /// model builder never sets it; the shared `build_codebase_map` pipeline attaches the
    /// graph's warnings here so the JSON renderer can surface them in the envelope's
    /// `warnings` array. The text/md
    /// renderers ignore it (they iterate `roots` only), so their output is unchanged.
    pub warnings: Vec<crate::graph::Warning>,
}

/// Annotation coverage across every code file the tree lists: how many carry a first-line
/// annotation (`annotated`) out of the `total`. This is the Layer-0 motivation signal — a
/// code file with no annotation is invisible to an agent reading the tree — surfaced on the
/// unconditional map surfaces (the text footer note and the JSON `coverage` object) so it
/// reaches agents that never invoke `--strict-check`. Counted over the `FileNode`s actually
/// in the tree (post `--max-per-node` truncation); `--strict-check` stays the authoritative,
/// untruncated per-file lister. Coverage is a HAS-ANY-annotation measure (a non-conforming
/// comment still renders in the tree, so the file is visible) — distinct from the strict
/// report's `annotated_count`, which counts strict conformance.
pub struct Coverage {
    pub annotated: u32,
    pub total: u32,
}

impl Coverage {
    /// Incomplete iff at least one listed code file lacks an annotation. Drives the
    /// silent-on-success contract: a fully-annotated tree (or one with no code files)
    /// reports no note and no JSON `coverage` object, so that output stays byte-identical.
    pub fn is_incomplete(&self) -> bool {
        self.total > 0 && self.annotated < self.total
    }
}

impl CodebaseMap {
    /// Whether any listed file takes its annotation from a `<name>.annotation` sidecar — i.e.
    /// whether this map suppressed a real file's row. Drives the one-line exclusion criterion
    /// the text map prints (`walk::ANNOTATION_FILE_CRITERION`); the JSON surface carries the
    /// same fact per row as `FileNode.sidecar` instead.
    pub fn has_sidecar_rows(&self) -> bool {
        self.roots.iter().any(any_sidecar)
    }

    /// Annotation coverage summed across every root's listed code files.
    pub fn coverage(&self) -> Coverage {
        let mut coverage = Coverage {
            annotated: 0,
            total: 0,
        };
        for root in &self.roots {
            accumulate_coverage(root, &mut coverage);
        }
        coverage
    }
}

fn any_sidecar(dir: &DirNode) -> bool {
    dir.files.iter().any(|f| f.sidecar) || dir.dirs.iter().any(any_sidecar)
}

/// Fold one directory subtree's files into the running coverage counts (recursing into
/// subdirectories). Only the `FileNode`s present in the tree are counted — files elided by
/// `--max-per-node` are gone by this point, consistent with counting what the tree shows.
fn accumulate_coverage(dir: &DirNode, coverage: &mut Coverage) {
    for file in &dir.files {
        coverage.total += 1;
        if file.annotation.is_some() {
            coverage.annotated += 1;
        }
    }
    for sub in &dir.dirs {
        accumulate_coverage(sub, coverage);
    }
}

#[derive(Serialize)]
pub struct DirNode {
    pub name: String,
    /// The directory's concern charter, resolved most-explicit-first (a `.annotation`
    /// breadcrumb, else the promoted annotation of the code entry file). `None` — and omitted
    /// from JSON — for a charter-less directory, so such a tree stays byte-for-byte unchanged
    /// and the schema stays additive under `schema: 1`. Rendered on the directory row beside
    /// the observed dep facts: authored intent cross-checked against the graph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charter: Option<Charter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<DirDeps>,
    pub dirs: Vec<DirNode>,
    pub files: Vec<FileNode>,
    /// Subdirectories hidden by the per-node display cap (`--max-per-node`); `0`
    /// (omitted in JSON) unless this directory's subdir count exceeds the cap.
    #[serde(skip_serializing_if = "is_zero")]
    pub elided_dirs: u32,
    /// Files hidden by the per-node display cap; `0` (omitted) unless the file
    /// count exceeds the cap. Distinct from `elided_dirs` so JSON consumers see the
    /// breakdown; the text/md renderers fold both into one marker row.
    #[serde(skip_serializing_if = "is_zero")]
    pub elided_files: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Serialize)]
pub struct FileNode {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<i64>,
    /// True when `annotation` came from the `<name>.annotation` SIDECAR beside this file
    /// rather than from its own first line — the one case where a real file (the sidecar)
    /// carries no row of its own. This is where a JSON consumer reads the exclusion criterion
    /// off the data, the way the text map reads it off the note. Omitted when false, so an
    /// in-file annotation serializes byte-identically.
    #[serde(skip_serializing_if = "is_false")]
    pub sidecar: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Intermediate sorted tree: dirs and files under a directory, keyed by name so
/// iteration is alphabetical (dirs then files, matching `tree`).
#[derive(Default)]
struct RawNode {
    dirs: BTreeMap<String, RawNode>,
    files: BTreeMap<String, PathBuf>,
}

/// Build the canonical model for one root. `files` and `dirs` are absolute paths under
/// `root` as the walk yielded them; `graph` is keyed by canonicalized directory path. All
/// annotation and mtime reads happen here — renderers are pure over the returned tree.
///
/// Every WALKED directory gets a node, whether or not a listable file lies beneath it. The
/// walk is what defines the tree's contents (`crate::walk`), and under a `-L` cap it stops
/// at the cutoff — so "has a listable descendant" is a question the deepest rows can no
/// longer answer, and asking it at one depth but not another would be the worse rule. An
/// empty directory is therefore listed at every depth.
pub fn build(
    root: &Path,
    files: &[PathBuf],
    dirs: &[PathBuf],
    graph: &HashMap<PathBuf, DirDeps>,
    config: &Config,
    max_depth: Option<usize>,
) -> DirNode {
    let mut raw = RawNode::default();
    for dir in dirs {
        if let Ok(rel) = dir.strip_prefix(root) {
            insert_dir(&mut raw, rel);
        }
    }
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

/// Ensure a node exists for `rel` and for every directory on the way to it. The root's own
/// entry (an empty `rel`) has no components, so it is the no-op it should be.
fn insert_dir(node: &mut RawNode, rel: &Path) {
    let mut cursor = node;
    for component in rel.components() {
        let name = component.as_os_str().to_string_lossy().into_owned();
        cursor = cursor.dirs.entry(name).or_default();
    }
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
        // A depth-pruned directory still shows its own row, so a `.annotation` breadcrumb
        // still resolves (a filesystem read needing no children); entry-file promotion cannot,
        // as the subtree is unexpanded — so only the explicit override can surface here.
        return DirNode {
            name,
            charter: charter::read_charter_file(abs_dir).and_then(|c| charter::from_file_body(&c)),
            deps,
            dirs: Vec::new(),
            files: Vec::new(),
            elided_dirs: 0,
            elided_files: 0,
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
            let (annotation, sidecar) = file_annotation(abs, config);
            FileNode {
                name: name.clone(),
                annotation,
                age_secs: age_secs(abs, now, config),
                sidecar,
            }
        })
        .collect();

    // Resolve the charter BEFORE truncation reads from the full child set — entry-file
    // promotion reaches into the already-built children (a crate's `src/lib.rs`, a package's
    // `__init__.py`), which a display cap could otherwise elide out from under it.
    let charter = resolve_charter(abs_dir, &dirs, &files);

    let cap = config.display.max_per_node;
    let (dirs, elided_dirs) = truncate(dirs, cap);
    let (files, elided_files) = truncate(files, cap);

    DirNode {
        name,
        charter,
        deps,
        dirs,
        files,
        elided_dirs,
        elided_files,
    }
}

/// Resolve a directory's charter from the already-built tree, most-explicit-first: a
/// `.annotation` breadcrumb (its presence overrides, even if malformed — then `None` here and
/// `--strict-check` flags it), else the promoted annotation of the code entry file. Promotion
/// REUSES the already-extracted `FileNode.annotation` (no re-parse) — a crate's `src/lib.rs`
/// (else `src/main.rs`), or a direct-child module/package/index/doc entry file. The one
/// annotation grammar splits it (`charter::from_line`); the entry-file tables live in
/// `charter`, so the model and the strict-check filesystem resolver share both.
fn resolve_charter(abs_dir: &Path, dirs: &[DirNode], files: &[FileNode]) -> Option<Charter> {
    if let Some(content) = charter::read_charter_file(abs_dir) {
        return charter::from_file_body(&content);
    }
    if abs_dir.join("Cargo.toml").is_file() {
        if let Some(src) = dirs.iter().find(|d| d.name == "src") {
            if let Some(c) = promote_first(&src.files, charter::CRATE_ENTRY_FILES) {
                return Some(c);
            }
        }
    }
    promote_first(files, charter::DIRECT_ENTRY_FILES)
}

/// The charter promoted from the first file in `files` whose name matches `names` (in order)
/// and that carries an annotation — reusing the child's already-extracted `FileNode.annotation`.
fn promote_first(files: &[FileNode], names: &[&str]) -> Option<Charter> {
    names.iter().find_map(|name| {
        files
            .iter()
            .find(|f| f.name == *name)
            .and_then(|f| f.annotation.as_deref())
            .and_then(charter::from_line)
    })
}

/// Keep at most `cap` items for display, returning the kept items and the count
/// dropped. `None` (no cap) or a list already within the cap is a no-op (0 elided).
fn truncate<T>(mut items: Vec<T>, cap: Option<usize>) -> (Vec<T>, u32) {
    match cap {
        Some(n) if items.len() > n => {
            let elided = (items.len() - n) as u32;
            items.truncate(n);
            (items, elided)
        }
        _ => (items, 0),
    }
}

fn dir_deps(abs_dir: &Path, graph: &HashMap<PathBuf, DirDeps>) -> Option<DirDeps> {
    // `abs_dir` is already canonical (descended from the canonicalized root), so this
    // is a direct lookup — no per-node canonicalize syscall.
    graph.get(abs_dir).cloned()
}

/// Resolve a file's annotation, and whether it came from a sidecar. A bounded head-only read
/// for a file that can hold a comment; an unreadable file, or one with no leading annotation,
/// yields `None` (graceful, never fatal).
fn file_annotation(abs: &Path, config: &Config) -> (Option<String>, bool) {
    if let Some(lang) = config.language_for_path(abs) {
        return (annotation::extract(abs, lang), false);
    }
    // No known language, so the file may carry a `<name>.annotation` sidecar — and its
    // presence WINS over a marker-agnostic read of the file's own head, the same
    // most-explicit-first rule by which a directory's `.annotation` overrides a promoted
    // entry-file line. Without a sidecar the file is here only because `--include` opted it
    // in, so fall back to reading its head with no marker known.
    let body = sidecar::body(abs)
        // A sidecar carrying prose past its one line resolves to NO annotation, exactly as a
        // directory charter does: the row stays silent rather than carrying a newline into it,
        // and `--strict-check` reports the file.
        .filter(|text| annotation::content_past_first_line(text).is_none())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    match body {
        Some(body) => (Some(body), true),
        None => (annotation::extract_any(abs), false),
    }
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
