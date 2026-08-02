// Concern: the process exit-code taxonomy — one disjoint code per failure class | Non-concern: the diagnostic text for any of them | IO: none

//! # Exit-code contract (`annotated-tree`)
//!
//! Exit codes are a dispatch key an agent branches recovery on, so each class is a distinct
//! integer, never overloaded. Every process-exit path routes through one of these constants,
//! and `--help` imports the same names, so the advertised contract cannot drift from the
//! enforced one.

/// A clean run: the tree rendered, or `--strict-check` passed.
pub const SUCCESS: i32 = 0;

/// `--strict-check` found at least one annotation or architectural-rule violation.
pub const STRICT_FAILURE: i32 = 1;

/// Usage / argument error — the invocation itself could not be acted on. Emitted by
/// clap's own error path (`Error::exit`) before `run()` is reached for a bad flag or
/// value, and returned FROM `run()` by the `bash-annotator` verb when no single mode
/// flag is set, or when the mode it names has nothing to work on.
pub const USAGE: i32 = 2;

/// Runaway-scope abort: a root exceeded the `--max-files` cap, so the walk was aborted
/// with EMPTY stdout (no partial tree / half-written JSON). Recover by raising
/// `--max-files <N>` or passing `--no-limit`.
pub const RUNAWAY_SCOPE: i32 = 3;

/// Precondition / environment error: the run could not start because its inputs were
/// not valid — a non-existent root directory, or a git / `--since` failure (not a repo,
/// missing git, bad ref), plus any other setup failure (bad config, I/O). Recover by
/// fixing the environment, not by re-issuing a differently-flagged command.
pub const PRECONDITION: i32 = 4;

/// Stable string dispatch codes — the JSON-error-envelope (`--format json`) counterpart
/// to the integer exit codes above. Same taxonomy, string form: an agent parsing stdout
/// JSON branches on `error.code` exactly
/// as a shell branches on `$?`. Prose messages drift; these codes are the API. Finer than
/// the integer codes (several precondition classes share [`PRECONDITION`]), so a caller
/// can tell a git failure from a bad directory. Every string a fallible surface emits is
/// named here — no code literal is scattered across the tree.
pub mod code {
    /// A supplied root path was not an existing directory. Pairs with
    /// [`super::PRECONDITION`].
    pub const NOT_A_DIRECTORY: &str = "not_a_directory";
    /// A root exceeded `--max-files`; nothing was produced. Pairs with
    /// [`super::RUNAWAY_SCOPE`].
    pub const SCOPE_EXCEEDED: &str = "scope_exceeded";
    /// A git / `--since` operation failed (not a repo, missing git, bad ref). Pairs with
    /// [`super::PRECONDITION`].
    pub const GIT_ERROR: &str = "git_error";
    /// Any other precondition/environment failure (bad config, invalid `-I` glob, I/O).
    /// Pairs with [`super::PRECONDITION`].
    pub const PRECONDITION: &str = "precondition";
    /// A package manifest (`Cargo.toml`, `package.json`, …) could not be read or parsed.
    /// NON-FATAL and so has no exit-code pairing: the map is still produced (the run exits
    /// [`super::SUCCESS`]), but the offending package contributes no edges, so this rides
    /// in the JSON envelope's `warnings` array (and the CLI's stderr) to signal that the
    /// dependency graph is incomplete — distinguishing "no deps" from "couldn't read them".
    pub const MANIFEST_PARSE_ERROR: &str = "manifest_parse_error";
}
