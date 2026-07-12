// Concern: embeds the canonical annotation-writing guide and renders it (with the enforced template/example substituted) for --help and a failing --strict-check | Non-concern: enforcing the format (strict.rs owns the grader) | IO: none

use crate::config;
use crate::strict;

/// The one canonical guide text, authored in [`docs/annotation-guide.md`] and embedded at
/// build time (like `default_config.toml`). It supersedes the old hand-written `--explain`
/// body: one source, rendered onto every teaching surface.
const GUIDE: &str = include_str!("../docs/annotation-guide.md");

/// Splits the compact `--help` head from the deeper `--strict-check` tail. Everything before
/// it is the essence; the full guide is both halves with the marker removed.
const MORE_MARKER: &str = "<!-- more -->\n";

/// The guide body with its own first-line annotation stripped and the `{TEMPLATE}` /
/// `{EXAMPLE}` placeholders replaced by the ENFORCED contract — so the guide and the grader
/// can never advertise a different shape (the same no-drift discipline `--help`/`--strict-check`
/// already share via [`strict::EXPECTED`]).
///
/// `GUIDE` is an embedded compile-time constant we author on both sides, so its shape is a
/// precondition, not untrusted input (DbC): a malformed doc — no first-line annotation, or a
/// missing section marker — fails loudly here rather than degrading to a silently wrong render.
fn substituted() -> String {
    let (first, rest) = GUIDE
        .split_once('\n')
        .expect("annotation guide has content past its first-line annotation");
    assert!(
        first.trim_start().starts_with("<!--"),
        "annotation guide line 1 must be its own `<!-- … -->` annotation, to strip"
    );
    assert!(
        GUIDE.contains(MORE_MARKER),
        "annotation guide must carry the `{MORE_MARKER}` marker splitting --help essence from the --strict-check tail"
    );
    rest.replace("{TEMPLATE}", strict::EXPECTED.template)
        .replace("{EXAMPLE}", &config::builtin_example())
}

/// The compact form for `--help`: the format, the fields, and the GOOD/FAILS contrast —
/// everything before the `<!-- more -->` marker (which `substituted` guarantees is present).
pub fn essence() -> String {
    let full = substituted();
    let head = full
        .split(MORE_MARKER)
        .next()
        .expect("split always yields a head segment");
    head.trim_end().to_string()
}

/// The full guide, printed on a failing `--strict-check` (unless `--no-guide`) — the
/// push-by-default teaching that replaced the old `--explain` pull command.
pub fn full() -> String {
    // Keep a blank line where the section marker was, so the two halves stay visually split.
    let body = substituted().replace(MORE_MARKER, "\n");
    format!("{}\n", body.trim_end())
}
