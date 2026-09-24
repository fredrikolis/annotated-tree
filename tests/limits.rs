// Concern: freezes the runaway-scope contract — exit 3, empty stdout, the override named, --no-limit completes | Non-concern: which files the walk counts | IO: (temp tree) -> asserted output

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Build a temp tree containing exactly `n_files` code files under `src/`.
fn temp_tree(tag: &str, n_files: usize) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-limit-{}-{tag}-{n}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..n_files {
        std::fs::write(src.join(format!("f{i}.py")), "# Concern: does f for the limits fixture | Non-concern: real behavior (a test stub) | IO: none\n").unwrap();
    }
    dir
}

fn run_capture(dir: &Path, extra: &[&str]) -> (String, String, i32) {
    // A CLI flag outranks any ambient env value, so no `env::remove_var` (unsafe on 2024) is needed.
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());

    let cli = Cli::parse_from(&argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap(), code)
}

#[test]
fn over_cap_aborts_empty_stdout_and_no_limit_completes() {
    let dir = temp_tree("over", 5);

    let (out, err, code) = run_capture(&dir, &["--max-files", "3"]);
    assert_eq!(code, 3, "over-cap run must exit 3 (RUNAWAY_SCOPE):\nstderr={err}");
    assert!(out.is_empty(), "stdout MUST be empty on abort (agent/JSON safety), got:\n{out}");
    assert!(err.contains("more than 3"), "stderr names the limit:\n{err}");
    assert!(err.contains("--max-files"), "stderr names --max-files:\n{err}");
    assert!(err.contains("--no-limit"), "stderr names --no-limit:\n{err}");
    assert!(err.contains(&dir.to_string_lossy().into_owned()), "stderr names the offending path:\n{err}");

    let (out2, _err2, code2) = run_capture(&dir, &["--no-limit"]);
    assert_eq!(code2, 0, "--no-limit must complete");
    assert!(!out2.is_empty(), "--no-limit renders the tree");
    assert!(out2.contains("f0.py"), "the tree lists the files:\n{out2}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cap_trips_only_when_count_exceeds_limit() {
    // Trips when the count EXCEEDS the limit, not when it reaches it — the boundary 3-vs-5 above misses.
    let dir = temp_tree("boundary", 5);

    let (_out, _err, code_at) = run_capture(&dir, &["--max-files", "5"]);
    assert_eq!(code_at, 0, "exactly at the cap must pass");

    let (out_over, _err, code_over) = run_capture(&dir, &["--max-files", "4"]);
    assert_eq!(code_over, 3, "one past the cap must abort (RUNAWAY_SCOPE)");
    assert!(out_over.is_empty(), "abort keeps stdout empty");

    let _ = std::fs::remove_dir_all(&dir);
}
