// Concern: embeds the canonical git-hook guide and renders it for --githook-guide | Non-concern: running any hook (.githooks/ owns that) or the annotation format | IO: none

/// The one canonical git-hook guide, embedded at build time. `--githook-guide` prints it whole, so
/// an agent can reproduce the repo's enforcement hooks without a human.
const GUIDE: &str = include_str!("githook-guide.md");

/// The guide body with its own first-line annotation stripped — scaffolding for this repo's gate,
/// not part of what a caller reads. An authored constant, so a malformed doc fails loudly (DbC).
pub fn text() -> String {
    let (first, rest) = GUIDE
        .split_once('\n')
        .expect("git-hook guide has content past its first-line annotation");
    assert!(
        first.trim_start().starts_with("<!--"),
        "git-hook guide line 1 must be its own `<!-- … -->` annotation, to strip"
    );
    format!("{}\n", rest.trim())
}
