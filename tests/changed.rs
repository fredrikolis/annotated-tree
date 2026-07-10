// Changed: End-to-end test for `--since` / `--changed` — editing a package surfaces
// that file PLUS its blast radius (every package that transitively depends on it),
// and a git failure (bad ref / non-repo) is an explicit error, not an empty view.
// Freezes the feature's whole value: "editing core surfaces api+worker". | I/O: (temp git repo) -> asserted (stdout, code)

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use annotated_tree::Cli;
use clap::Parser;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Build a temp tree of three Python packages where `api` and `worker` both depend
/// on `core` (mirroring `sample/`), so `core`'s reverse-dep closure is {api, worker}.
fn temp_workspace(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("at-changed-{}-{tag}-{n}", std::process::id()));

    let pkg = |name: &str, deps: &str, module: &str| {
        let base = dir.join("packages").join(name);
        std::fs::create_dir_all(base.join(module)).unwrap();
        std::fs::write(
            base.join("pyproject.toml"),
            format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\ndependencies = [{deps}]\n"),
        )
        .unwrap();
        std::fs::write(
            base.join(module).join("code.py"),
            format!("# {module}: does {module}. | I/O: () -> None\n"),
        )
        .unwrap();
    };

    pkg("acme-core", "\"pydantic>=2.0\"", "acme_core");
    pkg("acme-api", "\"acme-core\", \"fastapi>=0.110\"", "acme_api");
    pkg(
        "acme-worker",
        "\"acme-core\", \"celery>=5.3\"",
        "acme_worker",
    );
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git must be installed to run this test");
    assert!(status.success(), "git {args:?} failed");
}

fn run_capture(dir: &Path, extra: &[&str]) -> (String, i32) {
    let mut argv = vec!["annotated-tree".to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    argv.push(dir.to_string_lossy().into_owned());
    let cli = Cli::parse_from(&argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (String::from_utf8(out).unwrap(), code)
}

#[test]
fn editing_core_surfaces_api_and_worker_as_blast_radius() {
    let dir = temp_workspace("blast");
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "initial"]);

    // Edit a file inside `core` only.
    std::fs::write(
        dir.join("packages/acme-core/acme_core/code.py"),
        "# acme_core: edited. | I/O: () -> None\n",
    )
    .unwrap();

    let (out, code) = run_capture(&dir, &["--since", "HEAD"]);
    assert_eq!(code, 0, "a normal filtered run exits 0:\n{out}");

    // (a) The changed file appears.
    assert!(
        out.contains("acme_core"),
        "the edited core file must appear:\n{out}"
    );
    // (b) The blast radius — the reverse-dep closure of core — is surfaced: api and
    // worker both depend on core, so both must show even though neither was edited.
    assert!(
        out.contains("acme_api"),
        "api (depends on core) must surface as blast radius:\n{out}"
    );
    assert!(
        out.contains("acme_worker"),
        "worker (depends on core) must surface as blast radius:\n{out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn untracked_file_under_subdir_root_is_surfaced() {
    // Regression: `git diff` emits repo-root-relative paths but `git ls-files --others`
    // emitted cwd-relative ones; joining both onto the repo top-level silently dropped
    // untracked files whenever the analyzed root was a SUBDIRECTORY of the repo (the
    // tool's monorepo wheelhouse). `--full-name` on ls-files unifies the base.
    let dir = temp_workspace("subdir");
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "initial"]);

    // A brand-new UNTRACKED file under a subdirectory we then analyze directly.
    let subroot = dir.join("packages").join("acme-core");
    std::fs::write(
        subroot.join("acme_core").join("brandnew.py"),
        "# brandnew: a fresh untracked file. | I/O: () -> None\n",
    )
    .unwrap();

    let (out, code) = run_capture(&subroot, &["--changed"]);
    assert_eq!(code, 0, "a subdir --changed run exits 0:\n{out}");
    assert!(
        out.contains("brandnew"),
        "an untracked file under a subdirectory root must surface, not vanish:\n{out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_ref_and_non_repo_are_errors_not_empty() {
    // Fail-Fast: a bad ref inside a real repo must error, never silently return an
    // empty (mis-scoped) view.
    let dir = temp_workspace("badref");
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "initial"]);

    let cli = Cli::parse_from([
        "annotated-tree",
        "--since",
        "no-such-ref-xyz",
        &dir.to_string_lossy(),
    ]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(
        annotated_tree::run(&cli, &mut out, &mut err).is_err(),
        "a bad ref must be an explicit error, not an empty result"
    );

    // A directory that is not a git repository must also error.
    let non_repo = temp_workspace("nonrepo");
    let cli2 = Cli::parse_from(["annotated-tree", "--changed", &non_repo.to_string_lossy()]);
    let (mut out2, mut err2) = (Vec::new(), Vec::new());
    assert!(
        annotated_tree::run(&cli2, &mut out2, &mut err2).is_err(),
        "a non-git directory must be an explicit error"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&non_repo);
}
