// Age column (e2e): freezes that `--age` is actually wired into the render — every
// file row carries a relative-age token — over a tempdir of real files. Asserts the
// column's SHAPE (a `… ago)` suffix), never its nondeterministic value; the pure
// seconds -> bucket logic is unit-tested in `src/util.rs`. | I/O: (temp tree, --age) -> asserted stdout

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A temp dir under the system temp root (so no ancestor `.annotated-tree.toml` from
/// this repo leaks in) holding a `src/` for freshly written code files.
fn temp_tree() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-age-{}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

#[test]
fn age_flag_puts_a_relative_age_token_on_every_file_line() {
    let dir = temp_tree();
    std::fs::write(dir.join("src/a.py"), "# A: does a. | I/O: () -> None\n").unwrap();
    std::fs::write(dir.join("src/b.py"), "# B: does b. | I/O: () -> None\n").unwrap();

    let cli = Cli::parse_from(["annotated-tree", "--age", &dir.to_string_lossy()]);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    assert_eq!(code, 0, "a normal run over a valid tree exits 0");
    let out = String::from_utf8(out).unwrap();

    // Every `.py` leaf must carry the age suffix the text renderer appends (`  (… ago)`);
    // freeze that the column is present in SHAPE, never the exact (real-time) value.
    let file_lines: Vec<&str> = out.lines().filter(|l| l.contains(".py")).collect();
    assert_eq!(file_lines.len(), 2, "both files should be listed:\n{out}");
    for line in &file_lines {
        assert!(
            line.contains(" ago)"),
            "each file line must carry a `(… ago)` relative-age token:\n{line}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
