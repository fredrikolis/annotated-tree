// Concern: the library surface — run(), plus the walk, annotation and render primitives | Non-concern: argv parsing | IO: (Cli, writer) -> exit_code + edits
// Deliberately synchronous: a one-shot batch traversal with no concurrent I/O wait to overlap. The `ignore` crate parallelizes the disk work across a thread pool.

//! # `annotated-tree` as a library
//! Two entry styles: whole-tool ([`parse_cli`] + [`run`]), or primitives — [`walk`],
//! [`annotation`], [`config`] and [`build_globset`], plus the access-only map/render surface
//! ([`CodebaseMap`] + [`for_format`]). No stability promise; a break arrives as a compile error.
//! ```no_run
//! use annotated_tree::config::{CliOverrides, Config};
//! use annotated_tree::{annotation, walk};
//! use globset::GlobSet;
//! use std::path::Path;
//!
//! let root = Path::new(".");
//! let config = Config::load(root, &CliOverrides::default()).expect("resolve config");
//! // Empty exclude + include sets: recognized-language files only, no `--include` widening.
//! let empty = GlobSet::empty();
//! let files = walk::collect_code_files(root, &config, &empty, &empty).expect("walk");
//! for path in &files {
//!     if let Some(lang) = config.language_for_path(path) {
//!         if let Some(note) = annotation::extract(path, lang) {
//!             println!("{}: {note}", path.display());
//!         }
//!     }
//! }
//! ```

pub mod annotation;
pub(crate) mod bash_annotator;
pub(crate) mod changed;
pub(crate) mod charter;
pub(crate) mod cli;
pub mod config;
pub mod exit;
pub(crate) mod githook;
pub(crate) mod graph;
pub(crate) mod guide;
pub(crate) mod manifest;
pub(crate) mod model;
pub(crate) mod render;
pub(crate) mod rules;
pub(crate) mod sidecar;
pub(crate) mod strict;
pub(crate) mod strip;
pub(crate) mod util;
pub mod walk;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use globset::GlobSet;

pub use cli::{parse as parse_cli, Cli};
pub use util::build_globset;

/// Resolve a directory's charter from the filesystem. A relative path is taken against the
/// process working directory.
pub use charter::resolve_from_fs as resolve_charter;
pub use charter::Charter;
pub use cli::Format;
use config::{CliOverrides, Config};
pub use graph::{DirDeps, InternalDep, Warning};
pub use model::{CodebaseMap, Coverage, DirNode, FileNode};
pub use render::{for_format, Renderer};
use walk::LimitExceeded;

/// A build-pipeline failure, split so each caller classifies it into the right dispatch code:
/// `Limit` is the runaway-scope trip, `Git` a `--since` git failure, and `Other` any remaining
/// precondition failure. Git is split from `Other` only so the two map to distinct,
/// caller-actionable codes.
pub(crate) enum BuildError {
    Limit(LimitExceeded),
    Git(anyhow::Error),
    Other(anyhow::Error),
}

/// A classified run failure: its process exit code, a stable string dispatch `code`, a human
/// message, and the offending path when known. One object per failure class so an agent branches on
/// `code` and never on prose — mirroring how [`strict::AnnotationViolation`] carries a structured,
/// dispatchable diagnostic rather than one opaque string.
pub(crate) struct Failure {
    exit_code: i32,
    code: &'static str,
    message: String,
    path: Option<String>,
}

impl Failure {
    /// A supplied root path is not an existing directory ([`exit::code::NOT_A_DIRECTORY`]).
    fn not_a_directory(message: String) -> Self {
        Failure {
            exit_code: exit::PRECONDITION,
            code: exit::code::NOT_A_DIRECTORY,
            message,
            path: None,
        }
    }

    /// A `--since` git operation failed ([`exit::code::GIT_ERROR`]).
    fn git(message: String) -> Self {
        Failure {
            exit_code: exit::PRECONDITION,
            code: exit::code::GIT_ERROR,
            message,
            path: None,
        }
    }

    /// Any other precondition/environment failure — bad config, invalid `-I` glob, I/O
    /// ([`exit::code::PRECONDITION`]).
    fn precondition(message: String) -> Self {
        Failure {
            exit_code: exit::PRECONDITION,
            code: exit::code::PRECONDITION,
            message,
            path: None,
        }
    }

    /// Dual-render this failure: under `--format json` emit the structured error envelope
    /// to stdout and return the exit code (an agent parses stdout, never empty output +
    /// prose-only stderr); otherwise return the message as an `Err` so the binary renders
    /// it as `error:` prose on stderr exactly as before. One classification, two surfaces.
    fn dispatch(self, out: &mut impl Write, format: Format) -> Result<i32> {
        if format == Format::Json {
            writeln!(
                out,
                "{}",
                render::json::render_error(self.code, &self.message, self.path.as_deref())
            )?;
            Ok(self.exit_code)
        } else {
            Err(anyhow!(self.message))
        }
    }
}

/// Execute the parsed command, writing output to `out`. Returns a process exit code from the
/// [`exit`] taxonomy, which documents each class. On a failure under `--format json` a structured
/// error envelope (code from [`exit::code`]) is written to `out` first, so an agent parsing stdout
/// gets a dispatch key instead of empty output; other formats surface the failure as prose on `err`.
pub fn run(cli: &Cli, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    // A maintenance, which CORE4 exempts. It takes the same overrides and `-I` globs every other run does, on named files as well as walked ones: a destructive verb where the flag that narrows it went half-inert would be the worst of both.
    if let Some(cli::Command::Strip(args)) = &cli.command {
        return Ok(strip::dispatch(args, cli, out, err));
    }
    // An accessory: it points at no Workspace and emits no Report, so nothing in SPEC.md governs it.
    if let Some(cli::Command::BashAnnotator(args)) = &cli.command {
        return bash_annotator::dispatch(args, out, err);
    }

    // An info flag, printed before any traversal, so an agent can fetch the wire contract without a repo to walk.
    if cli.schema {
        return print_schema(out);
    }

    if cli.githook_guide {
        write!(out, "{}", githook::text())?;
        return Ok(exit::SUCCESS);
    }

    if cli.annotation_guide {
        write!(out, "{}", guide::full())?;
        return Ok(exit::SUCCESS);
    }

    // Strict-check accepts a single file as well as a directory; the tree render is directory-only, so its resolver stays strict.
    let roots = if cli.strict_check {
        match resolve_lint_targets(&cli.paths) {
            Ok(roots) => roots,
            Err(e) => return Failure::not_a_directory(format!("{e:#}")).dispatch(out, cli.format),
        }
    } else {
        match resolve_roots(&cli.paths) {
            Ok(roots) => roots,
            Err(e) => return Failure::not_a_directory(format!("{e:#}")).dispatch(out, cli.format),
        }
    };

    let overrides = cli.overrides();
    let excludes = match util::build_globset(&cli.ignore) {
        Ok(excludes) => excludes,
        Err(e) => return Failure::precondition(format!("{e:#}")).dispatch(out, cli.format),
    };

    if cli.strict_check {
        // Each target validates against ITS OWN discovered config, so a multi-target run never applies target A's languages to target B. Every verdict resolves before a single stdout byte, so an abort writes no partial report.
        let mut reports: Vec<(strict::StrictReport, Option<usize>)> = Vec::new();
        for target in &roots {
            let (report, max_per_node) = if target.is_file() {
                let parent = target.parent().filter(|p| !p.as_os_str().is_empty());
                let parent = parent.map_or_else(|| PathBuf::from("."), Path::to_path_buf);
                let config = match Config::load(&parent, &overrides) {
                    Ok(config) => config,
                    Err(e) => {
                        return Failure::precondition(format!("{e:#}")).dispatch(out, cli.format)
                    }
                };
                if config.language_for_path(target).is_none() {
                    return Failure::precondition(format!(
                        "not a lintable code file: {} — its extension maps to no configured language",
                        target.display()
                    ))
                    .dispatch(out, cli.format);
                }
                let files = vec![target.clone()];
                let report = strict::check_file(&parent, &files, &config);
                (report, config.display.max_per_node)
            } else {
                let config = match Config::load(target, &overrides) {
                    Ok(config) => config,
                    Err(e) => {
                        return Failure::precondition(format!("{e:#}")).dispatch(out, cli.format)
                    }
                };
                // `--include` never widens what the gate validates: a file whose comment grammar is unknown cannot be linted.
                let files =
                    match walk::collect_code_files(target, &config, &excludes, &GlobSet::empty()) {
                        Ok(files) => files,
                        Err(e) => return report_limit_exceeded(out, err, cli.format, &e),
                    };
                let report = strict::check_structured(target, &files, &config, &excludes);
                (report, config.display.max_per_node)
            };
            reports.push((report, max_per_node));
        }
        // `--format json` folds the targets into ONE document; text/md keep the per-target report. Exit 0 iff every violation set is empty.
        if cli.format == cli::Format::Json {
            let mut report = strict::StrictReport::empty();
            for (r, _) in reports {
                report.merge(r);
            }
            writeln!(out, "{}", report.to_json())?;
            return Ok(if report.passed {
                exit::SUCCESS
            } else {
                exit::STRICT_FAILURE
            });
        }
        let mut code = exit::SUCCESS;
        for (report, max_per_node) in &reports {
            let (text, root_code) = report.to_text(*max_per_node);
            out.write_all(text.as_bytes())?;
            code = code.max(root_code);
        }
        // The guide rides the surface an agent already reads. Never on success, and never on JSON, which stays a clean parse.
        if code == exit::STRICT_FAILURE && !cli.no_guide {
            write!(out, "\n{}", guide::full())?;
        }
        return Ok(code);
    }

    // ONE shared pipeline, and the walk happens inside it — so a runaway trips before any render or stdout write, making abort ⇒ empty stdout free for every format.
    let since = cli.since_ref();
    let (map, ascii) = match build_codebase_map(
        &roots,
        &overrides,
        &excludes,
        since.as_deref(),
        cli.max_depth,
    ) {
        Ok(built) => built,
        Err(BuildError::Limit(e)) => return report_limit_exceeded(out, err, cli.format, &e),
        Err(BuildError::Git(e)) => return Failure::git(format!("{e:#}")).dispatch(out, cli.format),
        Err(BuildError::Other(e)) => {
            return Failure::precondition(format!("{e:#}")).dispatch(out, cli.format)
        }
    };
    // `--ignore-parsing-errors` governs only the stderr echo: the JSON envelope carries the warnings regardless, so an agent parsing stdout never has to scrape stderr to learn the graph is incomplete.
    if !cli.ignore_parsing_errors {
        for warning in &map.warnings {
            writeln!(err, "warning: {}", warning.message)?;
        }
    }

    // The glyph set is a terminal concern, not a per-repo one, so the primary root's resolved `ascii` is reused rather than re-loading config on the render path.
    let renderer = render::for_format(cli.format, ascii);
    writeln!(out, "{}", renderer.render(&map))?;

    // Silent at full coverage, and never on the JSON surface, so the stdout tree stays a byte-identical parse.
    if cli.format == Format::Text && map.has_sidecar_rows() {
        writeln!(err, "note: {}.", walk::ANNOTATION_FILE_CRITERION)?;
    }

    if cli.format == Format::Text {
        let coverage = map.coverage();
        if coverage.is_incomplete() {
            writeln!(
                err,
                "note: {} of {} files carry an agent-navigable annotation; the rest are \
                 invisible to an agent reading this tree. Run 'annotated-tree --strict-check' \
                 to list them.",
                coverage.annotated, coverage.total,
            )?;
        }
    }
    Ok(exit::SUCCESS)
}

/// The one build pipeline: walk every root (runaway-scope and `max_depth` capped, so nothing below
/// the deepest displayable level is visited, counted or graphed), optionally filter to the
/// `--since` change set plus its blast radius, build the graph, and assemble the `CodebaseMap`.
/// Returns the map — which carries the manifest warnings — and the primary root's `ascii` choice.
pub(crate) fn build_codebase_map(
    roots: &[PathBuf],
    overrides: &CliOverrides,
    excludes: &GlobSet,
    since: Option<&str>,
    max_depth: Option<usize>,
) -> std::result::Result<(model::CodebaseMap, bool), BuildError> {
    // Each root uses its OWN discovered config; a multi-root run never applies one root's repo config to another. All roots are walked up front so a runaway trips before any graph build or render.
    let mut walked_roots = Vec::new();
    for root in roots {
        let config = Config::load(root, overrides).map_err(BuildError::Other)?;
        let include = util::build_globset(&config.display.include).map_err(BuildError::Other)?;
        // `max_depth` bounds the WALK, not just the render: below the cutoff nothing is visited, stat'd, read, or counted against `--max-files`.
        match walk::collect_tree(root, &config, excludes, &include, max_depth) {
            Ok(walked) => walked_roots.push((root, config, walked)),
            Err(e) => return Err(BuildError::Limit(e)),
        }
    }

    // The manifest walk uses the PRIMARY root's gitignore + include_tests, as the shared `ascii`/rules choices already do.
    let primary_config = &walked_roots[0].1;
    let graph = graph::build(
        roots,
        primary_config.display.gitignore,
        primary_config.display.include_tests,
        excludes,
        max_depth,
    );

    // A FILTER over the existing walk and graph, not a second traversal. Absent the ref, every downstream step stays byte-identical.
    if let Some(since) = since {
        let mut changed = std::collections::HashSet::new();
        for (root, _, _) in &walked_roots {
            changed.extend(changed::changed_files(root, since).map_err(BuildError::Git)?);
        }
        // Blast radius: the reverse closure over `used_by` edges, mapped back to directories to keep wholesale.
        let blast = graph.blast_radius_dirs(&changed);
        let in_change_set = |p: &PathBuf| {
            let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
            changed.contains(&canon) || blast.iter().any(|dir| canon.starts_with(dir))
        };
        // Directories take the SAME predicate as files, so `--since` means the change set rather than the whole skeleton with a few files in it.
        for (_, _, walked) in &mut walked_roots {
            walked.files.retain(&in_change_set);
            walked.dirs.retain(&in_change_set);
        }
    }

    let ascii = walked_roots[0].1.display.ascii;

    let map = model::CodebaseMap {
        roots: walked_roots
            .iter()
            .map(|(root, config, walked)| {
                model::build(
                    root,
                    &walked.files,
                    &walked.dirs,
                    &graph.dir_deps,
                    config,
                    max_depth,
                )
            })
            .collect(),
        warnings: graph.warnings,
    };
    Ok((map, ascii))
}

/// Surface a runaway-scope abort at exit [`exit::RUNAWAY_SCOPE`], stdout kept clean of any partial
/// tree either way. Under `--format json` the abort is emitted as the structured error envelope on
/// stdout, so an agent parsing stdout still gets a dispatch key; otherwise the human note goes to
/// `err` ONLY and stdout stays empty.
fn report_limit_exceeded(
    out: &mut impl Write,
    err: &mut impl Write,
    format: Format,
    e: &LimitExceeded,
) -> Result<i32> {
    if format == Format::Json {
        let message = format!(
            "'{}' has more than {} code files (limit --max-files {}); nothing written. \
             Raise with --max-files <N> or disable with --no-limit.",
            e.root.display(),
            e.limit,
            e.limit,
        );
        writeln!(
            out,
            "{}",
            render::json::render_error(
                exit::code::SCOPE_EXCEEDED,
                &message,
                Some(&e.root.display().to_string()),
            )
        )?;
    } else {
        writeln!(
            err,
            "annotated-tree: aborting — '{}' has more than {} code files (limit --max-files \
             {}); nothing written. Raise with --max-files <N> or disable with --no-limit.",
            e.root.display(),
            e.limit,
            e.limit,
        )?;
    }
    Ok(exit::RUNAWAY_SCOPE)
}

/// Print the machine-readable output schema (version 1) to `out` and return [`exit::SUCCESS`]: the
/// map document plus its sub-shapes and error envelope, then the strict-check report. Both strings
/// are the SAME text embedded in those modules' rustdoc, so the advertised wire contract is sourced
/// from ONE place per surface and cannot drift into a second copy.
fn print_schema(out: &mut impl Write) -> Result<i32> {
    write!(
        out,
        "annotated-tree — JSON output schema (schema version 1)\n\n{}\n{}",
        render::json::SCHEMA_DOC,
        strict::SCHEMA_DOC,
    )?;
    Ok(exit::SUCCESS)
}

pub(crate) fn resolve_roots(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    // Only the empty-args default is implicit. A path the user DID pass must exist, so `annotated-tree src typodir/` fails naming the offender instead of silently analyzing only the valid roots.
    if paths.is_empty() {
        return Ok(vec![PathBuf::from(".")]);
    }
    let invalid: Vec<String> = paths
        .iter()
        .filter(|p| !p.is_dir())
        .map(|p| p.display().to_string())
        .collect();
    if !invalid.is_empty() {
        bail!("not an existing directory: {}", invalid.join(", "));
    }
    Ok(paths.to_vec())
}

/// Resolve `--strict-check` targets. Like [`resolve_roots`], but a target may be a single FILE as
/// well as a directory, so an agent can lint the one file it just wrote and a pre-commit hook can
/// lint exactly the changed files. Empty args still default to `.`; a path that is neither an
/// existing file nor directory fails fast, naming the offender.
pub(crate) fn resolve_lint_targets(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Ok(vec![PathBuf::from(".")]);
    }
    let invalid: Vec<String> = paths
        .iter()
        .filter(|p| !p.exists())
        .map(|p| p.display().to_string())
        .collect();
    if !invalid.is_empty() {
        bail!("not an existing file or directory: {}", invalid.join(", "));
    }
    Ok(paths.to_vec())
}
