// Concern: a lean binary keeps an identical CLI surface — `--symbols` without the `symbols` feature still renders the tree AND notes the missing support on stderr; `--mcp` without the `mcp` feature exits nonzero with a rebuild message; spawns the binary cargo built for THIS run, so each assertion is gated to the build that actually lacks the feature | Non-concern: unit-level logic | IO: (argv) -> asserted (stdout, stderr, code)

// Both e2e cases below are compiled out under a full-feature build (each is gated on
// the ABSENCE of a feature), so the shared helpers are dead code when BOTH features
// are on — gate them to match, keeping `--features symbols,mcp -D warnings` clean.
#[cfg(any(not(feature = "symbols"), not(feature = "mcp")))]
use std::path::PathBuf;
#[cfg(any(not(feature = "symbols"), not(feature = "mcp")))]
use std::process::Command;

#[cfg(any(not(feature = "symbols"), not(feature = "mcp")))]
fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sample")
}

/// On a build WITHOUT the `symbols` feature, `--symbols` is inert-but-visible: the tree
/// still renders on stdout and a one-line "rebuild with --features symbols" note goes to
/// stderr. (Under `--features symbols` the flag works, so this contract does not apply
/// and the test is compiled out.)
#[cfg(not(feature = "symbols"))]
#[test]
fn symbols_on_lean_build_renders_tree_and_notes_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_annotated-tree");
    let output = Command::new(bin)
        .arg("--symbols")
        .arg(sample())
        .output()
        .expect("spawn binary");

    assert!(
        output.status.success(),
        "--symbols must stay non-fatal on a lean build (exit 0): {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("engine.py"),
        "the tree must still render on stdout:\n{stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--symbols ignored") && stderr.contains("--features symbols"),
        "stderr must carry the rebuild-with-symbols note:\n{stderr}"
    );
}

/// On a build WITHOUT the `mcp` feature, `--mcp` is a hard error: nonzero exit with a
/// "rebuild with --features mcp" message. (Under `--features mcp` it starts the server,
/// so this contract does not apply and the test is compiled out.)
#[cfg(not(feature = "mcp"))]
#[test]
fn mcp_on_lean_build_exits_nonzero_with_rebuild_message() {
    let bin = env!("CARGO_BIN_EXE_annotated-tree");
    let output = Command::new(bin)
        .arg("--mcp")
        .arg(sample())
        .output()
        .expect("spawn binary");

    assert!(
        !output.status.success(),
        "--mcp on a lean build must exit nonzero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--features mcp"),
        "stderr must carry the rebuild-with-mcp message:\n{stderr}"
    );
}
