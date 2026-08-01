// Concern: every command form the injector rewrites and every form it leaves alone, as one table | Non-concern: the annotated output, or the hook wire format | IO: (commands) -> rewrite or refusal

use std::process::Command;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Want {
    /// The command's stdout reaches the model unchanged, so route it through the wrapper.
    Rewrite,
    /// Anything that cannot be guaranteed. A missed rewrite costs nothing; a wrong one corrupts a
    /// pipeline the agent then debugs having never seen the command that ran.
    Leave,
}
use Want::{Leave, Rewrite};

/// THE DECISION TABLE.
///
/// One row per command form, which is how a decision surface should be frozen: a gap is then
/// visible as a missing row rather than as a test nobody happened to write. Every row is here
/// because a reviewer found the behaviour wrong, or because it guards a property the design rests
/// on — the third field says which.
const CASES: &[(&str, Want, &str)] = &[
    // ---- the mapped tools, plainly -------------------------------------------------------
    ("grep -rn foo .", Rewrite, "the base case"),
    ("ls -la", Rewrite, "the base case"),
    ("find . -name x", Rewrite, "the base case"),
    ("cat file.txt", Leave, "not a mapped tool"),
    ("awk '{print}' file.txt", Leave, "not a mapped tool"),
    // ---- prefixes that do not change how the program resolves -----------------------------
    ("LC_ALL=C ls", Rewrite, "a locale assignment is harmless"),
    (
        "timeout 5 grep -rn x .",
        Rewrite,
        "timeout does not change resolution",
    ),
    ("nice ls -la", Rewrite, "nor does nice"),
    // ---- prefixes that DO change it: rewriting these yields exit 127 ----------------------
    (
        "PATH=/usr/bin ls",
        Rewrite,
        "the annotator is spelled as an absolute path and the TOOL is resolved by the caller's \
         own shell, so a PATH change can strand neither",
    ),
    (
        "export PATH=/usr/bin; ls",
        Rewrite,
        "same, one stage later — the guard this used to need is gone with the substitution",
    ),
    (
        "env -i ls /tmp",
        Leave,
        "env is not a stepped-over wrapper, so the program reads as `env`",
    ),
    (
        "sudo grep -rn x .",
        Leave,
        "the program reads as `sudo`; the paths it prints are still unannotated, which is the \
         fail-closed side",
    ),
    // ---- spellings that explicitly ask for the REAL binary --------------------------------
    (
        "/usr/bin/grep pattern a.rs",
        Leave,
        "an explicit path means the binary",
    ),
    (
        "\\grep pattern a.rs",
        Rewrite,
        "a backslash asks for the real binary and GETS it — nothing is substituted, so honouring \
         the caller's spelling and annotating its output are no longer in tension",
    ),
    (
        "command grep pattern a.rs",
        Leave,
        "the program reads as `command`, which is not in the map",
    ),
    (
        "./grep pattern a.rs",
        Leave,
        "a relative path is a different program",
    ),
    // ---- grep flags on which the session function delegates to the real binary ------------
    (
        "grep -rlZ A .",
        Leave,
        "-Z emits NUL-delimited records, so there are no lines to append to",
    ),
    (
        "grep -rl --null A .",
        Leave,
        "the long spelling of the same",
    ),
    ("grep -rl --null-data A .", Leave, "and a third spelling"),
    ("ls --zero", Leave, "ls has its own NUL-delimited mode"),
    (
        "find . -name x -print0",
        Leave,
        "so does find, spelled as a PREDICATE rather than a flag",
    ),
    (
        "grep -rl --format-open x .",
        Rewrite,
        "the delegation table is gone: whichever engine the caller's own `grep` reaches is the \
         one that answers, and it still prints paths",
    ),
    ("grep -rl --config x .", Rewrite, "same"),
    ("grep -rl --save-config x .", Rewrite, "same"),
    // ---- control operators do not disqualify; each branch still writes to the result ------
    (
        "cd src && ls -la && grep -rn foo .",
        Rewrite,
        "&& keeps stdout",
    ),
    (
        "ls -R /a 2>/dev/null || ls -R /b",
        Rewrite,
        "|| keeps stdout",
    ),
    ("ls; grep -rn foo .", Rewrite, "; keeps stdout"),
    // ---- stderr redirection is not stdout redirection -------------------------------------
    ("ls 2>/dev/null", Rewrite, "2> is stderr"),
    ("ls 2>&1", Rewrite, "2>&1 is stderr"),
    ("grep -rn foo . 2>>err.log", Rewrite, "2>> is stderr"),
    // ---- stdout leaving for a file --------------------------------------------------------
    ("ls > listing.txt", Leave, "redirected"),
    ("ls >> listing.txt", Leave, "redirected"),
    ("ls 1> listing.txt", Leave, "redirected"),
    ("ls &> listing.txt", Leave, "redirected"),
    (
        "ls | sort > out.txt",
        Leave,
        "redirected DOWNSTREAM of a pipe",
    ),
    // ---- downstream stages whose unit is a line -------------------------------------------
    ("ls | head -20", Rewrite, "head counts lines"),
    ("ls | tail -5", Rewrite, "tail counts lines"),
    ("ls | sort", Rewrite, "sort is line-wise"),
    ("ls | cat", Rewrite, "cat is line-wise"),
    ("ls | tac", Rewrite, "tac is line-wise"),
    (
        "ls | wc -l",
        Leave,
        "the annotator is LAST, so what reaches it is a count — a bare number that could itself \
         resolve to a file. This flipped when annotation moved to the tail",
    ),
    (
        "find . -name x | sort | head -3",
        Rewrite,
        "a chain of line-wise stages",
    ),
    // ---- downstream stages that appended text would change --------------------------------
    (
        "ls | uniq",
        Rewrite,
        "uniq sees the tool's RAW lines and collapses exactly as it would without this installed \
         — annotating before it was what used to break it",
    ),
    ("ls | sort -u", Rewrite, "same reasoning, same fix"),
    (
        "ls | sort -k2",
        Rewrite,
        "a key reads the raw line; the contract is appended after sort has finished",
    ),
    ("ls | sort -t:", Rewrite, "a field separator likewise"),
    (
        "ls | sort -o out.txt",
        Leave,
        "sort can take stdout away with a FLAG rather than with `>`",
    ),
    ("ls | wc", Leave, "wc counts words and bytes too"),
    ("ls | wc -l -c", Leave, "-c counts bytes"),
    ("ls | head -c 200", Leave, "bytes can cut a record in half"),
    (
        "ls | head -qc 20",
        Leave,
        "the byte flag hides in a cluster",
    ),
    (
        "grep -rn foo . | grep -v test",
        Rewrite,
        "the filter matches the tool's own text, never appended text, so it drops exactly the \
         lines it would have dropped",
    ),
    (
        "cat x | grep foo",
        Leave,
        "a filter over a stream has no paths",
    ),
    // ---- `ls` layouts whose lines mean something only IN THE ORDER PRINTED --------------------
    (
        "ls -R | sort",
        Leave,
        "`ls -R` prints `<dir>:` HEADERS and then names entries relative to the last one, so a \
         bare name means a different file depending on which header precedes it. `sort` hoists \
         every header above the entries it scoped, and each name then resolves against the \
         wrong directory — the same-named-cwd-file defect `ls docs src` was fixed for, \
         reintroduced from downstream. Reproduced end to end: in a tree holding `x.rs` and \
         `docs/x.rs`, `ls docs sub | grep -v :` puts the CWD `x.rs`'s contract on the line \
         about `docs/x.rs`",
    ),
    (
        "ls -R | tail -20",
        Leave,
        "`tail` drops the LEADING headers, so every surviving entry resolves against the base \
         instead of its own block. Dropping a header is as damaging as reordering one",
    ),
    (
        "ls docs src | grep -v test",
        Leave,
        "two operands with a directory among them make `ls` print the same `<dir>:` headers \
         WITHOUT `-R` — the annotator already keys on exactly that — and a filter drops them. \
         Refusing any multi-operand `ls` that has a downstream stage needs no stat and costs \
         nothing",
    ),
    (
        "ls */ | sort",
        Leave,
        "a GLOB expands to several operands, so `ls` prints exactly the `<dir>:` headers the \
         row above refuses — but `header_scoped` counts words in the SOURCE, where `*/` is ONE \
         word, so the guard never fires. `sort` then hoists both headers above the entries they \
         scoped and every bare name resolves against whichever header landed last: in a tree \
         holding `docs/x.rs` and `sub/x.rs`, `ls */ | sort` puts sub/x.rs's contract on the line \
         about docs/x.rs. `ls $dirs | sort` and `ls {docs,sub} | sort` are the same hole — the \
         annotator sees the EXPANDED argv and keys on it correctly, the injector never can",
    ),
    (
        "ls docs src | nl",
        Leave,
        "`nl` is ORDER_PRESERVING because it moves no line — but it REWRITES every line, \
         prefixing a number and a tab. `docs:` arrives as `     1<TAB>docs:`, which no longer \
         parses as a `<dir>:` header, so the block scope is never set, each entry falls back to \
         the base and takes a same-named cwd file's contract. Reproduced: with `x.rs` and \
         `docs/x.rs` present, `ls docs sub | nl` puts the CWD x.rs's contract on the line about \
         docs/x.rs. Order-preservation is necessary but not sufficient; the stage must also \
         leave the line's TEXT alone",
    ),
    (
        "ls -R | cat -n",
        Leave,
        "`cat` is ORDER_PRESERVING and `line_safe` checks no flag for it at all, so `cat -n` \
         (and `-b`) reaches the annotator having renumbered every line — the same header \
         destruction as `nl`, arriving through the one stage documented as identity",
    ),
    (
        "ls -R | head -20",
        Rewrite,
        "the boundary of the rule above: `head` keeps a PREFIX of the stream, so every header \
         that survives still precedes the block it scoped",
    ),
    (
        "ls -R | cat",
        Rewrite,
        "and a stage that neither drops nor reorders leaves the scoping intact",
    ),
    // ---- output consumed rather than read ---------------------------------------------------
    ("grep -rl foo . | xargs rm", Leave, "xargs is not line-safe"),
    (
        "ls | sort | xargs rm",
        Leave,
        "nor is it line-safe further down the pipe",
    ),
    (
        "find . -exec grep foo {} \\;",
        Leave,
        "-exec is the only thing that sets consumes_args",
    ),
    (
        "find . -name x -ok rm {} \\;",
        Leave,
        "`-ok` and `-okdir` ARE `-exec`/`-execdir` with a prompt: they consume the found paths \
         as arguments in exactly the same way, and the exec'd program's own stdout lands in \
         the pipe — so the annotator would read another program's output as though it were \
         find's. `consumes_args` names only two of the four",
    ),
    // ---- separators the lexer must distinguish ----------------------------------------------
    (
        "ls -la\ngrep -rn foo .",
        Rewrite,
        "a newline separates commands; agents write multi-line Bash constantly",
    ),
    (
        "ls & grep -rn x .",
        Rewrite,
        "& is a separator, not a redirect and not &&",
    ),
    (
        "grep foo < file.txt",
        Rewrite,
        "an INPUT redirect does not take stdout away",
    ),
    // ---- the program token must be spelled bare ----------------------------------------------
    (
        "\"grep\" -rn x .",
        Rewrite,
        "quoting stopped mattering once nothing is substituted: the shell resolves `grep` however \
         it would have, and we only read what it printed",
    ),
    (
        "nice -n 10 ls",
        Leave,
        "a wrapper's own argument is not the program",
    ),
    // ---- long spellings of the flags that make a downstream stage unsafe ---------------------
    ("ls | head --bytes=20", Leave, "the long form of -c"),
    (
        "ls | sort --unique",
        Rewrite,
        "the long form of -u, now equally safe",
    ),
    (
        "ls | wc --lines",
        Leave,
        "the long form of -l, and still a count rather than paths",
    ),
    // ---- shapes the lexer must refuse rather than model ------------------------------------
    ("files=$(ls src)", Leave, "command substitution"),
    ("echo `ls`", Leave, "backtick substitution"),
    ("( ls; ls ) | wc -l", Leave, "subshell"),
    ("ls \"unclosed", Leave, "unbalanced double quote"),
    (
        "ls 'unclosed",
        Leave,
        "unbalanced single quote — a different lexer branch",
    ),
    ("ls \\", Leave, "a trailing backslash is a third"),
    (
        "grep \"$(cat p)\" .",
        Leave,
        "substitution INSIDE double quotes is its own branch",
    ),
    (
        "cat > f.sh <<'SH'\nls -la\nSH",
        Leave,
        "a heredoc body is DATA, not commands",
    ),
    (
        "cat > f.sh <<-EOF\nls\nEOF",
        Leave,
        "so is an indented heredoc",
    ),
    // ---- grouping the stage model does not represent -----------------------------------------
    (
        "{ ls; ls; } > out.txt",
        Leave,
        "a brace group's redirect belongs to the GROUP, and splitting on `;` cannot see that — \
         rewriting inside would write contracts into the agent's file",
    ),
    (
        "{ ls; ls; } | uniq",
        Leave,
        "same blindness on the pipe side",
    ),
    (
        "for f in docs\ndo\nls $f\ndone > out.txt",
        Leave,
        "a LOOP's redirect belongs to the whole compound exactly as a brace group's does, and the \
         stage model has no notion of `done` either. The body lexes as a free-standing `ls $f`, is \
         rewritten, and the contracts the annotator appends are then written into the agent's \
         file — the identical failure the `{ …; } > out.txt` row above forbids, reached through \
         `for`/`while`/`if` instead. Reproduced: `for f in docs; do ls $f; done > out.txt` leaves \
         `a.rs  # Concern: …` in out.txt where the agent asked for `a.rs`",
    ),
    (
        "for f in docs src\ndo\nls $f\ndone | sort -u",
        Leave,
        "the pipe side of the same hole, and the worse one: the stage after `done` reads the \
         annotator's OUTPUT, so the guarantee that `| sort -u` sees the tool's raw lines — the \
         reason the annotator was moved to the tail at all — is silently void. Two identical \
         `a.rs` lines collapse to one raw and stay two annotated",
    ),
    // ---- text the lexer must not treat as command words ---------------------------------------
    (
        "ls src   # sources",
        Rewrite,
        "a trailing COMMENT must fall outside the parentheses; spliced inside it commented out \
         the closing paren and the agent got a bash syntax error and no output",
    ),
    (
        "ls file#1",
        Rewrite,
        "`#` inside a word is an ordinary character, not a comment opener",
    ),
    // ---- redirects on either side of the command ----------------------------------------------
    (
        "2>/dev/null ls -la",
        Rewrite,
        "a LEADING redirect must be inside the parentheses — bash rejects a redirect before `(`",
    ),
    (
        "ls 2>&1",
        Rewrite,
        "`2>&1`'s `1` is a redirect TARGET; forwarded as an argument it made a directory named \
         `1` the operand and rebased every name in the listing",
    ),
    // ---- operands the SHELL expands, not us ---------------------------------------------------
    (
        "d=docs; ls \"$d\"",
        Rewrite,
        "the argv is forwarded as raw source so the shell expands it the same way twice; \
         re-quoted, `$d` reached the annotator literally and the base fell back to the cwd",
    ),
    ("ls ~/x", Rewrite, "a tilde expands for the same reason"),
    // ---- downstream stages that stop the lines being paths ------------------------------------
    (
        "ls | sed s/alpha/beta/",
        Leave,
        "sed rewrites the TEXT of a line, so the path read back is not the path printed",
    ),
    (
        "ls | grep -c rs",
        Leave,
        "a count, which is exactly why `wc` is refused",
    ),
    (
        "ls | grep -o x",
        Leave,
        "the matched fragment, not the line",
    ),
    (
        "ls | grep -l x",
        Leave,
        "over a STREAM this prints `(standard input)`, not a path",
    ),
    ("ls | tac -s x", Leave, "-s redefines the record separator"),
    ("ls | nl -s :", Leave, "so does nl's"),
    (
        "exec ls -la",
        Leave,
        "`exec` REPLACES the shell; inside our subshell it would replace only the subshell and \
         let a following command run that never ran before",
    ),
    (
        "ls |& grep foo",
        Rewrite,
        "`|&` is ONE operator and pipes stdout+stderr; lexed as `|` then `&` it made the \
         downstream stage look like a fresh command",
    ),
    (
        "find . -name x -printf '%f\\n'",
        Leave,
        "`-printf` replaces find's output with a CALLER-CHOSEN record. `%f` prints a bare \
         basename, which the annotator then resolves against the wrong directory and gives a \
         same-named file's contract — the same reason `-print0` is refused, one predicate over",
    ),
    (
        "find . -name x -printx",
        Leave,
        "`-printx` is `-print` with whitespace and quote characters ESCAPED, and it is the \
         `find` an agent's shell actually reaches: in a Claude Code session `find` is a shell \
         function running `bfs`, which is the whole reason the program token is never \
         substituted. An escaped name is not the name, so it does not resolve — and the suffix \
         scan then falls back to a SHORTER suffix that does. Reproduced with `a b.rs` and \
         `b.rs` side by side: `find . -name '*.rs' -printx` prints `./a\\ b.rs`, which fails to \
         resolve, so the line takes `b.rs`'s contract — and `./b.rs`'s own line then carries \
         nothing, because `seen` has already spent it. Two files misdescribed by one \
         predicate, from the same output-shaping family as `-printf` and `-print0` that \
         `nul_delimited` already refuses",
    ),
    (
        "ls >| listing.txt",
        Leave,
        "`>|` overrides `noclobber` and is still a stdout redirect, but the lexer emits it as `>` \
         followed by a PIPE — so the stage after it looks like a downstream stage of a pipeline \
         rather than a redirect target. It is refused only because the redirect flag is set on \
         the producer's own stage; frozen here so a change to the pipeline grouping cannot \
         quietly start writing contracts into the agent's file",
    ),
    (
        "ls 1>&2",
        Leave,
        "the inverse of the `2>&1` row above: `1>&` does not begin with `2`, so it is a stdout \
         redirect and the model never reads the result",
    ),
    (
        "ls |\n  sort",
        Rewrite,
        "a newline after `|` continues the SAME pipeline — bash reads this as `ls | sort`. The \
         lexer makes `\\n` a command separator unconditionally, so the downstream stage lexes as a \
         fresh command, the pipeline reads as `ls` alone, and every multi-line pipeline an agent \
         writes goes unannotated",
    ),
];

/// Drive the SHIPPED surface rather than an internal function: `--check` is what a user runs to
/// ask what would happen to a command, so freezing it freezes something observable.
fn check(cmd: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_annotated-tree"))
        .args(["bash-annotator", "--check", cmd])
        .output()
        .expect("spawn injector");
    assert_eq!(
        out.status.code(),
        Some(0),
        "the injector must never exit nonzero — a PreToolUse exit 2 BLOCKS the tool call. \
         Input: {cmd:?}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

#[test]
fn the_decision_table_holds() {
    // Collect every mismatch rather than stopping at the first: one run should report everything
    // that is wrong, not the earliest thing that is.
    let mut wrong = Vec::new();
    for (cmd, want, why) in CASES {
        let got = if check(cmd).starts_with("(unchanged)") {
            Leave
        } else {
            Rewrite
        };
        if got != *want {
            wrong.push(format!("  {cmd:?}\n    want {want:?} ({why}), got {got:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} cases decided wrongly:\n{}",
        wrong.len(),
        CASES.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_rest_of_the_command_survives_byte_for_byte() {
    // Nothing is substituted; the pipeline is wrapped. So the agent's own text — its spacing, its
    // quoting, its globs — must reappear VERBATIM inside the parentheses, and the surrounding
    // command must be untouched outside them.
    let spaced = check("ls    -la");
    assert!(
        spaced.contains("( ls    -la | "),
        "spacing changed: {spaced}"
    );
    let quoted = check("grep -rn \"a\\|b\" .");
    assert!(
        quoted.contains("( grep -rn \"a\\|b\" . | "),
        "a quoted pipe was treated as a pipeline: {quoted}"
    );
    let both = check("cd src && ls -la && grep -rn foo .");
    assert!(
        both.starts_with("cd src && ( ls -la | ") && both.contains("( grep -rn foo . | "),
        "the surrounding command was altered: {both}"
    );
    // The exit-code recovery must index the stage that was last BEFORE the annotator was
    // appended. (That the resulting CODE matches the unrewritten command, with and without
    // `pipefail`, is checked end to end in bash_annotator_equivalence.rs.)
    assert!(
        check("ls | sort").contains("__rc=${__ps[1]}"),
        "a two-stage pipeline must re-raise stage 1, not stage 0"
    );
    assert!(
        check("ls -la").contains("__rc=${__ps[0]}"),
        "a one-stage pipeline must re-raise stage 0"
    );
}

#[test]
fn no_input_makes_the_injector_exit_nonzero() {
    // A panic exits 101, which the harness surfaces as an error on a command the agent wrote
    // correctly. `check` asserts the exit code, so this is a totality sweep over shapes the
    // table does not otherwise reach.
    for cmd in [
        "grep -rn \"— the\" .", // em-dash: this repo's own annotations are full of them
        "ls # 日本語",
        "echo → ; ls",
        "ls 'unclosed",
        "grep -e $'\\x41' .",
        "ls \\",
        "",
        "   ",
        "|||&&&;;;",
        ">>><<<",
        "$(",
        "`",
        "<<",
    ] {
        check(cmd);
    }
}
