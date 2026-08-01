// Concern: end-to-end tests freezing the annotation form check and the whole-line length bound | Non-concern: the checker units or the golden report text | IO: (fixtures) -> asserted (stdout, code)

use annotated_tree::Cli;
use clap::Parser;

/// A throwaway tree under the OS temp dir holding `files` (relative path -> content), and the
/// `--strict-check` verdict over it. `name` keys the directory so parallel tests never collide.
/// Pass `--no-guide` when asserting on TEXT: a failing run otherwise appends the whole guide.
fn check(name: &str, args: &[&str], files: &[(&str, &str)]) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("at-form-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir fixture");
    for (rel, body) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir parent");
        }
        std::fs::write(&path, body).expect("write fixture");
    }
    let mut argv = vec!["annotated-tree".to_string(), "--strict-check".to_string()];
    argv.extend(args.iter().map(|s| (*s).to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (String::from_utf8(out).expect("utf8"), code)
}

#[test]
fn wording_is_never_a_finding() {
    // Form only: filler words, a Non-concern pointing at the file itself, and `<…>`
    // placeholder slots are all present and non-empty, so every one of them PASSES.
    let (out, code) = check(
        "wording",
        &["--no-guide"],
        &[
            (
                "filler.rs",
                "// Concern: utils | Non-concern: none | IO: none\n",
            ),
            (
                "inward.rs",
                "// Concern: caches lookups | Non-concern: this file's own state | IO: (Key) -> Value\n",
            ),
            (
                "slots.rs",
                "// Concern: <what it does> | Non-concern: <concern owned elsewhere> | IO: (<inputs>) -> <outputs>\n",
            ),
        ],
    );
    assert_eq!(
        code, 0,
        "a well-formed annotation passes whatever words it holds:\n{out}"
    );
    assert!(out.contains("All 3 files passed"), "{out}");
}

#[test]
fn the_emitted_suggestion_itself_passes() {
    // The sharpest single assertion for this change: the file-tailored stub the report prints
    // is itself a well-formed line, so applying it clears the form defect instead of stacking
    // a second one. Its `<…>` slots are still unwritten judgments, and a length bound would
    // apply to the stub like any other line.
    let (json, code) = check(
        "suggestion",
        &["--format", "json"],
        &[("gap.rs", "let x = 1;\n")],
    );
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json parses");
    let suggestion = doc["violations"][0]["suggestion"]
        .as_str()
        .expect("the violation carries a suggestion")
        .to_string();
    let (out, code) = check(
        "suggestion-round-trip",
        &["--no-guide"],
        &[("gap.rs", &format!("{suggestion}\n"))],
    );
    assert_eq!(
        code, 0,
        "the emitted suggestion must pass the check that emitted it:\n{out}"
    );
}

#[test]
fn an_empty_field_fails_and_the_text_says_which() {
    // Without the detail clause the human line claims `concern` is missing while `Concern:`
    // is plainly visible in `found` — the defect the carrier exists to fix.
    let (out, code) = check(
        "empty-text",
        &["--no-guide"],
        &[(
            "empty.rs",
            "// Concern:  | Non-concern: eviction | IO: (a) -> b\n",
        )],
    );
    assert_eq!(code, 1, "an empty field is fatal:\n{out}");
    assert!(
        out.contains("annotation is malformed")
            && out.contains("— the Concern field is present but empty"),
        "the TEXT line carries the detail clause: {out}"
    );
}

#[test]
fn every_empty_field_is_named_in_the_machine_defect() {
    let (json, code) = check(
        "empty-json",
        &["--format", "json"],
        &[("empty.rs", "// Concern:  | Non-concern:  | IO: (a) -> b\n")],
    );
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json parses");
    let v = &doc["violations"][0];
    assert_eq!(v["category"], serde_json::json!("malformed_annotation"));
    assert_eq!(
        v["defect"]["missing"],
        serde_json::json!(["concern", "non_concern"]),
        "both empty fields are named, not just the first: {v}"
    );
    assert!(
        v.get("suggestion").is_some(),
        "the converse of `annotation_too_long`: a malformed line DOES carry a stub: {v}"
    );
}

#[test]
fn a_broken_structure_with_every_key_reports_all_three() {
    let (json, code) = check(
        "broken",
        &["--format", "json"],
        &[("broken.rs", "// Concern: a|Non-concern: b|IO: c\n")],
    );
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json parses");
    let v = &doc["violations"][0];
    assert_eq!(
        v["defect"]["missing"],
        serde_json::json!(["concern", "non_concern", "io"]),
        "no part could be extracted, so all three are reported: {v}"
    );
    assert!(
        v["detail"]
            .as_str()
            .is_some_and(|d| d.contains("separators")),
        "the detail explains why every key is visible yet nothing parsed: {v}"
    );
}

#[test]
fn the_length_bound_fires_only_past_the_limit() {
    // The bound is on the WHOLE annotation, so these are measured line-length-first.
    let body = |n: usize| format!("// Concern: {} | Non-concern: b | IO: c\n", "x".repeat(n));
    // "Concern: " + n + " | Non-concern: b | IO: c" is n + 34 characters.
    let under = body(5); // 39
    let at = body(6); // 40
    let over = body(7); // 41

    let (out, code) = check(
        "len-pass",
        &["--no-guide", "--max-length", "40"],
        &[("under.rs", &under), ("at.rs", &at)],
    );
    assert_eq!(code, 0, "under and exactly at the bound pass:\n{out}");

    let (out, code) = check(
        "len-over",
        &["--no-guide", "--max-length", "40"],
        &[("over.rs", &over)],
    );
    assert_eq!(code, 1, "one character past the bound fails:\n{out}");
    assert!(
        out.contains("the annotation is 41 characters, over the 40 limit"),
        "the whole line is counted, and named as such: {out}"
    );

    // No flag, no repo config: the built-in layer supplies 200.
    let long = format!("// Concern: {} | Non-concern: b | IO: c\n", "x".repeat(250));
    let (out, code) = check("len-default", &["--no-guide"], &[("long.rs", &long)]);
    assert_eq!(
        code, 1,
        "the shipped 200 bound applies with no flag:\n{out}"
    );
    assert!(
        out.contains("over the 200 limit"),
        "the default bound is 200: {out}"
    );
}

#[test]
fn the_length_bound_is_machine_readable_and_covers_charters() {
    // A `.annotation` charter line is held to the SAME bound as a file annotation, and the
    // structured surface carries the annotation's length plus the bound, as two numbers.
    let long = "z".repeat(30);
    let (json, code) = check(
        "len-json",
        &["--format", "json", "--max-length", "20"],
        &[
            ("ok.rs", "// Concern: a | Non-concern: b | IO: c\n"),
            (
                ".annotation",
                &format!("Concern: {long} | Non-concern: b | IO: c\n"),
            ),
        ],
    );
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json parses");
    let v = doc["violations"]
        .as_array()
        .expect("violations array")
        .iter()
        .find(|v| v["path"] == serde_json::json!(".annotation"))
        .expect("the over-length charter surfaces as a violation");
    assert_eq!(v["category"], serde_json::json!("annotation_too_long"));
    assert_eq!(v["language"], serde_json::json!("charter"));
    assert_eq!(
        v["defect"]["length"],
        serde_json::json!(
            "Concern: zzzzzzzzzzzzzzzzzzzzzzzzzzzzzz | Non-concern: b | IO: c"
                .chars()
                .count()
        ),
        "the annotation's own length, not a field's: {v}"
    );
    assert_eq!(
        v["defect"]["max"],
        serde_json::json!(20),
        "the bound is one number per violation: {v}"
    );
    assert!(
        v["defect"].get("too_long").is_none(),
        "the retired per-field list is gone: {v}"
    );
}

#[test]
fn malformed_outranks_too_long() {
    // At most one outcome per file: an over-length line that also has an empty field stays
    // `malformed_annotation` — structure is the more basic defect.
    let long = "w".repeat(40);
    let (json, code) = check(
        "precedence",
        &["--format", "json", "--max-length", "10"],
        &[(
            "both.rs",
            &format!("// Concern: {long} | Non-concern:  | IO: c\n"),
        )],
    );
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json parses");
    let v = &doc["violations"][0];
    assert_eq!(
        v["category"],
        serde_json::json!("malformed_annotation"),
        "structure outranks length: {v}"
    );
    assert!(
        v["defect"].get("too_long").is_none(),
        "one violation carries one defect kind: {v}"
    );
}

#[test]
fn a_clean_tree_carries_no_warnings_surface() {
    // The advisory channel is gone: the JSON document has no `warnings` key at all, and the
    // TEXT report has no `Found N warning(s)` block.
    let files: &[(&str, &str)] = &[("ok.rs", "// Concern: a | Non-concern: b | IO: c\n")];
    let (json, code) = check("no-warn-json", &["--format", "json"], files);
    assert_eq!(code, 0);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json parses");
    assert!(
        doc.get("warnings").is_none(),
        "the strict report has no `warnings` key: {json}"
    );

    let (out, code) = check("no-warn-text", &[], files);
    assert_eq!(code, 0);
    assert!(
        !out.contains("warning"),
        "no advisory block on the TEXT surface: {out}"
    );
}

#[test]
fn a_comment_wrapped_charter_names_the_marker_and_suggests_the_bare_line() {
    // A `.annotation` file is the one place the line is written bare, so wrapping it in the
    // marker every other annotation carries is the easy mistake. Reporting "the separators
    // are missing" is the one diagnosis that cannot be true when they are plainly there, and
    // a stub seeded from the wrapped text embeds the marker, so it cannot be pasted either.
    let (out, code) = check(
        "wrapped-charter",
        &["--no-guide"],
        &[
            (
                ".annotation",
                "<!-- Concern: demo crate | Non-concern: y | IO: none -->\n",
            ),
            ("a.rs", "// Concern: a | Non-concern: b | IO: none\n"),
        ],
    );
    assert_eq!(code, 1, "a wrapped charter is still malformed:\n{out}");
    assert!(
        out.contains("remove the `<!--` and `-->`"),
        "the diagnosis names the marker to delete:\n{out}"
    );
    assert!(
        !out.contains("field separators are missing"),
        "and never claims the separators are missing when they are there:\n{out}"
    );
    assert!(
        out.contains("suggestion: Concern: demo crate | Non-concern: y | IO: none"),
        "the suggestion is the line from inside the wrapper — usable as printed:\n{out}"
    );
}

#[test]
fn yaml_frontmatter_keeps_line_one_and_the_annotation_still_counts() {
    // A Claude Code skill (and any static-site page) must keep its frontmatter at line 1, so
    // requiring the annotation above it made shipping skills and enforcing --strict-check
    // mutually exclusive. The block is skipped like a shebang; an unclosed `---` is not a
    // block, so a document that merely opens with a horizontal rule is unaffected.
    let (out, code) = check(
        "frontmatter",
        &["--no-guide"],
        &[
            (
                "SKILL.md",
                "---\ndescription: reviews code\n---\n<!-- Concern: the review brief | Non-concern: running it | IO: none -->\n\n# Skill\n",
            ),
            ("rule.md", "---\n\nopens with a horizontal rule\n"),
        ],
    );
    assert_eq!(code, 1, "only the rule-opener file fails:\n{out}");
    assert!(
        !out.contains("SKILL.md"),
        "an annotation under frontmatter passes:\n{out}"
    );
    assert!(
        out.contains("rule.md:1: missing annotation"),
        "an unclosed `---` is a horizontal rule, not a prefix to skip:\n{out}"
    );
    assert!(
        out.contains("1 of 2 files annotated"),
        "the skill counts toward coverage:\n{out}"
    );
}
