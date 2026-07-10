// Warnings: End-to-end tests for manifest parse-error reporting — a corrupt
// manifest warns to stderr and continues, `--ignore-parsing-errors` silences it,
// and a valid manifest without a package is not an error. | I/O: (temp tree) -> asserted (stdout, stderr, code)

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-test-{}-{tag}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn run_capture(dir: &Path, extra: &[&str]) -> (String, String, i32) {
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());

    let cli = Cli::parse_from(&argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        code,
    )
}

#[test]
fn corrupt_manifest_warns_and_continues() {
    let dir = temp_dir("corrupt");
    std::fs::write(dir.join("pyproject.toml"), "[project\nname = \"oops\n").unwrap();
    std::fs::write(dir.join("src/x.py"), "# X: does x. | I/O: () -> None\n").unwrap();

    let (out, err, code) = run_capture(&dir, &[]);
    assert_eq!(code, 0, "a corrupt manifest must not abort the run");
    assert!(out.contains("x.py"), "the tree still renders files:\n{out}");
    assert!(
        err.contains("warning:") && err.contains("pyproject.toml"),
        "stderr should name the unparseable manifest:\n{err}"
    );

    // --ignore-parsing-errors silences the warning; stdout is unchanged.
    let (out2, err2, code2) = run_capture(&dir, &["--ignore-parsing-errors"]);
    assert_eq!(code2, 0);
    assert_eq!(out, out2, "suppressing warnings must not change the tree");
    assert!(err2.is_empty(), "expected no warnings, got:\n{err2}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn valid_manifest_without_package_is_silent() {
    let dir = temp_dir("noproject");
    // Valid TOML, just no [project] table — not an error worth warning about.
    std::fs::write(
        dir.join("pyproject.toml"),
        "[tool.black]\nline-length = 88\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/y.py"), "# Y: does y. | I/O: () -> None\n").unwrap();

    let (_out, err, code) = run_capture(&dir, &[]);
    assert_eq!(code, 0);
    assert!(
        err.is_empty(),
        "a valid pyproject without [project] must not warn:\n{err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
