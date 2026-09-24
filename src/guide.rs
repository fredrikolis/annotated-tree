// Concern: embeds the canonical annotation-writing guide and renders it for --help and --strict-check | Non-concern: enforcing the format | IO: none

use crate::config;
use crate::embedded_doc;
use crate::strict;

const GUIDE: &str = include_str!("annotation-guide.md");

const MORE_MARKER: &str = "<!-- more -->\n";

/// `{TEMPLATE}`/`{EXAMPLE}` are filled from the ENFORCED contract, so guide and checker
/// cannot advertise different shapes.
fn substituted() -> String {
    let rest = embedded_doc::embedded_body(GUIDE, "annotation guide");
    assert!(GUIDE.contains(MORE_MARKER), "annotation guide must carry the `{MORE_MARKER}` marker splitting --help essence from the --strict-check tail");
    rest.replace("{TEMPLATE}", strict::EXPECTED.template).replace("{EXAMPLE}", &config::builtin_example())
}

pub fn essence() -> String {
    let full = substituted();
    let head = full.split(MORE_MARKER).next().expect("split always yields a head segment");
    head.trim_end().to_string()
}

pub fn full() -> String {
    let body = substituted().replace(MORE_MARKER, "\n");
    format!("{}\n", body.trim_end())
}
