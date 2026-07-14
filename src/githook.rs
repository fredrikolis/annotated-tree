// Concern: embeds the canonical git-hook guide and renders it for --githook-guide | Non-concern: running any hook (the shipped .githooks/ scripts do that) or the annotation format (guide.rs owns that) | IO: none

/// The one canonical git-hook guide, authored in [`docs/githook-guide.md`] and embedded at
/// build time (like the annotation guide in [`crate::guide`] and `default_config.toml`).
/// `--githook-guide` prints it whole, so an agent can reproduce the repo's local enforcement
/// hooks without a human — the same push-to-the-agent teaching the annotation guide gives.
const GUIDE: &str = include_str!("../docs/githook-guide.md");

/// The guide body with its own first-line annotation stripped. The doc carries a
/// `<!-- … -->` annotation because the repo's own pre-commit gate requires one on every
/// file — that line is scaffolding for the linter, not part of the guide a caller reads.
///
/// `GUIDE` is an embedded compile-time constant we author, so its shape is a precondition,
/// not untrusted input (DbC): a doc with no first-line annotation fails loudly here rather
/// than leaking the `<!-- … -->` line onto the rendered surface.
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
