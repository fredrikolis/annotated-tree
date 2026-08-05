// Concern: freezes the strip verb — what it removes, what it skips, and what each outcome reports | Non-concern: the conformance verdict itself | IO: (temp tree) -> asserted (contents, stdout, code)

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn tree(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-strip-{}-{tag}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let w = |name: &str, body: &str| std::fs::write(dir.join(name), body).unwrap();
    w(
        "ok.py",
        "# Concern: a | Non-concern: b | IO: none\n\nx = 1\n",
    );
    w("prose.py", "# mentions Concern: but is not one\ny = 2\n");
    w("half.py", "# Concern: two fields only | IO: none\nz = 3\n");
    w(
        "inline.md",
        "<!-- Concern: a | Non-concern: b | IO: none --><div>markup</div>\n<p>x</p>\n",
    );
    w(
        "doc.py",
        "\"\"\"Concern: a | Non-concern: b | IO: none\n\nprose\n\"\"\"\n\nq = 4\n",
    );
    dir
}

fn strip(args: &[&str]) -> i32 {
    let cli = Cli::parse_from(std::iter::once("annotated-tree").chain(args.iter().copied()));
    annotated_tree::run(&cli, &mut Vec::new(), &mut Vec::new()).unwrap()
}

/// The whole contract: nothing is written without confirmation, and what IS written is only the
/// conforming three-field line. An ordinary comment that mentions `Concern:` and a line missing a
/// field both survive — stripping on the extractor alone would take the first comment of every file.
#[test]
fn strip_removes_conforming_annotations_only_once_confirmed() {
    let dir = tree("contract");
    let p = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap();
    let root = dir.to_str().unwrap();

    assert_eq!(strip(&["strip", "-R", root]), 0);
    assert!(
        p("ok.py").starts_with("# Concern:"),
        "--dry-run writes nothing"
    );

    assert_eq!(strip(&["strip", "-R", "-y", root]), 0);
    assert_eq!(
        p("ok.py"),
        "x = 1\n",
        "the annotation's line goes, and the blank that separated it"
    );
    assert_eq!(p("prose.py"), "# mentions Concern: but is not one\ny = 2\n");
    assert_eq!(
        p("half.py"),
        "# Concern: two fields only | IO: none\nz = 3\n"
    );

    assert_eq!(
        strip(&["strip", "-R", "-y", root]),
        0,
        "a second run is a no-op"
    );
    assert_eq!(p("ok.py"), "x = 1\n");

    std::fs::remove_dir_all(&dir).ok();
}

/// An Annotation is a span, not a line. A block comment closing mid-line and a docstring
/// opening one are both refused, because deleting their line would take code with it. The same path
/// named twice must also not delete a second line — the verdict is re-established before each write.
#[test]
fn a_line_that_is_not_wholly_an_annotation_is_left_alone() {
    let dir = tree("span");
    let p = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap();
    let root = dir.to_str().unwrap();

    assert_eq!(strip(&["strip", "-R", "-y", root]), 0);
    assert!(
        p("inline.md").starts_with("<!-- Concern:"),
        "markup follows the closer"
    );
    assert!(
        p("doc.py").starts_with("\"\"\"Concern:"),
        "the docstring closes on a later line"
    );

    let dup = dir.join("ok.py");
    let twice = dup.to_str().unwrap();
    assert_eq!(strip(&["strip", "-y", twice, twice]), 0);
    assert_eq!(
        p("ok.py"),
        "x = 1\n",
        "the second pass must not take the next line"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The JSON surface reports AFTER the write pass and echoes the path as given, so a caller can
/// match a row to its input and tell a clean run from a partial one.
#[test]
fn the_json_report_is_emitted_for_success_as_well_as_failure() {
    let dir = tree("json");
    let root = dir.to_str().unwrap();
    let mut out = Vec::new();
    let cli = Cli::parse_from(["annotated-tree", "--format", "json", "strip", "-R", root]);
    assert_eq!(
        annotated_tree::run(&cli, &mut out, &mut Vec::new()).unwrap(),
        0
    );
    let doc: serde_json::Value = serde_json::from_slice(&out).expect("stdout parses as JSON");
    assert_eq!(doc["schema"], 1);
    assert_eq!(
        doc["strip"]["applied"], false,
        "the listing pass wrote nothing"
    );
    assert_eq!(doc["strip"]["files"][0]["code"], serde_json::Value::Null);
    std::fs::remove_dir_all(&dir).ok();
}

/// Each root is judged by ITS OWN config, so argument order cannot decide which files are edited.
/// Pooling every target under one resolved config made `strip -R A B` and `strip -R B A` disagree.
#[test]
fn each_root_uses_its_own_config() {
    let dir = tree("roots");
    let (a, b) = (dir.join("a"), dir.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(
        a.join(".annotated-tree.toml"),
        "[languages.zz]\nextensions = [\".zz\"]\ncomment = \"//\"\n",
    )
    .unwrap();
    let zz = a.join("f.zz");
    std::fs::write(&zz, "// Concern: a | Non-concern: b | IO: none\nx();\n").unwrap();
    std::fs::write(
        b.join("g.py"),
        "# Concern: a | Non-concern: b | IO: none\ny=1\n",
    )
    .unwrap();

    // The .zz language is A's alone; B must not decide whether A's file is recognized.
    assert_eq!(
        strip(&[
            "strip",
            "-R",
            "-y",
            b.to_str().unwrap(),
            a.to_str().unwrap()
        ]),
        0
    );
    assert_eq!(std::fs::read_to_string(&zz).unwrap(), "x();\n");
    std::fs::remove_dir_all(&dir).ok();
}

/// A file it cannot read is named with a dispatchable code and colours the exit, rather than
/// passing silently; and the mode of a file it rewrites survives the rename.
#[test]
fn an_unreadable_file_is_reported_and_a_mode_survives() {
    let dir = tree("io");
    let exec = dir.join("run.py");
    std::fs::write(&exec, "# Concern: a | Non-concern: b | IO: none\nx=1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exec, std::fs::Permissions::from_mode(0o755)).unwrap();
        let locked = dir.join("locked.py");
        std::fs::write(&locked, "# Concern: a | Non-concern: b | IO: none\nz=1\n").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let code = strip(&["strip", "-R", "-y", dir.to_str().unwrap()]);
        let mode = std::fs::metadata(&exec).unwrap().permissions().mode() & 0o777;
        // Restored BEFORE asserting: a regression must not leave an unreadable file behind and fail the cleanup, poisoning the next run.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(code, 4, "an unreadable file colours the exit code");
        assert_eq!(mode, 0o755, "the rename must carry the original mode");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// A skip is a reported row, not silence, and never colours the exit code. A symlink named beside
/// its own target must not collapse into it: they share a canonical path, and dropping one would
/// let argument order decide whether the real file is edited.
#[test]
fn a_skipped_path_is_reported_and_does_not_hide_its_target() {
    let dir = tree("skips");
    let real = dir.join("real.py");
    std::fs::write(&real, "# Concern: a | Non-concern: b | IO: none\nx = 1\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("real.py", dir.join("link.py")).unwrap();

    let (r, l) = (real.to_str().unwrap(), dir.join("link.py"));
    assert_eq!(strip(&["strip", "-y", l.to_str().unwrap(), r]), 0);
    assert_eq!(
        std::fs::read_to_string(&real).unwrap(),
        "x = 1\n",
        "the named symlink is skipped, but its target is still stripped"
    );

    let keep = dir.join("keep.py");
    std::fs::write(&keep, "# Concern: a | Non-concern: b | IO: none\ny = 2\n").unwrap();
    assert_eq!(
        strip(&["-I", "keep.py", "strip", "-y", keep.to_str().unwrap()]),
        0
    );
    assert!(
        std::fs::read_to_string(&keep)
            .unwrap()
            .starts_with("# Concern:"),
        "-I narrows an explicitly named file, not only the -R walk"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A directory needs `-R`, the way `rm` does.
#[test]
fn a_directory_without_recursive_is_a_usage_error() {
    let dir = tree("norecurse");
    assert_eq!(strip(&["strip", "-y", dir.to_str().unwrap()]), 2);
    assert!(std::fs::read_to_string(dir.join("ok.py"))
        .unwrap()
        .starts_with("# Concern:"));
    std::fs::remove_dir_all(&dir).ok();
}

/// `strip -R` reaches an extensionless script, and takes its annotation without touching the
/// shebang above it. Its language comes from that shebang, which is the whole safety argument for
/// editing the file at all: knowing where the comment ends.
#[test]
fn strip_reaches_an_extensionless_script() {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-strip-{}-shebang-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let hook = dir.join("pre-commit");
    std::fs::write(
        &hook,
        "#!/usr/bin/env bash\n# Concern: a | Non-concern: b | IO: none\n\nset -e\n",
    )
    .unwrap();

    assert_eq!(strip(&["strip", "-R", "-y", dir.to_str().unwrap()]), 0);
    assert_eq!(
        std::fs::read_to_string(&hook).unwrap(),
        "#!/usr/bin/env bash\nset -e\n",
        "the annotation and its blank line go; the shebang stays on line 1"
    );

    std::fs::remove_dir_all(&dir).ok();
}
