// Concern: parses argv and runs the tool, translating errors to a nonzero exit | Non-concern: any logic | IO: (argv) -> process exit

use std::io::{self, Write};
use std::process::ExitCode;

use annotated_tree::exit;

fn main() -> ExitCode {
    let cli = annotated_tree::parse_cli();
    // Wrap `io::stdout()` (which locks per write) rather than holding a persistent
    // `stdout.lock()` guard across the whole run: `--mcp` hands stdout to an rmcp
    // stdio server whose tokio writer locks std stdout from a blocking-pool thread,
    // which would deadlock against a guard held on this thread. BufWriter still
    // batches, so normal runs pay no extra locking.
    let mut handle = io::BufWriter::new(io::stdout());
    let mut errout = io::stderr();

    match annotated_tree::run(&cli, &mut handle, &mut errout) {
        Ok(code) => {
            let _ = handle.flush();
            ExitCode::from(code as u8)
        }
        // Any error out of `run()` is a precondition/environment failure (missing root
        // dir, git/`--since` failure, bad config, I/O) — distinct from a clap usage error
        // (exit 2, which clap emits itself before `run()`) and from a runaway-scope abort
        // (exit 3, returned as `Ok`). Agents branch recovery on this code.
        Err(err) => {
            let _ = handle.flush();
            let _ = writeln!(errout, "error: {err:#}");
            ExitCode::from(exit::PRECONDITION as u8)
        }
    }
}
