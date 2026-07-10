// Golden: End-to-end tests that pin the tool's output over the `sample/` fixture.
// The fixture + these expected files ARE the behavioral spec; a diff here means a
// deliberate decision changed. NOT concerned with unit-level logic. | I/O: (sample tree) -> asserted output

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
    let engine_line = out
        .lines()
        .find(|l| l.contains("engine.py"))
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
    assert_golden("strict_check.txt", &["--strict-check"], 1);
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
        "Engine: Runs the core computation loop. Responsible for scheduling work units. \
         NOT concerned with transport or persistence. | I/O: (Job) -> Result",
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
