// Concern: freezes the tool's output across the text, JSON, Markdown, strict-check and guide surfaces | Non-concern: what the sample tree contains | IO: (sample tree, fixtures) -> pass/fail

use std::path::{Path, PathBuf};

use annotated_tree::Cli;
use clap::Parser;

/// A fresh copy of `sample/`, walked from a temp dir instead of in the repo — `find_repo_config`
/// walks up looking for `.annotated-tree.toml`, so running in place risks an outside config
/// reaching these goldens.
fn sample_copy() -> tempfile::TempDir {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sample");
    let dir = tempfile::tempdir().expect("mkdtemp");
    copy_tree(&src, dir.path());
    dir
}

fn copy_tree(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).expect("read_dir sample") {
        let entry = entry.expect("dir entry");
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file_type").is_dir() {
            std::fs::create_dir(&to).expect("mkdir");
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).expect("copy fixture file");
        }
    }
}

fn run(extra: &[&str]) -> (String, i32) {
    let dir = sample_copy();
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.path().to_string_lossy().into_owned());

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

/// A single-delta view: `build/` is `.gitignore`d, so `generated.py` is the ONLY file
/// `--no-gitignore` adds. Freeze that, not the whole fixture again.
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

/// Same single-delta shape as `--no-gitignore` above: `.ci/`'s contents are what `--hidden`
/// adds. Byte-identical goldens with `.ci/` present prove the flag defaults OFF.
#[test]
fn hidden_reveals_the_dot_directory() {
    let (default_out, dcode) = run(&[]);
    let (shown_out, scode) = run(&["--hidden"]);
    assert_eq!(dcode, 0, "default exit code");
    assert_eq!(scode, 0, "--hidden exit code");
    assert!(
        !default_out.contains("notes.md"),
        "the default view skips the dot-directory .ci/:\n{default_out}"
    );
    assert!(
        shown_out.contains("notes.md") && shown_out.contains("deploy"),
        "--hidden reveals .ci/ whole — the doc, and the extensionless script its shebang \
         resolves:\n{shown_out}"
    );
}

/// `--ascii` is a pure glyph swap of the default view — assert ascii equals default with
/// `render/text.rs`'s box-glyph map.
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

#[test]
fn strict_check_reports_offenders() {
    // `--no-guide` pins this golden to the report; the guide itself is covered elsewhere.
    assert_golden("strict_check.txt", &["--strict-check", "--no-guide"], 1);
}

/// Accepts a single FILE, not only a directory — the unit an agent lints after writing one.
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

    // An unmapped extension is a precondition error: `run()` returns `Err`, never a pass or lint failure.
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

/// The machine counterpart to the TEXT report. Freeze the shape — `passed`/counts, and
/// each violation carrying the fields an agent acts on.
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
    assert_eq!(doc["files_checked"], serde_json::json!(25), "files checked");
    assert_eq!(
        doc["annotated_count"],
        serde_json::json!(24),
        "annotated progress numerator"
    );

    let violations = doc["violations"].as_array().expect("violations array");
    assert_eq!(violations.len(), 1, "one record per annotation gap");
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
    // The machine-coded delta an agent branches on; `length`/`max` are absent when inapplicable.
    assert_eq!(
        py["defect"]["missing"],
        serde_json::json!(["concern", "non_concern", "io"]),
        "defect names the missing fields, not prose"
    );
    assert!(
        py["defect"].get("length").is_none() && py["defect"].get("max").is_none(),
        "empty/absent defect fields are omitted (absent-key convention)"
    );
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
    // `<…>` slots mark what an agent replaces, on a line whose FORM already conforms.
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

/// The envelope is frozen by `json_schema_is_versioned`; here, a node's WHAT — its annotation,
/// and nesting directories/files rather than flattening them. Descending `find_file` IS that check.
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

/// Descending into `dirs` exercises that the JSON nests, rather than flattens.
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

/// Schema number + top-level shape: the stability guarantee external consumers depend on.
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

/// Markdown is human-facing, not a machine contract, so it is deliberately NOT byte-frozen —
/// a light structural check (each package as a heading) captures what matters.
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

/// A FAILING `--strict-check` prints the guide inline by default; `--no-guide` suppresses it,
/// and a PASSING run never shows it. Pins the enforced template, from `strict::EXPECTED`.
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
    assert!(
        out.contains(
            "Concern: {what it does} | Non-concern: {what it isn't} | IO: (in) -> out  OR  none"
        ),
        "the guide renders the enforced template"
    );
    // Past `<!-- more -->`: the failure surface gets the FULL guide, not help's essence.
    assert!(
        out.contains("HOW TO FIND THE NON-CONCERN"),
        "a failing check prints the full guide, got: {out}"
    );

    let (out, _err, code) = run_strict(&["--no-guide"]);
    assert_eq!(code, 1, "--no-guide does not change the verdict");
    assert!(
        !out.contains("ANNOTATION GUIDE"),
        "--no-guide suppresses the guide, got: {out}"
    );

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

/// Reads the guide without provoking a violation; the same text also reaches `--help`.
#[test]
fn annotation_guide_flag_prints_the_guide_alone() {
    let (out, code) = run(&["--annotation-guide"]);
    assert_eq!(
        code, 0,
        "--annotation-guide is an info flag: print and exit clean"
    );
    assert!(
        out.starts_with("ANNOTATION GUIDE"),
        "the guide is printed alone, with nothing before it, got: {out}"
    );
    assert!(
        !out.contains("EXIT CODES:") && !out.contains("Usage:"),
        "nothing but the guide goes to stdout, got: {out}"
    );

    let help = std::process::Command::new(env!("CARGO_BIN_EXE_annotated-tree"))
        .arg("--help")
        .output()
        .expect("run --help");
    let help = String::from_utf8(help.stdout).expect("utf8");
    assert!(
        help.contains("ANNOTATION GUIDE"),
        "--help still carries the guide, got: {help}"
    );
    assert!(
        !help.contains(out.trim_end()),
        "the flag prints more than --help's essence, got: {out}"
    );
    assert!(
        help.contains("max_annotation_length"),
        "--help states the bound, so an agent need not discover it by being rejected, got: {help}"
    );
    assert!(
        help.contains("--annotation-guide"),
        "a new flag lands with its --help text, got: {help}"
    );
}

/// Prints its guide alone and exits 0; the pointer it carries is what other surfaces promise.
#[test]
fn githook_guide_prints_the_guide_alone_and_names_the_tool() {
    let (out, code) = run(&["--githook-guide"]);
    assert_eq!(
        code, 0,
        "--githook-guide is an info flag: print and exit clean"
    );
    assert!(
        out.starts_with("GITHOOK GUIDE") && !out.contains("Usage:"),
        "the guide is printed alone, not appended to another surface, got: {out}"
    );
    // README and CONTRIBUTING route an agent here, so the pointer has to survive.
    assert!(
        out.contains("git-agent-verdict"),
        "the guide names where the commit-msg wiring lives, got: {out}"
    );
}
