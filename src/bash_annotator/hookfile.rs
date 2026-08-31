// Concern: adds or removes our hook entries in a settings file, keeping every other key | Non-concern: what to rewrite, or the hook event format | IO: (settings path) -> edited file, outcome

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// `SessionStart` names every source; `compact` matters most since a compacted session would
/// otherwise never hear the announcement again.
const ENTRIES: &[(&str, &str, &str, &str)] = &[
    (
        "PreToolUse",
        "Bash",
        "bash-annotator --rewrite-tool-call",
        "--rewrite-tool-call",
    ),
    (
        "SessionStart",
        "startup|resume|clear|compact|fork",
        "bash-annotator --session-announcement",
        "--session-announcement",
    ),
    (
        "PostToolUse",
        "Write|Edit|MultiEdit|NotebookEdit",
        "edit-hook --claude",
        "edit-hook",
    ),
];

/// A bare name (not absolute, unlike the rewritten-pipeline annotator): the harness runs it
/// directly, no pipe to SIGPIPE if it fails to resolve.
fn command(suffix: &str) -> String {
    format!("annotated-tree {suffix}")
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Added,
    AlreadyPresent,
    Replaced,
    Removed,
    NotPresent,
}

impl Outcome {
    pub fn describe(&self, path: &Path) -> String {
        let p = path.display();
        match self {
            Outcome::Added => format!("hook installed in {p}"),
            Outcome::AlreadyPresent => format!("hook already present in {p}; nothing changed"),
            Outcome::Replaced => format!(
                "hook installed in {p}; an entry of ours that ran the wrong verb for its event \
                 was replaced"
            ),
            Outcome::Removed => format!("hook removed from {p}"),
            Outcome::NotPresent => format!("no hook of ours in {p}; nothing changed"),
        }
    }
}

/// User-level settings; a per-repo hook instead passes `.claude/settings.local.json`.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".claude").join("settings.json"))
}

/// Substring, not final-path-segment: that tail is identical for a bare vs. absolute command.
fn names(entry: &Value, needle: &str) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains(needle))
            })
        })
}

/// Any entry naming one of our verbs, regardless of which event it sits under.
fn is_ours(entry: &Value) -> bool {
    ENTRIES.iter().any(|(_, _, _, verb)| names(entry, verb))
}

/// Read the settings file as an object, or an empty one when it does not exist yet.
///
/// A file that exists but does not parse is an ERROR, never an empty object: this file holds the
/// permissions a user has accepted over time, and silently starting fresh would discard them.
fn read(path: &Path) -> Result<Map<String, Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str(&text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(format!(
            "{} is valid JSON but not an object; refusing to touch it",
            path.display()
        )),
        Err(e) => Err(format!(
            "{} is not valid JSON ({e}); fix it first — it holds your accepted permissions and \
             this will not overwrite it",
            path.display()
        )),
    }
}

/// Write via a temporary file in the same directory, then rename.
///
/// A rename is atomic within a filesystem, so an interrupted write cannot leave the user with a
/// truncated settings file — the failure mode would cost them every permission they have accepted.
fn write(path: &Path, root: &Map<String, Value>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut text = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    text.push('\n');
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

/// Each event is considered on its own, so a file missing one entry still gains it.
pub fn install(path: &Path) -> Result<Outcome, String> {
    let mut root = read(path)?;
    let mut added = false;
    let mut replaced = false;
    for (event, matcher, suffix, verb) in ENTRIES {
        let hooks = root
            .entry("hooks")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| format!("`hooks` in {} is not an object", path.display()))?;
        let list = hooks
            .entry(*event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("`hooks.{event}` in {} is not an array", path.display()))?;
        // The verb, not the whole command: an entry a user rewrote to an absolute path is already doing this event's one job, and re-spelling it would be a change they did not ask for.
        if list.iter().any(|e| names(e, verb)) {
            continue;
        }
        let before = list.len();
        list.retain(|e| !is_ours(e));
        replaced |= list.len() != before;
        list.push(json!({
            "matcher": matcher,
            "hooks": [{ "type": "command", "command": command(suffix) }],
        }));
        added = true;
    }
    if !added {
        return Ok(Outcome::AlreadyPresent);
    }
    write(path, &root)?;
    Ok(if replaced {
        Outcome::Replaced
    } else {
        Outcome::Added
    })
}

/// Remove our entries and nothing else, pruning the containers we would have created so the file
/// returns to the shape it had before `install`.
pub fn uninstall(path: &Path) -> Result<Outcome, String> {
    let mut root = read(path)?;
    let mut removed = false;
    // A block, so the borrow of `root` ends before the last container is pruned off `root` itself.
    let hooks_empty = {
        let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
            return Ok(Outcome::NotPresent);
        };
        for (event, _, _, _) in ENTRIES {
            let Some(list) = hooks.get_mut(*event).and_then(Value::as_array_mut) else {
                continue;
            };
            let before = list.len();
            list.retain(|e| !is_ours(e));
            if list.len() == before {
                continue;
            }
            removed = true;
            if list.is_empty() {
                hooks.remove(*event);
            }
        }
        hooks.is_empty()
    };
    if !removed {
        return Ok(Outcome::NotPresent);
    }
    if hooks_empty {
        root.remove("hooks");
    }
    write(path, &root)?;
    Ok(Outcome::Removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("at-hookfile-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    #[test]
    fn installing_twice_adds_one_entry_per_event() {
        let p = tmp("twice");
        assert_eq!(install(&p).unwrap(), Outcome::Added);
        assert_eq!(install(&p).unwrap(), Outcome::AlreadyPresent);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        for (event, matcher, suffix, _verb) in ENTRIES {
            let list = v["hooks"][event].as_array().unwrap();
            assert_eq!(list.len(), 1, "{event} was duplicated");
            assert_eq!(list[0]["matcher"], *matcher);
            assert_eq!(list[0]["hooks"][0]["command"], command(suffix));
        }
    }

    #[test]
    fn re_installing_over_a_pretooluse_only_file_adds_the_sessionstart_entry() {
        let p = tmp("upgrade");
        std::fs::write(
            &p,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"annotated-tree bash-annotator --rewrite-tool-call"}]}]}}"#,
        )
        .unwrap();
        assert_eq!(install(&p).unwrap(), Outcome::Added);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn an_entry_under_an_event_that_runs_a_different_verb_is_replaced() {
        let p = tmp("stale");
        std::fs::write(
            &p,
            r#"{"hooks":{"SessionStart":[{"matcher":"startup","hooks":[{"type":"command","command":"annotated-tree bash-annotator --rewrite-tool-call"}]}]}}"#,
        )
        .unwrap();
        assert_eq!(install(&p).unwrap(), Outcome::Replaced);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let list = v["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(list.len(), 1, "the stale entry was left beside the new one");
        assert_eq!(
            list[0]["hooks"][0]["command"],
            command("bash-annotator --session-announcement")
        );
    }

    #[test]
    fn the_tracked_settings_example_is_what_install_writes() {
        // Generated, not authored: regenerate with `--install-claude-hook .claude/settings.json`.
        let p = tmp("example");
        install(&p).unwrap();
        let tracked = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".claude")
            .join("settings.json");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            std::fs::read_to_string(&tracked).unwrap(),
            "the tracked example has drifted from what install writes"
        );
    }

    #[test]
    fn both_entries_round_trip() {
        let p = tmp("roundtrip");
        std::fs::write(&p, "{\n  \"permissions\": {}\n}\n").unwrap();
        assert_eq!(install(&p).unwrap(), Outcome::Added);
        assert_eq!(uninstall(&p).unwrap(), Outcome::Removed);
        assert_eq!(uninstall(&p).unwrap(), Outcome::NotPresent);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "{\n  \"permissions\": {}\n}\n"
        );
    }

    #[test]
    fn every_other_key_survives_both_directions() {
        let p = tmp("keys");
        std::fs::write(
            &p,
            r#"{"permissions":{"allow":["Bash(ls:*)"]},"hooks":{"PreToolUse":[{"matcher":"Read","hooks":[{"type":"command","command":"other"}]}]}}"#,
        )
        .unwrap();
        install(&p).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls:*)");
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);

        assert_eq!(uninstall(&p).unwrap(), Outcome::Removed);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls:*)");
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Read");
        assert!(v["hooks"].get("SessionStart").is_none());
    }

    #[test]
    fn uninstall_restores_the_shape_install_found() {
        let p = tmp("shape");
        std::fs::write(&p, r#"{"permissions":{"allow":[]}}"#).unwrap();
        install(&p).unwrap();
        uninstall(&p).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(
            v.get("hooks").is_none(),
            "empty containers were left behind"
        );
        assert!(v.get("permissions").is_some());
    }

    #[test]
    fn an_absolute_command_path_is_still_recognised_as_ours() {
        let p = tmp("abs");
        std::fs::write(
            &p,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/home/x/.cargo/bin/annotated-tree bash-annotator --rewrite-tool-call"}]}]}}"#,
        )
        .unwrap();
        // The SessionStart half is still added, but the recognised PreToolUse entry is not duplicated — and uninstall takes the absolute-path spelling away with it.
        assert_eq!(install(&p).unwrap(), Outcome::Added);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(uninstall(&p).unwrap(), Outcome::Removed);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(v.get("hooks").is_none());
    }

    #[test]
    fn a_malformed_file_is_refused_rather_than_replaced() {
        let p = tmp("malformed");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(install(&p).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json");
    }

    #[test]
    fn uninstalling_what_was_never_installed_changes_nothing() {
        let p = tmp("absent");
        std::fs::write(&p, "{\n  \"permissions\": {}\n}\n").unwrap();
        assert_eq!(uninstall(&p).unwrap(), Outcome::NotPresent);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "{\n  \"permissions\": {}\n}\n"
        );
    }

    #[test]
    fn a_missing_file_is_created_with_only_our_entries() {
        let p = tmp("fresh");
        assert_eq!(install(&p).unwrap(), Outcome::Added);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Bash");
        assert!(v["hooks"]["SessionStart"][0]["matcher"]
            .as_str()
            .is_some_and(|m| m.contains("compact")));
        assert_eq!(
            v["hooks"]["PostToolUse"][0]["matcher"],
            "Write|Edit|MultiEdit|NotebookEdit"
        );
        assert_eq!(
            v["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "annotated-tree edit-hook --claude"
        );
        assert_eq!(v.as_object().unwrap().len(), 1);
        assert_eq!(v["hooks"].as_object().unwrap().len(), 3);
    }

    #[test]
    fn edit_hook_is_matched_by_its_subcommand_not_the_claude_flag() {
        let p = tmp("edit-hook-flag-only");
        // Flag-only matching would wrongly claim, then remove, someone else's entry.
        std::fs::write(
            &p,
            r#"{"hooks":{"PostToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"annotated-tree some-other-verb --claude"}]}]}}"#,
        )
        .unwrap();
        assert_eq!(uninstall(&p).unwrap(), Outcome::NotPresent);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
    }
}
