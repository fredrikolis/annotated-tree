// Concern: freezes what a -L capped run visits, counts, lists and graphs, and that --strict-check ignores the cap | Non-concern: the depth arithmetic | IO: (fixtures) -> asserted stdout

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

const ANNOTATION: &str =
    "// Concern: a depth-fixture file | Non-concern: real behavior (a test stub) | IO: none\n";

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-depth-{}-{tag}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir fixture");
    dir
}

fn run(dir: &Path, extra: &[&str]) -> (String, String, i32) {
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(&argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (
        String::from_utf8(out).expect("utf8"),
        String::from_utf8(err).expect("utf8"),
        code,
    )
}

/// Issue #15: the cap counts what the CAPPED walk visits, so 5 doesn't abort a `-L 1`
/// two-row render of 41 files.
#[test]
fn max_files_counts_only_what_the_capped_walk_visits() {
    let dir = temp_dir("maxfiles");
    let deep = dir.join("deep/a/b/c");
    std::fs::create_dir_all(&deep).expect("mkdir deep");
    for i in 0..40 {
        std::fs::write(deep.join(format!("f{i}.rs")), ANNOTATION).expect("write deep file");
    }
    std::fs::write(dir.join("top.rs"), ANNOTATION).expect("write top file");

    let (out, err, code) = run(&dir, &["-L", "1", "--max-files", "5"]);
    assert_eq!(
        code, 0,
        "a -L 1 render of 2 rows must not trip a 5-file cap on files it never shows:\n{err}"
    );
    assert!(
        out.contains("deep/"),
        "the capped render lists deep/:\n{out}"
    );
    assert!(out.contains("top.rs"), "and the top-level file:\n{out}");
    assert!(
        !out.contains("f0.rs"),
        "nothing below the cutoff is rendered:\n{out}"
    );

    // The cap still trips when the CAPPED walk exceeds it — bounded by -L, not disabled.
    let (_out, _err, code) = run(&dir, &["--max-files", "5"]);
    assert_eq!(code, 3, "an uncapped walk of 41 files still aborts at 5");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A directory earns its row by being VISITED, not by holding a listable file below, at every
/// depth. `emptyish/` holds only an unmapped extension, yet is listed.
#[test]
fn a_directory_with_nothing_listable_below_is_listed_at_every_depth() {
    let dir = temp_dir("emptyish");
    std::fs::create_dir_all(dir.join("deep/a/b/c")).expect("mkdir deep");
    std::fs::create_dir_all(dir.join("emptyish")).expect("mkdir emptyish");
    std::fs::write(dir.join("deep/a/b/c/f1.rs"), ANNOTATION).expect("write deep file");
    std::fs::write(dir.join("emptyish/notes.txt"), "just notes\n").expect("write notes");

    let (capped, _err, code) = run(&dir, &["-L", "1"]);
    assert_eq!(code, 0, "capped run exits clean");
    assert!(
        capped.contains("deep/") && capped.contains("emptyish/"),
        "at the cutoff, both directories are listed — the one with a listable file below \
         it and the one without:\n{capped}"
    );

    let (full, _err, code) = run(&dir, &[]);
    assert_eq!(code, 0, "uncapped run exits clean");
    assert!(
        full.contains("emptyish/"),
        "and the SAME rule applies with no cap at all — an inconsistency between depths \
         would be worse than either rule:\n{full}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Cuts the graph one level BELOW the deepest row, since a manifest lives inside its package.
#[test]
fn a_displayed_row_keeps_its_deps_while_a_package_below_the_cutoff_is_dropped() {
    let dir = temp_dir("graph");
    let manifest = |path: &std::path::Path, name: &str, deps: &str| {
        std::fs::create_dir_all(path.join("src")).expect("mkdir pkg");
        std::fs::write(
            path.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]{deps}"),
        )
        .expect("write manifest");
        std::fs::write(path.join("src/lib.rs"), ANNOTATION).expect("write lib.rs");
    };
    manifest(
        &dir.join("a"),
        "a",
        "\nb = { path = \"../b\" }\nc = { path = \"../nested/c\" }\n",
    );
    manifest(&dir.join("b"), "b", "\n");
    manifest(&dir.join("nested/c"), "c", "\n");

    // -L 1: both manifests read one level deeper; the edge draws both ways.
    let (shallow, _err, code) = run(&dir, &["-L", "1"]);
    assert_eq!(code, 0, "-L 1 exits clean");
    assert!(
        shallow.contains("used by: [a]"),
        "a row the map DISPLAYS states its own dependency facts:\n{shallow}"
    );
    assert!(
        shallow.contains("<- depends on [b"),
        "and the forward edge between two visible rows is drawn:\n{shallow}"
    );
    // `c` is two levels past the cutoff, never read, so its dependency resolves to nothing.
    assert!(
        shallow.contains("c (unresolved)"),
        "a package below the cutoff contributes no resolved edge:\n{shallow}"
    );

    // -L 2: `nested/c/` becomes a row, so its manifest is read and resolves.
    let (deep, _err, code) = run(&dir, &["-L", "2"]);
    assert_eq!(code, 0, "-L 2 exits clean");
    assert!(
        !deep.contains("c (unresolved)") && deep.contains("<- depends on [b, c]"),
        "once c/ is displayed, the edge to it resolves:\n{deep}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `--strict-check` gates the whole tree, not a view — a file five levels down still
/// violates under `-L 1`. Pins the cap out of the check path.
#[test]
fn strict_check_is_not_depth_capped() {
    let dir = temp_dir("strict");
    let deep = dir.join("a/b/c/d");
    std::fs::create_dir_all(&deep).expect("mkdir deep");
    std::fs::write(deep.join("unannotated.rs"), "fn f() {}\n").expect("write deep file");

    let (out, _err, code) = run(&dir, &["--strict-check", "--no-guide", "-L", "1"]);
    assert_eq!(
        code, 1,
        "a violation below the -L cutoff still fails the gate:\n{out}"
    );
    assert!(
        out.contains("unannotated.rs"),
        "and is still named in the report:\n{out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `-L 0` means uncapped, agreeing with `[limits] max_depth = 0` — not floored to a `-L 1`
/// view that would hide `sub/`'s own contents.
#[test]
fn depth_zero_is_uncapped() {
    let dir = temp_dir("zero");
    std::fs::create_dir_all(dir.join("sub")).expect("mkdir sub");
    std::fs::write(dir.join("sub/nested.rs"), ANNOTATION).expect("write nested");
    std::fs::write(dir.join("top.rs"), ANNOTATION).expect("write top");

    let (zero, _err, zcode) = run(&dir, &["-L", "0"]);
    let (uncapped, _err, ucode) = run(&dir, &[]);
    assert_eq!((zcode, ucode), (0, 0), "both exit clean");
    assert_eq!(zero, uncapped, "-L 0 is the same view as no -L at all");
    assert!(
        zero.contains("sub/") && zero.contains("nested.rs"),
        "and nested.rs, below the old floor, is visited:\n{zero}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
