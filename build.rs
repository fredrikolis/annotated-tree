// Concern: points a Claude Code user at the bash-annotator hook | Non-concern: installing it, or any other build step | IO: (HOME, Claude settings) -> at most one cargo warning

use std::path::PathBuf;

/// Fires on every build; warns only if Claude Code is present and the hook isn't on.
fn main() {
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

    let on_path = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("claude").is_file()));
    if !claude_dir.is_dir() && !on_path {
        return;
    }

    // Substring match, not JSON parse: a build script shouldn't take a JSON dependency.
    let installed = std::fs::read_to_string(claude_dir.join("settings.json"))
        .is_ok_and(|s| s.contains("--rewrite-tool-call") && s.contains("--session-announcement"));
    if installed {
        return;
    }

    println!(
        "cargo:warning=Run `annotated-tree bash-annotator --install-claude-hook` to let Claude \
         read each file's annotation in the results of its own grep, find and ls calls."
    );
}
