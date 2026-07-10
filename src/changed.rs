// Changed: Quarantines all git interaction — asks git which files changed since a ref and resolves them to absolute canonical paths. NOT concerned with filtering the tree or blast radius. | I/O: (root, ref) -> Result<set<abs file paths>>

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// The set of files that changed under `root` relative to `since` (a git ref such
/// as `HEAD`, a branch, or a commit SHA), returned as absolute, canonical paths so
/// they compare byte-for-byte against the walked file set.
///
/// Scope is the developer's *full working delta*, the union of:
///   * `git diff --name-only <since>` — every tracked file whose working-tree
///     content differs from `<since>` (staged and unstaged alike, since `git diff`
///     against a commit compares the working tree to that commit), and
///   * `git ls-files --others --exclude-standard --full-name` — untracked,
///     non-ignored files (`--full-name` keeps them repo-root-relative like `diff`).
///
/// Rationale: "what did I touch since `<since>`" must include brand-new files a
/// review cares about most, not just edits to already-tracked ones; `--exclude-standard`
/// keeps `.gitignore`d noise out. Deleted paths naturally drop out — they no longer
/// exist, so they canonicalize away and never match a walked file.
///
/// Fail-Fast (never a silent empty set): a missing `git`, a non-repo `root`, or a
/// bad `<since>` ref each surface as an explicit error, so an empty result always
/// means "nothing changed", never "git quietly failed".
pub fn changed_files(root: &Path, since: &str) -> Result<HashSet<PathBuf>> {
    // `rev-parse --show-toplevel` doubles as the is-this-a-git-repo probe and gives
    // us the base to resolve git's repo-root-relative paths against.
    let toplevel = git(root, &["rev-parse", "--show-toplevel"])?;
    let toplevel = PathBuf::from(toplevel.trim());

    // A bad ref fails here (Fail-Fast) rather than yielding an empty diff.
    // Both commands must emit REPO-ROOT-relative paths so they resolve against
    // `toplevel` identically: `diff` already does; `ls-files` needs `--full-name`
    // (without it, its paths are relative to the cwd `root`, which mis-resolves an
    // untracked file when `root` is a subdirectory of the repo).
    let diff = git(root, &["diff", "--name-only", since])?;
    let untracked = git(
        root,
        &["ls-files", "--others", "--exclude-standard", "--full-name"],
    )?;

    let mut out = HashSet::new();
    for line in diff.lines().chain(untracked.lines()) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let abs = toplevel.join(line);
        // Canonicalize so paths match the walked set; a path that no longer exists
        // (e.g. a deletion) simply won't intersect the walk, so keep the join.
        out.insert(abs.canonicalize().unwrap_or(abs));
    }
    Ok(out)
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| {
            format!(
                "failed to run `git {}` — is git installed and on PATH?",
                args.join(" ")
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "`git {}` failed in {}: {}",
            args.join(" "),
            root.display(),
            stderr.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
