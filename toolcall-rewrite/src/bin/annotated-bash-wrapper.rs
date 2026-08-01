// Concern: the entry point — hands stdin and the producer's argv to the annotator, and its result to a process exit | Non-concern: any logic | IO: (argv, stdin) -> annotated stdout + exit

use std::ffi::OsString;

// Declared by path so this logic stays a private detail of the binaries rather than growing the
// published `annotated_tree` library surface.
#[path = "../contracts.rs"]
mod contracts;
#[path = "../map.rs"]
mod map;
#[path = "../run.rs"]
mod run;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--help" || a == "-h") {
        println!("annotated-bash-wrapper — append each printed path's contract to the line it appeared on.");
        println!();
        println!("usage: <tool> [args...] | annotated-bash-wrapper <tool> [args...]");
        println!();
        println!("Reads the tool's output on STDIN and writes it back with contracts appended.");
        println!("The tool is NOT run here — the shell already ran it, which is what makes the");
        println!("session's own `grep` and `find` (shell functions, with gitignore filtering) the");
        println!("engines that answer. The argv is passed so the flags that decide how a line");
        println!("names its file are read rather than guessed.");
        println!();
        println!("One line in, one line out: a contract is appended to the line its path appeared");
        println!("on, never printed on a line of its own.");
        return;
    }
    if args.is_empty() {
        eprintln!("annotated-bash-wrapper: no producer argv given.");
        eprintln!("usage: <tool> [args...] | annotated-bash-wrapper <tool> [args...]");
        std::process::exit(2);
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout().lock();
    std::process::exit(run::annotate(&args, &mut input, &mut out));
}
