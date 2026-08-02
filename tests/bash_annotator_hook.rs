// Concern: freezes the hook wire contract — the JSON Claude Code parses and the exit code it reads | Non-concern: eligibility, or the annotated output | IO: (hook JSON) -> asserted stdout + code

use std::io::Write;
use std::process::{Command, Stdio};

/// Feed one hook event to the injector; return its stdout and exit code.
fn hook(event: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_annotated-tree"))
        .args(["bash-annotator", "--rewrite-tool-call"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn injector");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(event.as_bytes())
        .expect("write event");
    let out = child.wait_with_output().expect("wait");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn json(out: &str) -> serde_json::Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("not JSON: {e}\n{out}"))
}

/// Does this rewrite wrap `pipeline` in a subshell that pipes it into the annotator?
///
/// The annotator is `current_exe()` as a single-quoted absolute path, followed by the verb and flag
/// unquoted. The path itself is machine-dependent and not frozen. What IS frozen: the agent's
/// pipeline is reproduced verbatim, the annotator is appended to it, and the status of the
/// originally-last stage is re-raised.
fn wraps(command: &serde_json::Value, pipeline: &str) -> bool {
    let Some(c) = command.as_str() else {
        return false;
    };
    c.starts_with(&format!("( {pipeline} | "))
        && c.contains("bash-annotator --annotate-tool-output")
        // The exit code is recovered from PIPESTATUS and re-raised; the exact expression is not frozen here (bash_annotator_equivalence.rs checks the CODE against the unrewritten command, which is the property that matters), only that the subshell ends by re-raising one.
        && c.contains("PIPESTATUS")
        && c.contains("exit $__rc )")
}

#[test]
fn a_rewrite_is_returned_in_the_shape_the_harness_reads() {
    let (out, code) = hook(r#"{"tool_name":"Bash","tool_input":{"command":"ls src"}}"#);
    assert_eq!(code, 0);
    let v = json(&out);
    let hook_out = &v["hookSpecificOutput"];
    assert_eq!(hook_out["hookEventName"], "PreToolUse");
    assert!(
        wraps(&hook_out["updatedInput"]["command"], "ls src"),
        "unexpected rewrite: {hook_out}"
    );
    // No `additionalContext` here. PreToolUse fires before the command runs, so an explanation attached to a rewrite is repeated on every eligible call and describes contracts that may never be printed. SessionStart carries it once instead.
    assert!(
        hook_out.get("additionalContext").is_none(),
        "the per-call explanation is back: {hook_out}"
    );
}

#[test]
fn a_session_start_event_is_answered_with_the_announcement_once() {
    let (out, code) = hook(
        r#"{"hook_event_name":"SessionStart","session_id":"abc","source":"startup","cwd":"/tmp"}"#,
    );
    assert_eq!(code, 0);
    let hook_out = &json(&out)["hookSpecificOutput"];
    assert_eq!(hook_out["hookEventName"], "SessionStart");
    // The wire contract is that SOMETHING is said, not what. The wording is prose and will keep improving; freezing it here only breaks this test every time it does.
    assert!(
        hook_out["additionalContext"]
            .as_str()
            .is_some_and(|c| !c.is_empty()),
        "no announcement: {out}"
    );
    // No rewrite is proposed for an event that names no tool.
    assert!(hook_out.get("updatedInput").is_none(), "{out}");
}

#[test]
fn a_session_start_event_never_reaches_the_rewriter() {
    // `hook_event_name` is matched first, so a SessionStart payload that also happened to carry a `tool_name` is still answered as a SessionStart.
    let (out, code) = hook(
        r#"{"hook_event_name":"SessionStart","source":"compact","tool_name":"Bash","tool_input":{"command":"ls src"}}"#,
    );
    assert_eq!(code, 0);
    let hook_out = &json(&out)["hookSpecificOutput"];
    assert_eq!(hook_out["hookEventName"], "SessionStart");
    assert!(hook_out.get("updatedInput").is_none(), "{out}");
}

#[test]
fn no_permission_decision_is_ever_emitted() {
    // `permissionDecision: "allow"` means SKIP PERMISSION CHECKS, and it applies to the WHOLE Bash command — so `ls . && rm -rf x`, whose `ls` stage is eligible, would have the destructive half waved through too. `updatedInput` needs no decision, so none is sent.
    let (out, _) = hook(r#"{"tool_name":"Bash","tool_input":{"command":"ls . && rm -rf /tmp/x"}}"#);
    let v = json(&out);
    assert!(
        v["hookSpecificOutput"].get("permissionDecision").is_none(),
        "a permission decision leaked into the hook output: {out}"
    );
    // The eligible stage is still rewritten; only the permission bypass is withheld.
    let cmd = &v["hookSpecificOutput"]["updatedInput"]["command"];
    assert!(wraps(cmd, "ls ."), "unexpected rewrite: {v}");
    // Only the `ls` stage is wrapped; the destructive half is left exactly where it was.
    assert!(
        cmd.as_str()
            .is_some_and(|c| c.ends_with(") && rm -rf /tmp/x")),
        "the rest of the command was altered: {v}"
    );
}

#[test]
fn keys_the_caller_set_are_carried_through_untouched() {
    // `tool_input` may hold more than `command`; replacing the object would drop the rest.
    let (out, _) = hook(
        r#"{"tool_name":"Bash","tool_input":{"command":"ls src","description":"look","timeout":5}}"#,
    );
    let updated = &json(&out)["hookSpecificOutput"]["updatedInput"];
    assert_eq!(updated["description"], "look");
    assert_eq!(updated["timeout"], 5);
}

#[test]
fn silence_means_no_opinion() {
    // Anything not eligible must produce NO output: the harness then leaves the call untouched.
    for event in [
        r#"{"tool_name":"Bash","tool_input":{"command":"cat file.txt"}}"#,
        r#"{"tool_name":"Bash","tool_input":{"command":"ls > out.txt"}}"#,
        r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}"#,
        // Some other event this hook was never installed for.
        r#"{"hook_event_name":"SessionEnd","reason":"clear"}"#,
    ] {
        let (out, code) = hook(event);
        assert_eq!(out, "", "expected silence for {event}");
        assert_eq!(code, 0);
    }
}

#[test]
fn malformed_input_exits_zero_and_says_nothing() {
    for event in ["not json at all", "", "{}", r#"{"tool_name":"Bash"}"#] {
        let (out, code) = hook(event);
        assert_eq!(code, 0, "exit 2 blocks the tool call and other nonzero codes surface as errors: {event:?}");
        assert_eq!(out, "", "expected silence for {event:?}");
    }
}

#[test]
fn a_command_carrying_non_ascii_still_gets_a_hook_response() {
    // The em-dash case that used to panic. This repo's own annotations are full of them.
    let (out, code) =
        hook(r#"{"tool_name":"Bash","tool_input":{"command":"grep -rn \"— the\" src"}}"#);
    assert_eq!(code, 0);
    assert!(
        wraps(
            &json(&out)["hookSpecificOutput"]["updatedInput"]["command"],
            "grep -rn \"\u{2014} the\" src"
        ),
        "non-ASCII command was not handled: {out}"
    );
}
