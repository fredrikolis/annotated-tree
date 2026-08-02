// Concern: routes each bash-annotator verb to its module, and owns the hook wire format | Non-concern: eligibility, or what an annotation means | IO: (args, stdin) -> output + exit code

//! What gets rewritten, and why it is safe to leave on. Only `ls`, `find` and `grep` are eligible
//! (`map.rs` is that table); the annotator is appended as the LAST stage, one contract per line,
//! so downstream stages still see raw output. Refusals are stated where enforced, in `inject.rs`.
//! A Bash allowlist matches the REWRITTEN string, so a `Bash(grep:*)` rule stops matching.

mod contracts;
mod hookfile;
mod inject;
mod lex;
mod map;
mod run;

use std::io::{IsTerminal, Read, Write};

use anyhow::Result;

use crate::cli::BashAnnotator;
use crate::exit;

/// The six verbs and their one-line summaries, in the order `--help` lists them. Kept here so the
/// fail-fast usage message below cannot drift from what actually parses.
const VERBS: &[(&str, &str)] = &[
    (
        "--install-claude-hook [FILE]",
        "switch both hook entries on [default: ~/.claude/settings.json]",
    ),
    (
        "--uninstall-claude-hook [FILE]",
        "remove the entries this tool added, and nothing else",
    ),
    (
        "--check CMD…",
        "print what each quoted CMD would become; runs nothing",
    ),
    (
        "--rewrite-tool-call",
        "hook entry point: one PreToolUse event on stdin",
    ),
    (
        "--session-announcement",
        "hook entry point: print what a SessionStart tells the agent",
    ),
    (
        "--annotate-tool-output CMD…",
        "pipeline entry point: annotate the producer's output",
    ),
];

/// Which of the six mode flags are set, spelled as the user would type them.
fn modes(args: &BashAnnotator) -> Vec<&'static str> {
    let mut set = Vec::new();
    if args.install_claude_hook {
        set.push("--install-claude-hook");
    }
    if args.uninstall_claude_hook {
        set.push("--uninstall-claude-hook");
    }
    if args.check {
        set.push("--check");
    }
    if args.rewrite_tool_call {
        set.push("--rewrite-tool-call");
    }
    if args.session_announcement {
        set.push("--session-announcement");
    }
    if args.annotate_tool_output {
        set.push("--annotate-tool-output");
    }
    set
}

fn usage(err: &mut impl Write) -> Result<()> {
    writeln!(err, "usage: annotated-tree bash-annotator <one of>")?;
    for (flag, summary) in VERBS {
        writeln!(err, "    {flag:<32} {summary}")?;
    }
    Ok(())
}

/// Run one `bash-annotator` verb. Exactly one of the six mode flags must be set.
///
/// Every path returns a code the [`exit`] taxonomy already names: [`exit::SUCCESS`] on success,
/// [`exit::USAGE`] from the mode check, and [`exit::PRECONDITION`] from a failed settings write.
pub(crate) fn dispatch(
    args: &BashAnnotator,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<i32> {
    // `trailing_var_arg` reaches us with the producer's flags intact, and its cost is paid here: clap swallows a mistyped flag into `args` instead of rejecting it, so falling through would make `--install-claud-hook` indistinguishable from success.
    let set = modes(args);
    if set.len() != 1 {
        if set.len() > 1 {
            writeln!(
                err,
                "annotated-tree bash-annotator: {} and {} are exclusive; pass exactly one.",
                set[0], set[1]
            )?;
        } else if let Some(unknown) = args
            .args
            .first()
            .filter(|a| a.to_string_lossy().starts_with('-'))
        {
            writeln!(
                err,
                "annotated-tree bash-annotator: unknown flag {}",
                unknown.to_string_lossy()
            )?;
        } else {
            writeln!(err, "annotated-tree bash-annotator: no verb given.")?;
        }
        usage(err)?;
        return Ok(exit::USAGE);
    }

    if args.install_claude_hook || args.uninstall_claude_hook {
        return hook_file(args, set[0], out, err);
    }
    if args.check {
        return check(args, out);
    }
    if args.rewrite_tool_call {
        return rewrite_tool_call(out, err);
    }
    if args.session_announcement {
        return session_announcement(out);
    }
    annotate_tool_output(args, out, err)
}

/// Turning the hook on and off. `cargo install` has no post-install step and `cargo uninstall` no
/// pre-remove step — the only code cargo runs is build.rs, at BUILD time, which would fire on every
/// checkout build and in CI. Editing a user's settings from there would be wrong twice over, so
/// switching the hook on is an explicit command the user runs.
fn hook_file(
    args: &BashAnnotator,
    verb: &str,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<i32> {
    let path = match args.args.first() {
        Some(p) => std::path::PathBuf::from(p),
        None => match hookfile::default_path() {
            Some(p) => p,
            None => {
                writeln!(
                    err,
                    "annotated-tree bash-annotator: no HOME, so no default settings file."
                )?;
                writeln!(
                    err,
                    "Pass one: annotated-tree bash-annotator {verb} <path/to/settings.json>"
                )?;
                return Ok(exit::USAGE);
            }
        },
    };
    let done = if args.install_claude_hook {
        hookfile::install(&path)
    } else {
        hookfile::uninstall(&path)
    };
    match done {
        Ok(outcome) => {
            writeln!(out, "{}", outcome.describe(&path))?;
            Ok(exit::SUCCESS)
        }
        // A settings file that cannot be read, parsed or replaced is a precondition failure, and takes the code the taxonomy already has for one. Never 1: on this binary that is `--strict-check found at least one violation`, which `--help` advertises.
        Err(e) => {
            writeln!(err, "annotated-tree bash-annotator: {e}")?;
            Ok(exit::PRECONDITION)
        }
    }
}

/// Offline surface: show what each command would become, substituting nothing.
fn check(args: &BashAnnotator, out: &mut impl Write) -> Result<i32> {
    for cmd in &args.args {
        let cmd = cmd.to_string_lossy();
        match inject::rewrite(&cmd) {
            Some((rewritten, _)) => writeln!(out, "{rewritten}")?,
            None => writeln!(out, "(unchanged) {cmd}")?,
        }
    }
    Ok(exit::SUCCESS)
}

/// Said once, at the start of a session, instead of on every rewritten call. The reader is an agent
/// about to see `# Concern: …` trailing lines it did not ask for, and the whole job of this text is
/// that it recognises them and does not go debugging its own tools. Everything else it might want
/// is in `--help` and the README.
const SESSION_ANNOUNCEMENT: &str = concat!(
    "ls, find and grep output in this session carries each file's annotation:\n",
    "  src/render/text.rs  # Concern: … | Non-concern: … | IO: …\n",
    "Only output returned directly to your context is annotated (i.e. `ls -la > out.txt` is ",
    "unaffected).",
);

/// The `SessionStart` hook entry point: print the announcement, exit 0. Claude Code adds a
/// `SessionStart` hook's stdout to the agent's context verbatim, so no JSON envelope is needed and
/// none is emitted — the text a user reads here is exactly the text the agent is handed.
fn session_announcement(out: &mut impl Write) -> Result<i32> {
    writeln!(out, "{SESSION_ANNOUNCEMENT}")?;
    Ok(exit::SUCCESS)
}

/// The `PreToolUse` hook entry point: one hook event on stdin, one JSON object out, or nothing.
/// An `updatedInput` comes back when the event names an eligible Bash command; anything else gets
/// silence. ALWAYS [`exit::SUCCESS`] — a PreToolUse exit 2 BLOCKS the tool call, and other nonzero
/// codes surface as errors.
fn rewrite_tool_call(out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    // A hook is fed JSON on stdin. Run by hand from a terminal there is nothing to read, so say so instead of blocking forever on a pipe that will never carry anything.
    if std::io::stdin().is_terminal() {
        writeln!(
            err,
            "annotated-tree bash-annotator --rewrite-tool-call: reads one Claude Code \
             PreToolUse event on stdin; a terminal will never send one."
        )?;
        return Ok(exit::SUCCESS);
    }

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return Ok(exit::SUCCESS);
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Ok(exit::SUCCESS);
    };
    if payload.get("tool_name").and_then(|v| v.as_str()) != Some("Bash") {
        return Ok(exit::SUCCESS);
    }
    let Some(input) = payload.get("tool_input").and_then(|v| v.as_object()) else {
        return Ok(exit::SUCCESS);
    };
    let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let Some((rewritten, _swapped)) = inject::rewrite(command) else {
        return Ok(exit::SUCCESS);
    };

    let mut updated = input.clone();
    updated.insert("command".into(), serde_json::Value::String(rewritten));
    // No `permissionDecision`: "allow" would SKIP PERMISSION CHECKS for the whole Bash command, including anything `&&`-joined to the eligible stage. No `additionalContext` either — it would repeat on every call, often about contracts that never appear.
    writeln!(
        out,
        "{}",
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "updatedInput": serde_json::Value::Object(updated),
            }
        })
    )?;
    Ok(exit::SUCCESS)
}

/// The pipeline entry point: read the wrapped tool's stdout, append each printed path's contract.
/// The tool is NOT run here — the shell already ran it, which is what makes the session's own `grep`
/// and `find` the engines that answer. The argv is passed so the flags deciding how a line names its
/// file are read rather than guessed.
fn annotate_tool_output(
    args: &BashAnnotator,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<i32> {
    if args.args.is_empty() {
        writeln!(
            err,
            "annotated-tree bash-annotator: --annotate-tool-output needs the producer's argv."
        )?;
        writeln!(
            err,
            "usage: <tool> [args...] | annotated-tree bash-annotator \
             --annotate-tool-output <tool> [args...]"
        )?;
        return Ok(exit::USAGE);
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    // `annotate` returns a hardcoded 0, never the producer's status. That status is re-raised by the shell snippet `inject` emits, which runs in the agent's own shell — which is how `grep`'s exit 1 on no-match survives a rewrite.
    let _ = run::annotate(&args.args, &mut input, out);
    Ok(exit::SUCCESS)
}
