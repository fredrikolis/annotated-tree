// Concern: freezes the hook wire contract — what each entry writes to stdout and the exit code it reads | Non-concern: eligibility, or the annotated output | IO: (hook event | argv) -> stdout + code

use std::io::Write;
use std::process::{Command, Stdio};

fn hook(event: &str) -> (String, i32) {
    hook_via("--rewrite-tool-call", event)
}

fn hook_via(verb: &str, event: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_annotated-tree")).args(["bash-annotator", verb]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("spawn injector");
    child.stdin.take().expect("stdin").write_all(event.as_bytes()).expect("write event");
    let out = child.wait_with_output().expect("wait");
    (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code().unwrap_or(-1))
}

fn json(out: &str) -> serde_json::Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("not JSON: {e}\n{out}"))
}

/// Annotator's own path is machine-dependent, not frozen. Frozen: pipeline verbatim, annotator
/// appended, original last-stage status re-raised.
fn wraps(command: &serde_json::Value, pipeline: &str) -> bool {
    let Some(c) = command.as_str() else {
        return false;
    };
    c.starts_with(&format!("( exec 4>&1; __rc=$( {{ {{ __s=0; {pipeline} || __s=$?; "))
        && c.contains("bash-annotator --annotate-tool-output")
        // Exit code carried out on fd 3; equivalence.rs checks the CODE itself.
        && c.contains("echo $__s >&3")
        && c.contains("exit ${__rc:-1} )")
}

#[test]
fn a_rewrite_is_returned_in_the_shape_the_harness_reads() {
    let (out, code) = hook(r#"{"tool_name":"Bash","tool_input":{"command":"ls src"}}"#);
    assert_eq!(code, 0);
    let v = json(&out);
    let hook_out = &v["hookSpecificOutput"];
    assert_eq!(hook_out["hookEventName"], "PreToolUse");
    assert!(wraps(&hook_out["updatedInput"]["command"], "ls src"), "unexpected rewrite: {hook_out}");
    // No additionalContext: PreToolUse fires per call; SessionStart says it once.
    assert!(hook_out.get("additionalContext").is_none(), "the per-call explanation is back: {hook_out}");
}

/// Stdout reaches the agent verbatim. Frozen: bare text is said, not an envelope; not the wording.
#[test]
fn the_session_start_verb_prints_the_announcement_as_bare_text() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_annotated-tree")).args(["bash-annotator", "--session-announcement"]).output().expect("run the announcement verb");
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.trim().is_empty(), "no announcement");
    assert!(!text.trim_start().starts_with('{'), "a JSON envelope would be injected literally: {text}");
    // The agent must recognise contracts it didn't ask for, not debug its tools.
    assert!(text.contains("Concern:"), "{text}");
    assert!(text.contains("grep"), "{text}");
}

#[test]
fn the_rewriter_answers_no_session_start_event() {
    // `--rewrite-tool-call` answers PreToolUse only; SessionStart gets none.
    let (out, code) = hook(r#"{"hook_event_name":"SessionStart","session_id":"abc","source":"startup","cwd":"/tmp"}"#);
    assert_eq!(code, 0);
    assert_eq!(out, "", "the rewriter still answers SessionStart");
}

#[test]
fn no_permission_decision_is_ever_emitted() {
    // `allow` skips checks for the WHOLE command, waving `rm -rf` through — omit it.
    let (out, _) = hook(r#"{"tool_name":"Bash","tool_input":{"command":"ls . && rm -rf /tmp/x"}}"#);
    let v = json(&out);
    assert!(v["hookSpecificOutput"].get("permissionDecision").is_none(), "a permission decision leaked into the hook output: {out}");
    let cmd = &v["hookSpecificOutput"]["updatedInput"]["command"];
    assert!(wraps(cmd, "ls ."), "unexpected rewrite: {v}");
    assert!(cmd.as_str().is_some_and(|c| c.ends_with(") && rm -rf /tmp/x")), "the rest of the command was altered: {v}");
}

#[test]
fn keys_the_caller_set_are_carried_through_untouched() {
    // Replacing the object would drop other keys.
    let (out, _) = hook(r#"{"tool_name":"Bash","tool_input":{"command":"ls src","description":"look","timeout":5}}"#);
    let updated = &json(&out)["hookSpecificOutput"]["updatedInput"];
    assert_eq!(updated["description"], "look");
    assert_eq!(updated["timeout"], 5);
}

#[test]
fn silence_means_no_opinion() {
    // NO output leaves the call untouched.
    for event in [r#"{"tool_name":"Bash","tool_input":{"command":"cat file.txt"}}"#, r#"{"tool_name":"Bash","tool_input":{"command":"ls > out.txt"}}"#, r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}"#, r#"{"hook_event_name":"SessionEnd","reason":"clear"}"#] {
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
    // The em-dash case that used to panic; this repo's annotations are full of them.
    let (out, code) = hook(r#"{"tool_name":"Bash","tool_input":{"command":"grep -rn \"— the\" src"}}"#);
    assert_eq!(code, 0);
    assert!(wraps(&json(&out)["hookSpecificOutput"]["updatedInput"]["command"], "grep -rn \"\u{2014} the\" src"), "non-ASCII command was not handled: {out}");
}

/// Field for field what Codex 0.162 sends for its `exec_command` tool, which hooks see as `Bash`.
const CODEX_EVENT: &str = r#"{"session_id":"s","turn_id":"t","transcript_path":null,"cwd":"/tmp","hook_event_name":"PreToolUse","model":"gpt-5","permission_mode":"default","tool_name":"Bash","tool_input":{"command":"ls src"},"tool_use_id":"call_1"}"#;

#[test]
fn a_codex_rewrite_carries_the_allow_codex_requires_beside_it() {
    let (out, code) = hook_via("--rewrite-codex-tool-call", CODEX_EVENT);
    assert_eq!(code, 0);
    let hook_out = &json(&out)["hookSpecificOutput"];
    assert_eq!(hook_out["hookEventName"], "PreToolUse");
    // Codex discards an updatedInput that arrives without it.
    assert_eq!(hook_out["permissionDecision"], "allow");
    assert!(wraps(&hook_out["updatedInput"]["command"], "ls src"), "unexpected rewrite: {hook_out}");
}

#[test]
fn the_codex_rewriter_is_silent_where_the_claude_one_is() {
    for event in [CODEX_EVENT.replace("ls src", "cat file.txt"), CODEX_EVENT.replace("ls src", "ls > out.txt"), CODEX_EVENT.replace("\"Bash\"", "\"apply_patch\""), "not json".to_string()] {
        let (out, code) = hook_via("--rewrite-codex-tool-call", &event);
        assert_eq!(out, "", "expected silence for {event}");
        assert_eq!(code, 0);
    }
}
