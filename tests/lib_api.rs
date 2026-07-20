// Concern: locks the low-level public library surface — that a consumer can compose config + the walk + marker-based and marker-agnostic annotation extraction without the whole-tool run() | Non-concern: CLI/render behavior (other suites cover that) | IO: (temp tree, public API) -> asserted values

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::config::{CliOverrides, Config};
use annotated_tree::{annotation, build_globset, walk};
use globset::GlobSet;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A throwaway tree (under the system temp root, so no ancestor `.annotated-tree.toml` leaks in)
/// with a recognized `src/lib.rs` and an unrecognized-extension `runbook.ops` carrying a
/// marker-agnostic annotation.
fn temp_tree(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-libapi-{}-{tag}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src").join("lib.rs"),
        "// Concern: the recognized entry fixture | Non-concern: real behavior (a test stub) | IO: none\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("runbook.ops"),
        "# Concern: an unrecognized-extension runbook | Non-concern: real behavior (a test stub) | IO: none\n",
    )
    .unwrap();
    dir
}

#[test]
fn public_primitives_compose_config_walk_and_extraction() {
    let dir = temp_tree("compose");
    let config = Config::load(&dir, &CliOverrides::default()).expect("config resolves");

    // The recognized-only walk (empty include) sees just the `.rs`, and the marker-based
    // extractor reads its annotation through the resolved `Language`.
    let empty = GlobSet::empty();
    let recognized = walk::collect_code_files(&dir, &config, &empty, &empty).expect("walk");
    assert_eq!(recognized.len(), 1, "recognized walk sees only src/lib.rs");
    let lib_rs = &recognized[0];
    let lang = config
        .language_for_path(lib_rs)
        .expect("a .rs file resolves the rust language");
    assert_eq!(
        annotation::extract(lib_rs, lang).as_deref(),
        Some("Concern: the recognized entry fixture | Non-concern: real behavior (a test stub) | IO: none"),
    );

    // A caller-supplied include GlobSet (compiled with the exposed helper) widens the walk to the
    // unrecognized file, whose annotation the marker-agnostic extractor reads with no `Language`.
    let include = build_globset(&["*.ops".to_string()]).expect("glob compiles");
    let widened = walk::collect_code_files(&dir, &config, &empty, &include).expect("walk");
    assert_eq!(widened.len(), 2, "the selector adds runbook.ops");
    let runbook = widened
        .iter()
        .find(|p| p.extension().is_some_and(|e| e == "ops"))
        .expect("runbook.ops is in the widened set");
    assert!(
        config.language_for_path(runbook).is_none(),
        ".ops maps to no configured language",
    );
    assert_eq!(
        annotation::extract_any(runbook).as_deref(),
        Some("Concern: an unrecognized-extension runbook | Non-concern: real behavior (a test stub) | IO: none"),
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn configured_walk_is_directly_usable() {
    // The raw `ignore`-based walker is exposed too, so a consumer can apply its OWN keep policy
    // (e.g. for extensionless files or symlinks) instead of `collect_code_files`'s.
    let dir = temp_tree("raw-walk");
    let names: Vec<String> = walk::configured_walk(&dir, true, false, &GlobSet::empty())
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"lib.rs".to_string()));
    assert!(
        names.contains(&"runbook.ops".to_string()),
        "the raw walker yields every file, recognized or not: {names:?}",
    );
    let _ = std::fs::remove_dir_all(&dir);
}
