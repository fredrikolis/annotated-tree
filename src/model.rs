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

#[derive(Serialize)]
pub struct CodebaseMap {
    pub roots: Vec<DirNode>,
    /// NOT part of the tree — attached separately so text/markdown renderers stay unaffected.
    pub warnings: Vec<crate::graph::Warning>,
}

/// Distinct from the strict report's `annotated_count`; this counts post-truncation.
pub struct Coverage {
    pub annotated: u32,
    pub total: u32,
}

impl Coverage {
    pub fn is_incomplete(&self) -> bool {
        self.total > 0 && self.annotated < self.total
    }
}

impl CodebaseMap {
    /// Drives the text map's exclusion note (`walk::ANNOTATION_FILE_CRITERION`).
    pub fn has_sidecar_rows(&self) -> bool {
        self.roots.iter().any(any_sidecar)
    }

    pub fn coverage(&self) -> Coverage {
        let mut coverage = Coverage { annotated: 0, total: 0 };
        for root in &self.roots {
            accumulate_coverage(root, &mut coverage);
        }
        coverage
    }
}

fn any_sidecar(dir: &DirNode) -> bool {
    dir.files.iter().any(|f| f.sidecar) || dir.dirs.iter().any(any_sidecar)
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charter: Option<Charter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<DirDeps>,
    pub dirs: Vec<DirNode>,
    pub files: Vec<FileNode>,
    #[serde(skip_serializing_if = "is_zero")]
    pub elided_dirs: u32,
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
    /// From the `<name>.annotation` SIDECAR rather than the file's own first line.
    #[serde(skip_serializing_if = "is_false")]
    pub sidecar: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Default)]
struct RawNode {
    dirs: BTreeMap<String, RawNode>,
    files: BTreeMap<String, PathBuf>,
}

/// Every WALKED directory gets a node whether or not a listable file lies beneath it.
pub fn build(root: &Path, files: &[PathBuf], dirs: &[PathBuf], graph: &HashMap<PathBuf, DirDeps>, config: &Config, max_depth: Option<usize>) -> DirNode {
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
    // No per-node canonicalize syscall: joining child names onto a once-canonicalized root stays canonical.
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let now = SystemTime::now();
    convert(&raw, &canon_root, graph, config, now, max_depth, 0)
}

/// The root's own entry (an empty `rel`) has no components, so it is a no-op.
fn insert_dir(node: &mut RawNode, rel: &Path) {
    let mut cursor = node;
    for component in rel.components() {
        let name = component.as_os_str().to_string_lossy().into_owned();
        cursor = cursor.dirs.entry(name).or_default();
    }
}

fn insert(node: &mut RawNode, rel: &Path, abs: &Path) {
    let components: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let Some((file, dirs)) = components.split_last() else {
        return;
    };
    let mut cursor = node;
    for dir in dirs {
        cursor = cursor.dirs.entry(dir.clone()).or_default();
    }
    cursor.files.insert(file.clone(), abs.to_path_buf());
}

fn convert(node: &RawNode, abs_dir: &Path, graph: &HashMap<PathBuf, DirDeps>, config: &Config, now: SystemTime, max_depth: Option<usize>, depth: usize) -> DirNode {
    let deps = dir_deps(abs_dir, graph);
    let name = abs_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    let pruned = max_depth.is_some_and(|limit| depth >= limit);
    if pruned {
        // A `.annotation` breadcrumb still resolves (no children needed); entry-file promotion cannot.
        return DirNode { name, charter: charter::read_charter_file(abs_dir).and_then(|c| charter::from_file_body(&c)), deps, dirs: Vec::new(), files: Vec::new(), elided_dirs: 0, elided_files: 0 };
    }

    let dirs: Vec<DirNode> = node.dirs.iter().map(|(child, sub)| convert(sub, &abs_dir.join(child), graph, config, now, max_depth, depth + 1)).collect();

    let files: Vec<FileNode> = node
        .files
        .iter()
        .map(|(name, abs)| {
            let (annotation, sidecar) = file_annotation(abs, config);
            FileNode { name: name.clone(), annotation, age_secs: age_secs(abs, now, config), sidecar }
        })
        .collect();

    // BEFORE truncation: a display cap could otherwise elide the entry file promotion reaches into.
    let charter = resolve_charter(abs_dir, &dirs, &files);

    let cap = config.display.max_per_node;
    let (dirs, elided_dirs) = truncate(dirs, cap);
    let (files, elided_files) = truncate(files, cap);

    DirNode { name, charter, deps, dirs, files, elided_dirs, elided_files }
}

/// A `.annotation` breadcrumb overrides even when malformed; else the promoted entry file.
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

fn promote_first(files: &[FileNode], names: &[&str]) -> Option<Charter> {
    names.iter().find_map(|name| files.iter().find(|f| f.name == *name).and_then(|f| f.annotation.as_deref()).and_then(charter::from_line))
}

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
    graph.get(abs_dir).cloned()
}

/// A sidecar's presence WINS over a marker-agnostic read of the file's own head.
fn file_annotation(abs: &Path, config: &Config) -> (Option<String>, bool) {
    if let Some(lang) = config.language_for_path(abs) {
        return (annotation::extract(abs, lang), false);
    }
    let body = sidecar::body(abs)
        // Prose past the one line resolves to NO annotation; the row stays silent, `--strict-check` reports it.
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
    Some(now.duration_since(modified).map(|d| d.as_secs() as i64).unwrap_or(-1))
}
