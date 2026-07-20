// Concern: end-to-end tests for the file-visibility filters — `-I/--ignore` exclusion globs, the tests/ hide/reveal toggle (`--include-tests`), and the `--include` positive glob selector; runs over a throwaway temp tree so no ancestor config leaks in | Non-concern: rendering glyphs | IO: (temp tree, flags) -> asserted stdout

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
    let (code, out) = run_raw(dir, extra);
    assert_eq!(code, 0, "run must succeed");
    out
}

/// Like [`run`] but returns the exit code alongside stdout, for cases (a passing
/// `--strict-check`) where the code itself is under test.
fn run_raw(dir: &Path, extra: &[&str]) -> (i32, String) {
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(&argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (code, String::from_utf8(out).unwrap())
}

/// A temp tree with a recognized `src/main.rs`, an UNrecognized-extension file that carries a
/// marker-agnostic annotation (`deploy.zsh`, a `#`-comment shell script the config has no `.zsh`
/// language for), and a bare un-annotated `data.bin`. The three exercise `--include` selection.
fn temp_tree_mixed(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-include-{}-{tag}-{n}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "// Concern: the entry point fixture | Non-concern: real behavior (a test stub) | IO: none\nfn main() {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("deploy.zsh"),
        "#!/bin/zsh\n# Concern: an unrecognized-extension file the selector opts in | Non-concern: real behavior (a test stub) | IO: (artifact) -> none\necho hi\n",
    )
    .unwrap();
    std::fs::write(dir.join("data.bin"), "raw bytes, no annotation\n").unwrap();
    dir
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

#[test]
fn include_glob_adds_unrecognized_files_with_marker_agnostic_annotation() {
    let dir = temp_tree_mixed("glob");

    // Baseline: only the recognized `.rs` file shows; the `.zsh`/`.bin` are invisible.
    let base = run(&dir, &[]);
    assert!(base.contains("main.rs"), "baseline lists main.rs:\n{base}");
    assert!(
        !base.contains("deploy.zsh"),
        "an unrecognized extension is hidden by default:\n{base}"
    );

    // `--include '*.zsh'` ADDS the shell script (recognized files stay), and its annotation is
    // read marker-agnostically even though the config has no `.zsh` language.
    let included = run(&dir, &["--include", "*.zsh"]);
    assert!(
        included.contains("main.rs"),
        "--include is additive — recognized files remain:\n{included}"
    );
    assert!(
        included.contains("deploy.zsh"),
        "--include '*.zsh' surfaces the unrecognized file:\n{included}"
    );
    assert!(
        included.contains("Concern: an unrecognized-extension file the selector opts in"),
        "the marker-agnostic annotation is shown:\n{included}"
    );
    // A file NOT matched by the selector stays hidden.
    assert!(
        !included.contains("data.bin"),
        "--include '*.zsh' must not pull in the unmatched data.bin:\n{included}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn strict_check_stays_recognized_only_under_include() {
    let dir = temp_tree_mixed("strict");
    // `--include` governs the TREE view alone: `--strict-check` still lints recognized languages
    // only, so it examines just `main.rs` and never the opted-in `.zsh`/`.bin` (whose comment
    // grammar it could not validate). The tree's one `.rs` is annotated, so the gate passes.
    let (code, out) = run_raw(&dir, &["--strict-check", "--include", "*"]);
    assert_eq!(code, 0, "the one recognized file is annotated:\n{out}");
    assert!(
        out.contains("All 1 files passed"),
        "strict-check examined only the recognized file, not the opted-in ones:\n{out}"
    );
    assert!(
        !out.contains("deploy.zsh") && !out.contains("data.bin"),
        "strict-check must not reach --include files:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
