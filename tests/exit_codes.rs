// Exit-code contract: freezes `main.rs`'s error -> exit-2 + `error:`-to-stderr
// translation at the PROCESS boundary — an external contract scripts branch on, which
// the in-process `run()` tests (which get a `Result`, not an exit code) cannot pin.
// NOT concerned with which errors occur, only that any error maps to exit 2. | I/O: (argv) -> asserted (exit code, stderr)

use std::process::Command;

/// A run over a directory that does not exist has no valid roots, so the tool errors
/// out — and the binary must translate that into exit 2 with `error:` on stderr.
#[test]
fn nonexistent_directory_exits_2_with_error_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_annotated-tree");
    let output = Command::new(bin)
        .arg("/no/such/directory/annotated-tree-does-not-exist-xyz")
        .output()
        .expect("spawn binary");

    assert_eq!(
        output.status.code(),
        Some(2),
        "the binary must exit 2 on error (scripts branch on this)"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error:"),
        "stderr must carry the `error:` prefix, got: {stderr}"
    );
}
