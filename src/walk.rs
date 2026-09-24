// Concern: what a walk of a root visits — directories to the -L cap, plus recognized, opted-in and sidecar-carrying files | Non-concern: annotations or the graph | IO: (root, Config) -> dirs + files

use std::path::{Path, PathBuf};

use globset::GlobSet;
use ignore::{DirEntry, WalkBuilder};

use crate::config::Config;
use crate::sidecar;

/// METADATA, not content: [`collect_tree`] drops it BY NAME.
pub const CHARTER_FILE: &str = ".annotation";

pub const ANNOTATION_FILE_CRITERION: &str = "a `.annotation` file is never listed as its own row — a directory's `.annotation` charter \
     and a file's `<name>.annotation` sidecar are shown on the row of the path they annotate";

#[derive(Debug, Clone)]
pub struct LimitExceeded {
    pub limit: usize,
    pub root: PathBuf,
}

impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "'{}' exceeds the {}-file limit", self.root.display(), self.limit)
    }
}

impl std::error::Error for LimitExceeded {}

#[derive(Debug, Clone, Copy)]
pub struct WalkFilter<'a> {
    pub gitignore: bool,
    /// ORTHOGONAL to `gitignore`.
    pub hidden: bool,
    pub include_tests: bool,
    pub excludes: &'a GlobSet,
}

impl<'a> WalkFilter<'a> {
    pub fn from_config(config: &Config, excludes: &'a GlobSet) -> Self {
        WalkFilter { gitignore: config.display.gitignore, hidden: config.display.hidden, include_tests: config.display.include_tests, excludes }
    }
}

pub fn configured_walk(root: &Path, filter: WalkFilter<'_>) -> WalkBuilder {
    let root_owned = root.to_path_buf();
    let excludes = filter.excludes.clone();
    let include_tests = filter.include_tests;
    let mut builder = WalkBuilder::new(root);
    builder.hidden(!filter.hidden).parents(false).git_global(false).git_ignore(filter.gitignore).git_exclude(filter.gitignore).ignore(filter.gitignore).require_git(false).filter_entry(move |entry| keep_entry(entry, &root_owned, include_tests, &excludes));
    builder
}

pub(crate) struct WalkedTree {
    pub files: Vec<PathBuf>,
    pub dirs: Vec<PathBuf>,
}

/// `Some(0)` never arrives: `resolve_max_depth` maps 0 to uncapped at both layers.
fn row_depth(max_depth: Option<usize>) -> Option<usize> {
    max_depth
}

/// The manifest walk runs ONE level deeper — see [`cap_manifest_depth`].
pub(crate) fn cap_row_depth(builder: &mut WalkBuilder, max_depth: Option<usize>) {
    builder.max_depth(row_depth(max_depth));
}

/// A package's manifest lives INSIDE it, so this reads every DISPLAYED directory's manifest.
pub(crate) fn cap_manifest_depth(builder: &mut WalkBuilder, max_depth: Option<usize>) {
    builder.max_depth(row_depth(max_depth).map(|deepest_row| deepest_row + 1));
}

/// Pass an EMPTY `GlobSet` for recognized-languages-plus-sidecars.
pub(crate) fn collect_tree(root: &Path, config: &Config, filter: WalkFilter<'_>, include: &GlobSet, max_depth: Option<usize>) -> Result<WalkedTree, LimitExceeded> {
    let max_files = config.limits.max_files;
    let mut builder = configured_walk(root, filter);
    cap_row_depth(&mut builder, max_depth);
    let walker = builder.build();

    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in walker.flatten() {
        let path = entry.path();
        if entry.file_type().is_some_and(|t| t.is_dir()) {
            dirs.push(path.to_path_buf());
            continue;
        }
        let is_file = entry.file_type().is_some_and(|t| t.is_file());
        // A directory's charter and a file's sidecar, both dropped by name.
        if is_file && (path.file_name().is_some_and(|n| n == CHARTER_FILE) || sidecar::target_of(path, config).is_some()) {
            continue;
        }
        // `annotates` tested LAST: it re-resolves the path AND stats a second one.
        let keep = is_file && (config.known_for_path(path) || include_match(path, root, include) || sidecar::annotates(path, config));
        if keep {
            files.push(path.to_path_buf());
            if let Some(limit) = max_files {
                if files.len() > limit {
                    return Err(LimitExceeded { limit, root: root.to_path_buf() });
                }
            }
        }
    }
    Ok(WalkedTree { files, dirs })
}

/// UNCAPPED — `-L` must not shrink what `--strict-check` checks.
pub fn collect_code_files(root: &Path, config: &Config, excludes: &GlobSet, include: &GlobSet) -> Result<Vec<PathBuf>, LimitExceeded> {
    let filter = WalkFilter::from_config(config, excludes);
    collect_tree(root, config, filter, include, None).map(|walked| walked.files)
}

/// Mirrors how [`keep_entry`] tests `-I` excludes.
fn include_match(path: &Path, root: &Path, include: &GlobSet) -> bool {
    if include.is_empty() {
        return false;
    }
    let name = path.file_name().map(|n| n.to_string_lossy());
    if name.is_some_and(|n| include.is_match(n.as_ref())) {
        return true;
    }
    path.strip_prefix(root).is_ok_and(|rel| include.is_match(rel))
}

/// `.git` is pruned BY NAME, ahead of every other test, so NO flag combination reaches it.
fn keep_entry(entry: &DirEntry, root: &Path, include_tests: bool, excludes: &GlobSet) -> bool {
    let name = entry.file_name().to_string_lossy();
    if name == "node_modules" || name == "__pycache__" || name == ".git" {
        return false;
    }
    let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
    if is_dir && !include_tests && name == "tests" {
        return false;
    }
    if !excludes.is_empty() {
        if excludes.is_match(name.as_ref()) {
            return false;
        }
        if let Ok(rel) = entry.path().strip_prefix(root) {
            if excludes.is_match(rel) {
                return false;
            }
        }
    }
    true
}
