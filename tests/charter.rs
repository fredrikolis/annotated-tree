// Concern: end-to-end tests for the folder charter — resolution, render, JSON, and the require_package_charter gate | Non-concern: the resolution units or the grammar | IO: (fixtures) -> asserted output

use std::path::PathBuf;

use annotated_tree::Cli;
use clap::Parser;

fn run(extra: &[&str], fixture: &str) -> (String, i32) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (String::from_utf8(out).unwrap(), code)
}

/// The dir-row line for the directory named `name`/ (its trailing ` # …`), or `None`.
fn dir_line<'a>(out: &'a str, name: &str) -> Option<&'a str> {
    out.lines().find(|l| l.contains(&format!("{name}/")))
}

#[test]
fn annotation_breadcrumb_charters_a_grouping_folder() {
    // A pure grouping folder (no code entry file) carries a `.annotation` breadcrumb, so its
    // directory row shows the authored charter — the universal fallback for entry-file-less dirs.
    let (out, code) = run(&[], "charter");
    assert_eq!(code, 0);
    let line = dir_line(&out, "grouping").expect("grouping/ dir row is present");
    assert!(
        line.contains("# Concern: groups the reusable widgets")
            && line.contains("Non-concern: implementing any widget")
            && line.contains("IO: none"),
        "grouping/ shows its .annotation charter on the directory row: {line}"
    );
}

#[test]
fn annotation_breadcrumb_overrides_the_entry_file() {
    // Most-explicit-wins: a `.annotation` beside an entry file (`__init__.py`) overrides the
    // promoted entry-file annotation on the DIRECTORY row — while the entry file still carries
    // its own annotation on its OWN row (different subjects, both true; not a DRY violation).
    let (out, _) = run(&[], "charter");
    let dir = dir_line(&out, "overridden").expect("overridden/ dir row is present");
    assert!(
        dir.contains("AUTHORED directory charter"),
        "the .annotation charter wins on the directory row: {dir}"
    );
    assert!(
        !dir.contains("ENTRY FILE self-annotation"),
        "the entry file's annotation must NOT be promoted when .annotation overrides it: {dir}"
    );
    let file = out
        .lines()
        .find(|l| l.contains("__init__.py"))
        .expect("__init__.py file row is present");
    assert!(
        file.contains("ENTRY FILE self-annotation"),
        "the entry file keeps its own annotation on its own row: {file}"
    );
}

#[test]
fn entry_file_annotation_is_promoted_to_the_directory_row() {
    // With no `.annotation`, a package dir's charter is its code entry file's annotation,
    // promoted for free. `charter/overridden` has an override, so use the `charter_rule`
    // crate whose `src/lib.rs` is the entry file and whose dir carries no breadcrumb.
    let (out, code) = run(&[], "charter_rule");
    assert_eq!(code, 0, "rendering (not strict) exits 0");
    let line = dir_line(&out, "withcharter").expect("withcharter/ dir row is present");
    assert!(
        line.contains("# Concern: owns the widget rendering pipeline"),
        "the crate's src/lib.rs annotation is promoted onto the crate directory row: {line}"
    );
}

#[test]
fn charter_surfaces_as_a_keyed_object_in_json() {
    // JSON carries the charter as a keyed `{concern, non_concern, io}` object (agent dispatch),
    // and omits it on charter-less dirs (absent-key convention keeps such trees byte-identical).
    let (out, code) = run(&["--format", "json"], "charter");
    assert_eq!(code, 0);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("json parses");
    let dirs = doc["roots"][0]["dirs"].as_array().expect("dirs array");
    let grouping = dirs
        .iter()
        .find(|d| d["name"] == serde_json::json!("grouping"))
        .expect("grouping dir node");
    assert_eq!(
        grouping["charter"]["concern"],
        serde_json::json!("groups the reusable widgets"),
        "charter.concern is a keyed field"
    );
    assert_eq!(grouping["charter"]["io"], serde_json::json!("none"));
}

#[test]
fn malformed_annotation_breadcrumb_fails_strict_check() {
    // Opting in means doing it right: a present-but-malformed `.annotation` is a FATAL violation
    // (not a silent no-op), diagnosed by the SAME grammar as a file annotation and located at
    // the breadcrumb path.
    let (out, code) = run(&["--strict-check"], "charter_malformed");
    assert_eq!(code, 1, "a malformed .annotation fails the check:\n{out}");
    assert!(
        out.contains("bad/.annotation:1: annotation is malformed [charter]"),
        "the .annotation is enforced and located: {out}"
    );
}

#[test]
fn malformed_annotation_serializes_as_a_located_violation() {
    // The structured surface: the breadcrumb violation rides in `violations` with the
    // `.annotation` path and the shared `malformed_annotation` category — an agent dispatches
    // on it exactly like a file annotation violation.
    let (out, code) = run(&["--strict-check", "--format", "json"], "charter_malformed");
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("json parses");
    let v = doc["violations"]
        .as_array()
        .expect("violations array")
        .iter()
        .find(|v| v["path"] == serde_json::json!("bad/.annotation"))
        .expect("the .annotation surfaces as a violation");
    assert_eq!(v["category"], serde_json::json!("malformed_annotation"));
    assert_eq!(v["language"], serde_json::json!("charter"));
}

#[test]
fn require_package_charter_flags_a_charterless_package() {
    // The opt-in gate (the fixture's `.annotated-tree.toml` sets require_package_charter = true):
    // `nocharter` owns an annotated file but its crate resolves no charter (its annotated file is
    // not the entry file) — a fatal rule violation. `withcharter` (entry-file charter) is clean.
    // ALSO the regression guard for the shared `annotated_files` / `packages_owning_annotations`
    // path: the removed orphan advisory was the other consumer, and the graph build is now
    // reached only via `Rules::is_active()`. If either seam broke, this gate goes silent.
    let (out, code) = run(&["--strict-check"], "charter_rule");
    assert_eq!(
        code, 1,
        "a charterless package fails the opt-in gate:\n{out}"
    );
    assert!(
        out.contains("nocharter") && out.contains("no concern charter"),
        "the missing-charter package is named in the finding: {out}"
    );
    assert!(
        !out.contains("'withcharter'"),
        "a package with a resolved charter is not flagged: {out}"
    );
}

#[test]
fn require_package_charter_serializes_with_a_stable_code() {
    // Structured: the gate rides the existing rule-violation surface with the stable dispatch
    // code `missing_package_charter` and the offending package dir — no new envelope field.
    let (out, code) = run(&["--strict-check", "--format", "json"], "charter_rule");
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("json parses");
    let rv = doc["rule_violations"]
        .as_array()
        .expect("rule_violations array")
        .iter()
        .find(|v| v["code"] == serde_json::json!("missing_package_charter"))
        .expect("a missing_package_charter rule violation is present");
    assert_eq!(rv["packages"], serde_json::json!(["nocharter"]));
    assert_eq!(rv["path"], serde_json::json!("packages/nocharter"));
}

#[test]
fn the_trailing_whitespace_guard_cannot_be_silently_disarmed() {
    // THE TRAP, anchored. Every editor terminates a file with a newline, so a rule keyed on "the
    // body contains a newline" would have failed every real adopter on day one — this repo's own
    // root `.annotation` included — while the hand-crafted fixtures, which had NO trailing
    // newline, sailed through and kept the suite green.
    //
    // `annotation_breadcrumb_charters_a_grouping_folder` and
    // `charter_surfaces_as_a_keyed_object_in_json` are what prove such a charter still resolves
    // and renders. They prove it ONLY while this fixture ends in whitespace, and neither says so,
    // so a tidy-up that stripped it would disarm both without failing anything. This asserts the
    // byte fact they rest on.
    let body = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/charter/grouping/.annotation"),
    )
    .expect("the grouping charter fixture is readable");
    assert!(
        body.ends_with("\n\n"),
        "the fixture must end in a trailing newline AND a blank line, or the two tests that rest \
         on it stop testing the trap: {body:?}"
    );
    assert_eq!(
        body.lines().filter(|l| !l.trim().is_empty()).count(),
        1,
        "and it must still hold exactly ONE line of content: {body:?}"
    );
}

#[test]
fn a_charter_with_prose_below_its_line_resolves_to_no_charter() {
    // TREE1: a Report shows no other text in an Annotation's place. A `.annotation` holding more
    // than its one line resolves to NOTHING — the directory row carries no charter rather than a
    // charter with a newline inside it — and the stray prose reaches no surface at all.
    let (out, code) = run(&[], "charter_noisy");
    assert_eq!(code, 0, "rendering still exits 0:\n{out}");
    let line = dir_line(&out, "noisy").expect("noisy/ dir row is present");
    assert!(
        !line.contains('#'),
        "the directory row carries no charter: {line}"
    );
    assert!(
        !out.contains("STRAY-PROSE") && !out.contains("NOISY-CHARTER"),
        "neither the stray prose nor the line it was attached to reaches the map:\n{out}"
    );
    // One line per rendered path — noisy/, noisy/inner.rs, top.rs — which is the property the
    // whole plan exists to restore. Counted over the tree rows themselves, not over stdout
    // lines, so a footer note added later cannot fail this spuriously.
    assert_eq!(
        out.lines().filter(|l| l.contains("── ")).count(),
        3,
        "one row per path:\n{out}"
    );

    let (json, _) = run(&["--format", "json"], "charter_noisy");
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json parses");
    let noisy = doc["roots"][0]["dirs"]
        .as_array()
        .expect("dirs array")
        .iter()
        .find(|d| d["name"] == serde_json::json!("noisy"))
        .expect("noisy dir node");
    assert!(
        noisy.get("charter").is_none(),
        "and the machine contract carries no charter either: {noisy}"
    );
}

#[test]
fn a_charter_with_prose_below_its_line_fails_strict_check_alone() {
    // The gate that could not see this before. It FAILS, it is located at the file an author
    // edits and at the first stray line, and it is reported ALONE: `violations` stays empty,
    // because which line is the annotation is exactly what this file has not settled.
    let (out, code) = run(&["--strict-check", "--no-guide"], "charter_noisy");
    assert_eq!(code, 1, "a multi-line charter now fails:\n{out}");
    assert!(
        out.contains("noisy/.annotation:2: holds more than the one line"),
        "located at the charter, at the first stray line:\n{out}"
    );
    assert!(
        out.contains("Found 1 annotation file(s) with trailing content"),
        "with its own count, kept out of the annotation error count:\n{out}"
    );
    assert!(
        !out.contains("STRAY-PROSE"),
        "the stray prose is never echoed into the report:\n{out}"
    );

    let (json, code) = run(&["--strict-check", "--format", "json"], "charter_noisy");
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("strict json parses");
    assert_eq!(doc["passed"], serde_json::json!(false));
    assert_eq!(
        doc["error_count"],
        serde_json::json!(0),
        "not an annotation error — the parts were never judged: {json}"
    );
    assert!(
        doc["violations"]
            .as_array()
            .expect("violations array")
            .is_empty(),
        "the part diagnosis is suppressed, not stacked: {json}"
    );
    let trailing = doc["trailing_content"]
        .as_array()
        .expect("trailing_content array");
    assert_eq!(trailing.len(), 1, "{json}");
    assert_eq!(trailing[0]["path"], serde_json::json!("noisy/.annotation"));
    assert_eq!(trailing[0]["line"], serde_json::json!(2));
}
