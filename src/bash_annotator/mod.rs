// Concern: routes each bash-annotator verb to its module, and owns the hook wire format | Non-concern: eligibility, or what an annotation means | IO: (args, stdin) -> output + exit code

//! Annotator runs LAST in the pipeline; a Bash allowlist matches the REWRITTEN string, so `Bash(grep:*)` stops matching.

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

const VERBS: &[(&str, &str)] = &[("--install-claude-hook [FILE]", "switch all three hook entries on [default: ~/.claude/settings.json]"), ("--uninstall-claude-hook [FILE]", "remove the entries this tool added, and nothing else"), ("--install-codex-hook [FILE]", "the same three, for Codex [default: ~/.codex/hooks.json]"), ("--uninstall-codex-hook [FILE]", "remove the Codex entries this tool added, and nothing else"), ("--check CMD…", "print what each quoted CMD would become; runs nothing"), ("--rewrite-tool-call", "hook entry point: one Claude Code PreToolUse event on stdin"), ("--rewrite-codex-tool-call", "hook entry point: one Codex PreToolUse event on stdin"), ("--session-announcement", "hook entry point: print what a SessionStart tells the agent"), ("--annotate-tool-output CMD…", "pipeline entry point: annotate the producer's output")];

fn modes(args: &BashAnnotator) -> Vec<&'static str> {
    let flags = [(args.install_claude_hook, "--install-claude-hook"), (args.uninstall_claude_hook, "--uninstall-claude-hook"), (args.install_codex_hook, "--install-codex-hook"), (args.uninstall_codex_hook, "--uninstall-codex-hook"), (args.check, "--check"), (args.rewrite_tool_call, "--rewrite-tool-call"), (args.rewrite_codex_tool_call, "--rewrite-codex-tool-call"), (args.session_announcement, "--session-announcement"), (args.annotate_tool_output, "--annotate-tool-output")];
    flags.into_iter().filter_map(|(on, flag)| on.then_some(flag)).collect()
}

fn usage(err: &mut impl Write) -> Result<()> {
    writeln!(err, "usage: annotated-tree bash-annotator <one of>")?;
    for (flag, summary) in VERBS {
        writeln!(err, "    {flag:<32} {summary}")?;
    }
    Ok(())
}

pub(crate) fn dispatch(args: &BashAnnotator, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    // trailing_var_arg makes clap swallow a mistyped flag into `args` instead of rejecting it.
    let set = modes(args);
    if set.len() != 1 {
        if set.len() > 1 {
            writeln!(err, "annotated-tree bash-annotator: {} and {} are exclusive; pass exactly one.", set[0], set[1])?;
        } else if let Some(unknown) = args.args.first().filter(|a| a.to_string_lossy().starts_with('-')) {
            writeln!(err, "annotated-tree bash-annotator: unknown flag {}", unknown.to_string_lossy())?;
        } else {
            writeln!(err, "annotated-tree bash-annotator: no verb given.")?;
        }
        usage(err)?;
        return Ok(exit::USAGE);
    }

    if args.install_claude_hook || args.uninstall_claude_hook {
        return hook_file(&hookfile::CLAUDE, args.install_claude_hook, args, set[0], out, err);
    }
    if args.install_codex_hook || args.uninstall_codex_hook {
        return hook_file(&hookfile::CODEX, args.install_codex_hook, args, set[0], out, err);
    }
    if args.check {
        return check(args, out);
    }
    if args.rewrite_tool_call {
        return rewrite_tool_call(None, out, err);
    }
    if args.rewrite_codex_tool_call {
        return rewrite_tool_call(Some("allow"), out, err);
    }
    if args.session_announcement {
        return session_announcement(out);
    }
    annotate_tool_output(args, out, err)
}

/// cargo has no install/uninstall step; the only code it runs is build.rs, at BUILD time.
fn hook_file(harness: &hookfile::Harness, installing: bool, args: &BashAnnotator, verb: &str, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    let path = match args.args.first() {
        Some(p) => std::path::PathBuf::from(p),
        None => match harness.default_path() {
            Some(p) => p,
            None => {
                writeln!(err, "annotated-tree bash-annotator: no {}, so no default hook file.", harness.home_vars)?;
                writeln!(err, "Pass one: annotated-tree bash-annotator {verb} <path/to/hook-file.json>")?;
                return Ok(exit::USAGE);
            }
        },
    };
    let done = if installing { hookfile::install(harness, &path) } else { hookfile::uninstall(harness, &path) };
    match done {
        Ok(outcome) => {
            writeln!(out, "{}", outcome.describe(&path))?;
            if let (Some(step), hookfile::Outcome::Added | hookfile::Outcome::Replaced) = (harness.next_step, &outcome) {
                writeln!(out, "{step}")?;
            }
            Ok(exit::SUCCESS)
        }
        // Never exit 1: here that means "--strict-check found a violation".
        Err(e) => {
            writeln!(err, "annotated-tree bash-annotator: {e}")?;
            Ok(exit::PRECONDITION)
        }
    }
}

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

/// Holds its own annotation line; [`crate::embedded_doc::embedded_body`] drops it.
const SESSION_ANNOUNCEMENT_DOC: &str = include_str!("session-announcement.md");

/// `SessionStart` hook entry point: stdout goes to the agent's context verbatim, no JSON envelope.
fn session_announcement(out: &mut impl Write) -> Result<i32> {
    let text = crate::embedded_doc::embedded_body(SESSION_ANNOUNCEMENT_DOC, "session announcement");
    writeln!(out, "{}", text.trim_end())?;
    Ok(exit::SUCCESS)
}

/// `PreToolUse` hook entry point: JSON in, JSON out or nothing. ALWAYS exit::SUCCESS; exit 2 blocks the tool call.
///
/// `decision` is the harnesses' one wire difference. In Claude Code `allow` skips permission checks
/// for the whole `&&` chain; Codex drops an `updatedInput` sent without it, and still approval-checks the rewrite.
fn rewrite_tool_call(decision: Option<&str>, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    if std::io::stdin().is_terminal() {
        writeln!(err, "annotated-tree bash-annotator: a rewrite verb reads one PreToolUse event on stdin; a terminal will never send one.")?;
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
    let mut hook_out = serde_json::json!({ "hookEventName": "PreToolUse", "updatedInput": serde_json::Value::Object(updated) });
    if let Some(d) = decision {
        hook_out["permissionDecision"] = d.into();
    }
    writeln!(out, "{}", serde_json::json!({ "hookSpecificOutput": hook_out }))?;
    Ok(exit::SUCCESS)
}

/// Tool NOT run here — the shell already ran it; the session's own `grep`/`find` are the engines.
fn annotate_tool_output(args: &BashAnnotator, out: &mut impl Write, err: &mut impl Write) -> Result<i32> {
    if args.args.is_empty() {
        writeln!(err, "annotated-tree bash-annotator: --annotate-tool-output needs the producer's argv.")?;
        writeln!(
            err,
            "usage: <tool> [args...] | annotated-tree bash-annotator \
             --annotate-tool-output <tool> [args...]"
        )?;
        return Ok(exit::USAGE);
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    // `annotate` always returns 0; `inject`'s shell snippet re-raises the producer's real exit status.
    let _ = run::annotate(&args.args, &mut input, out);
    Ok(exit::SUCCESS)
}
