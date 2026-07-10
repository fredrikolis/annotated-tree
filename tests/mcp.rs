// MCP wire-contract test: Drives the built `--mcp` server through one real stdio
// JSON-RPC round-trip (initialize -> tools/list -> tools/call map). Freezes the
// EXTERNAL contract agents consume — the JSON-RPC envelope + the map tool payload —
// at one round-trip. NOT concerned with re-freezing the sync builders: `model`/
// `graph`/`strict` are already frozen by the golden suite; the MCP tools are thin
// adapters over them, so this test freezes only the wire boundary they add.
//
// The round-trip is DETERMINISTIC by construction: it reads line-delimited JSON-RPC
// responses on a drainer thread and waits for the specific ids it expects, bounded
// by a hard timeout, and only signals shutdown (stdin EOF) once every awaited
// response is in hand — so it never races server teardown and a wedged server fails
// fast instead of hanging the suite.
//
// Only compiled under `cargo test --features mcp` (the server does not exist otherwise).
#![cfg(feature = "mcp")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Hard cap on the whole exchange. A hung, wedged, or crashed server FAILS the test
/// fast instead of blocking the suite forever — Fail-Fast over an unbounded IPC read.
/// Generous vs. the sub-second real round-trip, so only a genuine hang trips it.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Spawn the feature-built binary as an MCP server over a fixture dir, feed it a
/// scripted session on stdin, and return the JSON-RPC responses carrying `await_ids`.
///
/// The round-trip is decoupled from process shutdown (the source of flakiness): a
/// drainer thread reads newline-delimited responses so a full stdout pipe can never
/// deadlock the writer, the main thread collects until every `await_ids` response
/// arrives (bounded by `TIMEOUT`, so a missing/late reply fails fast), and only THEN
/// is stdin closed to signal shutdown and the child reaped. `CARGO_BIN_EXE_*` points
/// at the binary cargo built with THIS test run's features — so it is the real `mcp`
/// binary.
fn round_trip(fixture: &PathBuf, requests: &str, await_ids: &[i64]) -> Vec<Value> {
    let bin = env!("CARGO_BIN_EXE_annotated-tree");
    let mut child = Command::new(bin)
        .arg("--mcp")
        .arg(fixture)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp server");

    // Hold stdin OPEN across the exchange: closing it signals shutdown, so we defer
    // the close until every awaited response is in hand — the round-trip must never
    // race server teardown against an in-flight tool response.
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(requests.as_bytes())
        .expect("write requests");
    stdin.flush().expect("flush requests");

    // Drain stdout on a dedicated thread so a full pipe can never deadlock the writer,
    // and so the main thread can bound the wait with a timeout. Each JSON-RPC message
    // is exactly one newline-delimited line.
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel::<String>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) if l.trim().is_empty() => continue,
                // Receiver gone (test already has what it needs) -> stop draining.
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                // stdout closed or errored -> nothing more to read.
                Err(_) => break,
            }
        }
    });

    // Collect until EVERY awaited id has a response, bounding the TOTAL wait so a
    // missing or delayed response fails fast rather than hanging the suite.
    let mut messages = Vec::new();
    let mut pending: Vec<i64> = await_ids.to_vec();
    let deadline = Instant::now() + TIMEOUT;
    while !pending.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                let msg: Value =
                    serde_json::from_str(&line).expect("each stdout line is a JSON-RPC message");
                if let Some(id) = msg["id"].as_i64() {
                    pending.retain(|&want| want != id);
                }
                messages.push(msg);
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("timed out after {TIMEOUT:?} awaiting JSON-RPC response ids {pending:?}");
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = child.wait();
                panic!("server closed stdout before responding to ids {pending:?}");
            }
        }
    }

    // Contract satisfied. Close stdin (EOF) to let the server exit, then reap it —
    // bounded, killing a server that overstays — so no zombie and no hang. The reader
    // thread ends when stdout hits EOF at exit.
    drop(stdin);
    reap(&mut child);
    let _ = reader.join();
    messages
}

/// Wait for the child to exit, but never forever: poll to a deadline, then kill and
/// wait. Guarantees the server process is reaped even if it ignores stdin EOF.
fn reap(child: &mut Child) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return,
        }
    }
}

fn by_id(messages: &[Value], id: i64) -> &Value {
    messages
        .iter()
        .find(|m| m["id"] == json!(id))
        .unwrap_or_else(|| panic!("no response with id {id}"))
}

#[test]
fn map_tool_round_trip_returns_versioned_map() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("mcp_fixture");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mk fixture");
    std::fs::write(
        dir.join("widget.py"),
        "# Widget: a fixture module. | I/O: () -> ()\n",
    )
    .expect("write fixture file");

    let session = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"map","arguments":{"path":"__DIR__"}}}"#,
        "\n",
    )
    .replace("__DIR__", &dir.to_string_lossy().replace('\\', "\\\\"));

    // Await all three request ids (the map call, id 3, is the slowest — it runs a
    // filesystem walk on the blocking pool), so the collection is order-independent.
    let messages = round_trip(&dir, &session, &[1, 2, 3]);

    // initialize: server advertises the tools capability.
    let init = by_id(&messages, 1);
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "server advertises tools capability"
    );

    // tools/list: the four adapter tools are exposed.
    let tools = by_id(&messages, 2)["result"]["tools"]
        .as_array()
        .expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in ["map", "dependencies", "dependents", "strict_check"] {
        assert!(names.contains(&expected), "tool `{expected}` listed");
    }

    // tools/call map: not an error, and the text payload is the schema-1 JSON map
    // carrying our fixture file — the wire contract external agents parse.
    let call = by_id(&messages, 3);
    assert_ne!(
        call["result"]["isError"],
        json!(true),
        "map call is not an error"
    );
    let text = call["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let map: Value = serde_json::from_str(text).expect("map payload is JSON");
    assert_eq!(map["schema"], json!(1), "schema version 1");
    assert!(
        text.contains("widget.py"),
        "map payload names the fixture file, got: {text}"
    );
}
