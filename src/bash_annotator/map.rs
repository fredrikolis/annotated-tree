// Concern: the table of which tools print lines about files, and where in a line each names the file | Non-concern: command eligibility, or reading a path from a line | IO: (tool name) -> Shape or None

/// Where in its output line a tool names the file that line is about.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    /// The whole line or its TRAILING field — `ls`, `find`. Scanned as suffixes, so spaces are fine.
    Listed,
    /// A `path:` PREFIX; everything after is CONTENT, so a later path is a match, not the subject.
    Prefixed,
}

/// THE MAP. A tool belongs here when its stdout is a list of paths an agent reads. THE TOOL IS NEVER
/// SUBSTITUTED — the token is left as typed, running the session's own `grep`/`find`.
pub const MAP: &[(&str, Shape)] = &[
    ("ls", Shape::Listed),
    ("find", Shape::Listed),
    ("grep", Shape::Prefixed),
];

pub fn shape_of(tool: &str) -> Option<Shape> {
    MAP.iter().find(|(name, _)| *name == tool).map(|(_, s)| *s)
}
