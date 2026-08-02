// Concern: asserts a rewrite's bytes and exit code match the original command's, bar appended contracts | Non-concern: eligibility, or which file a line names | IO: (commands, tree) -> asserted equality

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// THE COMMAND TABLE — append to it. Every entry runs twice, as written and as rewritten, and
/// the two must agree BYTE for byte; an ineligible command must come back untouched. That catches
/// structural damage but NOT a contract belonging to the wrong file, since appending the wrong
/// contract is still an append — `SHAPES` in `bash_annotator_run.rs` owns that class.
const COMMANDS: &[&str] = &[
    // plain shapes
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
    // downstream stages
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
    // shapes the rewrite must not corrupt
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
    // layouts the annotator must read without disturbing
    "ls -F",
    "ls -a",
    "ls -d docs src",
    "ls docs nosuchdir",
    "ls | uniq -c",
    "ls | nl -ba",
    "ls | tail -n +2",
    "grep -rn Concern a.rs",
    // Under `pipefail` bash derives a pipeline's status from the RIGHTMOST FAILING stage, and the appended annotator always succeeds — so the failure the agent branches on is erased.
    "set -o pipefail; grep -rl zzznomatch . | sort",
    "set -o pipefail; ls nosuchentry | sort",
    // `set -e` masks the same defect, because the failing pipeline ends the subshell before the `exit` line is reached. Here so a fix is not judged by the shape that already passes.
    "set -euo pipefail; ls nosuchentry | sort",
    // The subshell collapses a pipeline into ONE simple command, so the array the agent reads afterwards describes the subshell rather than the stages it wrapped.
    "ls nosuchentry | sort; echo ${PIPESTATUS[0]}",
    // The stage model has no notion of `if`/`for`/`while`, so a loop body reads as a free-standing stage and IS rewritten.
    "if [ -d docs ]; then ls docs; fi",
    "for f in docs src; do ls $f; done",
    // After `; then` and `; do` the first word is `then`/`do`, so `program_of` never reaches `ls` and these rows are silent about the property they name.
    "for f in docs src\ndo\nls $f\ndone",
    "if [ -d docs ]\nthen\nls docs\nfi",
    "echo start; ls; echo end",
    "ls; ls docs",
    // A backslash-newline is a LINE CONTINUATION, not a separator: the word it splits must be forwarded to the annotator as the raw source the shell rejoins, or the operand is lost.
    "ls \\\n   docs",
    "ls |& cat",
    "ls *.rs",
    "ls -s",
    // A redirect-only stage has no words, so the pipeline grouping steps OVER it and joins the stage after the pipe to the previous pipeline. Here so that the stitched span stays valid shell and the redirect keeps happening.
    "ls; > out.txt | cat",
    // The two argv mis-parses reported as `SHAPES` rows. Structurally these are clean appends, which is exactly why this table cannot see what is wrong with them — kept so the pair is findable from either table.
    "grep -d skip a.rs Makefile",
    "grep -rh -e -l -e Concern .",
    // Byte-wise these are perfect appends, but the contracts they append belong to other files — equivalence alone would have shipped the defect.
    "ls docs src | grep -v zzz",
    "ls -R | sort",
    "ls -R | tail -3",
    // Byte-wise both are perfect appends, so this table is blind to them; `bash_annotator_inject.rs` holds the rows that assert the decision.
    "ls */ | sort",
    "ls -R | nl",
    "ls docs src | cat -n",
    // The annotator ends up INSIDE the loop, so the stage after `done` reads annotated text rather than raw output — the one arrangement the "annotator is LAST" design exists to prevent.
    "for f in docs src\ndo\nls $f\ndone | sort -u",
    // A clean append byte-wise, which is why this table cannot see that the contract belongs to one of four names on the line rather than to the line.
    "ls --format across",
    // The same layout selected by an ABBREVIATED option name, which GNU ls accepts. Structurally clean here too; `bash_annotator_run.rs` holds the row that asserts the contract is wrong.
    "ls --form across",
];

/// A tree holding every hazard the reviews turned up, so one fixture serves every row.
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
    // A file whose name collides with an `ls -l` column, and one that collides with a count.
    std::fs::write(dir.join("1"), ann("ONE")).unwrap();
    std::fs::write(dir.join("2"), ann("TWO")).unwrap();
    // Prose naming a file with a colon after it — not a grep prefix.
    std::fs::write(dir.join("Makefile"), "a.rs: src/a.rs\n\tcp src/a.rs a.rs\n").unwrap();
    std::fs::write(
        dir.join("crlf.rs"),
        "// Concern: CR | Non-concern: n | IO: none\r\n",
    )
    .unwrap();
    // A dangling symlink: it does not `exists()`, which used to send resolution to the cwd.
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink("nowhere.rs", dir.join("docs/a2.rs"));
    dir
}

fn have_bash() -> bool {
    Command::new("bash")
        .arg("-c")
        .arg("true")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `cmd` through a real shell in `dir`; return stdout bytes and the exit code.
fn shell(dir: &Path, cmd: &str) -> (Vec<u8>, Option<i32>) {
    let out = Command::new("bash")
        .args(["-c", cmd])
        .current_dir(dir)
        .stderr(Stdio::null())
        .output()
        .expect("spawn bash");
    (out.stdout, out.status.code())
}

/// What the hook would substitute, or the command unchanged.
fn rewritten(dir: &Path, cmd: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_annotated-tree"))
        .args(["bash-annotator", "--check", cmd])
        .current_dir(dir)
        .output()
        .expect("spawn injector");
    let text = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    match text.strip_prefix("(unchanged) ") {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

/// Is `annotated` the same line as `raw` with a contract appended, and nothing else changed?
///
/// A contract is always `  # …` at the very end, before any carriage return. Anything else — a
/// different prefix, a lost `\r`, a reordering — fails.
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
    // Collect every disagreement rather than stopping at the first, so one run reports the whole truth about the table.
    let mut wrong: Vec<String> = Vec::new();
    let mut actually_rewritten = 0;
    for (i, cmd) in COMMANDS.iter().enumerate() {
        // A FRESH tree per side: a command that writes a file would otherwise be seen by the second run and not the first, and the two would differ for a reason that is not a bug.
        let raw_dir = fixture(&format!("raw{i}"));
        let new_dir = fixture(&format!("new{i}"));
        let (raw_out, raw_code) = shell(&raw_dir, cmd);
        let sub = rewritten(&new_dir, cmd);
        if sub != *cmd {
            actually_rewritten += 1;
        }
        let (new_out, new_code) = shell(&new_dir, &sub);

        if raw_code != new_code {
            wrong.push(format!(
                "  {cmd:?}\n    exit code {raw_code:?} -> {new_code:?}"
            ));
            continue;
        }
        let raw_lines: Vec<&[u8]> = raw_out.split(|b| *b == b'\n').collect();
        let new_lines: Vec<&[u8]> = new_out.split(|b| *b == b'\n').collect();
        if raw_lines.len() != new_lines.len() {
            wrong.push(format!(
                "  {cmd:?}\n    line count {} -> {}\n    raw: {:?}\n    new: {:?}",
                raw_lines.len(),
                new_lines.len(),
                String::from_utf8_lossy(&raw_out),
                String::from_utf8_lossy(&new_out)
            ));
            continue;
        }
        for (r, n) in raw_lines.iter().zip(new_lines.iter()) {
            if !same_line(r, n) {
                wrong.push(format!(
                    "  {cmd:?}\n    line changed, not appended to:\n      raw: {:?}\n      new: {:?}",
                    String::from_utf8_lossy(r),
                    String::from_utf8_lossy(n)
                ));
                break;
            }
        }
        let _ = std::fs::remove_dir_all(&raw_dir);
        let _ = std::fs::remove_dir_all(&new_dir);
    }
    assert!(
        wrong.is_empty(),
        "{} of {} commands did not survive the rewrite:\n{}",
        wrong.len(),
        COMMANDS.len(),
        wrong.join("\n")
    );
    // Guard against the whole table passing because NOTHING is being rewritten — a broken annotator lookup would otherwise turn this into a very thorough test of `bash`.
    assert!(
        actually_rewritten >= 25,
        "only {actually_rewritten} of {} commands were rewritten at all; this table is not \
         exercising the thing it exists to check",
        COMMANDS.len()
    );
}
