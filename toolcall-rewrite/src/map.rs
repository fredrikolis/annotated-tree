// Concern: the single table of which tools print lines that are about files, and where in a line each one names the file | Non-concern: deciding a command is eligible (inject.rs) and reading a path out of a line (run.rs) | IO: (tool name) -> Shape, or None when the tool is not mapped

/// Where in its output line a tool names the file that line is about.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    /// The path is the whole line, or its TRAILING field — `ls`, `find`. Scanned as suffixes, so
    /// a name containing spaces stays one name.
    Listed,
    /// The path is a `path:` PREFIX and everything after it is file CONTENT, so a path occurring
    /// later in the line is part of a match and not the subject — `grep`.
    Prefixed,
}

/// THE MAP. A tool belongs here when its stdout is a list of paths an agent reads. Adding one is a
/// single line and nothing else in this crate learns about it: the annotator describes whatever
/// paths a tool printed rather than predicting where they will be, so it needs no knowledge of the
/// new tool's flags. A tool absent from this table is never annotated.
///
/// THE TOOL IS NEVER SUBSTITUTED. `grep` and `find` are shell FUNCTIONS in a Claude Code session,
/// and leaving the program token exactly as the agent typed it is what makes the session's own
/// engine — and its gitignore filtering — run, precisely as it would have without this installed.
/// An earlier design spawned an engine itself and had to transcribe the session's flags to match;
/// letting the caller's own shell resolve the name is both smaller and correct by construction, and
/// it is why `\grep`, `command grep`, `/usr/bin/grep` and `sudo grep` need no special handling.
pub const MAP: &[(&str, Shape)] = &[
    ("ls", Shape::Listed),
    ("find", Shape::Listed),
    ("grep", Shape::Prefixed),
];

/// The shape for a tool, or `None` when the tool is not in the table.
pub fn shape_of(tool: &str) -> Option<Shape> {
    MAP.iter().find(|(name, _)| *name == tool).map(|(_, s)| *s)
}
