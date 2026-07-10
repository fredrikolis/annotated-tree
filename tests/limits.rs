// Limits: End-to-end tests for the runaway-scope safety valve — a tree over the
// cap aborts with exit 2 and EMPTY stdout (the JSON/agent-safety guarantee) while
// naming the override, and --no-limit completes. Freezes the external abort
// contract. | I/O: (temp tree) -> asserted (stdout, stderr, code)

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
        std::fs::write(
            src.join(format!("f{i}.py")),
            "# F: does f. | I/O: () -> None\n",
        )
        .unwrap();
    }
    dir
}

fn run_capture(dir: &Path, extra: &[&str]) -> (String, String, i32) {
    // The cap resolves CLI > env > config, and every assertion here pins the cap via a
    // CLI flag (`--max-files`/`--no-limit`), which outranks any ambient env value — so
    // this in-process harness needs no global `env::remove_var` (unsafe under edition
    // 2024). The env rung itself is covered out-of-process in `config_precedence.rs`.
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
fn over_cap_aborts_empty_stdout_and_no_limit_completes() {
    let dir = temp_tree("over", 5);

    // Over the cap: exit 2, EMPTY stdout (no partial tree / half-written JSON), and
    // stderr must name the limit and BOTH override routes.
    let (out, err, code) = run_capture(&dir, &["--max-files", "3"]);
    assert_eq!(code, 2, "over-cap run must exit 2:\nstderr={err}");
    assert!(
        out.is_empty(),
        "stdout MUST be empty on abort (agent/JSON safety), got:\n{out}"
    );
    assert!(
        err.contains("more than 3"),
        "stderr names the limit:\n{err}"
    );
    assert!(
        err.contains("--max-files"),
        "stderr names --max-files:\n{err}"
    );
    assert!(
        err.contains("--no-limit"),
        "stderr names --no-limit:\n{err}"
    );
    assert!(
        err.contains(&dir.to_string_lossy().into_owned()),
        "stderr names the offending path:\n{err}"
    );

    // --no-limit walks everything: exit 0, non-empty tree.
    let (out2, _err2, code2) = run_capture(&dir, &["--no-limit"]);
    assert_eq!(code2, 0, "--no-limit must complete");
    assert!(!out2.is_empty(), "--no-limit renders the tree");
    assert!(out2.contains("f0.py"), "the tree lists the files:\n{out2}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cap_trips_only_when_count_exceeds_limit() {
    // The valve trips when the count EXCEEDS the limit, not when it reaches it:
    // 5 files under a cap of 5 pass; a cap of 4 trips. Isolates the boundary the
    // integration test above (3 vs 5) does not pin exactly.
    let dir = temp_tree("boundary", 5);

    let (_out, _err, code_at) = run_capture(&dir, &["--max-files", "5"]);
    assert_eq!(code_at, 0, "exactly at the cap must pass");

    let (out_over, _err, code_over) = run_capture(&dir, &["--max-files", "4"]);
    assert_eq!(code_over, 2, "one past the cap must abort");
    assert!(out_over.is_empty(), "abort keeps stdout empty");

    let _ = std::fs::remove_dir_all(&dir);
}
