// Concern: the PreToolUse entry point — read one hook event and emit an updatedInput rewriting a mapped tool call, or emit nothing; and answer --check for one command | Non-concern: deciding what is eligible (inject.rs owns that) | IO: (hook JSON on stdin | --check ARGV) -> hook JSON, or the rewrite as plain text, always exit 0

use std::io::{IsTerminal, Read};

// Declared by path: private to this binary, not part of the published library surface.
#[path = "../lex.rs"]
mod lex;
// Only `shape_of` is used here — this binary decides eligibility and never reads a tool's
// output, so the rest of the map is dead code in it and live code in the annotator.
#[path = "../hookfile.rs"]
mod hookfile;
#[path = "../inject.rs"]
mod inject;
#[allow(dead_code)]
#[path = "../map.rs"]
mod map;

use inject::rewrite;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Turning the hook on and off. `cargo install` has no post-install step and `cargo uninstall`
    // has no pre-remove step -- the only code cargo runs is build.rs, at BUILD time, which would
    // fire on every checkout build and in CI. Editing a user's settings from there would be wrong
    // twice over, so switching the hook on is an explicit command the user runs.
    if let Some(verb @ ("--install-hook" | "--uninstall-hook")) = args.first().map(String::as_str) {
        let path = match args.get(1) {
            Some(p) => std::path::PathBuf::from(p),
            None => match hookfile::default_path() {
                Some(p) => p,
                None => {
                    eprintln!("annotated-toolcall-rewrite: no HOME, so no default settings file.");
                    eprintln!("Pass one: {verb} <path/to/settings.json>");
                    std::process::exit(2);
                }
            },
        };
        let done = if verb == "--install-hook" {
            hookfile::install(&path)
        } else {
            hookfile::uninstall(&path)
        };
        match done {
            Ok(outcome) => {
                println!("{}", outcome.describe(&path));
                return;
            }
            Err(e) => {
                eprintln!("annotated-toolcall-rewrite: {e}");
                std::process::exit(1);
            }
        }
    }

    if args.first().map(String::as_str) == Some("--check") {
        // Offline surface: show what each command would become, substituting nothing.
        for cmd in &args[1..] {
            match rewrite(cmd) {
                Some((out, _)) => println!("{out}"),
                // Distinguish "not eligible" from "cannot find my own annotator": README step 3
                // points users here to confirm the install, and one message for both told them
                // nothing about which had happened.
                None if inject::wrapper_missing() => {
                    println!("(no rewrite) annotated-bash-wrapper not found — see setup step 1")
                }
                None => println!("(unchanged) {cmd}"),
            }
        }
        return;
    }

    if args.first().is_some_and(|a| a == "--help" || a == "-h") || std::io::stdin().is_terminal() {
        // A hook is fed JSON on stdin. Run by hand from a terminal there is nothing to read, so
        // print usage instead of blocking forever on a pipe that will never carry anything.
        println!("annotated-toolcall-rewrite — a Claude Code PreToolUse hook.");
        println!();
        println!("Reads one hook event on stdin. When a Bash command runs a mapped tool in a");
        println!("stage whose stdout reaches the model, emits an updatedInput routing it through");
        println!("annotated-bash-wrapper. Otherwise emits nothing. Always exits 0.");
        println!();
        println!("usage: annotated-toolcall-rewrite                    (as a hook, JSON on stdin)");
        println!("       annotated-toolcall-rewrite --check CMD…       (what CMD would become)");
        println!("       annotated-toolcall-rewrite --install-hook [F] (switch it on)");
        println!("       annotated-toolcall-rewrite --uninstall-hook [F] (switch it off)");
        println!();
        println!("--install-hook merges one PreToolUse entry into F, keeping every other key —");
        println!(
            "your accepted permissions live in that file. Default F is ~/.claude/settings.json"
        );
        println!("(all projects); pass .claude/settings.local.json for one repo. Running it twice");
        println!("changes nothing, and --uninstall-hook removes only the entry it added.");
        return;
    }

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    if payload.get("tool_name").and_then(|v| v.as_str()) != Some("Bash") {
        return;
    }
    let Some(input) = payload.get("tool_input").and_then(|v| v.as_object()) else {
        return;
    };
    let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let Some((rewritten, swapped)) = rewrite(command) else {
        return;
    };

    let mut updated = input.clone();
    updated.insert("command".into(), serde_json::Value::String(rewritten));
    // No `permissionDecision`: "allow" would SKIP PERMISSION CHECKS for the whole Bash command,
    // including anything `&&`-joined to the eligible stage. `updatedInput` needs no decision.
    println!(
        "{}",
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "updatedInput": serde_json::Value::Object(updated),
                "additionalContext": format!(
                    "`{}` runs exactly as written; its output is piped through \
                     `annotated-bash-wrapper`, which appends each file's Concern/Non-concern/IO \
                     contract to the line that file's path appeared on.",
                    swapped.join("`, `")
                ),
            }
        })
    );
    // Always 0. A PreToolUse hook's exit 2 BLOCKS the tool call; other nonzero codes surface as
    // errors. Neither is an acceptable way to say "nothing to do here".
}
