// Concern: the table of which tools print lines about files, and where in a line each names the file | Non-concern: command eligibility, or reading a path from a line | IO: (tool name) -> Shape or None

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Listed,
    Prefixed,
}

pub const MAP: &[(&str, Shape)] = &[("ls", Shape::Listed), ("find", Shape::Listed), ("grep", Shape::Prefixed)];

pub fn shape_of(tool: &str) -> Option<Shape> {
    MAP.iter().find(|(name, _)| *name == tool).map(|(_, s)| *s)
}
