// Concern: the process exit-code taxonomy — one disjoint code per failure class | Non-concern: the diagnostic text for any of them | IO: none

//! # Exit-code contract (`annotated-tree`)
//! Exit codes are a dispatch key an agent branches recovery on, so each class is a distinct integer,
//! never overloaded, and `--help` imports these same names. The strings in [`code`] are the
//! JSON-envelope counterpart, finer-grained: several of them pair with `PRECONDITION`.

pub const SUCCESS: i32 = 0;
pub const STRICT_FAILURE: i32 = 1;
pub const USAGE: i32 = 2;
pub const RUNAWAY_SCOPE: i32 = 3;
pub const PRECONDITION: i32 = 4;

pub mod code {
    pub const NOT_A_DIRECTORY: &str = "not_a_directory";
    pub const SCOPE_EXCEEDED: &str = "scope_exceeded";
    pub const GIT_ERROR: &str = "git_error";
    pub const PRECONDITION: &str = "precondition";
    /// NON-FATAL: the run still exits `SUCCESS`, and this rides the JSON envelope's `warnings`.
    pub const MANIFEST_PARSE_ERROR: &str = "manifest_parse_error";
}
