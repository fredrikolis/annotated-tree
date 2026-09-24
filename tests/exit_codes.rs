// Concern: end-to-end test that any error out of main.rs becomes exit 4 (PRECONDITION) with an `error:` line on stderr | Non-concern: which errors occur | IO: (argv) -> asserted (exit code, stderr)

use std::process::Command;

/// A missing root is a precondition failure: exit 4 (`PRECONDITION`), distinct from clap's exit 2.
#[test]
fn nonexistent_directory_exits_precondition_with_error_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_annotated-tree");
    let output = Command::new(bin).arg("/no/such/directory/annotated-tree-does-not-exist-xyz").output().expect("spawn binary");

    assert_eq!(output.status.code(), Some(4), "a missing root dir must exit 4 (PRECONDITION), which scripts branch on");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"), "stderr must carry the `error:` prefix, got: {stderr}");
}
