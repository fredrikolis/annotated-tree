// Concern: embeds the canonical annotation-writing guide and renders it for --help and --strict-check | Non-concern: enforcing the format | IO: none

use crate::config;
use crate::strict;

/// The one canonical guide text, embedded at build time: one source, rendered onto every teaching
/// surface.
const GUIDE: &str = include_str!("annotation-guide.md");

/// Splits the compact `--help` head from the deeper `--strict-check` tail.
const MORE_MARKER: &str = "<!-- more -->\n";

/// The guide body, annotation stripped and `{TEMPLATE}`/`{EXAMPLE}` filled from the ENFORCED
/// contract, so guide and checker cannot advertise different shapes. An authored constant, so a
/// malformed doc fails loudly (DbC) rather than degrading to a silently wrong render.
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

/// The compact form for `--help`: everything before the `<!-- more -->` marker.
pub fn essence() -> String {
    let full = substituted();
    let head = full
        .split(MORE_MARKER)
        .next()
        .expect("split always yields a head segment");
    head.trim_end().to_string()
}

/// The full guide, printed on a failing `--strict-check` unless `--no-guide`.
pub fn full() -> String {
    // Keep a blank line where the section marker was, so the two halves stay visually split.
    let body = substituted().replace(MORE_MARKER, "\n");
    format!("{}\n", body.trim_end())
}
