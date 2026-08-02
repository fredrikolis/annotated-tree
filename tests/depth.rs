// Concern: end-to-end tests for the -L cap — what a capped walk visits, counts, lists and graphs, and that --strict-check ignores it | Non-concern: unit-level logic | IO: (fixtures) -> asserted stdout

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

/// The issue's own repro (#15): 41 code files, two of which a `-L 1` render can show. The
/// runaway-scope cap must count what the CAPPED walk visits, so a cap of 5 does not abort a
/// two-row render — the walk never reaches the 40 files below the cutoff.
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

    // The cap still trips when the CAPPED walk itself exceeds it — the guard is bounded by -L, not disabled by it.
    let (_out, _err, code) = run(&dir, &["--max-files", "5"]);
    assert_eq!(code, 3, "an uncapped walk of 41 files still aborts at 5");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A directory earns its row by being VISITED, not by holding a listable file somewhere
/// beneath it — and that rule holds at EVERY depth, not only at the cutoff. `notes.txt` maps
/// to no known language, so `emptyish/` has nothing listable below it at any depth; it is
/// listed all the same, in the capped view and the full one alike.
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

/// `-L LEVEL` cuts the input to the dependency-graph analyzer at the level BELOW the deepest
/// row, not at the row itself: a package's manifest lives inside the package, one level under
/// the row that names it, so a row that IS displayed states its own dependency facts. What the
/// cap still cuts is a package the render does not show — it is no row, its manifest is two
/// levels past the cutoff, and it contributes no resolved edge.
///
/// The fixture puts `a/` and `b/` at depth 1 (rows under `-L 1`, manifests at depth 2) and
/// `nested/c/` at depth 2 (a row only from `-L 2` up, manifest at depth 3). `a` declares a
/// path dependency on both.
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

    // -L 1: `a/` and `b/` are rows, so both manifests are read one level deeper and the edge between two visible rows is stated in both directions.
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
    // `c` is two levels past the cutoff: never a row, never read, so the declared path dependency on it resolves to nothing.
    assert!(
        shallow.contains("c (unresolved)"),
        "a package below the cutoff contributes no resolved edge:\n{shallow}"
    );

    // -L 2: `nested/c/` becomes a row, so its manifest is read and the same dependency now resolves — the graph deepens exactly with the view.
    let (deep, _err, code) = run(&dir, &["-L", "2"]);
    assert_eq!(code, 0, "-L 2 exits clean");
    assert!(
        !deep.contains("c (unresolved)") && deep.contains("<- depends on [b, c]"),
        "once c/ is displayed, the edge to it resolves:\n{deep}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `--strict-check` is a gate over the whole tree, not a rendered view, so `-L` must never
/// shrink what it lints: an unannotated file five levels down is still a violation under
/// `-L 1`. Pins that the walk cap did not leak from the map path into the check path.
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

/// `-L 0` and `-L 1` are the same view: depth 0 is the root's own contents, which every
/// render expands. Pins the floor the walk cap applies — a walk literally bounded at 0 would
/// yield the root alone and render nothing.
#[test]
fn depth_zero_renders_the_same_view_as_depth_one() {
    let dir = temp_dir("zero");
    std::fs::create_dir_all(dir.join("sub")).expect("mkdir sub");
    std::fs::write(dir.join("sub/nested.rs"), ANNOTATION).expect("write nested");
    std::fs::write(dir.join("top.rs"), ANNOTATION).expect("write top");

    let (zero, _err, zcode) = run(&dir, &["-L", "0"]);
    let (one, _err, ocode) = run(&dir, &["-L", "1"]);
    assert_eq!((zcode, ocode), (0, 0), "both exit clean");
    assert_eq!(zero, one, "-L 0 is -L 1, not an empty tree");
    assert!(
        zero.contains("sub/") && zero.contains("top.rs"),
        "and it is the root's own contents:\n{zero}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
