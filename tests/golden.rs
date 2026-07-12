// Concern: end-to-end tests that pin the tool's output over the `sample/` fixture — the fixture + expected files ARE the behavioral spec, so a diff means a deliberate decision changed | Non-concern: unit-level logic | IO: (sample tree) -> asserted output

use std::path::PathBuf;

use annotated_tree::Cli;
use clap::Parser;

/// Run the tool with `args` (after the program name) against the in-repo `sample/`
/// tree and return (stdout, exit_code). An absolute sample path keeps output
/// independent of the test's working directory.
fn run(extra: &[&str]) -> (String, i32) {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sample");
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(sample.to_string_lossy().into_owned());

    let cli = Cli::parse_from(&argv);
    let mut buf: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut buf, &mut err).expect("run failed");
    (String::from_utf8(buf).expect("utf8"), code)
}

fn golden(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    std::fs::read_to_string(path).expect("read golden")
}

fn assert_golden(name: &str, args: &[&str], expected_code: i32) {
    let (got, code) = run(args);
    assert_eq!(code, expected_code, "exit code for {name}");
    assert_eq!(got, golden(name), "output drift for {name}");
}

#[test]
fn default_tree() {
    assert_golden("default.txt", &[], 0);
}

#[test]
fn depth_one() {
    assert_golden("depth1.txt", &["-L", "1"], 0);
}

/// `--no-gitignore` is a single-delta view over the default: the fixture's `build/`
/// is `.gitignore`d, so `build/generated.py` is the ONLY file it adds. Freeze exactly
/// that relationship (default hides it, `--no-gitignore` reveals it) rather than
/// re-freezing the whole fixture a second time.
#[test]
fn no_gitignore_reveals_gitignored_build_dir() {
    let (default_out, dcode) = run(&[]);
    let (shown_out, scode) = run(&["--no-gitignore"]);
    assert_eq!(dcode, 0, "default exit code");
    assert_eq!(scode, 0, "--no-gitignore exit code");
    assert!(
        !default_out.contains("generated.py"),
        "default view respects .gitignore and hides build/generated.py:\n{default_out}"
    );
    assert!(
        shown_out.contains("generated.py"),
        "--no-gitignore reveals the gitignored build/generated.py:\n{shown_out}"
    );
}

/// `--ascii` is a pure glyph swap of the default view, so freeze exactly THAT
/// relationship rather than re-freezing the whole fixture a second time: run both
/// and assert the ascii output equals the default with the box glyphs substituted
/// (the mapping `render/text.rs` actually uses). No content is re-frozen.
#[test]
fn ascii_is_default_with_glyphs_substituted() {
    let (default_out, dcode) = run(&[]);
    let (ascii_out, acode) = run(&["--ascii"]);
    assert_eq!(dcode, 0, "default exit code");
    assert_eq!(acode, 0, "ascii exit code");
    let substituted = default_out
        .replace("├── ", "|-- ")
        .replace("└── ", "`-- ")
        .replace("│   ", "|   ");
    assert_eq!(
        ascii_out, substituted,
        "--ascii must be exactly the default view with box glyphs swapped for ASCII"
    );
}

/// `--tokens` adds a `[~N tok]` column; freeze the CONTRACT of that column (a file
/// shows `ceil(bytes/4)` of its own size, a package dir shows the aggregated subtree
/// total) over representative nodes, rather than re-pinning every annotation in the
/// fixture. The `ceil(bytes/4)` heuristic itself is unit-tested in `tokens.rs`.
#[test]
fn tokens_estimate_per_file_and_package() {
    let (out, code) = run(&["--tokens"]);
    assert_eq!(code, 0, "tokens exit code");
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sample");

    // A representative leaf: its column is ceil(bytes/4) of its own byte length.
    let engine = sample.join("packages/core/acme_core/engine.py");
    let engine_tok = std::fs::metadata(&engine).unwrap().len().div_ceil(4);
    // Anchor on the tree connector so this selects the `engine.py` FILE row and not a
    // sibling whose annotation text merely mentions `engine.py` (e.g. __init__.py naming
    // it as the owner of a concern it leaves alone) — a fixture reword can never silently
    // pick the wrong node.
    let engine_line = out
        .lines()
        .find(|l| l.contains("── engine.py"))
        .expect("engine.py is listed");
    assert!(
        engine_line.contains(&format!("[~{engine_tok} tok]")),
        "engine.py must show its own ceil(bytes/4)={engine_tok} estimate:\n{engine_line}"
    );

    // A package dir shows the aggregated subtree total: the sum of its files'
    // per-file estimates (core/ holds __init__.py + engine.py + utils.py).
    let core = sample.join("packages/core/acme_core");
    let core_tok: u64 = ["__init__.py", "engine.py", "utils.py"]
        .iter()
        .map(|f| std::fs::metadata(core.join(f)).unwrap().len().div_ceil(4))
        .sum();
    // Anchor on the tree connector so this selects the `core/` package dir line and
    // NOT `acme_core/` (whose name segment is `acme_core/`, so `── core/` can't match
    // it) — a fixture reorder can never silently pick the wrong node.
    let core_line = out
        .lines()
        .find(|l| l.contains("── core/"))
        .expect("core/ package dir is listed");
    assert!(
        core_line.contains(&format!("[~{core_tok} tok]")),
        "core/ must show the aggregated subtree total {core_tok}:\n{core_line}"
    );
}

#[test]
fn strict_check_reports_offenders() {
    // `--no-guide` keeps this golden pinned to the report itself; the guide that a bare
    // failing `--strict-check` prints is covered by `strict_failure_prints_guide_on_stdout`.
    assert_golden("strict_check.txt", &["--strict-check", "--no-guide"], 1);
}

/// `--strict-check` accepts a single FILE, not only a directory — the natural unit for an
/// agent linting the one file it just wrote (and a pre-commit hook over changed files). A
/// conforming file passes (exit 0, one file checked); a malformed one fails (exit 1); a file
/// whose extension maps to no language fails fast as a precondition (never a silent pass).
#[test]
fn strict_check_accepts_a_single_file() {
    let dir = std::env::temp_dir().join(format!("at-single-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir fixture");
    let good = dir.join("good.rs");
    let bad = dir.join("bad.rs");
    let opaque = dir.join("data.bin");
    std::fs::write(
        &good,
        "// Concern: sums a slice | Non-concern: parsing (caller owns it) | IO: (&[i32]) -> i32\nfn s() {}\n",
    )
    .expect("write good");
    std::fs::write(&bad, "// just some helpers\nfn h() {}\n").expect("write bad");
    std::fs::write(&opaque, "\x00\x01binary\n").expect("write opaque");

    let check = |path: &std::path::Path| {
        let cli = Cli::parse_from([
            "annotated-tree",
            "--strict-check",
            "--no-guide",
            &path.to_string_lossy(),
        ]);
        let mut buf: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let code = annotated_tree::run(&cli, &mut buf, &mut err).expect("run failed");
        (String::from_utf8(buf).expect("utf8"), code)
    };

    let (out, code) = check(&good);
    assert_eq!(code, 0, "a conforming single file passes: {out}");
    assert!(
        out.contains("All 1 files passed"),
        "reports exactly one file checked: {out}"
    );

    let (_out, code) = check(&bad);
    assert_eq!(code, 1, "a malformed single file fails strict-check");

    // A file whose extension maps to no language is a precondition error: in text mode
    // `run()` returns `Err` (the binary renders it as `error:` prose and exits PRECONDITION),
    // never a silent pass or a lint failure.
    let cli = Cli::parse_from([
        "annotated-tree",
        "--strict-check",
        "--no-guide",
        &opaque.to_string_lossy(),
    ]);
    let mut buf: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let result = annotated_tree::run(&cli, &mut buf, &mut err);
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        result.is_err(),
        "a non-lintable file fails fast as a precondition, not a pass or a lint failure"
    );
}

/// `--strict-check --format json` is the machine-consumable counterpart to the TEXT
/// report: the same verdict, structured. Freeze the shape (not the whole fixture) —
/// `passed`/counts at the envelope, and each violation carrying the fields an agent
/// acts on (category, real line, a conformant example). The exit code stays 1 on
/// failure, matching the text path.
#[test]
fn strict_check_json_emits_structured_violations() {
    let (got, code) = run(&["--strict-check", "--format", "json"]);
    assert_eq!(
        code, 1,
        "strict-check json exit code on a fixture with gaps"
    );
    let doc: serde_json::Value = serde_json::from_str(&got).expect("strict json parses");
    assert_eq!(doc["passed"], serde_json::json!(false), "gaps ⇒ not passed");
    assert_eq!(
        doc["error_count"],
        serde_json::json!(1),
        "one annotation gap (only the intentionally-malformed utils.py)"
    );
    assert_eq!(doc["files_checked"], serde_json::json!(20), "files checked");
    // The convergence numerator: 19 of the 20 files carry a conforming annotation; only
    // the one malformed file does not.
    assert_eq!(
        doc["annotated_count"],
        serde_json::json!(19),
        "annotated progress numerator"
    );

    let violations = doc["violations"].as_array().expect("violations array");
    assert_eq!(violations.len(), 1, "one record per annotation gap");
    // The Python util has a comment but not the three-field shape — a malformed_annotation
    // at line 1, echoing the offending content and a conformant example to copy.
    let py = violations
        .iter()
        .find(|v| v["path"] == serde_json::json!("packages/core/acme_core/utils.py"))
        .expect("python util surfaces as a violation");
    assert_eq!(py["category"], serde_json::json!("malformed_annotation"));
    assert_eq!(py["line"], serde_json::json!(1));
    assert_eq!(py["language"], serde_json::json!("python"));
    assert_eq!(
        py["found"],
        serde_json::json!("small helpers used across the engine")
    );
    assert!(
        py["example"]
            .as_str()
            .is_some_and(|e| e.contains("Concern:") && e.contains("IO:")),
        "the example is a conformant annotation line: {:?}",
        py["example"]
    );
    // The machine-coded delta an agent branches on: the comment carries NONE of the three
    // keyed fields, so all are missing; `vacuous` is absent when empty.
    assert_eq!(
        py["defect"]["missing"],
        serde_json::json!(["concern", "non_concern", "io"]),
        "defect names the missing fields, not prose"
    );
    assert!(
        py["defect"].get("vacuous").is_none(),
        "empty defect lists are omitted (absent-key convention)"
    );
    // The contract to converge on: the fill-in template plus which fields are enforced.
    assert!(
        py["expected"]["template"]
            .as_str()
            .is_some_and(|t| t.contains("Concern:")
                && t.contains("Non-concern:")
                && t.contains("IO:")),
        "expected.template carries the annotation shape: {:?}",
        py["expected"]["template"]
    );
    assert_eq!(
        py["expected"]["required"],
        serde_json::json!(["concern", "non_concern", "io"]),
        "all three fields are enforced"
    );
    assert!(
        py["expected"]["recommended"]
            .as_array()
            .is_some_and(|r| r.is_empty()),
        "no recommended-only fields (all three are required): {:?}",
        py["expected"]["recommended"]
    );
    // The file-tailored scaffold: reuses the file's own text as the Concern seed, opens
    // with the language marker, and leaves the judgment slots as `<…>` placeholders —
    // which themselves FAIL `annotation_vacuous`, so the stub can't be submitted unedited.
    let suggestion = py["suggestion"].as_str().expect("suggestion string");
    assert!(
        suggestion.starts_with("# Concern: small helpers used across the engine")
            && suggestion.contains("Non-concern: <concern owned elsewhere>")
            && suggestion.contains("(<inputs>) -> <outputs>"),
        "suggestion is file-tailored with placeholder judgment slots: {suggestion:?}"
    );
    assert!(
        doc["rule_violations"].is_array(),
        "rule_violations is always present (empty here)"
    );
}

/// Freeze the `--symbols` TEXT contract: how a file's top-level definitions are
/// indented beneath it. Runs only in a `symbols`-feature build (the extractor is
/// gated) and over a DEDICATED fixture of real `.py/.rs/.go/.ts` definitions — the
/// `sample/` files are annotation-only, so they exercise no extraction. Deterministic
/// over committed fixtures; the per-language unit tests cover extraction edge cases.
#[cfg(feature = "symbols")]
#[test]
fn symbols_outline_over_fixture() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/symbols");
    let cli = Cli::parse_from([
        "annotated-tree",
        "--symbols",
        "--ascii",
        &fixture.to_string_lossy(),
    ]);
    let mut buf: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut buf, &mut err).expect("run failed");
    assert_eq!(code, 0, "symbols exit code");
    let got = String::from_utf8(buf).expect("utf8");
    assert_eq!(got, golden("symbols.txt"), "symbols text drift");
}

/// The versioned envelope is frozen by `json_schema_is_versioned`; here we freeze
/// the WHAT of a node instead of re-freezing the whole fixture: the JSON renderer
/// surfaces a representative file's annotation and nests directories/files (rather
/// than flattening them). `find_file` descending through `dirs` IS the nesting check.
#[test]
fn json_surfaces_representative_node_annotation_and_nesting() {
    let (got, code) = run(&["--format", "json"]);
    assert_eq!(code, 0, "json exit code");
    let doc: serde_json::Value = serde_json::from_str(&got).expect("tool json parses");
    let root = &doc["roots"][0];
    let engine = find_file(root, "engine.py").expect("engine.py surfaces as a nested file node");
    assert_eq!(
        engine["annotation"]
            .as_str()
            .expect("engine.py annotation is a string"),
        "Concern: runs the core computation loop, scheduling work units | \
         Non-concern: transport or persistence (api and db own those) | IO: (Job) -> Result",
        "the file's first-line annotation survives the model build into JSON"
    );
}

/// Recursively search a DirNode's `dirs`/`files` for a file by name. Descending into
/// `dirs` exercises that the JSON nests directories and files rather than flattening.
fn find_file<'a>(node: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    if let Some(files) = node["files"].as_array() {
        if let Some(found) = files.iter().find(|f| f["name"] == serde_json::json!(name)) {
            return Some(found);
        }
    }
    if let Some(dirs) = node["dirs"].as_array() {
        for dir in dirs {
            if let Some(found) = find_file(dir, name) {
                return Some(found);
            }
        }
    }
    None
}

/// Freeze the versioned envelope: schema number + top-level shape. This is the
/// stability guarantee external consumers depend on, independent of the fixture.
#[test]
fn json_schema_is_versioned() {
    let (got, _) = run(&["--format", "json"]);
    let value: serde_json::Value = serde_json::from_str(&got).expect("tool json parses");
    assert_eq!(value["schema"], serde_json::json!(1), "schema version");
    let roots = value["roots"].as_array().expect("roots is an array");
    let first = roots.first().expect("at least one root");
    assert!(first["name"].is_string(), "node has string name");
    assert!(first["dirs"].is_array(), "node has dirs array");
    assert!(first["files"].is_array(), "node has files array");
}

/// Markdown is a human-facing presentation format (like the text view), NOT a
/// machine contract, so it is deliberately NOT byte-frozen — freezing its exact
/// formatting would freeze "how", not "what". A light structural check that each
/// package surfaces as a heading captures the contract that matters.
#[test]
fn md_format_surfaces_package_headings() {
    let (got, code) = run(&["--format", "md"]);
    assert_eq!(code, 0, "md exit code");
    for pkg in ["api", "core", "worker", "gateway", "ui", "web"] {
        assert!(
            got.lines()
                .any(|line| line.starts_with('#') && line.contains(pkg)),
            "expected a markdown heading for package `{pkg}`"
        );
    }
}

/// A FAILING `--strict-check` prints the annotation guide inline on stdout by default (the
/// teaching rides on the surface the agent already reads); `--no-guide` suppresses it; a
/// PASSING run never shows it. Assert the guide's load-bearing invariants — the enforced
/// template, the GOOD/FAILS contrast, and that a filler boundary is stated to FAIL — rather
/// than byte-freezing instructional prose. Uses a throwaway fixture with one unannotated file.
#[test]
fn strict_failure_prints_guide_on_stdout() {
    let dir = std::env::temp_dir().join(format!("at-guide-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir fixture");
    std::fs::write(dir.join("bad.py"), "x = 1\n").expect("write fixture");

    let run_strict = |args: &[&str]| {
        let mut argv = vec!["annotated-tree", "--strict-check"];
        argv.extend_from_slice(args);
        let path = dir.to_string_lossy().into_owned();
        argv.push(&path);
        let cli = Cli::parse_from(&argv);
        let mut buf: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let code = annotated_tree::run(&cli, &mut buf, &mut err).expect("run failed");
        (
            String::from_utf8(buf).expect("utf8"),
            String::from_utf8(err).expect("utf8"),
            code,
        )
    };

    let (out, _err, code) = run_strict(&[]);
    assert_eq!(code, 1, "an unannotated file fails strict-check");
    assert!(
        out.contains("ANNOTATION GUIDE"),
        "the failing report carries the guide inline, got: {out}"
    );
    // The template is derived from `strict::EXPECTED`; pin that the guide surfaces it.
    assert!(
        out.contains(
            "Concern: {what it does} | Non-concern: {what it isn't} | IO: (in) -> out  OR  none"
        ),
        "the guide renders the enforced template"
    );
    assert!(
        out.contains("GOOD") && out.contains("FAILS") && out.contains("annotation_vacuous"),
        "the guide shows the GOOD/FAILS contrast and names the vacuous failure category"
    );
    assert!(
        out.contains("HOW TO FIND THE NON-CONCERN"),
        "the full guide renders the post-`<!-- more -->` tail, not just the --help essence"
    );

    // `--no-guide` keeps the report clean for a caller that already knows the format.
    let (out, _err, code) = run_strict(&["--no-guide"]);
    assert_eq!(code, 1, "--no-guide does not change the verdict");
    assert!(
        !out.contains("ANNOTATION GUIDE"),
        "--no-guide suppresses the guide, got: {out}"
    );

    // A PASSING run never shows the guide (nothing to fix).
    std::fs::write(
        dir.join("bad.py"),
        "# Concern: exercises the strict gate | Non-concern: rendering (the golden suite owns that) | IO: (x) -> y\n",
    )
    .expect("rewrite fixture");
    let (out, _err, code) = run_strict(&[]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(code, 0, "annotated fixture passes");
    assert!(
        !out.contains("ANNOTATION GUIDE"),
        "a passing run never shows the guide, got: {out}"
    );
}
