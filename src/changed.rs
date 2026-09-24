// Concern: the files changed since a git ref as absolute canonical paths, and every git call behind it | Non-concern: filtering the tree, or a change's blast radius | IO: (root, ref) -> Result<paths>

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Fail-fast — a missing git, non-repo root or bad ref errors rather than returning empty.
pub fn changed_files(root: &Path, since: &str) -> Result<HashSet<PathBuf>> {
    let toplevel = git(root, &["rev-parse", "--show-toplevel"])?;
    let toplevel = PathBuf::from(toplevel.trim());

    let diff = git(root, &["diff", "--name-only", since])?;
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "--full-name"])?;

    let mut out = HashSet::new();
    for line in diff.lines().chain(untracked.lines()) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let abs = toplevel.join(line);
        // A deleted path won't canonicalize; the plain join still fails to intersect the walk.
        out.insert(abs.canonicalize().unwrap_or(abs));
    }
    Ok(out)
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").arg("-C").arg(root).args(args).output().with_context(|| format!("failed to run `git {}` — is git installed and on PATH?", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`git {}` failed in {}: {}", args.join(" "), root.display(), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
