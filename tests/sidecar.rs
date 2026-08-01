// Concern: end-to-end tests for the per-file `<name>.annotation` sidecar — what it lists, what it suppresses, and how a malformed or dangling one is reported | Non-concern: the resolution units (src/sidecar.rs) or the annotation grammar | IO: (temp fixtures) -> asserted (stdout, stderr, code)

use annotated_tree::Cli;
use clap::Parser;

/// A throwaway tree under the OS temp dir holding `files` (relative path -> content), and one
/// run over it. `name` keys the directory so parallel tests never collide. Returns stdout,
/// stderr (the channel the map's advisory notes use) and the exit code.
fn run(name: &str, args: &[&str], files: &[(&str, &str)]) -> (String, String, i32) {
    let dir = std::env::temp_dir().join(format!("at-sidecar-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir fixture");
    for (rel, body) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir parent");
        }
        std::fs::write(&path, body).expect("write fixture");
    }
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(args.iter().map(|s| (*s).to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (
        String::from_utf8(out).expect("utf8"),
        String::from_utf8(err).expect("utf8"),
        code,
    )
}

/// The three fixture files every case below shares: a CSV that cannot hold a comment and the
/// sidecar carrying its contract, plus one ordinary annotated code file so the tree is not
/// degenerate.
const CONTRACT: &str =
    "Concern: raw measurements from the runs | Non-concern: interpretation | IO: none";

fn tree_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("trials.csv", "a,b\n1,2\n"),
        (
            "trials.csv.annotation",
            "Concern: raw measurements from the runs | Non-concern: interpretation | IO: none\n",
        ),
        (
            "main.rs",
            "// Concern: the entry point | Non-concern: parsing | IO: none\n",
        ),
    ]
}

#[test]
fn a_sidecar_lists_a_comment_less_file_and_takes_no_row_of_its_own() {
    // The README's own showcase: a plain CSV renders WITH its contract. Three claims in one
    // run — the file is listed with no `--include` (writing the sidecar IS the opt-in), the
    // sidecar itself is not a row, and the report states the criterion by which it is missing
    // (TREE2: an excluded path falls under a rule a reader can apply to any path).
    let (out, err, code) = run("render", &[], &tree_files());
    assert_eq!(code, 0, "rendering a tree exits 0:\n{out}");
    let row = out
        .lines()
        .find(|l| l.contains("trials.csv"))
        .expect("the CSV is listed even though its extension maps to no language");
    assert!(
        row.contains(CONTRACT),
        "the sidecar's line is shown on the file's own row: {row}"
    );
    assert!(
        !out.contains("trials.csv.annotation"),
        "the sidecar never takes a row of its own:\n{out}"
    );
    assert!(
        err.contains("`.annotation` file is never listed as its own row"),
        "the report states the exclusion criterion it applied:\n{err}"
    );
}

#[test]
fn the_row_that_took_the_contract_says_so_in_json() {
    // The structured counterpart of the text note: a JSON consumer reads the same exclusion
    // off the data. `sidecar` is omitted on every other row, so an in-file annotation
    // serializes byte-identically.
    let (out, _err, code) = run("json", &["--format", "json"], &tree_files());
    assert_eq!(code, 0);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("map json parses");
    let files = doc["roots"][0]["files"].as_array().expect("files array");
    let csv = files
        .iter()
        .find(|f| f["name"] == serde_json::json!("trials.csv"))
        .expect("the CSV is a node");
    assert_eq!(csv["annotation"], serde_json::json!(CONTRACT));
    assert_eq!(csv["sidecar"], serde_json::json!(true));
    let rs = files
        .iter()
        .find(|f| f["name"] == serde_json::json!("main.rs"))
        .expect("the code file is a node");
    assert!(
        rs.get("sidecar").is_none(),
        "an in-file annotation omits the key entirely: {rs}"
    );
    assert!(
        !out.contains("trials.csv.annotation"),
        "the sidecar is not a node:\n{out}"
    );
}

#[test]
fn a_file_that_can_hold_a_comment_never_takes_a_sidecar() {
    // CORE2 by construction: a sidecar is read ONLY for a file with no comment marker, so an
    // annotation's location is never ambiguous. `main.rs.annotation` is therefore not a
    // sidecar at all — it is an ordinary file (shown here only because `--include` asks for
    // it), and `main.rs` keeps its own first line.
    let mut files = tree_files();
    files.push((
        "main.rs.annotation",
        "Concern: NOT A SIDECAR | Non-concern: y | IO: none\n",
    ));
    let (out, _err, code) = run("not-a-sidecar", &["--include", "*"], &files);
    assert_eq!(code, 0);
    let row = out
        .lines()
        .find(|l| l.contains("main.rs ") || l.ends_with("main.rs"))
        .or_else(|| out.lines().find(|l| l.contains("main.rs  #")))
        .expect("main.rs is listed");
    assert!(
        row.contains("Concern: the entry point"),
        "the code file keeps its own first-line annotation: {row}"
    );
    assert!(
        out.contains("main.rs.annotation"),
        "and the `.annotation` beside it is an ordinary file, not a suppressed sidecar:\n{out}"
    );
}

#[test]
fn a_malformed_sidecar_fails_the_check_at_the_sidecar_path() {
    // Opting in means doing it right, exactly as for a folder charter: the body is held to
    // the ONE three-field grammar and reported at the file an author edits to fix it.
    let (out, _err, code) = run(
        "malformed",
        &["--strict-check", "--no-guide"],
        &[
            ("trials.csv", "a,b\n"),
            ("trials.csv.annotation", "just some numbers\n"),
            ("main.rs", "// Concern: a | Non-concern: b | IO: none\n"),
        ],
    );
    assert_eq!(code, 1, "a malformed sidecar is fatal:\n{out}");
    assert!(
        out.contains("trials.csv.annotation:1: annotation is malformed [sidecar]"),
        "located at the sidecar, under its own language label:\n{out}"
    );
    assert!(
        out.contains("suggestion: Concern: just some numbers"),
        "the marker-less stub reuses what the sidecar already said:\n{out}"
    );
    assert!(
        out.contains("2 files checked"),
        "the annotated file counts as checked, not skipped for having no language:\n{out}"
    );
}

#[test]
fn a_conforming_sidecar_counts_toward_coverage() {
    // The convergence numerator must not under-report: a CSV whose contract is written is
    // annotated, and the tree that shows it emits no coverage note.
    let (out, err, code) = run("coverage", &["--strict-check", "--no-guide"], &tree_files());
    assert_eq!(code, 0, "a conforming sidecar passes:\n{out}");
    assert!(out.contains("2 of 2 files annotated"), "{out}");
    assert!(err.is_empty(), "no advisory on a clean strict run: {err}");
}

#[test]
fn a_sidecar_with_no_target_is_reported_and_fails() {
    // A sidecar whose named file is absent annotates nothing. That is a dangling PATH, not a
    // claim about any Annotation's parts, so it rides its own `path: message` list — and it
    // fails loudly rather than sitting there being ignored. Nothing is deleted: the remedy is
    // named in the message and left to the author.
    let (out, _err, code) = run(
        "orphan",
        &["--strict-check", "--no-guide"],
        &[
            ("main.rs", "// Concern: a | Non-concern: b | IO: none\n"),
            (
                "ghost.csv.annotation",
                "Concern: a | Non-concern: b | IO: none\n",
            ),
        ],
    );
    assert_eq!(code, 1, "a dangling sidecar fails the check:\n{out}");
    assert!(
        out.contains("ghost.csv.annotation: annotates no file — 'ghost.csv' does not exist"),
        "reported as `path: message`:\n{out}"
    );
    assert!(
        out.contains("Found 1 orphan sidecar(s)"),
        "with its own count, kept out of the annotation error count:\n{out}"
    );
    assert!(
        out.contains("All 1 files passed"),
        "the annotation check itself is untouched by it:\n{out}"
    );
}

#[test]
fn the_orphan_finding_is_structured_and_located() {
    // An agent branches on the list and its located fields, never on the prose.
    let (out, _err, code) = run(
        "orphan-json",
        &["--strict-check", "--format", "json"],
        &[
            ("main.rs", "// Concern: a | Non-concern: b | IO: none\n"),
            (
                "data/ghost.csv.annotation",
                "Concern: a | Non-concern: b | IO: none\n",
            ),
            (
                "data/kept.rs",
                "// Concern: c | Non-concern: d | IO: none\n",
            ),
        ],
    );
    assert_eq!(code, 1);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("strict json parses");
    assert_eq!(doc["passed"], serde_json::json!(false));
    assert_eq!(
        doc["error_count"],
        serde_json::json!(0),
        "a dangling path is not an annotation error: {out}"
    );
    let orphans = doc["orphan_sidecars"].as_array().expect("orphan array");
    assert_eq!(orphans.len(), 1, "{out}");
    assert_eq!(
        orphans[0]["path"],
        serde_json::json!("data/ghost.csv.annotation")
    );
    assert_eq!(orphans[0]["target"], serde_json::json!("data/ghost.csv"));
}

#[test]
fn a_directory_charter_is_never_read_as_a_sidecar() {
    // The two scales share one metadata name, so the bare `.annotation` must resolve as the
    // DIRECTORY's charter and never as a sidecar for a file called "" — the one case where
    // the shared suffix could alias, and where a false orphan would otherwise be reported.
    let (out, err, code) = run(
        "charter",
        &[],
        &[
            (
                ".annotation",
                "Concern: the fixture root | Non-concern: anything real | IO: none\n",
            ),
            ("main.rs", "// Concern: a | Non-concern: b | IO: none\n"),
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        !err.contains("`.annotation` file is never listed"),
        "no sidecar row was suppressed, so the criterion note stays silent: {err}"
    );
    let (strict, _err, code) = run(
        "charter-strict",
        &["--strict-check", "--no-guide"],
        &[
            (
                ".annotation",
                "Concern: the fixture root | Non-concern: anything real | IO: none\n",
            ),
            ("main.rs", "// Concern: a | Non-concern: b | IO: none\n"),
        ],
    );
    assert_eq!(
        code, 0,
        "a directory charter is not a dangling sidecar:\n{strict}"
    );
}
