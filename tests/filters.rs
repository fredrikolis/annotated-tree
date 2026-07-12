// Concern: end-to-end tests for the two file-visibility filters — `-I/--ignore` exclusion globs and the tests/ hide/reveal toggle (`--include-tests`); runs over a throwaway temp tree so no ancestor config leaks in | Non-concern: rendering glyphs | IO: (temp tree, flags) -> asserted stdout

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A temp tree under the system temp root (so no ancestor `.annotated-tree.toml` from
/// this repo leaks in) with `keep.rs`, `skip.rs`, and a `tests/checks.rs`.
fn temp_tree(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-filters-{}-{tag}-{n}", std::process::id()));
    let src = dir.join("src");
    let tests = dir.join("tests");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::write(src.join("keep.rs"), "// Concern: a file that stays for the filter fixture | Non-concern: real behavior (a test stub) | IO: none\n").unwrap();
    std::fs::write(src.join("skip.rs"), "// Concern: a file excluded by the filter fixture | Non-concern: real behavior (a test stub) | IO: none\n").unwrap();
    std::fs::write(
        tests.join("checks.rs"),
        "// Concern: a test file for the tests-dir toggle fixture | Non-concern: real behavior (a test stub) | IO: none\n",
    )
    .unwrap();
    dir
}

fn run(dir: &Path, extra: &[&str]) -> String {
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(&argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    assert_eq!(code, 0, "run must succeed");
    String::from_utf8(out).unwrap()
}

#[test]
fn ignore_glob_excludes_the_matched_file() {
    let dir = temp_tree("ignore");
    // Baseline: both files show.
    let base = run(&dir, &[]);
    assert!(base.contains("keep.rs"), "baseline lists keep.rs:\n{base}");
    assert!(base.contains("skip.rs"), "baseline lists skip.rs:\n{base}");

    // `-I 'skip.rs'` drops exactly the matched file, leaving the sibling.
    let filtered = run(&dir, &["-I", "skip.rs"]);
    assert!(
        !filtered.contains("skip.rs"),
        "-I 'skip.rs' must exclude skip.rs:\n{filtered}"
    );
    assert!(
        filtered.contains("keep.rs"),
        "-I 'skip.rs' must not affect the sibling keep.rs:\n{filtered}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tests_dir_hidden_by_default_and_revealed_by_flag() {
    let dir = temp_tree("tests-toggle");
    // Default: the tests/ directory is pruned wholesale.
    let default = run(&dir, &[]);
    assert!(
        !default.contains("checks.rs"),
        "tests/ is hidden by default:\n{default}"
    );

    // --include-tests reveals it.
    let shown = run(&dir, &["--include-tests"]);
    assert!(
        shown.contains("checks.rs"),
        "--include-tests reveals tests/checks.rs:\n{shown}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
