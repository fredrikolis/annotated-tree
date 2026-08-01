// Concern: freezes the toolcall annotator's contract — one line in, one line out, each contract on its path's line | Non-concern: eligibility or the hook wire format | IO: (fixture) -> asserted stdout

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A throwaway tree keyed by process id, so parallel tests never collide.
fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("at-run-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).expect("mkdir fixture");
    std::fs::write(
        dir.join("declared.rs"),
        "// Concern: the annotated one | Non-concern: the other | IO: none\nfn a() {}\n",
    )
    .unwrap();
    std::fs::write(dir.join("silent.rs"), "fn b() {}\n").unwrap();
    std::fs::write(
        dir.join("sub/nested.rs"),
        "// Concern: nested | Non-concern: flat | IO: none\n",
    )
    .unwrap();
    dir
}

/// Reproduce the shape the rewrite produces: the tool runs, and the annotator reads its output
/// with the tool's own argv. The annotator never spawns the tool — the shell does — so a test that
/// handed it the argv and expected it to run the tool would be testing something that never
/// happens in a session.
fn run(dir: &Path, args: &[&str]) -> String {
    String::from_utf8_lossy(&run_bytes(dir, args)).into_owned()
}

/// The same, kept as BYTES: a path that is not valid UTF-8, a CRLF ending and a missing final
/// newline are all invisible once the output has been through `from_utf8_lossy`.
fn run_bytes(dir: &Path, args: &[&str]) -> Vec<u8> {
    let mut tool = Command::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn tool");
    let out = Command::new(env!("CARGO_BIN_EXE_annotated-tree"))
        .args(["toolcall-injector", "--annotate-tool-output"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::from(tool.stdout.take().expect("tool stdout")))
        .output()
        .expect("spawn the annotator");
    // Reap the producer. A shell would have done this; leaving it a zombie makes a long test run
    // leak processes.
    let _ = tool.wait();
    out.stdout
}

fn native(dir: &Path, tool: &str, args: &[&str]) -> String {
    let out = Command::new(tool)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn native tool");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `ls` exists on every platform this is expected to run on, but not on Windows. Skip loudly.
fn have_ls() -> bool {
    // `--version` must SUCCEED, not merely spawn. BSD `ls` on macOS spawns and then fails, and it
    // is a different program: `-I` takes no argument there and `--color` does not exist, so the
    // GNU-only rows below would fail that CI leg rather than skip.
    let ok = Command::new("ls")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("SKIPPED: no GNU `ls` on this host");
    }
    ok
}

#[test]
fn one_line_in_one_line_out() {
    if !have_ls() {
        return;
    }
    let dir = fixture("lines");
    // The invariant every downstream line-unit modifier depends on.
    for args in [
        vec!["ls"],
        vec!["ls", "-l"],
        vec!["ls", "-R"],
        vec!["ls", "-a"],
    ] {
        let mut with = vec!["ls"];
        with.extend(args.iter().skip(1));
        let wrapped = run(&dir, &with);
        let plain = native(&dir, "ls", &args[1..]);
        assert_eq!(
            wrapped.lines().count(),
            plain.lines().count(),
            "line count changed for {args:?}"
        );
    }
}

#[test]
fn the_tools_own_bytes_are_reproduced_verbatim() {
    if !have_ls() {
        return;
    }
    let dir = fixture("verbatim");
    // A path that is not valid UTF-8 must survive. Comparing decoded Strings cannot detect this —
    // both sides would be mangled identically — so compare RAW BYTES.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let raw = std::ffi::OsStr::from_bytes(b"bad\xffname.rs");
        std::fs::write(dir.join(raw), "x\n").unwrap();
    }
    std::fs::write(
        dir.join("plain.rs"),
        "// Concern: p | Non-concern: q | IO: none\n",
    )
    .unwrap();
    let native = Command::new("ls")
        .current_dir(&dir)
        .output()
        .expect("spawn ls")
        .stdout;
    let wrapped = run_bytes(&dir, &["ls"]);
    let n: Vec<&[u8]> = native.split(|b| *b == b'\n').collect();
    let w: Vec<&[u8]> = wrapped.split(|b| *b == b'\n').collect();
    assert_eq!(n.len(), w.len(), "line count changed");
    for (a, b) in n.iter().zip(w.iter()) {
        assert!(
            b.starts_with(a),
            "bytes were altered, not appended to: {:?} vs {:?}",
            String::from_utf8_lossy(a),
            String::from_utf8_lossy(b)
        );
    }
    #[cfg(unix)]
    assert!(
        wrapped.windows(2).any(|w| w == [b'd', 0xff]),
        "the invalid byte was rewritten rather than passed through"
    );
}

#[test]
fn a_crlf_line_keeps_its_carriage_return_last() {
    if !have_ls() {
        return;
    }
    let dir = fixture("crlf");
    std::fs::write(
        dir.join("z.rs"),
        "// Concern: CR | Non-concern: n | IO: none\r\nhit\r\n",
    )
    .unwrap();
    // `-rn .`, NOT `-n z.rs`: given a single file operand grep prints no `path:` prefix, so no
    // contract is appended and the assertion below would hold without ever reaching the branch it
    // exists to guard.
    let out = run_bytes(&dir, &["grep", "-rn", "Concern", "."]);
    // A contract appended AFTER the `\r` makes a terminal overwrite the line it describes.
    // Pick the CRLF file's own line: the fixture holds other files, and `-r` visits them all.
    let line = out
        .split(|b| *b == b'\n')
        .find(|l| l.starts_with(b"./z.rs:"))
        .expect("z.rs was not in the grep output");
    assert_eq!(
        line.last(),
        Some(&b'\r'),
        "the carriage return must stay last: {:?}",
        String::from_utf8_lossy(line)
    );
}

#[test]
fn a_final_line_without_a_newline_does_not_gain_one() {
    if !have_ls() {
        return;
    }
    let dir = fixture("noeol");
    // `find -print0` emits no trailing newline; inventing one changes the byte stream and a
    // hand-rolled line count.
    let native = Command::new("find")
        .args([".", "-name", "*.rs", "-print0"])
        .current_dir(&dir)
        .output()
        .expect("spawn find")
        .stdout;
    let wrapped = run_bytes(&dir, &["find", ".", "-name", "*.rs", "-print0"]);
    assert_eq!(
        native.last() == Some(&b'\n'),
        wrapped.last() == Some(&b'\n'),
        "trailing-newline presence changed"
    );
}

#[test]
fn a_declared_file_gains_its_contract_and_a_silent_one_does_not() {
    if !have_ls() {
        return;
    }
    let dir = fixture("contracts");
    let out = run(&dir, &["ls"]);
    let declared = out
        .lines()
        .find(|l| l.starts_with("declared.rs"))
        .expect("declared.rs listed");
    assert!(
        declared.contains("Concern: the annotated one"),
        "contract not appended: {declared}"
    );
    let silent = out
        .lines()
        .find(|l| l.starts_with("silent.rs"))
        .expect("silent.rs listed");
    assert_eq!(
        silent, "silent.rs",
        "a file declaring nothing must print exactly what the tool printed"
    );
}

#[test]
fn a_file_is_described_once_however_often_it_appears() {
    if !have_ls() {
        return;
    }
    let dir = fixture("once");
    // `ls` twice over the same directory: the second block must carry no contracts.
    let out = run(&dir, &["ls", ".", "."]);
    let hits = out.matches("Concern: the annotated one").count();
    assert_eq!(hits, 1, "described {hits} times, expected once:\n{out}");
}

#[test]
fn the_rewritten_pipeline_reraises_the_tools_own_exit_code() {
    if !have_ls() {
        return;
    }
    // The annotator is the LAST stage now, so a naive `tool | annotator` would report the
    // annotator's status and lose `grep`'s exit 1 on no-match — which agents branch on. The
    // rewrite emits `exit ${PIPESTATUS[n]}` to prevent that, and this runs the emitted command
    // through a real shell rather than trusting the string.
    let dir = fixture("exit");
    let status = |cmd: &str| {
        let rewritten = Command::new(env!("CARGO_BIN_EXE_annotated-tree"))
            .args(["toolcall-injector", "--check", cmd])
            .current_dir(&dir)
            .output()
            .expect("spawn injector");
        let rewritten = String::from_utf8_lossy(&rewritten.stdout)
            .trim_end()
            .to_string();
        assert!(
            !rewritten.starts_with("(unchanged)"),
            "{cmd:?} must be rewritten for this to test anything: {rewritten}"
        );
        Command::new("bash")
            .args(["-c", &rewritten])
            .current_dir(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("spawn bash")
            .code()
    };
    assert_eq!(
        status("ls declared.rs"),
        Some(0),
        "a success became a failure"
    );
    assert_ne!(
        status("ls no-such-entry"),
        Some(0),
        "a failing tool must fail the pipeline"
    );
    assert_eq!(
        status("grep -rl zzzznomatch ."),
        Some(1),
        "grep's no-match exit 1 was swallowed by the appended annotator"
    );
}

/// A path a listing names is OPENED to read its first line, and a path is not always a file whose
/// first line arrives. Opening a FIFO blocks until someone writes to it, and reading a character
/// device (`/dev/ptmx`, `/dev/tty`) blocks until it has something to say — so `ls` over a directory
/// holding either never returns, and the agent loses the whole tool call to a harness timeout
/// rather than merely losing the contracts.
///
/// Not a `SHAPES` row: a row that hangs cannot report anything, so this one runs the annotator
/// under `timeout` and fails on the timeout's own exit code.
#[test]
fn a_path_that_is_not_a_regular_file_is_never_read() {
    if !have_ls() {
        return;
    }
    let dir = fixture("fifo");
    if !Command::new("mkfifo")
        .arg(dir.join("pipe.fifo"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        eprintln!("SKIPPED: no `mkfifo` on this host");
        return;
    }
    let cmd = format!(
        "ls | timeout 5 '{}' toolcall-injector --annotate-tool-output ls > /dev/null",
        env!("CARGO_BIN_EXE_annotated-tree")
    );
    let code = Command::new("bash")
        .args(["-c", &cmd])
        .current_dir(&dir)
        .status()
        .expect("spawn bash")
        .code();
    assert_ne!(
        code,
        Some(124),
        "the annotator opened a named pipe to look for a contract and blocked there forever; \
         `ls /dev`, or any listing that names a FIFO or a character device, hangs until the \
         harness kills it and the tool's own output is lost with it"
    );
}

/// THE ANNOTATION TABLE — which file each printed line is ABOUT.
///
/// Rounds 3 and 4 each fixed one shape and broke another, because each was written and tested
/// alone: making `ls docs` resolve against its operand broke `ls FILE DIR` and every
/// operand-prefixed grep path, and adding `sole_file` for unprefixed grep left matched content
/// outranking it. These only hold simultaneously, so they are asserted simultaneously, and every
/// mismatch is reported in one run.
///
/// Columns: the argv, substrings that MUST appear, substrings that must NOT, and why the row is
/// here.
/// One row of the annotation table: what to run, what must and must not appear, and why the row
/// exists. Named fields rather than a tuple — a reader should not have to count positions.
struct Shape {
    args: &'static [&'static str],
    must: &'static [&'static str],
    must_not: &'static [&'static str],
    why: &'static str,
}

const SHAPES: &[Shape] = &[
    Shape {
        args: &["ls", "docs"],
        must: &["DOCS-X"],
        must_not: &["ROOT-X"],
        why: "`ls <onedir>` names entries relative to that directory",
    },
    Shape {
        args: &["ls", "x.rs", "docs"],
        must: &["ROOT-X"],
        must_not: &[],
        why: "a file operand is cwd-relative even beside a directory operand",
    },
    Shape {
        args: &["grep", "-rn", "hit", "sub"],
        must: &["OUTER", "INNER"],
        must_not: &[],
        why: "grep paths already carry the operand; joining it again picks the nested twin",
    },
    Shape {
        args: &["grep", "-rl", "hit", "sub"],
        must: &["OUTER"],
        must_not: &[],
        why: "same for the path-only shape",
    },
    Shape {
        args: &["find", "sub", "-name", "n.rs"],
        must: &["OUTER", "INNER"],
        must_not: &[],
        why: "find prints cwd-relative paths too",
    },
    Shape {
        args: &["grep", "docs:", "Makefile"],
        must: &[],
        must_not: &["DOCS-CHARTER", "MK"],
        why: "given one file grep prints no prefix, so there is NO subject to name — guessing one \
              from argv put a pattern's or a flag value's contract on every line, so the line now \
              carries nothing rather than something wrong",
    },
    Shape {
        args: &["grep", "beta.rs is", "delta.rs"],
        must: &[],
        must_not: &["BETA"],
        why: "given one file, grep prints no prefix — the subject is that file",
    },
    Shape {
        args: &["grep", "-rh", "beta.rs is", "."],
        must: &[],
        must_not: &["BETA"],
        why: "with the filename suppressed there is no subject at all",
    },
    Shape {
        args: &["grep", "-rn", "Concern", "beta.rs"],
        must: &["BETA"],
        must_not: &[],
        why: "a prefix that IS printed is read from the line, not guessed from argv",
    },
    Shape {
        args: &["ls"],
        must: &["SPACED"],
        must_not: &[],
        why: "a filename containing spaces is one name, not three tokens",
    },
    Shape {
        args: &["ls", "-l"],
        must: &["PLAIN"],
        must_not: &[],
        why: "the long format puts the name last",
    },
    Shape {
        args: &["ls"],
        must: &["TWOSPACE", "TABBED"],
        must_not: &[],
        why: "a name may hold two spaces or a tab; rebuilding it from tokens loses both",
    },
    Shape {
        args: &["ls", "-l"],
        must: &["TWOSPACE", "TABBED"],
        must_not: &[],
        why: "the same, where the name is the line's trailing run",
    },
    Shape {
        args: &["ls", "-l", "plain.rs"],
        must: &["PLAIN"],
        must_not: &["TEN"],
        why: "a timestamp field must never be read as a path, even when a file is named `10`",
    },
    Shape {
        args: &["ls"],
        must: &["DOCS-CHARTER"],
        must_not: &[],
        why: "a directory entry shows its charter, not nothing",
    },
    Shape {
        args: &["ls", "-I", "sub"],
        must: &["ROOT-X"],
        must_not: &["DOCS-X"],
        why: "an excluded pattern is a flag's VALUE, not the directory being listed",
    },
    Shape {
        args: &["ls", "-aI", "docs"],
        must: &["ROOT-X"],
        must_not: &["DOCS-X"],
        why: "the value flag ENDS a cluster — whole-token matching missed this and took docs/",
    },
    Shape {
        args: &["ls", "-Idist", "docs"],
        must: &["DOCS-X"],
        must_not: &["ROOT-X"],
        why:
            "the value is ATTACHED to -I, so `dist` is not more option letters and docs/ is still \
              the operand — scanning the token for a bare `d` read this as -d",
    },
    Shape {
        args: &["ls", "-Iw", "docs"],
        must: &["DOCS-X"],
        must_not: &["ROOT-X"],
        why:
            "the attached value happens to spell another value option; only the FIRST one counts, \
              so -I still does not reach past its own token to eat the operand",
    },
    Shape {
        args: &["ls", "-lI", "docs"],
        must: &["ROOT-X"],
        must_not: &["DOCS-X"],
        why: "-I ends the cluster with no attached value, so the NEXT token is that value and the \
              listing is of the cwd",
    },
    Shape {
        args: &["ls", "-s", "--block-size=1G", "-I", "1*"],
        must: &[],
        must_not: &["ONE-BLOCK"],
        why: "`ls -s` and `ls -l` open a directory listing with a `total N` SUMMARY line, which \
              is about no file at all. The suffix scan tries `N` as a path, so a file named `1` \
              — the block total under `--block-size=1G`, and `1`/`2`/`4` are ordinary names — \
              hands that line its contract. Worse in the usual case where the file IS listed: \
              `seen` has already consumed it, so its own line then carries nothing. `-I 1*` only \
              makes the total deterministic and keeps the file off the listing, so the row can \
              name one line rather than count them",
    },
    Shape {
        args: &["ls", "-C"],
        must: &[],
        must_not: &["ROOT-X", "DOCS-CHARTER", "PLAIN"],
        why: "a column layout puts many names on one line, and one line takes at most one \
              contract — appending anything would attribute one file's contract to all of them",
    },
    Shape {
        args: &["ls", "--color", "docs"],
        must: &["DOCS-X"],
        must_not: &["ROOT-X"],
        why: "--color takes an OPTIONAL argument, so it never consumes the operand after it",
    },
    Shape {
        args: &["ls", "-d", "docs"],
        must: &["DOCS-CHARTER"],
        must_not: &[],
        why: "under -d the operand is the entry printed, not a directory to resolve against",
    },
    Shape {
        args: &["ls", "-R", "sub"],
        must: &["OUTER", "INNER"],
        must_not: &[],
        why: "each -R block resolves against its own header, not the first one",
    },
    Shape {
        args: &["grep", "-rn", "beta.rs", "."],
        must: &["GAMMA"],
        must_not: &["BETA"],
        why: "a PATTERN that happens to name a file is not an operand",
    },
    Shape {
        args: &["grep", "-rn", "-f", "pats.txt", "sub"],
        must: &["OUTER", "INNER"],
        must_not: &[],
        why: "nor is a pattern FILE — annotations must not vanish because -f was used",
    },
    Shape {
        args: &["ls", "docs", "sub"],
        must: &["DOCS-X"],
        must_not: &["ROOT-X"],
        why: "TWO directory operands make `ls` print `<dir>:` headers WITHOUT -R; leaving them \
              unconsumed resolved every bare name against the cwd and took a same-named cwd \
              file's contract",
    },
    Shape {
        args: &["grep", "-h", "docs/x.rs is", "prose.md"],
        must: &[],
        must_not: &["DOCS-X"],
        why: "`-h` suppresses the path prefix, so a line of PROSE that names a file with a colon \
              after it has no subject — trying every colon gave it that file's contract",
    },
    Shape {
        args: &["grep", "-rn", "Concern", "a file name.rs"],
        must: &["SPACED"],
        must_not: &[],
        why: "a grep prefix may itself contain spaces",
    },
    Shape {
        args: &["grep", "-r", "beta.rs", "notes.md"],
        must: &[],
        must_not: &["BETA"],
        why: "`-r` over a SINGLE FILE operand prints no `path:` prefix — GNU grep keys the prefix \
              on more than one operand or on recursing a directory, not on `-r` itself. Believing \
              a prefix is there makes every colon in a line of CONTENT a candidate path, so a \
              line reading `beta.rs: …` takes beta.rs's contract while being about notes.md",
    },
    Shape {
        args: &["grep", "-d", "skip", "beta.rs", "notes.md"],
        must: &[],
        must_not: &["BETA"],
        why: "`-d ACTION` takes its value as a SEPARATE argv entry and is absent from \
              GREP_VALUE_SHORT, so `skip` is eaten as the pattern and the real pattern \
              `beta.rs` is counted as a second operand. The single-file search then looks \
              multi-file, a `path:` prefix grep never printed is believed to be there, and the \
              colon scan puts beta.rs's contract on a line of notes.md's CONTENT — the \
              `--regexp` defect, one option letter over",
    },
    Shape {
        args: &["grep", "-rh", "-e", "-l", "-e", "beta", "."],
        must: &[],
        must_not: &["BETA"],
        why: "the option-letter scan reads EVERY argv entry beginning with `-`, a value handed \
              to `-e` included. Searching for the literal text `-l` therefore sets `path_only`, \
              which re-opens the whole-line and colon scans on a `-h` run that printed no \
              subject at all — so a content line takes the contract of the file it merely \
              names, which the `-rh` row above exists to forbid",
    },
    Shape {
        args: &["ls", "-d", "noisy"],
        must: &[],
        must_not: &["NOISY-CHARTER", "STRAY-PROSE"],
        why: "an `.annotation` holding more than its one line resolves to NO charter, so there is \
              nothing to paste and the line leaves exactly as it arrived. The fixture is the shape \
              a maintainer writes when the charter needs a note; it used to yield a contract with \
              a newline inside it, and one input line left as TWO — `| head -20` lost a path and \
              `| wc -l` over-counted. Both halves are asserted: the stray prose never appears, and \
              neither does the charter line it was attached to",
    },
    Shape {
        args: &["ls", "--format", "across"],
        must: &[],
        must_not: &["  # "],
        why: "`--format` takes its value as a SEPARATE argv entry — `run.rs` already knows that, \
              which is why `--format` sits in LS_VALUE_LONG and its value is skipped. But \
              `multi_name` looks for the literal `--format=across`, so only the ATTACHED spelling \
              is recognised and the separate one is not. `ls --format across` is `ls -C`: several \
              names on one line, the shape the `-C` row above forbids a contract on. Unguarded, \
              the suffix scan finds the LAST name on the line that resolves and pastes THAT one \
              file's contract onto a line naming five or six others, on every line of the layout \
              at once. `--format commas` and `--format horizontal` are the same hole. The \
              assertion is `  # ` rather than a tag, because which of the several names wins \
              depends on the column layout and no line here may carry a contract at all",
    },
    Shape {
        args: &["grep", "--regexp", "beta.rs", "notes.md"],
        must: &[],
        must_not: &["BETA"],
        why: "the long spelling of `-e` takes its value as a SEPARATE argv entry. Unconsumed, the \
              pattern is counted as a second operand, the invocation looks multi-file, and the \
              same colon scan puts the pattern's namesake contract on a content line",
    },
    Shape {
        args: &["grep", "--file", "bare.rs", "notes.md"],
        must: &[],
        must_not: &["BETA"],
        why: "the abbreviation-tolerant `long()` in run.rs accepts any option that is a >=5-char \
              PREFIX of the name it is asking about, and `--file` — grep's own long spelling of \
              `-f`, a REAL option that is nothing like it — is a prefix of \
              `--files-with-matches`. So a pattern FILE switches on `path_only`, which is the \
              one flag that re-opens the whole-line and colon scans on a run that printed no \
              `path:` prefix at all. `grep --file P onefile` is single-operand, so grep prints \
              pure content, and a content line reading `beta.rs: …` takes beta.rs's contract \
              while being about notes.md — the exact defect the `-rh` and `-r … notes.md` rows \
              above forbid, reached through the abbreviation fix instead. The attached spelling \
              `--file=P` is unaffected, which is why this hides: only the separate-value form \
              lands `--file` in `options`",
    },
    Shape {
        args: &["ls", "--form", "across"],
        must: &[],
        must_not: &["  # "],
        why: "`long()` was made abbreviation-tolerant because `--recur` IS `--recursive` to \
              getopt_long — but `long_value`/`format_is`, added in the same round for \
              `--format across`, still compare the FULL spelling. `ls --form across` is accepted \
              by GNU ls and is exactly `ls -C`, so `multi_name` stays false and one file's \
              contract is pasted onto a line naming five, on every line of the layout — the \
              shape the `-C` and `--format across` rows above forbid, reached by abbreviating \
              the option rather than separating its value",
    },
    Shape {
        args: &["ls", "--format", "acr"],
        must: &[],
        must_not: &["  # "],
        why: "the same hole on the VALUE axis: ls resolves `--format`'s argument with \
              XARGMATCH, so an unambiguous abbreviation of the value is accepted too and \
              `--format acr` is `--format across`. `format_is` compares the value for equality, \
              so every abbreviated layout name — `acr`, `hor`, `com` — reads as no layout at \
              all and a multi-name line takes a contract",
    },
];

#[test]
fn every_printed_shape_names_the_file_its_line_is_about() {
    if !have_ls() {
        return;
    }
    let dir = fixture("shapes");
    let ann = |c: &str| format!("// Concern: {c} | Non-concern: n | IO: none\n");
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::create_dir_all(dir.join("sub/sub")).unwrap();
    std::fs::write(dir.join("x.rs"), ann("ROOT-X")).unwrap();
    std::fs::write(dir.join("prose.md"), "docs/x.rs is the docs entry point\n").unwrap();
    std::fs::write(dir.join("docs/x.rs"), ann("DOCS-X")).unwrap();
    std::fs::write(
        dir.join("docs/.annotation"),
        "Concern: DOCS-CHARTER | Non-concern: n | IO: none\n",
    )
    .unwrap();
    std::fs::write(dir.join("sub/n.rs"), format!("{}hit\n", ann("OUTER"))).unwrap();
    std::fs::write(dir.join("sub/sub/n.rs"), format!("{}hit\n", ann("INNER"))).unwrap();
    std::fs::write(dir.join("Makefile"), format!("{}docs: build\n", ann("MK"))).unwrap();
    std::fs::write(dir.join("beta.rs"), ann("BETA")).unwrap();
    std::fs::write(dir.join("delta.rs"), "beta.rs is the thing\n").unwrap();
    // Prose that names an existing file with a COLON after it — the shape a grep content line
    // takes when the invocation printed no `path:` prefix and the scan has nothing to anchor on.
    std::fs::write(dir.join("notes.md"), "beta.rs: the beta file\n").unwrap();
    // A matched line that is EXACTLY a path — indistinguishable from `grep -l` output without
    // parsing flags, which run.rs deliberately does not. Recorded so the behaviour is chosen
    // rather than accidental.
    std::fs::write(dir.join("bare.rs"), "beta.rs\n").unwrap();
    std::fs::write(dir.join("a file name.rs"), ann("SPACED")).unwrap();
    // Whitespace that is NOT a single space. Re-joining tokens with " " silently loses these.
    std::fs::write(dir.join("two  spaces.rs"), ann("TWOSPACE")).unwrap();
    std::fs::write(dir.join("tab\tname.rs"), ann("TABBED")).unwrap();
    // A file whose NAME is a number, to collide with the hour in an `ls -l` timestamp.
    std::fs::write(dir.join("10"), ann("TEN")).unwrap();
    // And one whose name is the BLOCK TOTAL a listing opens with, to collide with `total N`.
    std::fs::write(dir.join("1"), ann("ONE-BLOCK")).unwrap();
    // A pattern FILE, so `-f` has an argv token that exists but is not an operand.
    std::fs::write(dir.join("pats.txt"), "hit\n").unwrap();
    std::fs::write(
        dir.join("gamma.rs"),
        format!("{}see beta.rs\n", ann("GAMMA")),
    )
    .unwrap();
    std::fs::write(dir.join("plain.rs"), ann("PLAIN")).unwrap();
    // A directory whose `.annotation` carries the charter line AND a line of prose under it —
    // the shape a maintainer writes when the charter needs a note. It resolves to no charter at
    // all, so no contract reaches the line.
    std::fs::create_dir_all(dir.join("noisy")).unwrap();
    std::fs::write(
        dir.join("noisy/.annotation"),
        "Concern: NOISY-CHARTER | Non-concern: n | IO: none\nSTRAY-PROSE about this directory\n",
    )
    .unwrap();

    let mut wrong = Vec::new();
    for Shape {
        args,
        must,
        must_not,
        why,
    } in SHAPES
    {
        let text = run(&dir, args);
        for m in must.iter() {
            if !text.contains(m) {
                wrong.push(format!("  {args:?}\n    missing {m:?} ({why})\n{text}"));
            }
        }
        for m in must_not.iter() {
            if text.contains(m) {
                wrong.push(format!(
                    "  {args:?}\n    should NOT contain {m:?} ({why})\n{text}"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} shapes wrong:\n{}",
        wrong.len(),
        SHAPES.len(),
        wrong.join("\n")
    );
}
