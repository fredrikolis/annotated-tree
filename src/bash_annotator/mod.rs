// Concern: routes each bash-annotator verb to its module, and owns the hook wire format | Non-concern: eligibility, or what an annotation means | IO: (args, stdin) -> output + exit code

//! What gets rewritten, and why it is safe to leave on.
//!
//! Three tools are eligible, and each names the file a line is about in its own place:
//!
//! | the agent types | where that tool names the file a line is about |
//! |---|---|
//! | `ls` | the line, or its trailing field |
//! | `find` | the line |
//! | `grep` | the `path:` prefix (anything after it is file content, not a subject) |
//!
//! Everything else is left exactly as written. `map.rs` is that table, and its own doc states the
//! extension procedure and the never-substituted guarantee behind it, including why `\grep`,
//! `command grep`, `/usr/bin/grep` and `sudo grep` all need no special handling. What it does not
//! buy is the tool's flag grammar: `ls` and `grep` each have a table in `run.rs` naming the options
//! that take a value, which is what tells an operand from a flag's argument, and a new tool starts
//! with an empty one and needs its own if its flags matter.
//!
//! **One line in, one line out.** A contract is appended to the line its path appeared on and
//! never printed on a line of its own, so no downstream stage sees a line count it did not expect.
//! The annotator is the LAST stage, so `| sort`, `| uniq` and `| grep -v` see the tool's raw
//! output and behave exactly as they would without this installed.
//!
//! **It refuses anything it cannot guarantee.** A command is left alone whenever the lines
//! reaching the annotator would no longer be the paths the tool printed. A missed rewrite costs
//! nothing; a wrong one would corrupt a pipeline the agent then has to debug. Each rule and the
//! failure behind it is stated where it is enforced, in `inject.rs`: `LINE_SAFE` and `line_safe`
//! for what may follow the producer, `ORDER_PRESERVING` and `header_scoped` for the `<dir>:`
//! headers `ls -R` prints, `nul_delimited` for NUL-delimited output, and `rewrite` itself for
//! redirects, compound commands and `xargs`.
//!
//! It emits no permission decision of its own, so nothing is ever waved through that would not
//! have been, and it never blocks a command. Two things it does change. `PreToolUse` runs before
//! permissions are evaluated, so the string a Bash allowlist is matched against is the rewritten
//! one, which begins `( grep …` and names the annotator by absolute path; a `Bash(grep:*)` rule
//! stops matching. And the pipeline is wrapped in a subshell, so `${PIPESTATUS[i]}` inspected
//! afterwards would describe that subshell, which is why a command mentioning `PIPESTATUS` is left
//! alone entirely. `$?` is unaffected.
//!
//! **It makes output bigger.** A contract runs about 200 bytes. The bargain is that the agent
//! stops opening files for the rest of the session to find out what they are: the same trade
//! `annotated-tree` makes with the tree, moved onto the agent's own tool calls. That is the
//! mechanism, not a measurement — `README_APPENDIX.md`'s future-work section names first-pass
//! yield as the thing it most wants measured and has not measured.

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

/// The five verbs and their one-line summaries, in the order `--help` lists them. Kept here so the
/// fail-fast usage message below cannot drift from what actually parses.
const VERBS: &[(&str, &str)] = &[
    (
        "--install-claude-hook [FILE]",
        "switch the hook on [default: ~/.claude/settings.json]",
    ),
    (
        "--uninstall-claude-hook [FILE]",
        "remove the entry this tool added, and nothing else",
    ),
    (
        "--check CMD…",
        "print what each quoted CMD would become; runs nothing",
    ),
    (
        "--rewrite-tool-call",
        "hook entry point: one PreToolUse or SessionStart event on stdin",
    ),
    (
        "--annotate-tool-output CMD…",
        "pipeline entry point: annotate the producer's output",
    ),
];

/// Which of the five mode flags are set, spelled as the user would type them.
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

/// Run one `bash-annotator` verb. Exactly one of the five mode flags must be set.
///
/// Every path returns a code the [`exit`] taxonomy already names: [`exit::SUCCESS`] on success,
/// [`exit::USAGE`] from the mode check, and [`exit::PRECONDITION`] from a failed settings write.
pub(crate) fn dispatch(
    args: &BashAnnotator,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<i32> {
    // `trailing_var_arg` is what lets `--annotate-tool-output grep -rn --color=never foo .` reach
    // us with the producer's flags intact, and its one cost is paid here: clap no longer rejects a
    // mistyped flag, it swallows it into `args`. Falling through would make `--install-claud-hook`
    // indistinguishable from a successful run — no output, exit 0 — which is exactly what the
    // hand-rolled guard in the retired binary existed to prevent.
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
    annotate_tool_output(args, out, err)
}

/// Turning the hook on and off.
///
/// `cargo install` has no post-install step and `cargo uninstall` has no pre-remove step -- the
/// only code cargo runs is build.rs, at BUILD time, which would fire on every checkout build and
/// in CI. Editing a user's settings from there would be wrong twice over, so switching the hook on
/// is an explicit command the user runs.
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
        // A settings file that cannot be read, parsed or replaced is a precondition failure, and
        // takes the code the taxonomy already has for one. Never 1: on this binary that is
        // `--strict-check found at least one violation`, which `--help` advertises.
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

/// Said once, at the start of a session, instead of on every rewritten call.
///
/// The reader is an agent that is about to see `# Concern: …` trailing lines it did not ask for.
/// The whole job of this text is that it recognises them and does not go debugging its own tools;
/// everything else it might want to know is in `--help` and the README.
const SESSION_ANNOUNCEMENT: &str = concat!(
    "ls, find and grep output in this session carries each file's contract:\n",
    "  src/render/text.rs  # Concern: … | Non-concern: … | IO: …\n",
    "Only output returned directly to your context is annotated (i.e. `ls -la > out.txt` is ",
    "unaffected).",
);

/// The Claude Code hook entry point: one hook event on stdin, one JSON object out, or nothing.
///
/// Two events are answered. `SessionStart` gets the announcement above — ONCE, which is why no
/// rewritten call carries one; `PreToolUse` gets an `updatedInput` when it names an eligible Bash
/// command. Anything else is not ours and gets silence.
///
/// ALWAYS [`exit::SUCCESS`]. A PreToolUse hook's exit 2 BLOCKS the tool call; other nonzero codes
/// surface as errors. Neither is an acceptable way to say "nothing to do here".
fn rewrite_tool_call(out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    // A hook is fed JSON on stdin. Run by hand from a terminal there is nothing to read, so say so
    // instead of blocking forever on a pipe that will never carry anything.
    if std::io::stdin().is_terminal() {
        writeln!(
            err,
            "annotated-tree bash-annotator --rewrite-tool-call: reads one Claude Code \
             PreToolUse or SessionStart event on stdin; a terminal will never send one."
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
    // Claude Code stamps the event name on every payload. Matched before `tool_name` because a
    // SessionStart event carries no tool at all.
    if payload.get("hook_event_name").and_then(|v| v.as_str()) == Some("SessionStart") {
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": SESSION_ANNOUNCEMENT,
                }
            })
        )?;
        return Ok(exit::SUCCESS);
    }
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
    // No `permissionDecision`: "allow" would SKIP PERMISSION CHECKS for the whole Bash command,
    // including anything `&&`-joined to the eligible stage. `updatedInput` needs no decision.
    //
    // And no `additionalContext`. PreToolUse fires BEFORE the command runs, so an explanation
    // attached here is repeated on every eligible call and is often about contracts that never
    // appear — a `grep` over unannotated files would carry the whole speech and annotate nothing.
    // SESSION_ANNOUNCEMENT says it once instead.
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
///
/// The tool is NOT run here — the shell already ran it, which is what makes the session's own
/// `grep` and `find` the engines that answer. The argv is passed so the flags that decide how a
/// line names its file are read rather than guessed.
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
    // `annotate` returns a hardcoded 0, never the producer's status. That status is re-raised by
    // the shell snippet `inject` emits, which runs in the agent's own shell — which is how
    // `grep`'s exit 1 on no-match survives a rewrite.
    let _ = run::annotate(&args.args, &mut input, out);
    Ok(exit::SUCCESS)
}
