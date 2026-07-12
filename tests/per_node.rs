// Concern: end-to-end tests for the per-directory display cap (`--max-per-node`) — a directory over the cap renders N entries plus a single `[+N folders and F files, use --full to expand]` marker (exit 0, tree still shown), while `--full` expands everything, freezing the soft-truncation contract | Non-concern: unit-level logic | IO: (temp tree) -> asserted (stdout, code, json)

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Build a temp tree with `n_dirs` subdirectories (each holding one file) and
/// `n_files` files, all under `src/`.
fn temp_tree(tag: &str, n_dirs: usize, n_files: usize) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-pernode-{}-{tag}-{n}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let body = "# Concern: does f for the per-node fixture | Non-concern: real behavior (a test stub) | IO: none\n";
    for i in 0..n_files {
        std::fs::write(src.join(format!("f{i}.py")), body).unwrap();
    }
    for i in 0..n_dirs {
        let sub = src.join(format!("batch{i}"));
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("m.py"), body).unwrap();
    }
    dir
}

fn run_capture(dir: &Path, extra: &[&str]) -> (String, String, i32) {
    // Every assertion pins the cap via a CLI flag (`--max-per-node`/`--full`), which
    // outranks the built-in default of 50, so this in-process harness needs no env
    // manipulation (unsafe under edition 2024).
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
fn files_over_cap_render_single_marker_and_full_expands() {
    let dir = temp_tree("files", 0, 60);

    // Soft truncation: exit 0, the tree IS rendered, overflow folds into one marker
    // naming its own escape hatch.
    let (out, _err, code) = run_capture(&dir, &["--max-per-node", "5"]);
    assert_eq!(code, 0, "per-node cap never aborts");
    assert!(
        out.contains("[+55 files, use --full to expand]"),
        "one marker reports the 55 elided files:\n{out}"
    );

    // --full expands every file: no marker, the highest-index file is present.
    let (out2, _err2, code2) = run_capture(&dir, &["--full"]);
    assert_eq!(code2, 0);
    assert!(!out2.contains("[+"), "--full shows no marker:\n{out2}");
    assert!(out2.contains("f59.py"), "--full lists every file:\n{out2}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn marker_trips_only_when_count_exceeds_cap() {
    // Exactly-at-cap shows no marker; one past shows `[+1 files`.
    let dir = temp_tree("boundary", 0, 6);

    let (out_at, _err, _code) = run_capture(&dir, &["--max-per-node", "6"]);
    assert!(
        !out_at.contains("[+"),
        "exactly at the cap: no marker:\n{out_at}"
    );

    let (out_over, _err, _code) = run_capture(&dir, &["--max-per-node", "5"]);
    assert!(
        out_over.contains("[+1 files, use --full to expand]"),
        "one past the cap elides exactly one:\n{out_over}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn combined_marker_folds_dirs_and_files_into_one_row() {
    // A directory overflowing on BOTH axes gets ONE row carrying both counts.
    let dir = temp_tree("combo", 10, 60);

    let (out, _err, code) = run_capture(&dir, &["--max-per-node", "5"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("[+5 folders and 55 files, use --full to expand]"),
        "single combined marker with both counts:\n{out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_exposes_distinct_elision_counts() {
    // JSON keeps the breakdown as two structured integers (unlike the folded text
    // marker), and the visible arrays are truncated to the cap.
    let dir = temp_tree("json", 10, 60);

    let (out, _err, code) = run_capture(&dir, &["--format", "json", "--max-per-node", "5"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let src = &value["roots"][0]["dirs"][0];
    assert_eq!(src["name"], "src");
    assert_eq!(src["elided_dirs"], 5, "10 dirs, cap 5 -> 5 elided");
    assert_eq!(src["elided_files"], 55, "60 files, cap 5 -> 55 elided");
    assert_eq!(
        src["dirs"].as_array().unwrap().len(),
        5,
        "dirs truncated to 5"
    );
    assert_eq!(
        src["files"].as_array().unwrap().len(),
        5,
        "files truncated to 5"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn default_and_full_omit_elision_fields_in_json() {
    // Omitted-when-zero: a small tree under the default cap carries neither field,
    // keeping default JSON byte-identical for existing consumers.
    let dir = temp_tree("small", 2, 3);

    let (out, _err, _code) = run_capture(&dir, &["--format", "json"]);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    let src = &value["roots"][0]["dirs"][0];
    assert!(
        src.get("elided_dirs").is_none(),
        "no elision field when unexceeded"
    );
    assert!(
        src.get("elided_files").is_none(),
        "no elision field when unexceeded"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
