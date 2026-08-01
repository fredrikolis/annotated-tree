// Concern: adds or removes the PreToolUse entry in a settings file, keeping every other key | Non-concern: what to rewrite, or the hook event format | IO: (settings path) -> edited file, outcome

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// The command written into the settings file. A bare name, resolved from `PATH` by the harness,
/// so the entry stays valid when the binary is reinstalled somewhere else.
const HOOK_COMMAND: &str = "annotated-toolcall-rewrite";

/// What a call did. Distinguishing "already there" from "added" is what makes running this twice
/// safe to suggest in a README.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Added,
    AlreadyPresent,
    Removed,
    NotPresent,
}

impl Outcome {
    pub fn describe(&self, path: &Path) -> String {
        let p = path.display();
        match self {
            Outcome::Added => format!("hook installed in {p}"),
            Outcome::AlreadyPresent => format!("hook already present in {p}; nothing changed"),
            Outcome::Removed => format!("hook removed from {p}"),
            Outcome::NotPresent => format!("no hook of ours in {p}; nothing changed"),
        }
    }
}

/// Where the hook goes when no path is given: the user-level settings Claude Code reads for every
/// project. A caller who wants one repo only passes `.claude/settings.local.json` instead.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".claude").join("settings.json"))
}

/// Is this one of OUR entries? Matched on the command's final path segment, so an entry written
/// as an absolute path is still recognised as ours and is not duplicated or orphaned.
fn is_ours(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.rsplit('/').next() == Some(HOOK_COMMAND))
            })
        })
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

/// Add the PreToolUse entry, keeping every other key. Running it again changes nothing.
pub fn install(path: &Path) -> Result<Outcome, String> {
    let mut root = read(path)?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("`hooks` in {} is not an object", path.display()))?;
    let list = hooks
        .entry("PreToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("`hooks.PreToolUse` in {} is not an array", path.display()))?;

    if list.iter().any(is_ours) {
        return Ok(Outcome::AlreadyPresent);
    }
    list.push(json!({
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": HOOK_COMMAND }],
    }));
    write(path, &root)?;
    Ok(Outcome::Added)
}

/// Remove our entry and nothing else, pruning the containers we would have created so the file
/// returns to the shape it had before `install`.
pub fn uninstall(path: &Path) -> Result<Outcome, String> {
    let mut root = read(path)?;
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(Outcome::NotPresent);
    };
    let Some(list) = hooks.get_mut("PreToolUse").and_then(Value::as_array_mut) else {
        return Ok(Outcome::NotPresent);
    };
    let before = list.len();
    list.retain(|e| !is_ours(e));
    if list.len() == before {
        return Ok(Outcome::NotPresent);
    }
    if list.is_empty() {
        hooks.remove("PreToolUse");
    }
    if hooks.is_empty() {
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
    fn installing_twice_adds_one_entry() {
        let p = tmp("twice");
        assert_eq!(install(&p).unwrap(), Outcome::Added);
        assert_eq!(install(&p).unwrap(), Outcome::AlreadyPresent);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn every_other_key_survives_both_directions() {
        // The file holds accepted permissions. Losing them is the failure this guards.
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
        // Someone else's PreToolUse entry is not ours to remove.
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Read");
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
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/home/x/.cargo/bin/annotated-toolcall-rewrite"}]}]}}"#,
        )
        .unwrap();
        assert_eq!(install(&p).unwrap(), Outcome::AlreadyPresent);
        assert_eq!(uninstall(&p).unwrap(), Outcome::Removed);
    }

    #[test]
    fn a_malformed_file_is_refused_rather_than_replaced() {
        let p = tmp("malformed");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(install(&p).is_err());
        // Untouched: the user's file is still exactly what it was.
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
    fn a_missing_file_is_created_with_only_our_entry() {
        let p = tmp("fresh");
        assert_eq!(install(&p).unwrap(), Outcome::Added);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Bash");
        assert_eq!(v.as_object().unwrap().len(), 1);
    }
}
