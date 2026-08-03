// Concern: the files changed since a git ref as absolute canonical paths, and every git call behind it | Non-concern: filtering the tree, or a change's blast radius | IO: (root, ref) -> Result<paths>

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Files changed under `root` relative to `since`, as absolute canonical paths so they compare
/// byte-for-byte against the walked set: the union of `git diff --name-only` and `git ls-files
/// --others --exclude-standard --full-name`, so new files count and deleted ones canonicalize
/// away. Fail-fast — a missing git, non-repo root or bad ref errors rather than returning empty.
pub fn changed_files(root: &Path, since: &str) -> Result<HashSet<PathBuf>> {
    // `rev-parse --show-toplevel` doubles as the is-this-a-git-repo probe and gives us the base to resolve git's repo-root-relative paths against.
    let toplevel = git(root, &["rev-parse", "--show-toplevel"])?;
    let toplevel = PathBuf::from(toplevel.trim());

    // Both commands must emit REPO-ROOT-relative paths to resolve against `toplevel` identically: `ls-files` needs `--full-name`, or its cwd-relative paths mis-resolve an untracked file when `root` is a subdirectory.
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
        // Canonicalize so paths match the walked set; a path that no longer exists (e.g. a deletion) simply won't intersect the walk, so keep the join.
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
