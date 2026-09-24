// Concern: freezes which config layer a run resolves — --config over discovery, a discovered repo file, and one per root | Non-concern: what any setting controls | IO: (temp trees) -> asserted stdout

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// System temp root, not the repo, so no ancestor `.annotated-tree.toml` leaks into discovery.
fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-scope-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(paths: &[&Path], extra: &[&str]) -> String {
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    for p in paths {
        argv.push(p.to_string_lossy().into_owned());
    }
    let cli = Cli::parse_from(&argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    assert_eq!(code, 0, "run must succeed");
    String::from_utf8(out).unwrap()
}

#[test]
fn explicit_config_flag_takes_effect_and_bypasses_discovery() {
    // Both extensions are unknown to the built-in config, so neither shows unless a layer teaches it.
    let dir = temp_dir("config-flag");
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("thing.foo"), "# Concern: a foo file for the config-scoping fixture | Non-concern: real behavior (a test stub) | IO: none\n").unwrap();
    std::fs::write(src.join("other.bar"), "# Concern: a bar file for the config-scoping fixture | Non-concern: real behavior (a test stub) | IO: none\n").unwrap();

    // Discovered config (what `--config` must OVERRIDE): teaches only `.bar`.
    std::fs::write(dir.join(".annotated-tree.toml"), "[languages.bar]\nextensions = [\".bar\"]\ncomment = \"#\"\n").unwrap();

    let explicit = dir.join("custom.toml");
    std::fs::write(&explicit, "[languages.foo]\nextensions = [\".foo\"]\ncomment = \"#\"\n").unwrap();

    let out = run(&[&dir], &["--config", &explicit.to_string_lossy()]);

    assert!(out.contains("thing.foo"), "--config's language must take effect (thing.foo shown):\n{out}");
    assert!(!out.contains("other.bar"), "--config must bypass the discovered .annotated-tree.toml (.bar not shown):\n{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_root_run_scopes_config_per_root() {
    let root_a = temp_dir("multi-a");
    let root_b = temp_dir("multi-b");

    for (root, marker) in [(&root_a, "alpha_check.rs"), (&root_b, "beta_check.rs")] {
        let tests = root.join("tests");
        std::fs::create_dir_all(&tests).unwrap();
        std::fs::write(tests.join(marker), "// Concern: a test file for the multi-root config fixture | Non-concern: real behavior (a test stub) | IO: none\n").unwrap();
    }

    std::fs::write(root_a.join(".annotated-tree.toml"), "[display]\ninclude_tests = true\n").unwrap();

    let out = run(&[&root_a, &root_b], &[]);

    assert!(out.contains("alpha_check.rs"), "root A's own include_tests=true must reveal its test file:\n{out}");
    assert!(!out.contains("beta_check.rs"), "root B (no config) must NOT inherit root A's include_tests; its tests/ stays hidden:\n{out}");

    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}

/// A discovered repo file is a full peer of the CLI: a repo whose workflows live behind a dot
/// states `[display] hidden = true` once instead of on every invocation.
#[test]
fn repo_config_hidden_reveals_a_dot_directory_without_the_flag() {
    let dir = temp_dir("hidden-config");
    std::fs::create_dir_all(dir.join(".ci")).unwrap();
    std::fs::write(dir.join(".ci/deploy.sh"), "#!/usr/bin/env bash\n# Concern: a dot-directory script for the config fixture | Non-concern: real behavior (a test stub) | IO: none\n").unwrap();

    let base = run(&[&dir], &[]);
    assert!(!base.contains("deploy.sh"), "with no config the dot-directory is pruned:\n{base}");

    std::fs::write(dir.join(".annotated-tree.toml"), "[display]\nhidden = true\n").unwrap();
    let out = run(&[&dir], &[]);
    assert!(out.contains("deploy.sh"), "the repo config reveals it with no flag on the command line:\n{out}");

    let _ = std::fs::remove_dir_all(&dir);
}
