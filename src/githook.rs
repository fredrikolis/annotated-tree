// Concern: embeds the canonical git-hook guide and renders it for --githook-guide | Non-concern: running any hook (.githooks/ owns that) or the annotation format | IO: none

const GUIDE: &str = include_str!("githook-guide.md");

pub fn text() -> String {
    let body = crate::embedded_doc::embedded_body(GUIDE, "git-hook guide");
    format!("{}\n", body.trim())
}
