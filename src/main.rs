// Binary entrypoint: Parses argv and runs the tool, translating errors to a nonzero exit. NOT concerned with any logic. | I/O: (argv) -> process exit

use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

use annotated_tree::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
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
        Err(err) => {
            let _ = handle.flush();
            let _ = writeln!(errout, "error: {err:#}");
            ExitCode::from(2)
        }
    }
}
