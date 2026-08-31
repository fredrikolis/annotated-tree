// Concern: parses argv and runs the tool, translating errors to a nonzero exit | Non-concern: any logic | IO: (argv) -> process exit

use std::io::{self, Write};
use std::process::ExitCode;

use annotated_tree::exit;

fn main() -> ExitCode {
    let cli = annotated_tree::parse_cli();
    let mut handle = io::BufWriter::new(io::stdout());
    let mut errout = io::stderr();

    match annotated_tree::run(&cli, &mut handle, &mut errout) {
        Ok(code) => {
            let _ = handle.flush();
            ExitCode::from(code as u8)
        }
        // Exit 2: clap or bash-annotator Ok(2); exit 3: runaway-scope abort.
        Err(err) => {
            let _ = handle.flush();
            let _ = writeln!(errout, "error: {err:#}");
            ExitCode::from(exit::PRECONDITION as u8)
        }
    }
}
