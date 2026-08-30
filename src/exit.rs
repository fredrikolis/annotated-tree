// Concern: the process exit-code taxonomy — one disjoint code per failure class | Non-concern: the diagnostic text for any of them | IO: none

pub const SUCCESS: i32 = 0;
pub const STRICT_FAILURE: i32 = 1;
pub const USAGE: i32 = 2;
pub const RUNAWAY_SCOPE: i32 = 3;
pub const PRECONDITION: i32 = 4;

pub mod code {
    pub const NOT_A_DIRECTORY: &str = "not_a_directory";
    pub const USAGE: &str = "usage";
    pub const NOT_FOUND: &str = "not_found";
    pub const SCOPE_EXCEEDED: &str = "scope_exceeded";
    pub const GIT_ERROR: &str = "git_error";
    pub const PRECONDITION: &str = "precondition";
    /// NON-FATAL: still exits `SUCCESS`, rides `warnings`.
    pub const MANIFEST_PARSE_ERROR: &str = "manifest_parse_error";
}
