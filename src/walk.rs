// Concern: yields the set of files to annotate under a root — recognized-language files plus any `--include` glob matches — applying gitignore, test/vendor pruning, and exclude globs | Non-concern: annotations or graph | IO: (root, Config, excludes, include) -> [file paths]

use std::path::{Path, PathBuf};

use globset::GlobSet;
use ignore::{DirEntry, WalkBuilder};

use crate::config::Config;

/// The metadata filename a directory carries its concern charter in — a bare three-field
/// annotation line whose only subject is the enclosing directory. Recognized as METADATA,
/// not content: it is dot-hidden (so the walk below, which sets `.hidden(true)`, never emits
/// it as a tree node) and extension-less (so `collect_code_files` never treats it as a code
/// file). It is instead read directly by charter resolution (`crate::charter`), the one read
/// the display filters must not hide. Named here, at the walk that defines what the tree shows,
/// so "the file the tree omits" and "the file charter resolution reads" reference one constant.
pub const CHARTER_FILE: &str = ".annotation";

/// The walk was aborted because a root exceeded its `max_files` cap. A typed
/// boundary error (Fail-Fast): the walk stops before any model/graph/render work,
/// and the caller decides how to surface it (`lib::run` exits 2; the `--mcp` surface
/// returns a structured tool error). Carries the `limit` and offending `root` — all
/// either surface needs to phrase its diagnostic.
#[derive(Debug, Clone)]
pub struct LimitExceeded {
    pub limit: usize,
    pub root: PathBuf,
}

impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' exceeds the {}-file limit",
            self.root.display(),
            self.limit
        )
    }
}

// A real `std::error::Error` so a library consumer can bubble `collect_code_files` failures
// through `?` into `anyhow`/`Box<dyn Error>` like any other error, not just match the struct.
impl std::error::Error for LimitExceeded {}

/// The single directory-filtering policy shared by every walk of the tree: honor
/// `.gitignore` (per `gitignore`), skip hidden files, prune `node_modules`/
/// `__pycache__`/`.git`/`tests` (the last unless `include_tests`), and apply the
/// `-I/--ignore` `excludes`. Both the code-file walk and the manifest/graph walk
/// build on this so that "what's graphed" equals "what's shown" — they differ ONLY
/// in which surviving entries they keep (known-extension files vs. manifest names).
pub fn configured_walk(
    root: &Path,
    gitignore: bool,
    include_tests: bool,
    excludes: &GlobSet,
) -> WalkBuilder {
    let root_owned = root.to_path_buf();
    let excludes = excludes.clone();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .parents(false)
        .git_global(false)
        .git_ignore(gitignore)
        .git_exclude(gitignore)
        .ignore(gitignore)
        .require_git(false)
        .filter_entry(move |entry| keep_entry(entry, &root_owned, include_tests, &excludes));
    builder
}

/// Collect every file under `root` to annotate, in the walker's order: those whose extension
/// maps to a known language, PLUS any that match the `include` selector globs (the `--include`
/// positive filter, letting an unrecognized or extensionless file into the tree). Pass an
/// EMPTY `GlobSet` for the recognized-languages-only behaviour (the strict-check path does, so
/// linting never reaches a file whose comment grammar is unknown). Directories named
/// `node_modules`, `__pycache__`, `.git`, and `tests` (unless enabled) are pruned wholesale.
/// Aborts with `LimitExceeded` the instant the (already-filtered) file count exceeds
/// `config.limits.max_files`; a `None` cap never trips.
pub fn collect_code_files(
    root: &Path,
    config: &Config,
    excludes: &GlobSet,
    include: &GlobSet,
) -> Result<Vec<PathBuf>, LimitExceeded> {
    let max_files = config.limits.max_files;
    let walker = configured_walk(
        root,
        config.display.gitignore,
        config.display.include_tests,
        excludes,
    )
    .build();

    let mut files = Vec::new();
    for entry in walker.flatten() {
        let path = entry.path();
        let keep = entry.file_type().is_some_and(|t| t.is_file())
            && (config.known_for_path(path) || include_match(path, root, include));
        if keep {
            files.push(path.to_path_buf());
            if let Some(limit) = max_files {
                if files.len() > limit {
                    return Err(LimitExceeded {
                        limit,
                        root: root.to_path_buf(),
                    });
                }
            }
        }
    }
    Ok(files)
}

/// Whether `path` matches an `--include` selector — by bare file name (so `--include '*.sh'`
/// catches a script anywhere) OR by root-relative path (so `--include 'scripts/**'` scopes to a
/// subtree), mirroring how [`keep_entry`] tests `-I` excludes. An empty selector set never
/// matches, so the default walk (recognized languages only) is unchanged.
fn include_match(path: &Path, root: &Path, include: &GlobSet) -> bool {
    if include.is_empty() {
        return false;
    }
    let name = path.file_name().map(|n| n.to_string_lossy());
    if name.is_some_and(|n| include.is_match(n.as_ref())) {
        return true;
    }
    path.strip_prefix(root)
        .is_ok_and(|rel| include.is_match(rel))
}

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
