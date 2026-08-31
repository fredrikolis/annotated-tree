// Concern: the library surface — run(), plus the walk, annotation and render primitives | Non-concern: argv parsing | IO: (Cli, writer) -> exit_code + edits

//! # `annotated-tree` as a library
//! Two entry styles: whole-tool ([`parse_cli`] + [`run`]), or primitives — [`walk`],
//! [`annotation`], [`config`] and [`build_globset`]. No stability promise.
//! ```no_run
//! use annotated_tree::config::{CliOverrides, Config};
//! use annotated_tree::{annotation, walk};
//! use globset::GlobSet;
//! use std::path::Path;
//!
//! let root = Path::new(".");
//! let config = Config::load(root, &CliOverrides::default()).expect("resolve config");
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
pub(crate) mod embedded_doc;
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

pub use charter::resolve_from_fs as resolve_charter;
pub use charter::Charter;
pub use config::Format;
use config::{CliOverrides, Config};
pub use graph::{DirDeps, InternalDep, Warning};
pub use model::{CodebaseMap, Coverage, DirNode, FileNode};
pub use render::{for_format, Renderer};
use walk::LimitExceeded;

pub(crate) enum BuildError {
    Limit(LimitExceeded, Format),
    Git(anyhow::Error, Format),
    /// `None` only when no root's config had resolved yet.
    Other(anyhow::Error, Option<Format>),
}

/// What to run — display/behavioural settings travel separately, as `CliOverrides`.
struct Invocation<'a> {
    paths: &'a [PathBuf],
    strict_check: bool,
    schema: bool,
    githook_guide: bool,
    annotation_guide: bool,
    since_ref: Option<String>,
    command: &'a Option<cli::Command>,
}

impl<'a> Invocation<'a> {
    fn from_cli(cli: &'a Cli) -> Self {
        Invocation {
            paths: &cli.paths,
            strict_check: cli.strict_check,
            schema: cli.schema,
            githook_guide: cli.githook_guide,
            annotation_guide: cli.annotation_guide,
            since_ref: cli.since_ref(),
            command: &cli.command,
        }
    }
}

pub(crate) struct Failure {
    exit_code: i32,
    code: &'static str,
    message: String,
    path: Option<String>,
}

impl Failure {
    fn not_a_directory(message: String) -> Self {
        Failure {
            exit_code: exit::PRECONDITION,
            code: exit::code::NOT_A_DIRECTORY,
            message,
            path: None,
        }
    }

    fn git(message: String) -> Self {
        Failure {
            exit_code: exit::PRECONDITION,
            code: exit::code::GIT_ERROR,
            message,
            path: None,
        }
    }

    fn precondition(message: String) -> Self {
        Failure {
            exit_code: exit::PRECONDITION,
            code: exit::code::PRECONDITION,
            message,
            path: None,
        }
    }

    /// Under `--format json` this returns the exit code with the error already written to
    /// stdout; otherwise it returns `Err` so the binary renders `error:` prose on stderr.
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

/// Returns a process exit code from the [`exit`] taxonomy.
pub fn run(cli: &Cli, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    let invocation = Invocation::from_cli(cli);
    let overrides = cli.overrides();
    // The only format an abort before any Config resolves can honour: argv, else the built-in default.
    let bootstrap_format = overrides.format.unwrap_or(Format::Text);

    if let Some(cli::Command::Strip(args)) = invocation.command {
        return Ok(strip::dispatch(
            args,
            &overrides,
            bootstrap_format,
            out,
            err,
        ));
    }
    if let Some(cli::Command::BashAnnotator(args)) = invocation.command {
        return bash_annotator::dispatch(args, out, err);
    }

    if invocation.schema {
        return print_schema(out);
    }

    if invocation.githook_guide {
        write!(out, "{}", githook::text())?;
        return Ok(exit::SUCCESS);
    }

    if invocation.annotation_guide {
        write!(out, "{}", guide::full())?;
        return Ok(exit::SUCCESS);
    }

    let roots = if invocation.strict_check {
        match resolve_lint_targets(invocation.paths) {
            Ok(roots) => roots,
            Err(e) => {
                return Failure::not_a_directory(format!("{e:#}")).dispatch(out, bootstrap_format)
            }
        }
    } else {
        match resolve_roots(invocation.paths) {
            Ok(roots) => roots,
            Err(e) => {
                return Failure::not_a_directory(format!("{e:#}")).dispatch(out, bootstrap_format)
            }
        }
    };

    if invocation.strict_check {
        // Every verdict resolves before a single stdout byte, so an abort writes no partial report.
        let mut reports: Vec<(strict::StrictReport, Option<usize>)> = Vec::new();
        let mut primary: Option<Config> = None;
        for target in &roots {
            let (report, max_per_node) = if target.is_file() {
                let parent = target.parent().filter(|p| !p.as_os_str().is_empty());
                let parent = parent.map_or_else(|| PathBuf::from("."), Path::to_path_buf);
                let config = match Config::load(&parent, &overrides) {
                    Ok(config) => config,
                    Err(e) => {
                        let format = primary
                            .as_ref()
                            .map_or(bootstrap_format, |c| c.display.format);
                        return Failure::precondition(format!("{e:#}")).dispatch(out, format);
                    }
                };
                if config.language_for_path(target).is_none() {
                    let reason = if target.extension().is_some() {
                        "its extension maps to no configured language"
                    } else {
                        "it opens with no recognized `#!` interpreter"
                    };
                    return Failure::precondition(format!(
                        "not a lintable code file: {} — {reason}",
                        target.display()
                    ))
                    .dispatch(out, config.display.format);
                }
                let files = vec![target.clone()];
                let report = strict::check_file(&parent, &files, &config);
                let max_per_node = config.display.max_per_node;
                if primary.is_none() {
                    primary = Some(config);
                }
                (report, max_per_node)
            } else {
                let config = match Config::load(target, &overrides) {
                    Ok(config) => config,
                    Err(e) => {
                        let format = primary
                            .as_ref()
                            .map_or(bootstrap_format, |c| c.display.format);
                        return Failure::precondition(format!("{e:#}")).dispatch(out, format);
                    }
                };
                let excludes = match util::build_globset(&config.display.ignore) {
                    Ok(excludes) => excludes,
                    Err(e) => {
                        return Failure::precondition(format!("{e:#}"))
                            .dispatch(out, config.display.format)
                    }
                };
                let files =
                    match walk::collect_code_files(target, &config, &excludes, &GlobSet::empty()) {
                        Ok(files) => files,
                        Err(e) => {
                            return report_limit_exceeded(out, err, config.display.format, &e)
                        }
                    };
                let report = strict::check_structured(target, &files, &config, &excludes);
                let max_per_node = config.display.max_per_node;
                if primary.is_none() {
                    primary = Some(config);
                }
                (report, max_per_node)
            };
            reports.push((report, max_per_node));
        }
        let primary = primary.expect("resolve_lint_targets never returns an empty Vec");
        if primary.display.format == Format::Json {
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
        if code == exit::STRICT_FAILURE && !primary.display.no_guide {
            write!(out, "\n{}", guide::full())?;
        }
        return Ok(code);
    }

    let (map, primary) =
        match build_codebase_map(&roots, &overrides, invocation.since_ref.as_deref()) {
            Ok(built) => built,
            Err(BuildError::Limit(e, format)) => {
                return report_limit_exceeded(out, err, format, &e)
            }
            Err(BuildError::Git(e, format)) => {
                return Failure::git(format!("{e:#}")).dispatch(out, format)
            }
            Err(BuildError::Other(e, format)) => {
                return Failure::precondition(format!("{e:#}"))
                    .dispatch(out, format.unwrap_or(bootstrap_format))
            }
        };
    // `--ignore-parsing-errors` governs only the stderr echo; the JSON envelope carries warnings regardless.
    if !primary.display.ignore_parsing_errors {
        for warning in &map.warnings {
            writeln!(err, "warning: {}", warning.message)?;
        }
    }

    let renderer = render::for_format(primary.display.format, primary.display.ascii);
    writeln!(out, "{}", renderer.render(&map))?;

    if primary.display.format == Format::Text && map.has_sidecar_rows() {
        writeln!(err, "note: {}.", walk::ANNOTATION_FILE_CRITERION)?;
    }

    if primary.display.format == Format::Text {
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

/// `max_depth`-capped so nothing below the deepest displayable level is visited, counted or
/// graphed. Returns the map and the primary root's resolved `Config`.
pub(crate) fn build_codebase_map(
    roots: &[PathBuf],
    overrides: &CliOverrides,
    since: Option<&str>,
) -> std::result::Result<(model::CodebaseMap, Config), BuildError> {
    let mut configs = Vec::with_capacity(roots.len());
    let mut ignore_globs = Vec::with_capacity(roots.len());
    for root in roots {
        let known_format = configs.first().map(|c: &Config| c.display.format);
        let config =
            Config::load(root, overrides).map_err(|e| BuildError::Other(e, known_format))?;
        let format = config.display.format;
        ignore_globs.push(
            util::build_globset(&config.display.ignore)
                .map_err(|e| BuildError::Other(e, Some(format)))?,
        );
        configs.push(config);
    }
    // `-L` caps the walk for every root alike, so the primary root settles it.
    let primary = configs[0].clone();
    let max_depth = primary.limits.max_depth;
    let format = primary.display.format;

    // All roots are walked up front so a runaway trips before any graph build or render.
    let mut walked_roots = Vec::new();
    for ((root, config), excludes) in roots.iter().zip(&configs).zip(&ignore_globs) {
        let include = util::build_globset(&config.display.include)
            .map_err(|e| BuildError::Other(e, Some(format)))?;
        let filter = walk::WalkFilter::from_config(config, excludes);
        match walk::collect_tree(root, config, filter, &include, max_depth) {
            Ok(walked) => walked_roots.push((root, config, walked, filter)),
            Err(e) => return Err(BuildError::Limit(e, format)),
        }
    }

    // The manifest walk uses the PRIMARY root's filter, so a graphed path is under the same policy.
    let graph = graph::build(roots, walked_roots[0].3, max_depth);

    if let Some(since) = since {
        let mut changed = std::collections::HashSet::new();
        for (root, _, _, _) in &walked_roots {
            changed.extend(
                changed::changed_files(root, since).map_err(|e| BuildError::Git(e, format))?,
            );
        }
        let blast = graph.blast_radius_dirs(&changed);
        let in_change_set = |p: &PathBuf| {
            let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
            changed.contains(&canon) || blast.iter().any(|dir| canon.starts_with(dir))
        };
        for (_, _, walked, _) in &mut walked_roots {
            walked.files.retain(&in_change_set);
            walked.dirs.retain(&in_change_set);
        }
    }

    let map = model::CodebaseMap {
        roots: walked_roots
            .iter()
            .map(|(root, config, walked, _)| {
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
    Ok((map, primary))
}

/// Stdout stays empty either way; under `--format json` the abort is emitted as the structured
/// error envelope on stdout instead of the human note going to `err`.
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

/// Both strings are the SAME text embedded in those modules' rustdoc, so the wire contract
/// cannot drift into a second copy.
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
