// Concern: wires config -> walk -> (tree | strict) and exposes run() for the binary and golden tests | Non-concern: argv parsing | IO: (Cli, writer) -> exit_code
//
// This tool is a one-shot batch traversal of the local filesystem, so it is
// deliberately synchronous: the `ignore` crate parallelizes the walk across a thread
// pool (throughput-bound disk work), with no concurrent I/O wait to overlap.

// Minimal API surface: only `Cli` + `run` (below) are public — everything the
// binary and the integration tests need. Every other module is crate-internal.
pub(crate) mod annotation;
pub(crate) mod changed;
pub(crate) mod charter;
pub(crate) mod cli;
pub(crate) mod config;
pub mod exit;
pub(crate) mod githook;
pub(crate) mod graph;
pub(crate) mod guide;
pub(crate) mod manifest;
pub(crate) mod mcp;
pub(crate) mod model;
pub(crate) mod render;
pub(crate) mod rules;
pub(crate) mod strict;
pub(crate) mod symbols;
pub(crate) mod tokens;
pub(crate) mod util;
pub(crate) mod walk;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use globset::GlobSet;

pub use cli::{parse as parse_cli, Cli};

use cli::Format;
use config::{CliOverrides, Config};
use walk::LimitExceeded;

/// A build-pipeline failure, split so each caller renders it for its own surface and
/// classifies it into the right dispatch code: `Limit` is the runaway-scope trip (CLI →
/// exit [`exit::RUNAWAY_SCOPE`] + [`exit::code::SCOPE_EXCEEDED`]; MCP → a tool error that
/// keeps the server alive), `Git` is a `--since` git failure ([`exit::code::GIT_ERROR`],
/// exit [`exit::PRECONDITION`]), and `Other` is any remaining precondition failure (bad
/// config, I/O → [`exit::code::PRECONDITION`]). Git is split from `Other` only so the two
/// map to distinct, caller-actionable codes.
pub(crate) enum BuildError {
    Limit(LimitExceeded),
    Git(anyhow::Error),
    Other(anyhow::Error),
}

/// A classified run failure: its process exit code, a stable string dispatch `code` (from
/// [`exit::code`] — the JSON-envelope / MCP counterpart to the integer exit code), a human
/// message, and the offending path when known. One object per failure class so an agent
/// branches on `code`, never on prose — mirroring how [`strict::AnnotationViolation`]
/// carries a structured, dispatchable diagnostic rather than one opaque string.
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

/// Execute the parsed command, writing output to `out`. Returns the process exit code
/// from the [`exit`] taxonomy — one disjoint code per failure class an agent branches on:
///
/// - [`exit::SUCCESS`] (0) — clean run (tree rendered, or `--strict-check` passed).
/// - [`exit::STRICT_FAILURE`] (1) — `--strict-check` found at least one violation.
/// - [`exit::RUNAWAY_SCOPE`] (3) — a root exceeded `--max-files`; nothing written.
/// - `Err(_)` — a precondition/environment error (missing root dir, git/`--since`
///   failure, bad config); the binary maps it to [`exit::PRECONDITION`] (4).
///
/// [`exit::USAGE`] (2) is never returned here: clap emits it directly for a bad flag or
/// value before `run()` is reached.
///
/// On a failure under `--format json`, the same exit code is returned but a structured
/// error envelope (`{"schema":1,"error":{"code",…}}`, code from [`exit::code`]) is written
/// to `out` first, so an agent parsing stdout gets a dispatch key instead of empty output;
/// under any other format the failure surfaces as prose on `err` (behaviour unchanged).
pub fn run(cli: &Cli, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    // `--schema` is a self-correcting-help info flag (like `--help`): print the wire
    // contract to stdout and exit clean, before any traversal, so an agent can fetch the
    // output schema without a repo to walk or a human to read source.
    if cli.schema {
        return print_schema(out);
    }

    // `--githook-guide` is likewise a self-correcting-help info flag: print the canonical
    // guide for reproducing the repo's local enforcement hooks and exit clean, before any
    // traversal, so an agent can set enforcement up from the tool itself without a human.
    if cli.githook_guide {
        write!(out, "{}", githook::text())?;
        return Ok(exit::SUCCESS);
    }

    // MCP server mode is the one async surface. It owns the whole process (a
    // long-lived stdio server), so it is handled first, before any tree/strict
    // path — `mcp::serve` creates its own tokio runtime internally and returns a
    // sync exit code. On a build without the `mcp` feature the stub returns a
    // clear "rebuild with --features mcp" error (surfaced to stderr, nonzero exit).
    if cli.mcp {
        return mcp::serve(cli);
    }

    // Strict-check accepts a single file as well as a directory (lint the one file you just
    // wrote); the tree render is directory-only, so its resolver stays strict.
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

    // `--symbols` on a binary built WITHOUT the `symbols` feature is inert: the
    // extractor registry is empty, so every file yields no symbols. Say so once,
    // explicitly (Fail-Fast/explicit-over-silent), rather than silently doing
    // nothing — but keep going, so scripts targeting a lean binary don't hard-fail.
    #[cfg(not(feature = "symbols"))]
    if cli.symbols {
        writeln!(
            err,
            "annotated-tree: --symbols ignored — built without symbols support \
             (rebuild with --features symbols)."
        )?;
    }

    let overrides = cli.overrides();
    let excludes = match util::build_globset(&cli.ignore) {
        Ok(excludes) => excludes,
        Err(e) => return Failure::precondition(format!("{e:#}")).dispatch(out, cli.format),
    };

    if cli.strict_check {
        // Per-target config: each target validates against ITS OWN discovered
        // `.annotated-tree.toml` (a multi-target run must never apply target A's
        // convention/languages to target B); a FILE target discovers config by walking up
        // from its parent. Resolve every target's verdict FIRST so a runaway trips before a
        // single stdout byte — then no partial report is ever written on abort.
        //
        // A directory target's verdict is annotation linting PLUS its own configured
        // architectural `[rules]`, folded by the ONE shared producer
        // (`strict::check_structured`, also driven by the MCP `strict_check` tool). A single
        // FILE target has no package neighbourhood, so it is annotation-lint only
        // (`strict::check_file`, no graph/rules/charter) — those are directory-scale concerns.
        // Both yield the SAME `StrictReport`, so text and JSON render uniformly below.
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
                // Fail fast, explicitly: an explicitly-named file whose extension maps to no
                // configured language cannot be linted (its comment grammar is unknown).
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
                let files = match walk::collect_code_files(target, &config, &excludes) {
                    Ok(files) => files,
                    Err(e) => return report_limit_exceeded(out, err, cli.format, &e),
                };
                let report = strict::check_structured(target, &files, &config, &excludes);
                (report, config.display.max_per_node)
            };
            reports.push((report, max_per_node));
        }
        // `--format json` emits ONE structured document (the machine-consumable counterpart
        // to the default TEXT report), the targets folded together; text/md keep the
        // per-target TEXT report. The exit-code contract is the same on both: 0 iff every
        // violation set is empty. Every verdict is already computed above, so a runaway still
        // trips before a single stdout byte on either format.
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
            // The TEXT report reuses the same per-node display cap that bounds the tree
            // render, so a run with hundreds of findings stays scannable (JSON stays complete).
            let (text, root_code) = report.to_text(*max_per_node);
            out.write_all(text.as_bytes())?;
            code = code.max(root_code);
        }
        // On a FAILING text run, print the annotation guide (how to write a conforming,
        // non-vacuous annotation) inline after the report — the teaching rides on the surface
        // an agent already reads, instead of behind a separate command. Suppressed by
        // `--no-guide` (a caller that knows the format), never shown on success (nothing to
        // fix), and never on the JSON surface (which stays a clean parse; an agent there
        // dispatches on the structured `suggestion`/`expected` fields instead).
        if code == exit::STRICT_FAILURE && !cli.no_guide {
            write!(out, "\n{}", guide::full())?;
        }
        return Ok(code);
    }

    // Build via the ONE shared pipeline (also driven by the MCP `map` tool), so a
    // rendered map is identical whichever surface asks for it. The walk happens up
    // front inside it: a runaway-scope trip fires before any render or stdout write,
    // which is what makes every output format (including --format json) safe — abort
    // ⇒ empty stdout, for free.
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
    // Manifest-parse warnings ride inside `map` (so the `--format json` envelope and the
    // MCP `map` tool both surface them structurally); the CLI additionally echoes the human
    // `message` to stderr, unless silenced. The JSON envelope carries them regardless — an
    // agent parsing stdout should not have to also scrape stderr to learn the graph is
    // incomplete — so `--ignore-parsing-errors` only governs this stderr echo.
    if !cli.ignore_parsing_errors {
        for warning in &map.warnings {
            writeln!(err, "warning: {}", warning.message)?;
        }
    }

    // The render glyph set is a global/terminal concern, not a per-repo one, so it is
    // the primary (first root's) resolved `ascii`, handed back by the pipeline that
    // already loaded that config — no second `Config::load` (re-parse + regex recompile)
    // on the render path. Per-file/per-tree settings were resolved per-root inside it.
    let renderer = render::for_format(cli.format, ascii);
    writeln!(out, "{}", renderer.render(&map))?;

    // Layer-0 motivation, TEXT map only: a code file with no first-line annotation is
    // invisible to an agent reading this tree. When some listed file lacks one, emit ONE
    // self-extinguishing note to `err` — the advisory channel the manifest warnings above
    // already use — so the stdout tree stays a clean, byte-identical parse. Silent at full
    // coverage (`is_incomplete` is false), and never on the JSON surface, where the SAME
    // fact rides structurally as the `coverage` object instead. `--strict-check` is the
    // authoritative per-file lister, so point at it rather than restate the gaps here.
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

/// The one build pipeline: walk every root (runaway-scope capped), optionally
/// filter down to the `--since` change set plus its blast radius, build the
/// dependency graph, and assemble the canonical `CodebaseMap`. Both `run`
/// (text/json/md) and the MCP `map` tool go through here, so the map is byte-for-byte
/// the same on either surface. Returns the map — which now CARRIES the manifest-parse
/// warnings (so the JSON envelope and MCP `map` surface them, and the CLI reads them off
/// `map.warnings` to echo to stderr) — and the primary (first root's) resolved `ascii`
/// glyph choice, handed back so the render path reuses the config this pipeline already
/// loaded instead of re-loading (re-parse + regex recompile) it.
pub(crate) fn build_codebase_map(
    roots: &[PathBuf],
    overrides: &CliOverrides,
    excludes: &GlobSet,
    since: Option<&str>,
    max_depth: Option<usize>,
) -> std::result::Result<(model::CodebaseMap, bool), BuildError> {
    // Per-root config: each root uses its OWN discovered `.annotated-tree.toml`
    // (languages, gitignore, display) — a multi-root run never applies one root's
    // repo config to another. The CLI overrides + `-I` excludes are shared. Walk all
    // roots up front so the runaway-scope trip happens before any graph build, model
    // build, or render.
    let mut root_files = Vec::new();
    for root in roots {
        let config = Config::load(root, overrides).map_err(BuildError::Other)?;
        match walk::collect_code_files(root, &config, excludes) {
            Ok(files) => root_files.push((root, config, files)),
            Err(e) => return Err(BuildError::Limit(e)),
        }
    }

    // Multi-root: the manifest walk uses the PRIMARY (first) root's gitignore +
    // include_tests settings, consistent with how the primary root's config already
    // drives the shared `ascii`/rules choices for a multi-root run.
    let primary_config = &root_files[0].1;
    let graph = graph::build(
        roots,
        primary_config.display.gitignore,
        primary_config.display.include_tests,
        excludes,
    );

    // `--since`/`--changed`: filter the already-walked file set down to what changed
    // plus its blast radius. This is a FILTER over the existing walk + graph — not a
    // second traversal. Absent the ref, `root_files` is untouched and every
    // downstream step (and every golden) is byte-identical.
    if let Some(since) = since {
        // Fail-Fast: a git error (not a repo / missing git / bad ref) aborts here with
        // an explicit message, never a silent empty view.
        let mut changed = std::collections::HashSet::new();
        for (root, _, _) in &root_files {
            changed.extend(changed::changed_files(root, since).map_err(BuildError::Git)?);
        }
        // Blast radius: for each changed file's owning package, every package that
        // transitively depends on it (reverse closure over the `used_by` edges),
        // mapped back to directories to keep wholesale.
        let blast = graph.blast_radius_dirs(&changed);
        for (_, _, files) in &mut root_files {
            files.retain(|f| {
                let canon = f.canonicalize().unwrap_or_else(|_| f.clone());
                changed.contains(&canon) || blast.iter().any(|dir| canon.starts_with(dir))
            });
        }
    }

    // The render glyph set is a global/terminal concern read from the primary (first
    // root's) resolved config. `roots` is never empty (`resolve_roots` yields at least
    // `.`), so `root_files[0]` exists.
    let ascii = root_files[0].1.display.ascii;

    let map = model::CodebaseMap {
        roots: root_files
            .iter()
            .map(|(root, config, files)| {
                model::build(root, files, &graph.dir_deps, config, max_depth)
            })
            .collect(),
        // The graph's manifest-parse warnings travel WITH the map so every render surface
        // (JSON envelope, MCP `map`) can emit them, and the CLI can echo them to stderr.
        warnings: graph.warnings,
    };
    Ok((map, ascii))
}

/// Surface a runaway-scope abort at exit [`exit::RUNAWAY_SCOPE`], stdout kept clean of any
/// partial tree either way. Under `--format json` the abort is emitted as the structured
/// error envelope on stdout (code [`exit::code::SCOPE_EXCEEDED`]), so an agent parsing
/// stdout still gets a dispatch key; otherwise the human note goes to `err` ONLY (stdout
/// stays empty — no half-written JSON). The `--mcp` surface instead catches
/// `LimitExceeded` and returns a structured tool error, keeping the server alive.
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
            "annotated-tree: aborting — '{}' has more than {} files (limit --max-files {}); \
             nothing written. Raise with --max-files <N> or disable with --no-limit.",
            e.root.display(),
            e.limit,
            e.limit,
        )?;
    }
    Ok(exit::RUNAWAY_SCOPE)
}

/// Print the machine-readable output schema (version 1) to `out` and return
/// [`exit::SUCCESS`]: the map document plus its sub-shapes and `warnings`/error envelope
/// ([`render::json::SCHEMA_DOC`]), then the strict-check report ([`strict::SCHEMA_DOC`]).
/// Both strings are the SAME text embedded in those modules' rustdoc, so the advertised
/// wire contract is sourced from ONE place per surface and cannot drift into a second copy.
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
    // Only the empty-args default is implicit (analyze `.`). Any path the user DID
    // pass must exist and be a directory — a typo like `annotated-tree src typodir/`
    // fails fast naming the offender, rather than silently dropping it and analyzing
    // only the valid roots (which would exit 0 on a mistyped invocation).
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

/// Resolve `--strict-check` targets. Like [`resolve_roots`], but a target may be a single
/// FILE as well as a directory — so an agent can lint the one file it just wrote, and a
/// pre-commit hook can lint exactly the changed files, without pointing the check at a whole
/// tree. Empty args still default to `.`; any path that is neither an existing file nor an
/// existing directory fails fast, naming the offender (never a silent drop).
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
