// Concern: declares the command-line surface and maps flags to config overrides | Non-concern: execution | IO: (argv) -> Cli

use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches, Parser, ValueEnum};

use crate::config::CliOverrides;
use crate::exit;

/// Output format. A render-time concern only — deliberately NOT threaded through
/// `CliOverrides`/`Config`, which hold persistent display state, not the one-shot
/// choice of renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Format {
    #[default]
    Text,
    Json,
    Md,
}

/// Annotated directory tree with a cross-ecosystem package dependency graph.
#[derive(Debug, Parser)]
#[command(name = "annotated-tree", version)]
pub struct Cli {
    /// Directories to analyze [default: current directory].
    pub paths: Vec<PathBuf>,

    /// Exit non-zero if any code file lacks a conforming first-line annotation. Each PATH
    /// may be a directory (lint every code file under it) or a single file (lint just that
    /// file — the natural unit for a pre-commit hook or checking the file you just wrote).
    #[arg(long)]
    pub strict_check: bool,

    /// Suppress the annotation-writing guide that a failing --strict-check prints by
    /// default after the violations (for a caller that already knows the format). The
    /// violations, counts, and exit code are unaffected.
    #[arg(long)]
    pub no_guide: bool,

    /// Descend at most LEVEL directories deep.
    #[arg(short = 'L', long, value_name = "LEVEL")]
    pub max_depth: Option<usize>,

    /// Show "tests" directories (hidden by default).
    #[arg(long)]
    pub include_tests: bool,

    /// Ignore .gitignore rules (respected by default).
    #[arg(long)]
    pub no_gitignore: bool,

    /// Append each file's modification time.
    #[arg(long)]
    pub age: bool,

    /// Append an estimated token count per file and package (~4 bytes/token
    /// heuristic, not an exact tokenizer). No package total below a -L cutoff.
    #[arg(long)]
    pub tokens: bool,

    /// List each file's top-level definitions (functions, classes, methods)
    /// (requires a build with --features symbols; inert otherwise).
    #[arg(long)]
    pub symbols: bool,

    /// Draw the tree with ASCII characters instead of Unicode.
    #[arg(long)]
    pub ascii: bool,

    /// Output format: text|json|md.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    /// Exclude paths matching GLOB (repeatable; pipe-separated allowed).
    #[arg(short = 'I', long = "ignore", value_name = "GLOB")]
    pub ignore: Vec<String>,

    /// Do not warn about manifests that fail to parse.
    #[arg(long)]
    pub ignore_parsing_errors: bool,

    /// Limit to files changed since git REF (branch, tag, or SHA) plus every
    /// package transitively depending on them. Root must be in a git repo.
    #[arg(long, value_name = "REF")]
    pub since: Option<String>,

    /// Shorthand for --since HEAD (working-tree changes, including untracked).
    /// --since wins if both are given.
    #[arg(long)]
    pub changed: bool,

    /// Abort (exit 3, no output) if a root exceeds N code files [default: 10000].
    #[arg(long, value_name = "N")]
    pub max_files: Option<usize>,

    /// Remove the --max-files cap entirely.
    #[arg(long, visible_alias = "force")]
    pub no_limit: bool,

    /// Show at most N subdirectories and N files per directory, collapsing the
    /// rest into a `[+N folders and F files]` marker [default: 50]. 0 disables.
    #[arg(long, value_name = "N")]
    pub max_per_node: Option<usize>,

    /// Expand every directory in full (disable the --max-per-node cap).
    #[arg(long)]
    pub full: bool,

    /// Read config from FILE instead of discovering .annotated-tree.toml.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Serve over stdio as an MCP server exposing the map, dependency, and
    /// strict-check tools (requires a build with --features mcp).
    #[arg(long)]
    pub mcp: bool,

    /// Print the JSON output schema (map document, strict-check report, and the error
    /// and warning shapes) to stdout and exit, so an agent can fetch the wire contract
    /// programmatically without a human.
    #[arg(long)]
    pub schema: bool,
}

const EXAMPLES: &str = "\
EXAMPLES:
    annotated-tree                    Annotate the current directory
    annotated-tree -L 2 packages/     Limit depth, scope to a subdirectory
    annotated-tree --format json .    Emit machine-readable JSON
    annotated-tree --strict-check .   Lint annotations, exit non-zero on gaps
    annotated-tree --strict-check f.rs  Lint a single file (e.g. a pre-commit hook)
    annotated-tree --since main .     Changed files plus their blast radius";

/// The `ANNOTATION FORMAT:` help section — the compact head of the one canonical annotation
/// guide ([`crate::guide::essence`]), whose `{TEMPLATE}`/`{EXAMPLE}` placeholders are filled
/// from the ENFORCED contract ([`crate::strict::EXPECTED`] + [`crate::config::builtin_example`]), so
/// `--help`, a failing `--strict-check`, and the guide doc advertise the SAME exemplar (no
/// advertise-vs-enforce drift). Built at runtime because it is derived, not a literal.
fn annotation_help_block() -> String {
    crate::guide::essence()
}

/// The `EXIT CODES:` help section. Each line is sourced from the [`exit`] taxonomy
/// constants (not a hand-typed literal), so `--help` cannot advertise a code that has
/// drifted from what `run()`/`main` actually return — the self-correcting-help contract.
fn exit_codes_block() -> String {
    format!(
        "\
EXIT CODES:
    {}  clean run (tree rendered, or --strict-check passed)
    {}  --strict-check found at least one violation
    {}  usage error — bad flag or value (emitted by clap before the run)
    {}  a root exceeded --max-files; nothing written
    {}  precondition/environment error (missing dir, git/--since failure, bad config, I/O)",
        exit::SUCCESS,
        exit::STRICT_FAILURE,
        exit::USAGE,
        exit::RUNAWAY_SCOPE,
        exit::PRECONDITION,
    )
}

/// Parse argv into a [`Cli`]. Builds `after_help` at runtime (rather than a derive
/// literal) so the annotation-format example is sourced from the embedded config
/// via [`annotation_help_block`] and the EXIT CODES block from the [`exit`] constants,
/// keeping help and enforcement in lockstep.
pub fn parse() -> Cli {
    let command = Cli::command().after_help(format!(
        "{EXAMPLES}\n\n{}\n\n{}",
        annotation_help_block(),
        exit_codes_block()
    ));
    let matches = command.get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

impl Cli {
    /// The git ref to diff against, if change-filtering is active: an explicit
    /// `--since <REF>` (which wins), else `HEAD` when `--changed` is set, else
    /// `None` (no filtering — behaviour is unchanged).
    pub fn since_ref(&self) -> Option<String> {
        self.since
            .clone()
            .or_else(|| self.changed.then(|| "HEAD".to_string()))
    }

    pub fn overrides(&self) -> CliOverrides {
        CliOverrides {
            show_age: self.age.then_some(true),
            show_tokens: self.tokens.then_some(true),
            show_symbols: self.symbols.then_some(true),
            ascii: self.ascii.then_some(true),
            gitignore: self.no_gitignore.then_some(false),
            include_tests: self.include_tests.then_some(true),
            config_file: self.config.clone(),
            // `--no-limit`/`--force` wins over `--max-files`; either present means
            // the CLI spoke (outer Some), so env/config are not consulted.
            max_files: if self.no_limit {
                Some(None)
            } else {
                self.max_files.map(Some)
            },
            // `--full` wins over `--max-per-node`; either present means the CLI
            // spoke (outer Some), so config/default are not consulted.
            max_per_node: if self.full {
                Some(None)
            } else {
                self.max_per_node.map(Some)
            },
        }
    }
}
