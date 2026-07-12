// Concern: end-to-end tests freezing the `annotation_on_orphan` cross-file advisory — a fully-annotated crate the graph shows orphaned inside a structured ecosystem earns a NON-FATAL warning naming the dead package, without failing the check or flagging connected packages | Non-concern: the reusable orphan definition (unit-tested in src/rules.rs) | IO: (orphan fixture) -> asserted (stdout, code)

use std::path::PathBuf;

use annotated_tree::Cli;
use clap::Parser;

fn run_strict(fixture: &str, extra: &[&str]) -> (String, i32) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let mut argv = vec!["annotated-tree".to_string(), "--strict-check".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (String::from_utf8(out).unwrap(), code)
}

#[test]
fn annotated_orphan_package_earns_a_non_fatal_warning() {
    // The fixture: alpha -> beta (a real Cargo path edge, so the ecosystem has structure)
    // plus `ghost`, a fully-annotated crate nothing imports and that imports nothing. No
    // `[rules]` are configured, so this fires independently of the opt-in `forbid_orphans`
    // rule — it is the always-on advisory.
    let (out, code) = run_strict("orphan_annotation", &[]);

    // NON-FATAL: every annotation conforms and no rule is active, so the check PASSES.
    assert_eq!(
        code, 0,
        "an orphan advisory must not fail the check:\n{out}"
    );
    assert!(
        out.contains("annotation_on_orphan") && out.contains("ghost"),
        "the advisory must name the orphaned package by its code and name:\n{out}"
    );
    assert!(
        out.contains("does not fail the check"),
        "the advisory must state it is non-fatal:\n{out}"
    );
    // The connected packages are not orphans and must NOT be flagged.
    assert!(
        !out.contains("'alpha'") && !out.contains("'beta'"),
        "a package with an edge in or out is not an orphan:\n{out}"
    );
}

#[test]
fn a_scan_root_package_is_never_flagged_as_an_orphan() {
    // The charter carve-out: the fixture's ROOT carries its own annotated crate (`charter`),
    // which is depended-on-by-nothing and imports nothing internal — orphaned by the raw
    // definition, but a top-level deliverable BY DESIGN, not by accident. It must stay silent
    // while `ghost`, a genuinely disconnected INNER crate, is still flagged. alpha -> beta
    // gives the Cargo ecosystem real structure so the advisory is armed.
    let (out, code) = run_strict("orphan_root_charter", &[]);

    assert_eq!(
        code, 0,
        "an orphan advisory must not fail the check:\n{out}"
    );
    // The disconnected inner package IS still surfaced.
    assert!(
        out.contains("annotation_on_orphan") && out.contains("ghost"),
        "the disconnected inner crate must still be flagged:\n{out}"
    );
    // The scan-root (charter) package is NOT an orphan finding, despite being edgeless.
    assert!(
        !out.contains("'charter'"),
        "a package whose directory IS the scan root is a charter, never an orphan:\n{out}"
    );
}

#[test]
fn orphan_advisory_serializes_as_a_located_warning_in_json() {
    // The structured surface: the advisory rides in `warnings` with its stable code and the
    // package DIRECTORY as `path`; being package-level, it omits `line`/`language` (absent,
    // not null — the schema's key-presence convention). `passed` stays true.
    let (json, code) = run_strict("orphan_annotation", &["--format", "json"]);
    assert_eq!(code, 0, "JSON path stays non-fatal:\n{json}");

    let doc: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(doc["passed"], serde_json::json!(true));
    let warnings = doc["warnings"].as_array().expect("warnings array");
    let orphan = warnings
        .iter()
        .find(|w| w["code"] == serde_json::json!("annotation_on_orphan"))
        .expect("an annotation_on_orphan warning is present");
    assert_eq!(orphan["path"], serde_json::json!("packages/ghost"));
    assert!(
        orphan.get("line").is_none() && orphan.get("language").is_none(),
        "a package-level advisory omits line/language (absent, not null): {orphan}"
    );
}
