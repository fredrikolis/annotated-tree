// Library root: Wires config -> walk -> (tree | strict) and exposes run() for the binary and golden tests. NOT concerned with argv parsing. | I/O: (Cli, writer) -> exit_code
//
// This tool is a one-shot batch traversal of the local filesystem, so it is
// deliberately synchronous: the `ignore` crate parallelizes the walk across a thread
// pool (throughput-bound disk work), with no concurrent I/O wait to overlap.

// Minimal API surface: only `Cli` + `run` (below) are public — everything the
// binary and the integration tests need. Every other module is crate-internal.
pub(crate) mod annotation;
pub(crate) mod changed;
pub(crate) mod cli;
pub(crate) mod config;
pub(crate) mod graph;
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
use std::path::PathBuf;

use anyhow::{bail, Result};
use globset::GlobSet;

pub use cli::Cli;

use config::{CliOverrides, Config};
use walk::LimitExceeded;

/// A build-pipeline failure, split so each caller renders it for its own surface:
/// `Limit` is the runaway-scope trip (CLI → exit 2 + stderr note; MCP → a tool
/// error that keeps the server alive), `Other` is any other failure (e.g. a git
/// error from `--since`).
pub(crate) enum BuildError {
    Limit(LimitExceeded),
    Other(anyhow::Error),
}

/// Execute the parsed command, writing output to `out`. Returns the process exit
/// code (0 success, 1 strict-check failure, 2 runaway-scope abort).
pub fn run(cli: &Cli, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    // MCP server mode is the one async surface. It owns the whole process (a
    // long-lived stdio server), so it is handled first, before any tree/strict
    // path — `mcp::serve` creates its own tokio runtime internally and returns a
    // sync exit code. On a build without the `mcp` feature the stub returns a
    // clear "rebuild with --features mcp" error (surfaced to stderr, nonzero exit).
    if cli.mcp {
        return mcp::serve(cli);
    }

    let roots = resolve_roots(&cli.paths)?;

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
    let excludes = util::build_globset(&cli.ignore)?;

    if cli.strict_check {
        // Per-root config: each root validates against ITS OWN discovered
        // `.annotated-tree.toml` (a multi-root run must never apply root A's
        // convention/languages to root B). Walk every root FIRST so a runaway trips
        // before a single stdout byte — then no partial report is ever written on abort.
        let mut collected = Vec::new();
        for root in &roots {
            let config = Config::load(root, &overrides)?;
            match walk::collect_code_files(root, &config, &excludes) {
                Ok(files) => collected.push((root, config, files)),
                Err(e) => return report_limit_exceeded(err, &e),
            }
        }
        // Each root's verdict is annotation linting PLUS its own configured
        // architectural `[rules]`, folded together by the ONE shared composition
        // (`strict::check_with_rules`, also driven by the MCP `strict_check` tool) so a
        // rule violation reports and exits identically on either surface. Rules stay
        // per-root, consistent with the per-root annotation config above; a root with no
        // `[rules]` builds no graph and its output is byte-identical to before.
        let mut code = 0;
        for (root, config, files) in &collected {
            let (report, root_code) = strict::check_with_rules(root, files, config, &excludes);
            out.write_all(report.as_bytes())?;
            code = code.max(root_code);
        }
        return Ok(code);
    }

    // Build via the ONE shared pipeline (also driven by the MCP `map` tool), so a
    // rendered map is identical whichever surface asks for it. The walk happens up
    // front inside it: a runaway-scope trip fires before any render or stdout write,
    // which is what makes every output format (including --format json) safe — abort
    // ⇒ empty stdout, for free.
    let since = cli.since_ref();
    let (map, warnings, ascii) = match build_codebase_map(
        &roots,
        &overrides,
        &excludes,
        since.as_deref(),
        cli.max_depth,
    ) {
        Ok(built) => built,
        Err(BuildError::Limit(e)) => return report_limit_exceeded(err, &e),
        Err(BuildError::Other(e)) => return Err(e),
    };
    if !cli.ignore_parsing_errors {
        for warning in &warnings {
            writeln!(err, "warning: {warning}")?;
        }
    }

    // The render glyph set is a global/terminal concern, not a per-repo one, so it is
    // the primary (first root's) resolved `ascii`, handed back by the pipeline that
    // already loaded that config — no second `Config::load` (re-parse + regex recompile)
    // on the render path. Per-file/per-tree settings were resolved per-root inside it.
    let renderer = render::for_format(cli.format, ascii);
    writeln!(out, "{}", renderer.render(&map))?;
    Ok(0)
}

/// The one build pipeline: walk every root (runaway-scope capped), optionally
/// filter down to the `--since` change set plus its blast radius, build the
/// dependency graph, and assemble the canonical `CodebaseMap`. Both `run`
/// (text/json/md) and the MCP `map` tool go through here, so the map is byte-for-byte
/// the same on either surface. Returns the map, manifest-parse warnings (the CLI prints
/// them to stderr; the JSON payload never carries them), and the primary (first root's)
/// resolved `ascii` glyph choice — handed back so the render path reuses the config this
/// pipeline already loaded instead of re-loading (re-parse + regex recompile) it.
pub(crate) fn build_codebase_map(
    roots: &[PathBuf],
    overrides: &CliOverrides,
    excludes: &GlobSet,
    since: Option<&str>,
    max_depth: Option<usize>,
) -> std::result::Result<(model::CodebaseMap, Vec<String>, bool), BuildError> {
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
            changed.extend(changed::changed_files(root, since).map_err(BuildError::Other)?);
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
    };
    Ok((map, graph.warnings, ascii))
}

/// Surface a runaway-scope abort: diagnostic to `err` ONLY (never `out`, so stdout
/// stays empty — no partial tree, no half-written JSON), exit code 2. A future
/// `--mcp` surface (#7) will instead catch `LimitExceeded` and return a structured
/// tool error, keeping the server alive rather than exiting.
fn report_limit_exceeded(err: &mut impl Write, e: &LimitExceeded) -> Result<i32> {
    writeln!(
        err,
        "annotated-tree: aborting — '{}' has more than {} files (limit --max-files {}); \
         nothing written. Raise with --max-files <N> or disable with --no-limit.",
        e.root.display(),
        e.limit,
        e.limit,
    )?;
    Ok(2)
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
