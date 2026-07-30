// Concern: end-to-end test that a binary built without the mcp feature keeps the same CLI surface and fails --mcp with a rebuild message | Non-concern: unit-level logic | IO: (argv) -> asserted (stdout, stderr, code)

// The e2e case below is compiled out under an `mcp`-feature build (it is gated on the
// ABSENCE of the feature), so the helpers are dead code there — gate them to match,
// keeping `--features mcp -D warnings` clean.
#[cfg(not(feature = "mcp"))]
use std::path::PathBuf;
#[cfg(not(feature = "mcp"))]
use std::process::Command;

#[cfg(not(feature = "mcp"))]
fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sample")
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
