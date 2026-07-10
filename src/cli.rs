// CLI: Declares the command-line surface and maps flags to config overrides. NOT concerned with execution. | I/O: (argv) -> Cli

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::config::CliOverrides;

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
#[command(
    name = "annotated-tree",
    version,
    after_help = EXAMPLES
)]
pub struct Cli {
    /// Directories to analyze [default: current directory].
    pub paths: Vec<PathBuf>,

    /// Exit non-zero if any code file lacks a conforming first-line annotation.
    #[arg(long)]
    pub strict_check: bool,

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

    /// Abort (exit 2, no output) if a root exceeds N code files [default: 10000].
    #[arg(long, value_name = "N")]
    pub max_files: Option<usize>,

    /// Remove the --max-files cap entirely.
    #[arg(long, visible_alias = "force")]
    pub no_limit: bool,

    /// Read config from FILE instead of discovering .annotated-tree.toml.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Serve over stdio as an MCP server exposing the map, dependency, and
    /// strict-check tools (requires a build with --features mcp).
    #[arg(long)]
    pub mcp: bool,
}

const EXAMPLES: &str = "\
EXAMPLES:
    annotated-tree                    Annotate the current directory
    annotated-tree -L 2 packages/     Limit depth, scope to a subdirectory
    annotated-tree --format json .    Emit machine-readable JSON
    annotated-tree --strict-check .   Lint annotations, exit non-zero on gaps
    annotated-tree --since main .     Changed files plus their blast radius";

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
        }
    }
}
