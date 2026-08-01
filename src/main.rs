// Concern: parses argv and runs the tool, translating errors to a nonzero exit | Non-concern: any logic | IO: (argv) -> process exit

use std::io::{self, Write};
use std::process::ExitCode;

use annotated_tree::exit;

fn main() -> ExitCode {
    let cli = annotated_tree::parse_cli();
    // Wrap `io::stdout()` (which locks per write) rather than holding a persistent
    // `stdout.lock()` guard across the whole run: the run writes through the BufWriter,
    // so the lock is taken once per flushed block, not once per formatted fragment —
    // and nothing else in the process needs stdout, so a held guard would buy nothing.
    let mut handle = io::BufWriter::new(io::stdout());
    let mut errout = io::stderr();

    match annotated_tree::run(&cli, &mut handle, &mut errout) {
        Ok(code) => {
            let _ = handle.flush();
            ExitCode::from(code as u8)
        }
        // Any error out of `run()` is a precondition/environment failure (missing root
        // dir, git/`--since` failure, bad config, I/O). Exit 2 arrives two ways — clap
        // emits it itself before `run()` for a bad flag or value, and `toolcall-injector`
        // returns it as `Ok(2)` for an invocation it cannot act on — and a runaway-scope
        // abort is exit 3, also returned as `Ok`. Agents branch recovery on this code.
        Err(err) => {
            let _ = handle.flush();
            let _ = writeln!(errout, "error: {err:#}");
            ExitCode::from(exit::PRECONDITION as u8)
        }
    }
}
