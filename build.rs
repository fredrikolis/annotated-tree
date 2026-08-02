// Concern: points a Claude Code user at the bash-annotator hook | Non-concern: installing it, or any other build step | IO: (HOME, Claude settings) -> at most one cargo warning

use std::path::PathBuf;

/// The only signal cargo gives a package for reaching the person running `cargo install`.
///
/// It is a blunt one: it fires on every build, not only on an install, so it is spent only when
/// BOTH halves hold — the machine runs Claude Code, and the hook is not on yet. A developer who
/// has already set it up, and anyone with no Claude Code at all, never sees it. Nothing here
/// installs, reads a secret, or writes: `--install-claude-hook` remains something a user
/// chooses to run.
fn main() {
    // Re-run when the thing being checked changes, or the warning would linger until a clean build long after the hook was switched on.
    println!("cargo:rerun-if-env-changed=HOME");
    println!("cargo:rerun-if-changed=build.rs");

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let claude_dir = home.join(".claude");
    println!(
        "cargo:rerun-if-changed={}",
        claude_dir.join("settings.json").display()
    );

    // (a) Is this a Claude Code machine at all? No `~/.claude` and no `claude` on PATH means the hook has nothing to attach to, and the line would be noise.
    let on_path = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("claude").is_file()));
    if !claude_dir.is_dir() && !on_path {
        return;
    }

    // (b) Is the hook already on? A plain substring search over the settings file, deliberately dependency-free — a build script is the wrong place to take a JSON dependency, and a false POSITIVE here only costs a line that was never printed.
    let installed = std::fs::read_to_string(claude_dir.join("settings.json"))
        .is_ok_and(|s| s.contains("--rewrite-tool-call"));
    if installed {
        return;
    }

    println!(
        "cargo:warning=Run `annotated-tree bash-annotator --install-claude-hook` to let Claude \
         read each file's contract in the results of its own grep, find and ls calls."
    );
}
