// Concern: asserts a rewrite's bytes and exit code match the original command's, bar appended contracts | Non-concern: eligibility, or which file a line names | IO: (commands, tree) -> asserted equality

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Each entry runs as written and rewritten; the two must agree byte for byte. Catches
/// structural damage, not a wrong-file contract — `bash_annotator_run.rs` owns that.
const COMMANDS: &[&str] = &[
    "ls",
    "ls -la",
    "ls -l",
    "ls docs",
    "ls docs src",
    "ls -R",
    "ls -C",
    "ls -m",
    "ls -1",
    "ls a.rs docs",
    "ls nosuchentry",
    "find . -name '*.rs'",
    "find . -type d",
    "grep -rn Concern .",
    "grep -rl Concern .",
    "grep Concern a.rs",
    "grep src Makefile",
    "grep -h src Makefile",
    "grep zzznomatch a.rs",
    "grep -rn zzznomatch .",
    "ls | sort",
    "ls | sort -u",
    "ls | uniq",
    "ls | head -2",
    "ls | tail -2",
    "ls | cat",
    "ls | tac",
    "ls | nl",
    "ls | wc -l",
    "ls | sort -k1",
    "ls | grep -v nothing",
    "ls | sed s/a/b/",
    "ls | grep -c rs",
    "find . -name '*.rs' | sort | head -3",
    "ls src   # a trailing comment",
    "ls # comment then newline\nls docs",
    "2>/dev/null ls -la",
    "ls 2>/dev/null",
    "ls 2>&1",
    "cd docs && ls",
    "ls > /dev/null",
    "ls | sort > /dev/null",
    "PATH=/usr/bin ls",
    "export PATH=/usr/bin:$PATH; ls",
    "{ ls; ls; } | cat",
    "( ls; ls )",
    "files=$(ls); echo done",
    "ls && echo after",
    "ls || echo after",
    "ls; echo after",
    "ls | xargs echo",
    "find . -name '*.rs' -print0 | xargs -0 echo",
    "find . -name '*.rs' -printf '%f\\n'",
    "grep -rlZ Concern . | xargs -0 echo",
    "ls 'a file name.rs'",
    "ls \"a file name.rs\"",
    "d=docs; ls \"$d\"",
    "ls ~+/docs",
    "LC_ALL=C ls",
    "timeout 5 ls",
    "nice ls",
    "exec ls; echo NEVER",
    "\\grep -rn Concern .",
    "command ls",
    "/usr/bin/env ls",
    "ls -I docs",
    "grep -Ir Concern src",
    "grep -rI Concern src",
    "ls -F",
    "ls -a",
    "ls -d docs src",
    "ls docs nosuchdir",
    "ls | uniq -c",
    "ls | nl -ba",
    "ls | tail -n +2",
    "grep -rn Concern a.rs",
    // pipefail: the status carried out is the pipeline's own, so an early stage's failure shows.
    "set -o pipefail; grep -rl zzznomatch . | sort",
    "set -o pipefail; ls nosuchentry | sort",
    // `set -e` must not end the group before it carries the status out.
    "set -euo pipefail; ls nosuchentry | sort",
    // Subshell reduces the pipeline to ONE command, hiding stages from PIPESTATUS.
    "ls nosuchentry | sort; echo ${PIPESTATUS[0]}",
    // No stage model for if/for/while — the loop body IS rewritten.
    "if [ -d docs ]; then ls docs; fi",
    "for f in docs src; do ls $f; done",
    // After `; then`/`; do`, `program_of` never reaches `ls`.
    "for f in docs src\ndo\nls $f\ndone",
    "if [ -d docs ]\nthen\nls docs\nfi",
    "echo start; ls; echo end",
    "ls; ls docs",
    // Backslash-newline continues the line; the annotator needs the raw word.
    "ls \\\n   docs",
    "ls |& cat",
    "ls *.rs",
    "ls -s",
    // A redirect-only stage is grouped into the prior pipeline.
    "ls; > out.txt | cat",
    "grep -d skip a.rs Makefile",
    "grep -rh -e -l -e Concern .",
    // Contract targets the wrong file.
    "ls docs src | grep -v zzz",
    "ls -R | sort",
    "ls -R | tail -3",
    "ls */ | sort",
    "ls -R | nl",
    "ls docs src | cat -n",
    // Annotator lands INSIDE the loop; `done`'s stage reads annotated text.
    "for f in docs src\ndo\nls $f\ndone | sort -u",
    // Contract picks one of four names on the line, not the line.
    "ls --format across",
    "ls --form across",
];

fn fixture(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("at-eq-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let ann = |c: &str| format!("// Concern: {c} | Non-concern: n | IO: none\n");
    std::fs::write(dir.join("a.rs"), ann("TOPLEVEL a")).unwrap();
    std::fs::write(dir.join("src/a.rs"), ann("NESTED a")).unwrap();
    std::fs::write(dir.join("docs/a.rs"), ann("DOCS a")).unwrap();
    std::fs::write(dir.join("silent.rs"), "fn b() {}\n").unwrap();
    std::fs::write(dir.join("a file name.rs"), ann("SPACED")).unwrap();
    std::fs::write(dir.join("two  spaces.rs"), ann("TWOSPACE")).unwrap();
    // Collides with an `ls -l` column, or a count.
    std::fs::write(dir.join("1"), ann("ONE")).unwrap();
    std::fs::write(dir.join("2"), ann("TWO")).unwrap();
    // A colon in prose, not a grep prefix.
    std::fs::write(dir.join("Makefile"), "a.rs: src/a.rs\n\tcp src/a.rs a.rs\n").unwrap();
    std::fs::write(dir.join("crlf.rs"), "// Concern: CR | Non-concern: n | IO: none\r\n").unwrap();
    // Dangling: `exists()` used to send resolution to the cwd.
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink("nowhere.rs", dir.join("docs/a2.rs"));
    dir
}

fn have_bash() -> bool {
    Command::new("bash").arg("-c").arg("true").status().map(|s| s.success()).unwrap_or(false)
}

fn shell(dir: &Path, cmd: &str) -> (Vec<u8>, Option<i32>) {
    run_in(&["bash", "-c"], dir, cmd)
}

fn run_in(sh: &[&str], dir: &Path, cmd: &str) -> (Vec<u8>, Option<i32>) {
    let out = Command::new(sh[0]).args(&sh[1..]).arg(cmd).current_dir(dir).stderr(Stdio::null()).output().expect("spawn shell");
    (out.stdout, out.status.code())
}

fn rewritten(dir: &Path, cmd: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_annotated-tree")).args(["bash-annotator", "--check", cmd]).current_dir(dir).output().expect("spawn injector");
    let text = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    match text.strip_prefix("(unchanged) ") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

fn same_line(raw: &[u8], annotated: &[u8]) -> bool {
    fn strip_cr(b: &[u8]) -> (&[u8], bool) {
        match b.last() {
            Some(&b'\r') => (&b[..b.len() - 1], true),
            _ => (b, false),
        }
    }
    let (raw, raw_cr) = strip_cr(raw);
    let (ann, ann_cr) = strip_cr(annotated);
    if raw_cr != ann_cr {
        return false;
    }
    match ann.strip_prefix(raw) {
        Some(b"") => true,
        Some(rest) => rest.starts_with(b"  # "),
        None => false,
    }
}

#[test]
fn every_rewritten_command_agrees_with_the_command_the_agent_wrote() {
    if !have_bash() {
        eprintln!("SKIPPED: no bash on this host");
        return;
    }
    let mut wrong: Vec<String> = Vec::new();
    let mut actually_rewritten = 0;
    for (i, cmd) in COMMANDS.iter().enumerate() {
        // Fresh tree per side: a written file must not leak into the other run.
        let raw_dir = fixture(&format!("raw{i}"));
        let new_dir = fixture(&format!("new{i}"));
        let (raw_out, raw_code) = shell(&raw_dir, cmd);
        let sub = rewritten(&new_dir, cmd);
        if sub != *cmd {
            actually_rewritten += 1;
        }
        let (new_out, new_code) = shell(&new_dir, &sub);

        if raw_code != new_code {
            wrong.push(format!("  {cmd:?}\n    exit code {raw_code:?} -> {new_code:?}"));
            continue;
        }
        let raw_lines: Vec<&[u8]> = raw_out.split(|b| *b == b'\n').collect();
        let new_lines: Vec<&[u8]> = new_out.split(|b| *b == b'\n').collect();
        if raw_lines.len() != new_lines.len() {
            wrong.push(format!("  {cmd:?}\n    line count {} -> {}\n    raw: {:?}\n    new: {:?}", raw_lines.len(), new_lines.len(), String::from_utf8_lossy(&raw_out), String::from_utf8_lossy(&new_out)));
            continue;
        }
        for (r, n) in raw_lines.iter().zip(new_lines.iter()) {
            if !same_line(r, n) {
                wrong.push(format!("  {cmd:?}\n    line changed, not appended to:\n      raw: {:?}\n      new: {:?}", String::from_utf8_lossy(r), String::from_utf8_lossy(n)));
                break;
            }
        }
        let _ = std::fs::remove_dir_all(&raw_dir);
        let _ = std::fs::remove_dir_all(&new_dir);
    }
    assert!(wrong.is_empty(), "{} of {} commands did not survive the rewrite:\n{}", wrong.len(), COMMANDS.len(), wrong.join("\n"));
    // Guards against passing while testing `bash` rather than the annotator.
    assert!(
        actually_rewritten >= 25,
        "only {actually_rewritten} of {} commands were rewritten at all; this table is not \
         exercising the thing it exists to check",
        COMMANDS.len()
    );
}

/// Codex runs a command under the session's shell, and Claude Code under zsh when `$SHELL` is zsh.
const PORTABLE: &[&str] = &["ls a.rs && echo AND", "ls nosuchentry && echo AND", "ls nosuchentry || echo OR", "ls a.rs || echo OR", "grep -rl zzznomatch .; echo rc=$?", "ls nosuchentry | sort; echo rc=$?", "d=docs; ls \"$d\"", "ls docs src | head -1; echo rc=$?", "set -e; ls nosuchentry", "set -e; ls a.rs; echo after"];

#[test]
fn a_rewrite_keeps_its_status_and_control_flow_under_sh_and_zsh() {
    let zsh = Command::new("zsh").args(["-fc", "true"]).status().is_ok_and(|s| s.success());
    if !zsh {
        eprintln!("SKIPPED the zsh half: no zsh on PATH");
    }
    let shells: Vec<&[&str]> = if zsh { vec![&["sh", "-c"], &["zsh", "-fc"]] } else { vec![&["sh", "-c"]] };
    let dir = fixture("portable");
    let mut wrong: Vec<String> = Vec::new();
    for sh in shells {
        for cmd in PORTABLE {
            let sub = rewritten(&dir, cmd);
            assert_ne!(sub, *cmd, "{cmd:?} must be rewritten for this to test anything");
            let (raw_out, raw_code) = run_in(sh, &dir, cmd);
            let (new_out, new_code) = run_in(sh, &dir, &sub);
            let raw_lines: Vec<&[u8]> = raw_out.split(|b| *b == b'\n').collect();
            let new_lines: Vec<&[u8]> = new_out.split(|b| *b == b'\n').collect();
            let lines_agree = raw_lines.len() == new_lines.len() && raw_lines.iter().zip(&new_lines).all(|(r, n)| same_line(r, n));
            if raw_code != new_code || !lines_agree {
                wrong.push(format!("  {} {cmd:?}\n    exit {raw_code:?} -> {new_code:?}\n    raw: {:?}\n    new: {:?}", sh[0], String::from_utf8_lossy(&raw_out), String::from_utf8_lossy(&new_out)));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(wrong.is_empty(), "rewrites that changed behaviour outside bash:\n{}", wrong.join("\n"));
}
