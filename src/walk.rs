// Concern: what a walk of a root visits — directories to the -L cap, plus recognized, opted-in and sidecar-carrying files | Non-concern: annotations or the graph | IO: (root, Config) -> dirs + files

use std::path::{Path, PathBuf};

use globset::GlobSet;
use ignore::{DirEntry, WalkBuilder};

use crate::config::Config;
use crate::sidecar;

/// The metadata filename a directory carries its concern charter in — a bare three-field line whose
/// only subject is the enclosing directory. METADATA, not content: [`collect_tree`] drops it BY NAME
/// so no flag combination lists it, and `crate::charter` reads it directly.
/// Suffixed onto a FILE (`trials.csv.annotation`) the same name is that file's sidecar.
pub const CHARTER_FILE: &str = ".annotation";

/// Why a `.annotation` file never appears as its own row — the exclusion criterion a Report states
/// so a reader can apply it to any path. It covers both scales: a directory's charter and a file's
/// sidecar, both dropped by [`collect_tree`]. Declared here, at the walk that
/// enforces it, and rendered by `lib::run` rather than restated there.
pub const ANNOTATION_FILE_CRITERION: &str =
    "a `.annotation` file is never listed as its own row — a directory's `.annotation` charter \
     and a file's `<name>.annotation` sidecar are shown on the row of the path they annotate";

/// The walk was aborted because a root exceeded its `max_files` cap. A typed boundary error: the
/// walk stops before any model, graph or render work, and the caller decides how to surface it.
/// Carries the `limit` and offending `root` — all a caller needs to phrase its diagnostic.
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

/// Everything one walk filters ON, as a single value. A params struct rather than four positional
/// arguments, three of them `bool`, which is how a caller passes them in the wrong order — and so
/// the code-file walk and the manifest walk are handed the SAME policy instead of two lists free to
/// drift apart.
#[derive(Debug, Clone, Copy)]
pub struct WalkFilter<'a> {
    pub gitignore: bool,
    /// Descend into dot-directories and list dot-files. ORTHOGONAL to `gitignore`: a hidden path
    /// that `.gitignore` also names stays pruned unless `gitignore` is off too. `.git` is never
    /// walked whatever this holds — [`keep_entry`] prunes it by name.
    pub hidden: bool,
    pub include_tests: bool,
    pub excludes: &'a GlobSet,
}

impl<'a> WalkFilter<'a> {
    /// The walk policy a resolved [`Config`] plus the run's `-I` globs describe — the ONE place
    /// display settings become filtering, so what a root shows and what it graphs cannot disagree.
    pub fn from_config(config: &Config, excludes: &'a GlobSet) -> Self {
        WalkFilter {
            gitignore: config.display.gitignore,
            hidden: config.display.hidden,
            include_tests: config.display.include_tests,
            excludes,
        }
    }
}

/// The single directory-filtering policy shared by every walk of the tree: honor `.gitignore`, skip
/// hidden files unless `filter.hidden`, prune `node_modules`/`__pycache__`/`.git`/`tests`, and apply
/// the `-I` excludes. Both the code-file walk and the manifest walk build on it, so "what's graphed"
/// equals "what's shown" — they differ ONLY in which surviving entries they keep.
pub fn configured_walk(root: &Path, filter: WalkFilter<'_>) -> WalkBuilder {
    let root_owned = root.to_path_buf();
    let excludes = filter.excludes.clone();
    let include_tests = filter.include_tests;
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!filter.hidden)
        .parents(false)
        .git_global(false)
        .git_ignore(filter.gitignore)
        .git_exclude(filter.gitignore)
        .ignore(filter.gitignore)
        .require_git(false)
        .filter_entry(move |entry| keep_entry(entry, &root_owned, include_tests, &excludes));
    builder
}

/// Everything ONE walk of a root yields: the files to annotate, and every directory visited. A
/// directory earns its row by being VISITED, not by holding a listable file beneath it — with the
/// walk capped by `-L` nothing below the cutoff is visited, so that question cannot be asked there,
/// and answering it at one depth but not another would be worse. Both lists are in walker order.
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

/// Bound a walk at the deepest ROW the render can show, so the traversal STOPS there instead of
/// visiting the whole tree and discarding it at render time. This governs what the map LISTS. The
/// manifest walk deliberately runs ONE level deeper ([`cap_manifest_depth`]): different numbers for
/// different jobs — this decides which paths are listed, that which rows state their own dep facts.
pub(crate) fn cap_row_depth(builder: &mut WalkBuilder, max_depth: Option<usize>) {
    builder.max_depth(row_depth(max_depth));
}

/// Bound the MANIFEST walk one level below the deepest row ([`cap_row_depth`]), because a package's
/// manifest lives INSIDE the package. So this reads the manifest of every DISPLAYED directory and
/// of no other: a package below the cutoff is no row and contributes no edges. This walk yields no
/// rows itself, so the extra level can never add a path to the map.
pub(crate) fn cap_manifest_depth(builder: &mut WalkBuilder, max_depth: Option<usize>) {
    builder.max_depth(row_depth(max_depth).map(|deepest_row| deepest_row + 1));
}

/// Walk `root` once to `max_depth`, collecting every directory plus every file to annotate: known
/// extensions, `include` glob matches, and files carrying a `<name>.annotation` sidecar. Pass an
/// EMPTY `GlobSet` for recognized-languages-plus-sidecars. `node_modules`, `__pycache__`, `.git`
/// and `tests` are pruned. Aborts with `LimitExceeded` once the filtered FILE count passes the cap.
pub(crate) fn collect_tree(
    root: &Path,
    config: &Config,
    filter: WalkFilter<'_>,
    include: &GlobSet,
    max_depth: Option<usize>,
) -> Result<WalkedTree, LimitExceeded> {
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
        // Both scales of the one criterion, dropped where the tree's contents are defined: a directory's charter, and a sidecar. The charter is named rather than left to the hidden prune `--hidden` switches off, which `target_of` cannot stand in for — the bare name has no target file.
        if is_file
            && (path.file_name().is_some_and(|n| n == CHARTER_FILE)
                || sidecar::target_of(path, config).is_some())
        {
            continue;
        }
        // `known_for_path` answers from the extension alone wherever there is one, so only an extensionless path reaches its shebang probe. `annotates` is tested LAST because it re-resolves the path AND stats a second one, which a recognized or include-matched file never pays for.
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
    let filter = WalkFilter::from_config(config, excludes);
    collect_tree(root, config, filter, include, None).map(|walked| walked.files)
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

/// Whether one visited entry survives the shared policy. `.git` is pruned BY NAME, ahead of every
/// other test, so NO flag combination reaches it: `--hidden` makes it visible to the walker and
/// `--no-gitignore` drops the ignore rules, yet a repository's own object store is never a row.
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
