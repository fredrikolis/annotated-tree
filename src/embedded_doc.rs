// Concern: strips a compiled-in document's own first-line annotation | Non-concern: reading a file's annotation, or what any doc says | IO: (doc text) -> body

/// The body of a document embedded with `include_str!`, its own first-line annotation dropped.
/// Read with the BUILT-IN markdown grammar, never a [`Config`](crate::config::Config): a
/// compiled-in document's grammar cannot vary with a user's file. Panics unless line 1 is wholly
/// one conforming annotation — its FORM only, no length bound — so a malformed doc fails loudly.
pub(crate) fn embedded_body<'a>(doc: &'a str, name: &str) -> &'a str {
    assert_eq!(
        crate::annotation::sole_annotation_line(doc, &crate::config::builtin_markdown()),
        Some(0),
        "{name} line 1 must be nothing but a conforming three-field `<!-- … -->` annotation, to strip"
    );
    doc.split_once('\n')
        .expect("a located annotation on line 1 is followed by a newline")
        .1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_body_strips_line_one_and_keeps_the_rest_verbatim() {
        let doc = "<!-- Concern: a | Non-concern: b | IO: none -->\n# Title\n\nbody\n";
        assert_eq!(embedded_body(doc, "doc"), "# Title\n\nbody\n");
    }

    #[test]
    #[should_panic(expected = "doc line 1")]
    fn embedded_body_panics_rather_than_emit_a_line_it_cannot_strip() {
        // The failure that matters: a doc whose line 1 is prose would otherwise render whole, and a near-miss annotation would ship its own scaffolding into an agent's context.
        embedded_body("# Title\n\nbody\n", "doc");
    }

    #[test]
    #[should_panic(expected = "doc line 1")]
    fn embedded_body_panics_on_a_malformed_annotation() {
        embedded_body("<!-- Concern: a | IO: none -->\nbody\n", "doc");
    }
}
