// Concern: parses argv and runs the tool, translating errors to a nonzero exit | Non-concern: any logic | IO: (argv) -> process exit

use std::io::{self, Write};
use std::process::ExitCode;

use annotated_tree::exit;

fn main() -> ExitCode {
    let cli = annotated_tree::parse_cli();
    // Wrapping `io::stdout()` takes the lock once per flushed block rather than once per formatted fragment, and nothing else in the process needs stdout.
    let mut handle = io::BufWriter::new(io::stdout());
    let mut errout = io::stderr();

    match annotated_tree::run(&cli, &mut handle, &mut errout) {
        Ok(code) => {
            let _ = handle.flush();
            ExitCode::from(code as u8)
        }
        // Exit 2 arrives two ways — clap emits it before `run()` for a bad flag, and `bash-annotator` returns `Ok(2)` for an invocation it cannot act on — and a runaway-scope abort is exit 3. Agents branch recovery on this code.
        Err(err) => {
            let _ = handle.flush();
            let _ = writeln!(errout, "error: {err:#}");
            ExitCode::from(exit::PRECONDITION as u8)
        }
    }
}
