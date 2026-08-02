// Concern: what a walk of a root visits — directories to the -L cap, plus recognized, opted-in and sidecar-carrying files | Non-concern: annotations or the graph | IO: (root, Config) -> dirs + files

use std::path::{Path, PathBuf};

use globset::GlobSet;
use ignore::{DirEntry, WalkBuilder};

use crate::config::Config;
use crate::sidecar;

/// The metadata filename a directory carries its concern charter in — a bare three-field
/// annotation line whose only subject is the enclosing directory. Recognized as METADATA,
/// not content: it is dot-hidden (so the walk below, which sets `.hidden(true)`, never emits
/// it as a tree node) and extension-less (so `collect_code_files` never treats it as a code
/// file). It is instead read directly by charter resolution (`crate::charter`), the one read
/// the display filters must not hide. Named here, at the walk that defines what the tree shows,
/// so "the file the tree omits" and "the file charter resolution reads" reference one constant.
///
/// The same name suffixed onto a FILE (`trials.csv.annotation`) is that file's sidecar
/// (`crate::sidecar`) — one metadata name at both scales, and one exclusion criterion below.
pub const CHARTER_FILE: &str = ".annotation";

/// Why a `.annotation` file never appears as its own row — the exclusion criterion a Report
/// states so a reader can apply it to any path. It covers both scales: a directory's charter
/// (dot-hidden, pruned by the walk's `hidden(true)`) and a file's `<name>.annotation` sidecar
/// (dropped by [`collect_code_files`] below). Declared here, at the walk that enforces it, and
/// rendered by `lib::run` rather than restated there.
pub const ANNOTATION_FILE_CRITERION: &str =
    "a `.annotation` file is never listed as its own row — a directory's `.annotation` charter \
     and a file's `<name>.annotation` sidecar are shown on the row of the path they annotate";

/// The walk was aborted because a root exceeded its `max_files` cap. A typed
/// boundary error (Fail-Fast): the walk stops before any model/graph/render work,
/// and the caller decides how to surface it (`lib::run` exits with `exit::RUNAWAY_SCOPE`,
/// on `--format json` after emitting the structured error envelope). Carries the `limit`
/// and offending `root` — all a caller needs to phrase its diagnostic.
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

// A real `std::error::Error` so a library consumer can bubble `collect_code_files` failures through `?` into `anyhow`/`Box<dyn Error>` like any other error, not just match the struct.
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

/// Everything ONE walk of a root yields: the files to annotate, and every directory visited
/// (the root itself included). Directories are carried alongside the files because a directory
/// earns its row by being VISITED, not by holding a listable file somewhere beneath it — with
/// the walk capped by `-L` nothing below the cutoff is visited, so "does a listable file lie
/// under this directory?" can no longer be asked there, and answering it at one depth but not
/// another would be worse than either rule. Both lists are in the walker's order.
pub(crate) struct WalkedTree {
    pub files: Vec<PathBuf>,
    pub dirs: Vec<PathBuf>,
}

/// The deepest level a `-L LEVEL` render can put a ROW on. Floored at 1: depth 0 is the
/// root's own contents, which every render expands (`-L 0` and `-L 1` are the same view), so
/// a walk bounded at 0 would starve the render of every row. `None` is unbounded.
fn row_depth(max_depth: Option<usize>) -> Option<usize> {
    max_depth.map(|level| level.max(1))
}

/// Bound a walk at the deepest ROW the render can show, so the traversal STOPS there instead
/// of visiting the whole tree and discarding it at render time. This governs what the map
/// LISTS — every directory and file it yields is a row.
///
/// The manifest walk deliberately runs ONE level deeper ([`cap_manifest_depth`]). The two
/// bounds are different numbers for different jobs and must not be collapsed back into one:
/// this one decides which paths are listed, that one decides which rows can state their own
/// dependency facts.
pub(crate) fn cap_row_depth(builder: &mut WalkBuilder, max_depth: Option<usize>) {
    builder.max_depth(row_depth(max_depth));
}

/// Bound the MANIFEST walk one level below the deepest row ([`cap_row_depth`]) — because a
/// package's manifest lives INSIDE the package, one level under the row that names it. At
/// row depth N a package directory is listed only if it sits at depth ≤ N, and its
/// `Cargo.toml`/`package.json`/`pyproject.toml`/`go.mod` therefore sits at depth ≤ N+1. So
/// this bound reads the manifest of every directory the map DISPLAYS, and of no other:
/// a package below the cutoff is not a row, its manifest is at depth ≥ N+2, and it
/// contributes no edges — the `-L` cap still cuts the graph's input, it just no longer cuts
/// a visible row's own facts out from under it.
///
/// This walk yields no rows (`crate::graph` keeps only manifests, and the model looks up
/// dep facts by directory), so the extra level can never add a path to the map.
pub(crate) fn cap_manifest_depth(builder: &mut WalkBuilder, max_depth: Option<usize>) {
    builder.max_depth(row_depth(max_depth).map(|deepest_row| deepest_row + 1));
}

/// Walk `root` once, down to `max_depth`, and collect what the tree shows: every directory
/// visited, plus every file to annotate — those whose extension maps to a known language, PLUS
/// any that match the `include` selector globs (the `--include` positive filter, letting an
/// unrecognized or extensionless file into the tree), PLUS any that carry a `<name>.annotation`
/// sidecar (writing the sidecar is itself the opt-in — see [`sidecar::annotates`]). Pass an
/// EMPTY `GlobSet` for the recognized-languages-plus-sidecars behaviour (the strict-check path
/// does, so linting never reaches a file whose comment grammar is unknown and which has no
/// sidecar either). Sidecar files themselves are dropped, per [`ANNOTATION_FILE_CRITERION`].
/// Directories named `node_modules`, `__pycache__`, `.git`, and `tests` (unless enabled) are
/// pruned wholesale. Aborts with `LimitExceeded` the instant the (already-filtered) FILE count
/// exceeds `config.limits.max_files`; a `None` cap never trips. The count is over what this
/// capped walk visits — a file below the depth cutoff is never reached, so it can no longer
/// abort a run that would never have shown it.
pub(crate) fn collect_tree(
    root: &Path,
    config: &Config,
    excludes: &GlobSet,
    include: &GlobSet,
    max_depth: Option<usize>,
) -> Result<WalkedTree, LimitExceeded> {
    let max_files = config.limits.max_files;
    let mut builder = configured_walk(
        root,
        config.display.gitignore,
        config.display.include_tests,
        excludes,
    );
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
        // A sidecar is metadata about the file beside it, not a file the map describes, so it is dropped here where the tree's contents are defined — the file-scale twin of the dot-hidden directory charter, under the one stated criterion.
        if is_file && sidecar::target_of(path, config).is_some() {
            continue;
        }
        // `annotates` stats the sidecar path, so it is tested LAST: a recognized or include-matched file is already kept and never pays for it.
        let keep = is_file
            && (config.known_for_path(path)
                || include_match(path, root, include)
                || sidecar::annotates(path, config));
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
    Ok(WalkedTree { files, dirs })
}

/// The UNCAPPED, files-only view of [`collect_tree`] — the whole in-scope file set under
/// `root`, whatever its depth. This is what `--strict-check` lints (a gate is not a rendered
/// view, so `-L` must not shrink what it checks) and what a library consumer composing its own
/// renderer gets.
pub fn collect_code_files(
    root: &Path,
    config: &Config,
    excludes: &GlobSet,
    include: &GlobSet,
) -> Result<Vec<PathBuf>, LimitExceeded> {
    collect_tree(root, config, excludes, include, None).map(|walked| walked.files)
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
