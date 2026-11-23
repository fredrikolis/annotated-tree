// Concern: tells an agent what it just wrote violates annotation form | Non-concern: what an annotation means, or installing the hook entry | IO: (a PostToolUse event) -> hookSpecificOutput, or nothing

//! `PostToolUse`, not Pre: the write already happened. This informs and always exits 0 —
//! `.githooks/pre-commit` stays the thing that refuses.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use globset::GlobSet;
use serde_json::{json, Value};

use crate::cli::EditHook;
use crate::config::{CliOverrides, Config};
use crate::exit;
use crate::strict::{self, AnnotationViolation};
use crate::walk::{self, WalkedTree, CHARTER_FILE};
use crate::{guide, sidecar, util};

/// Consulted only AT the git root — a budget found further up is not this file's to answer from.
const CONFIG_FILE: &str = ".annotated-tree.toml";

pub(crate) fn dispatch(args: &EditHook, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    if !args.claude && !args.codex {
        writeln!(err, "annotated-tree edit-hook: pass --claude or --codex.")?;
        return Ok(exit::USAGE);
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return Ok(exit::SUCCESS);
    }
    let Ok(event) = serde_json::from_str::<Value>(&raw) else {
        return Ok(exit::SUCCESS);
    };
    let files = if args.codex { patched(&event) } else { written(&event).into_iter().collect() };
    let mut found = Vec::new();
    let mut unchecked = Vec::new();
    for file in files {
        match defects(&file) {
            Ok(Some(d)) => found.push(d),
            Ok(None) => {}
            Err(reason) => unchecked.push(format!("annotated-tree edit-hook: couldn't check {} — {reason}", file.display())),
        }
    }
    if !found.is_empty() {
        writeln!(out, "{}", envelope(&found, &unchecked))?;
    } else if !unchecked.is_empty() {
        writeln!(out, "{}", could_not_check(&unchecked.join("\n")))?;
    }
    Ok(exit::SUCCESS)
}

fn cwd(event: &Value) -> PathBuf {
    event.pointer("/cwd").and_then(Value::as_str).map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn written(event: &Value) -> Option<PathBuf> {
    let cwd = cwd(event);
    ["/tool_input/file_path", "/tool_response/filePath", "/tool_input/notebook_path"].iter().filter_map(|p| event.pointer(p).and_then(Value::as_str)).map(|p| cwd.join(p)).find(|p| p.is_file())
}

/// Codex hands over the patch text itself; a moved file is checked under the name it moved to.
fn patched(event: &Value) -> Vec<PathBuf> {
    let Some(patch) = event.pointer("/tool_input/command").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = Vec::new();
    // `Move to:` renames only the file the `Update File:` right above it named.
    let mut updating = false;
    for line in patch.lines() {
        if let Some(p) = line.strip_prefix("*** Add File: ").or_else(|| line.strip_prefix("*** Update File: ")) {
            files.push(PathBuf::from(p.trim()));
            updating = line.starts_with("*** Update");
        } else if let Some(p) = line.strip_prefix("*** Move to: ").filter(|_| updating) {
            files.pop();
            files.push(PathBuf::from(p.trim()));
        } else if line.starts_with("*** ") {
            updating = false;
        }
    }
    let cwd = cwd(event);
    let mut seen = std::collections::HashSet::new();
    files.into_iter().map(|p| cwd.join(p)).filter(|p| p.is_file() && seen.insert(p.clone())).collect()
}

fn git_root(file: &Path) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// One file's violations, with what [`envelope`] needs to state them.
struct Defects {
    root: PathBuf,
    violations: Vec<AnnotationViolation>,
    no_guide: bool,
}

/// `Ok(None)` where the repo never opted in, or the same walk `.githooks/pre-commit` runs would
/// never reach `file` — the gate's own predicate, so this cannot report what it would pass.
/// `Err` names a reason this couldn't be checked at all — distinct from "nothing to report".
fn defects(file: &Path) -> Result<Option<Defects>, String> {
    let Some(root) = git_root(file) else {
        return Ok(None);
    };
    let config_path = root.join(CONFIG_FILE);
    if !config_path.is_file() {
        return Ok(None);
    }
    let overrides = CliOverrides { config_file: Some(config_path), ..CliOverrides::default() };
    let config = Config::load(&root, &overrides).map_err(|e| format!("couldn't read {CONFIG_FILE}: {e}"))?;
    let excludes = util::build_globset(&config.display.ignore).map_err(|e| format!("couldn't build the [display] ignore matcher: {e}"))?;
    let filter = walk::WalkFilter::from_config(&config, &excludes);
    let walked = walk::collect_tree(&root, &config, filter, &GlobSet::empty(), None).map_err(|e| e.to_string())?;
    let canon = file.canonicalize().map_err(|e| format!("couldn't resolve {}: {e}", file.display()))?;

    let Some(subject) = subject(file, &canon, &root, &walked) else {
        return Ok(None);
    };

    let report = strict::check_file(&root, std::slice::from_ref(&subject), &config);
    if report.violations.is_empty() {
        return Ok(None);
    }
    Ok(Some(Defects { root, violations: report.violations, no_guide: config.display.no_guide }))
}

/// Resolves what to actually hand [`strict::check_file`]. A `.annotation` charter and a
/// `<name>.annotation` sidecar are dropped BY NAME from `walked` (they carry no code of their
/// own), so a write to either would otherwise vanish before it's ever checked — this is what
/// keeps them in the loop, still gated by whatever the walk would or would not have reached.
fn subject(file: &Path, canon: &Path, root: &Path, walked: &WalkedTree) -> Option<PathBuf> {
    if file.file_name().is_some_and(|n| n == CHARTER_FILE) {
        let dir_canon = file.parent()?.canonicalize().ok()?;
        let root_canon = root.canonicalize().ok()?;
        let reached = dir_canon == root_canon || walked.dirs.iter().any(|d| d.canonicalize().is_ok_and(|c| c == dir_canon));
        // `check_file`'s charter check walks ancestors of whatever it's handed, so the charter's own path works.
        return reached.then(|| canon.to_path_buf());
    }
    if let Some(target) = sidecar::named_target(file) {
        let target_canon = target.canonicalize().ok()?;
        return walked.files.iter().find(|p| p.canonicalize().is_ok_and(|c| c == target_canon)).cloned();
    }
    walked.files.iter().find(|p| p.canonicalize().is_ok_and(|c| c == *canon)).cloned()
}

fn envelope(found: &[Defects], unchecked: &[String]) -> String {
    let whose = if found.len() > 1 { "these files' annotations are" } else { "this file's annotation is" };
    let headline = format!("annotated-tree: {whose} malformed, missing, or over the bound — --strict-check reports it.");
    let mut lines: Vec<String> = Vec::new();
    let mut body = String::new();
    for d in found {
        let _ = writeln!(body, "# paths relative to {}", d.root.display());
        for l in d.violations.iter().map(AnnotationViolation::message) {
            let _ = writeln!(body, "{l}");
            lines.push(l);
        }
    }
    for u in unchecked {
        let _ = writeln!(body, "{u}");
        lines.push(u.clone());
    }
    let guide = if found.iter().all(|d| d.no_guide) { String::new() } else { format!("\n{}", guide::full()) };
    let context = format!(
        "{headline}\n\n{body}{guide}\nDeleting the annotation is not a fix, and neither is \
         raising [rules] max_annotation_length.",
    );

    let mut message = headline.clone();
    for l in lines.iter().take(6) {
        let _ = write!(message, "\n{l}");
    }
    json!({
        "systemMessage": message,
        "hookSpecificOutput": { "hookEventName": "PostToolUse", "additionalContext": context },
    })
    .to_string()
}

fn could_not_check(message: &str) -> String {
    json!({
        "systemMessage": message,
        "hookSpecificOutput": { "hookEventName": "PostToolUse", "additionalContext": message },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("at-edit-hook-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn findings(file: &Path) -> Result<Option<String>, String> {
        defects(file).map(|d| d.map(|d| envelope(&[d], &[])))
    }

    fn init_repo(dir: &Path) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(CONFIG_FILE), "").unwrap();
    }

    #[test]
    fn no_config_at_the_git_root_is_silent() {
        let dir = tmp("no-config");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let file = dir.join("f.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        assert_eq!(findings(&file), Ok(None));
    }

    #[test]
    fn a_config_above_the_git_root_is_not_consulted() {
        let dir = tmp("above-root");
        std::fs::write(dir.join(CONFIG_FILE), "").unwrap();
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("f.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        assert_eq!(findings(&file), Ok(None));
    }

    #[test]
    fn a_broken_annotation_is_reported() {
        let dir = tmp("broken");
        init_repo(&dir);
        let file = dir.join("f.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let out = findings(&file).unwrap().expect("a violation was expected");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["systemMessage"].as_str().is_some_and(|s| s.contains("annotation")));
        assert!(!v["systemMessage"].as_str().unwrap().contains("pre-commit hook will refuse"));
        assert!(v["hookSpecificOutput"]["additionalContext"].as_str().is_some_and(|s| s.contains("f.rs")));
    }

    #[test]
    fn a_conforming_annotation_is_silent() {
        let dir = tmp("conforming");
        init_repo(&dir);
        let file = dir.join("f.rs");
        std::fs::write(&file, "// Concern: a fixture | Non-concern: real behavior | IO: none\nfn main() {}\n").unwrap();
        assert_eq!(findings(&file), Ok(None));
    }

    #[test]
    fn a_path_the_configured_walk_excludes_is_silent() {
        let dir = tmp("excluded");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(CONFIG_FILE), "[display]\nignore = [\"skip\"]\n").unwrap();
        let sub = dir.join("skip");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("f.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        assert_eq!(findings(&file), Ok(None));
    }

    #[test]
    fn a_malformed_directory_charter_is_reported() {
        let dir = tmp("charter");
        init_repo(&dir);
        let sub = dir.join("pkg");
        std::fs::create_dir_all(&sub).unwrap();
        let charter = sub.join(CHARTER_FILE);
        std::fs::write(&charter, "not an annotation\n").unwrap();
        let out = findings(&charter).unwrap().expect("a violation was expected");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["hookSpecificOutput"]["additionalContext"].as_str().is_some_and(|s| s.contains("pkg")));
    }

    #[test]
    fn a_conforming_directory_charter_is_silent() {
        let dir = tmp("charter-ok");
        init_repo(&dir);
        let sub = dir.join("pkg");
        std::fs::create_dir_all(&sub).unwrap();
        let charter = sub.join(CHARTER_FILE);
        std::fs::write(&charter, "Concern: a fixture | Non-concern: real behavior | IO: none\n").unwrap();
        assert_eq!(findings(&charter), Ok(None));
    }

    #[test]
    fn a_malformed_sidecar_is_reported() {
        let dir = tmp("sidecar");
        init_repo(&dir);
        let target = dir.join("data.csv");
        std::fs::write(&target, "a,b\n1,2\n").unwrap();
        let side = sidecar::path_for(&target);
        std::fs::write(&side, "not an annotation\n").unwrap();
        let out = findings(&side).unwrap().expect("a violation was expected");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["hookSpecificOutput"]["additionalContext"].as_str().is_some_and(|s| s.contains("data.csv")));
    }

    #[test]
    fn a_conforming_sidecar_is_silent() {
        let dir = tmp("sidecar-ok");
        init_repo(&dir);
        let target = dir.join("data.csv");
        std::fs::write(&target, "a,b\n1,2\n").unwrap();
        let side = sidecar::path_for(&target);
        std::fs::write(&side, "Concern: a fixture | Non-concern: real behavior | IO: none\n").unwrap();
        assert_eq!(findings(&side), Ok(None));
    }

    #[test]
    fn a_syntax_error_in_the_repo_config_cannot_be_checked() {
        let dir = tmp("bad-config");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(CONFIG_FILE), "not valid toml [[[").unwrap();
        let file = dir.join("f.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        assert!(findings(&file).is_err());
    }

    #[test]
    fn no_guide_suppresses_the_guide_text() {
        let dir = tmp("no-guide");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(CONFIG_FILE), "[display]\nno_guide = true\n").unwrap();
        let file = dir.join("f.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let out = findings(&file).unwrap().expect("a violation was expected");
        let v: Value = serde_json::from_str(&out).unwrap();
        let ctx = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(!ctx.contains(guide::full().lines().next().unwrap()));
    }

    #[test]
    fn a_codex_patch_names_every_file_it_leaves_behind() {
        let dir = tmp("codex-patch");
        for f in ["added.rs", "updated.rs", "moved.rs"] {
            std::fs::write(dir.join(f), "fn main() {}\n").unwrap();
        }
        // `updated.rs` twice, and a stray `Move to:` after a delete that must not rename `moved.rs`.
        let patch = "*** Begin Patch\n*** Add File: added.rs\n+fn main() {}\n*** Update File: updated.rs\n@@\n-a\n+b\n*** Update File: old.rs\n*** Move to: moved.rs\n*** Update File: updated.rs\n*** Delete File: gone.rs\n*** Move to: added.rs\n*** End Patch\n";
        let event = json!({ "cwd": dir, "tool_name": "apply_patch", "tool_input": { "command": patch } });
        assert_eq!(patched(&event), vec![dir.join("added.rs"), dir.join("updated.rs"), dir.join("moved.rs")]);
    }

    #[test]
    fn two_broken_files_in_one_patch_share_one_envelope() {
        let dir = tmp("codex-two");
        init_repo(&dir);
        for f in ["a.rs", "b.rs"] {
            std::fs::write(dir.join(f), "fn main() {}\n").unwrap();
        }
        let found: Vec<Defects> = ["a.rs", "b.rs"].iter().filter_map(|f| defects(&dir.join(f)).unwrap()).collect();
        let v: Value = serde_json::from_str(&envelope(&found, &["annotated-tree edit-hook: couldn't check c.rs — a reason".to_string()])).unwrap();
        let ctx = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(ctx.contains("a.rs") && ctx.contains("b.rs"), "{ctx}");
        assert_eq!(ctx.matches(guide::full().lines().next().unwrap()).count(), 1, "the guide was repeated per file");
        assert!(ctx.contains("couldn't check c.rs"), "a file that could not be checked was dropped beside the findings: {ctx}");
        assert!(v["systemMessage"].as_str().is_some_and(|m| m.contains("these files'")), "{v}");
    }
}
