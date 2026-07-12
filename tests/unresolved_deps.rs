// Concern: end-to-end test that a workspace/path dependency pointing at a package absent from the scanned tree renders with an `(unresolved)` marker instead of masquerading as a normal internal edge | Non-concern: unit-level logic | IO: (temp tree) -> asserted stdout

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-test-{}-{tag}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("pkg/src")).unwrap();
    dir
}

fn run_stdout(dir: &Path) -> String {
    let argv = [
        "annotated-tree".to_string(),
        dir.to_string_lossy().into_owned(),
    ];
    let cli = Cli::parse_from(argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    String::from_utf8(out).unwrap()
}

#[test]
fn dangling_workspace_dep_is_marked_unresolved() {
    let dir = temp_dir("dangling");
    // `@acme/ghost` is declared as a workspace dep but no package by that name
    // exists in the tree — a dangling internal edge.
    std::fs::write(
        dir.join("pkg/package.json"),
        r#"{"name": "@acme/app", "dependencies": {"@acme/ghost": "workspace:*"}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("pkg/src/index.ts"), "export const x = 1;\n").unwrap();

    let out = run_stdout(&dir);
    assert!(
        out.contains("@acme/ghost (unresolved)"),
        "a dangling workspace dep must be shown as unresolved:\n{out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
