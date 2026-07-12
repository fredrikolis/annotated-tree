// Concern: the runaway-scope cap resolves CLI > env > repo config, and a garbage env value fails fast; these paths only run for real through a spawned process (the env var and `.annotated-tree.toml` discovery are process/CWD state), so freeze them at the boundary scripts see | Non-concern: rendering | IO: (env, temp repo, argv) -> asserted (exit code, stderr)

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A temp dir under the system temp root (so no ancestor `.annotated-tree.toml` from
/// this repo leaks in) holding exactly `n_files` code files under `src/`.
fn temp_tree(tag: &str, n_files: usize) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-cfg-{}-{tag}-{n}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..n_files {
        std::fs::write(
            src.join(format!("f{i}.py")),
            "# Concern: does f for the config-precedence fixture | Non-concern: real behavior (a test stub) | IO: none\n",
        )
        .unwrap();
    }
    dir
}

/// Spawn the built binary over `dir` with an explicit, per-process env (each pair set;
/// each key in `clear` removed), so nothing leaks between tests. Returns (stderr, code).
fn run(dir: &Path, set: &[(&str, &str)], clear: &[&str], args: &[&str]) -> (String, i32) {
    let bin = env!("CARGO_BIN_EXE_annotated-tree");
    let mut cmd = Command::new(bin);
    for key in clear {
        cmd.env_remove(key);
    }
    for (key, value) in set {
        cmd.env(key, value);
    }
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg(dir);
    let output = cmd.output().expect("spawn binary");
    (
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn env_max_files_below_count_aborts_runaway_scope() {
    // 3 files under an env cap of 1: the env path (never exercised by the in-process
    // tests, which clear the var) trips the runaway-scope abort.
    let dir = temp_tree("env-low", 3);
    let (_stderr, code) = run(&dir, &[("ANNOTATED_TREE_MAX_FILES", "1")], &[], &[]);
    assert_eq!(
        code, 3,
        "env ANNOTATED_TREE_MAX_FILES below the count must abort (exit 3, RUNAWAY_SCOPE)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn env_max_files_garbage_fails_fast() {
    // A non-numeric env value is a hard config error (Fail-Fast), not silently ignored.
    let dir = temp_tree("env-garbage", 1);
    let (stderr, code) = run(
        &dir,
        &[("ANNOTATED_TREE_MAX_FILES", "notanumber")],
        &[],
        &[],
    );
    assert_ne!(code, 0, "a garbage env value must fail, not fall through");
    assert!(
        stderr.contains("error:"),
        "the failure surfaces as `error:` on stderr, got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn repo_config_limit_applies_and_cli_overrides_it() {
    let dir = temp_tree("cfg-limit", 3);
    std::fs::write(
        dir.join(".annotated-tree.toml"),
        "[limits]\nmax_files = 1\n",
    )
    .unwrap();

    // With the env cleared, the discovered repo `.annotated-tree.toml` cap (1) is below
    // the file count (3): the config limit takes effect.
    let (_e1, code_cfg) = run(&dir, &[], &["ANNOTATED_TREE_MAX_FILES"], &[]);
    assert_eq!(
        code_cfg, 3,
        "a repo .annotated-tree.toml limit must take effect (exit 3, RUNAWAY_SCOPE)"
    );

    // `--max-files` outranks the config file (precedence: CLI > env > config).
    let (_e2, code_cli) = run(
        &dir,
        &[],
        &["ANNOTATED_TREE_MAX_FILES"],
        &["--max-files", "100"],
    );
    assert_eq!(
        code_cli, 0,
        "CLI --max-files must override the repo config limit"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn env_max_files_overrides_repo_config() {
    // The MIDDLE rung of CLI > env > config: a repo `.annotated-tree.toml` cap of 1
    // would abort a 3-file tree, but env ANNOTATED_TREE_MAX_FILES=100 outranks the
    // config file, so the walk completes (exit 0). No CLI flag is given, so this
    // isolates env-beats-config specifically.
    let dir = temp_tree("env-over-cfg", 3);
    std::fs::write(
        dir.join(".annotated-tree.toml"),
        "[limits]\nmax_files = 1\n",
    )
    .unwrap();

    let (_stderr, code) = run(&dir, &[("ANNOTATED_TREE_MAX_FILES", "100")], &[], &[]);
    assert_eq!(
        code, 0,
        "env ANNOTATED_TREE_MAX_FILES must override the lower repo-config cap"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
