// Concern: the library surface — run(), which drives config, the walk and either tree or strict output, plus the reusable primitives | Non-concern: argv parsing | IO: (Cli, writer) -> exit_code
//
// This tool is a one-shot batch traversal of the local filesystem, so it is
// deliberately synchronous: the `ignore` crate parallelizes the walk across a thread
// pool (throughput-bound disk work), with no concurrent I/O wait to overlap.

//! # `annotated-tree` as a library
//!
//! The crate powers the `annotated-tree` binary but also exposes its core building blocks so
//! another program can reuse the efficient tree walk and the annotation grammar over files of
//! ANY shape — extensionless files, symlinks, files whose whole content (not just a first-line
//! comment) is the annotation — and drive its OWN rendering. The two entry styles are:
//!
//! - **Whole-tool**: [`parse_cli`] + [`run`] — parse argv and execute exactly as the binary does.
//! - **Primitives** (this is the library surface): the [`walk`] module (the `ignore`-based
//!   [`walk::configured_walk`] and [`walk::collect_code_files`]), the [`annotation`] module
//!   (marker-based [`annotation::extract`] and marker-agnostic [`annotation::extract_any`], plus
//!   the [`annotation::analyze`] checker), the [`config`] module ([`config::Config`] /
//!   [`config::Language`]), and the glob-compile helper [`build_globset`]. Compose them freely; a
//!   consumer that wants its own model/renderer never touches the internal tree/graph/strict
//!   machinery.
//! - **Map + render** (access-only): assemble a [`CodebaseMap`] by hand from [`DirNode`] /
//!   [`FileNode`] (the `charter`/`deps`/`warnings` fields may be `None`/`Vec::new()`)
//!   and render it via [`for_format`] + the [`Renderer`] trait — the tool's own text/json/md
//!   output over a tree you built yourself. The node field types ([`DirDeps`], [`Charter`],
//!   …) are re-exported so every field is nameable.
//!
//! **No stability promise.** This surface exists for a known consumer. There is no semver policy
//! and no deprecation cycle: a breaking change arrives as a compile error rather than through a
//! deprecation window, and 0.6.0 is such a change.
//!
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

// Library surface: the whole-tool `Cli` + `run` (re-exported below), the low-level
// `walk`/`annotation`/`config`/`util` primitives a downstream consumer composes, and the map +
// render DATA surface (`CodebaseMap`/`DirNode`/`FileNode` + the `Renderer`) re-exported below so a
// consumer can assemble and render its own tree. Every module stays `pub(crate)` — only the
// curated re-exports are public, so the graph/strict BUILDER machinery stays internal.
pub mod annotation;
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
// `pub(crate)`, never `pub`. The two retired accessory binaries used `#[path]` precisely to keep
// this code off the published library surface, and that intent survives the merge.
pub(crate) mod toolcall_injector;
pub(crate) mod util;
pub mod walk;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use globset::GlobSet;

pub use cli::{parse as parse_cli, Cli};
// The one `util` helper a downstream walk-composer needs: compile `--include`/`-I` glob
// patterns into the `GlobSet` `walk::collect_code_files` takes. Re-exported at the crate root so
// the rest of `util` (internal path/time helpers) stays off the public surface.
pub use util::build_globset;

// The map + render surface (issue #11), access-only. A consumer assembles a `CodebaseMap` from
// `DirNode`/`FileNode` (the optional/collection fields — `charter`, `deps`, `warnings` — may be
// `None`/`Vec::new()`) and renders it via the exposed `Renderer`/`for_format`, driving its own
// tree without the internal `build` pipeline. The node field types (`DirDeps`, `InternalDep`,
// `Charter`, `Warning`) are re-exported too so every field is nameable — but the graph BUILDER
// functions stay crate-internal.
/// Resolve a directory's charter from the filesystem. A relative path is taken against the
/// process working directory.
pub use charter::resolve_from_fs as resolve_charter;
pub use charter::Charter;
pub use graph::{DirDeps, InternalDep, Warning};
pub use model::{CodebaseMap, Coverage, DirNode, FileNode};
pub use render::{for_format, Renderer};
// `Format` is both re-exported (a consumer picks the renderer via `for_format(Format, ascii)`)
// and used internally below, so this one `pub use` serves both.
pub use cli::Format;
use config::{CliOverrides, Config};
use walk::LimitExceeded;

/// A build-pipeline failure, split so each caller renders it for its own surface and
/// classifies it into the right dispatch code: `Limit` is the runaway-scope trip (exit
/// [`exit::RUNAWAY_SCOPE`] + [`exit::code::SCOPE_EXCEEDED`]), `Git` is a `--since` git
/// failure ([`exit::code::GIT_ERROR`],
/// exit [`exit::PRECONDITION`]), and `Other` is any remaining precondition failure (bad
/// config, I/O → [`exit::code::PRECONDITION`]). Git is split from `Other` only so the two
/// map to distinct, caller-actionable codes.
pub(crate) enum BuildError {
    Limit(LimitExceeded),
    Git(anyhow::Error),
    Other(anyhow::Error),
}

/// A classified run failure: its process exit code, a stable string dispatch `code` (from
/// [`exit::code`] — the JSON-envelope counterpart to the integer exit code), a human
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
/// - [`exit::USAGE`] (2) — clap emits it for a bad flag or value before `run()` is
///   reached, and `toolcall-injector` returns it directly for an invocation it cannot
///   act on: no verb, two verbs, a mistyped flag `trailing_var_arg` swallowed, no `HOME`
///   with no settings path given, or `--annotate-tool-output` with no producer argv.
/// - [`exit::RUNAWAY_SCOPE`] (3) — a root exceeded `--max-files`; nothing written.
/// - [`exit::PRECONDITION`] (4) — a precondition/environment error. Usually an `Err(_)`
///   (missing root dir, git/`--since` failure, bad config) that the binary maps to 4;
///   `toolcall-injector` returns `Ok(4)` directly when a settings file cannot be read,
///   parsed or replaced.
///
/// On a failure under `--format json`, the same exit code is returned but a structured
/// error envelope (`{"schema":1,"error":{"code",…}}`, code from [`exit::code`]) is written
/// to `out` first, so an agent parsing stdout gets a dispatch key instead of empty output;
/// under any other format the failure surfaces as prose on `err` (behaviour unchanged).
pub fn run(cli: &Cli, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    // The `toolcall-injector` accessory verb dispatches FIRST: none of its five modes points at a
    // Workspace or emits a Report, so nothing below it applies. Routing it through `run()` is an
    // implementation choice about where dispatch lives — it does not make any of them a run in
    // SPEC.md's sense.
    if let Some(cli::Command::ToolcallInjector(args)) = &cli.command {
        return toolcall_injector::dispatch(args, out, err);
    }

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
        // (`strict::check_structured`). A single
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
                // Strict-check stays recognized-languages-only (an empty include set): a file
                // whose comment grammar is unknown cannot be linted, so `--include` never widens
                // what the gate validates — it governs the tree view alone.
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
        // brief annotation) inline after the report — the teaching rides on the surface
        // an agent already reads, instead of behind a separate command. Suppressed by
        // `--no-guide` (a caller that knows the format), never shown on success (nothing to
        // fix), and never on the JSON surface (which stays a clean parse; an agent there
        // dispatches on the structured `suggestion`/`expected` fields instead).
        if code == exit::STRICT_FAILURE && !cli.no_guide {
            write!(out, "\n{}", guide::full())?;
        }
        return Ok(code);
    }

    // Build via the ONE shared pipeline, so a rendered map is identical whichever
    // format asks for it. The walk happens up
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
    // Manifest-parse warnings ride inside `map` (so the `--format json` envelope surfaces
    // them structurally); the CLI additionally echoes the human
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
    // TREE2: a file the map does not list must fall under a criterion the Report STATES. A
    // sidecar's own row is the one this run suppressed, so name the rule — once, and only when
    // one was actually suppressed — on the same advisory channel the coverage note uses. The
    // JSON surface says it structurally instead (`FileNode.sidecar` on the row that took the
    // contract), so this stays off the machine parse.
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

/// The one build pipeline: walk every root (runaway-scope capped, and `max_depth`-capped —
/// the walk STOPS at the deepest level the render can show, so nothing below it is visited,
/// counted, or graphed), optionally
/// filter down to the `--since` change set plus its blast radius, build the
/// dependency graph, and assemble the canonical `CodebaseMap`. Every `run`
/// render (text/json/md) goes through here, so the map is byte-for-byte the same on
/// each. Returns the map — which now CARRIES the manifest-parse
/// warnings (so the JSON envelope surfaces them, and the CLI reads them off
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
    let mut walked_roots = Vec::new();
    for root in roots {
        let config = Config::load(root, overrides).map_err(BuildError::Other)?;
        // The `--include`/`[display] include` selectors are per-root (each root uses its own
        // resolved config), compiled here next to the shared `-I` excludes. A bad pattern fails
        // fast as a precondition error, exactly like a bad `-I` glob.
        let include = util::build_globset(&config.display.include).map_err(BuildError::Other)?;
        // `max_depth` bounds the WALK, not just the render: below the cutoff nothing is
        // visited, stat'd, read — or counted against `--max-files`.
        match walk::collect_tree(root, &config, excludes, &include, max_depth) {
            Ok(walked) => walked_roots.push((root, config, walked)),
            Err(e) => return Err(BuildError::Limit(e)),
        }
    }

    // Multi-root: the manifest walk uses the PRIMARY (first) root's gitignore +
    // include_tests settings, consistent with how the primary root's config already
    // drives the shared `ascii`/rules choices for a multi-root run.
    let primary_config = &walked_roots[0].1;
    let graph = graph::build(
        roots,
        primary_config.display.gitignore,
        primary_config.display.include_tests,
        excludes,
        max_depth,
    );

    // `--since`/`--changed`: filter the already-walked path set down to what changed
    // plus its blast radius. This is a FILTER over the existing walk + graph — not a
    // second traversal. Absent the ref, `walked_roots` is untouched and every
    // downstream step (and every golden) is byte-identical.
    if let Some(since) = since {
        // Fail-Fast: a git error (not a repo / missing git / bad ref) aborts here with
        // an explicit message, never a silent empty view.
        let mut changed = std::collections::HashSet::new();
        for (root, _, _) in &walked_roots {
            changed.extend(changed::changed_files(root, since).map_err(BuildError::Git)?);
        }
        // Blast radius: for each changed file's owning package, every package that
        // transitively depends on it (reverse closure over the `used_by` edges),
        // mapped back to directories to keep wholesale.
        let blast = graph.blast_radius_dirs(&changed);
        let in_change_set = |p: &PathBuf| {
            let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
            changed.contains(&canon) || blast.iter().any(|dir| canon.starts_with(dir))
        };
        // Directories take the SAME predicate as files, so `--since` keeps meaning "the
        // change set" rather than the whole skeleton with a few files in it. A directory on
        // the way to a surviving file still appears — the model recreates every ancestor.
        for (_, _, walked) in &mut walked_roots {
            walked.files.retain(&in_change_set);
            walked.dirs.retain(&in_change_set);
        }
    }

    // The render glyph set is a global/terminal concern read from the primary (first
    // root's) resolved config. `roots` is never empty (`resolve_roots` yields at least
    // `.`), so `walked_roots[0]` exists.
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
        // The graph's manifest-parse warnings travel WITH the map so the render surface
        // that can carry them (the JSON envelope) emits them, and the CLI can echo them
        // to stderr.
        warnings: graph.warnings,
    };
    Ok((map, ascii))
}

/// Surface a runaway-scope abort at exit [`exit::RUNAWAY_SCOPE`], stdout kept clean of any
/// partial tree either way. Under `--format json` the abort is emitted as the structured
/// error envelope on stdout (code [`exit::code::SCOPE_EXCEEDED`]), so an agent parsing
/// stdout still gets a dispatch key; otherwise the human note goes to `err` ONLY (stdout
/// stays empty — no half-written JSON).
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
