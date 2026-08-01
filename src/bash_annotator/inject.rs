// Concern: appends the annotator to a Bash pipeline whose stdout the model reads | Non-concern: lexing, the tool table, or annotating a line | IO: (command) -> rewrite + tools, or None

use super::lex::{lex, Kind, Token};
use super::map::shape_of;

/// How to spell the annotator so the rewritten command can actually run.
///
/// The annotator is this same binary, so this is `current_exe()` plus the verb. `None` is
/// effectively unreachable (it needs `current_exe()` itself to fail) and the caller still treats it
/// as "leave the command alone", which stays the correct answer for a process that cannot locate
/// itself.
fn wrapper_command() -> Option<String> {
    // ALWAYS absolute, never the bare name. The rewritten command runs in the agent's shell, whose
    // `PATH` the command itself may have changed — `export PATH=/usr/bin; ls` left a bare name
    // unresolvable, and because the annotator is the READER of the pipe, failing to start it made
    // the producer take SIGPIPE: no output at all, exit 141. An absolute path cannot be stranded.
    let found = std::env::current_exe().ok()?;
    let path = found.to_string_lossy().into_owned();
    // ALWAYS quoted. A conditional rule has to enumerate every shell metacharacter and will miss
    // one — `$`, `(` and whitespace each produced a broken command — and quoting a plain path
    // costs nothing.
    Some(format!(
        "'{}' bash-annotator --annotate-tool-output",
        path.replace('\'', "'\\''")
    ))
}

/// Downstream stages that leave the output still a list of path-bearing lines.
///
/// The annotator is the LAST stage of the pipeline, so these programs see the tool's RAW output
/// and behave exactly as they would without this installed — `sort` orders the real paths, `uniq`
/// compares the real lines. That is why the list can be permissive where an earlier design, which
/// annotated BEFORE them, had to refuse `sort`, `uniq` and `sort -u` outright.
///
/// What matters now is only whether what reaches US is still a list of paths. Each entry here
/// emits a subset, a reordering, or a re-numbering of the lines it read. `wc` is excluded because
/// it emits a COUNT — a number that could itself resolve to a file — and a byte count (`head -c`)
/// is excluded because it can cut a record in half.
/// `sed` is deliberately absent: `sed s/a/b/` rewrites the TEXT of a line, so the path the
/// annotator then reads is not the path the tool printed — it took the substituted name's
/// contract and put it on the original's line. `grep` is present only as a FILTER; the flags that
/// make it emit something other than whole matched lines are refused in `line_safe`.
const LINE_SAFE: &[&str] = &["head", "tail", "sort", "uniq", "cat", "nl", "tac", "grep"];

/// The subset of `LINE_SAFE` that emits its input's lines IN ORDER, dropping only from the END.
///
/// `ls -R` and multi-operand `ls` name entries relative to the last `<dir>:` header they printed,
/// so their output means what it says ONLY in the order printed. A stage that reorders (`sort`,
/// `tac`) hoists headers away from their blocks, and one that drops from anywhere (`tail`,
/// `grep -v`, `uniq`) can delete a header outright; either way the entries under it fall back to
/// the base directory and take a same-named cwd file's contract.
///
/// Keeping the ORDER is necessary but not sufficient: the stage must also leave the line's TEXT
/// alone. A header is recognised by ending in `:` and naming a directory, so `nl` and `cat -n` —
/// which move nothing but prefix every line with a number and a tab — stop it being recognised at
/// all. Only `cat` (identity) and `head` (a leading run) qualify, and `cat`'s own text-altering
/// flags are refused in `line_safe`.
const ORDER_PRESERVING: &[&str] = &["cat", "head"];

/// Stepped over when looking for the program a stage actually runs.
///
/// `command`, `sudo` and `env` are deliberately NOT here. Each changes how the program resolves:
/// `command grep` and `\grep` are the canonical ways to say "the real binary, not the shell
/// function", so honouring them means leaving the command alone; `sudo` runs under `secure_path`
/// and a different `$HOME`; `env -i` clears the environment. Rewriting any of them either inverts
/// the caller's intent or produces a command that exits 127.
/// `exec` is deliberately absent: it REPLACES the shell, so `exec ls; echo x` never reaches the
/// `echo`. Run inside our subshell it would replace only the subshell and the `echo` would start
/// running — a change to what the command does, not to what it prints.
const WRAPPERS: &[&str] = &["nohup", "nice", "stdbuf"];
/// Same, but consumes one argument of its own first.
const ARGFUL_WRAPPERS: &[&str] = &["timeout"];

/// The rewritten command, or `None` to leave it exactly as the agent wrote it.
///
/// Fail-closed throughout: a missed rewrite costs nothing, while a wrong one corrupts a pipeline
/// the agent then debugs having never seen the command that actually ran.
pub fn rewrite(command: &str) -> Option<(String, Vec<String>)> {
    let lexed = lex(command);
    if lexed.unmodellable() {
        return None;
    }

    // A COMPOUND command's redirect and pipe belong to the whole construct, not to any stage
    // inside it, and `split_stages` has no notion of nesting: `for f in docs` newline `do`
    // newline `ls $f` newline `done > out.txt` would rewrite the BODY, writing contracts into
    // the agent's file, and `… done | sort -u` would put a stage AFTER the annotator, voiding the
    // one guarantee that makes downstream stages safe. `{ }` is the same construct spelled with
    // braces. A keyword only counts in COMMAND POSITION, so `grep -rn for src` stays a search.
    const COMPOUND: &[&str] = &[
        "{", "}", "for", "while", "until", "if", "then", "else", "elif", "fi", "do", "done",
        "case", "esac", "select", "function",
    ];
    if split_stages(&lexed.tokens).iter().any(|st| {
        st.words.first().is_some_and(|t| match &t.kind {
            Kind::Word(w) => COMPOUND.contains(&w.as_str()),
            _ => false,
        })
    }) {
        return None;
    }

    // Wrapping a pipeline in a subshell makes it ONE command in the parent's pipeline, so the
    // caller's `${PIPESTATUS[i]}` afterwards describes the subshell rather than the stages. `$?`
    // is preserved; the array cannot be. A command that reads it is left alone rather than
    // answered with a number that is quietly wrong.
    if command.contains("PIPESTATUS") {
        return None;
    }

    let annotator = wrapper_command()?;
    let stages = split_stages(&lexed.tokens);

    // Group stages into PIPELINES. A pipeline is the unit that matters now: the annotator is
    // appended to its end, so every stage inside it sees the tool's raw output.
    let mut pipelines: Vec<Vec<usize>> = Vec::new();
    for (i, st) in stages.iter().enumerate() {
        if st.piped_in {
            if let Some(last) = pipelines.last_mut() {
                last.push(i);
                continue;
            }
        }
        if !st.words.is_empty() {
            pipelines.push(vec![i]);
        }
    }

    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut annotated: Vec<String> = Vec::new();

    for pipe in &pipelines {
        let producer = &stages[pipe[0]];
        let Some((tok, prog)) = program_of(&producer.words) else {
            continue;
        };
        if shape_of(&prog).is_none() {
            continue;
        }
        // The annotator frames records by NEWLINE. A producer asked for NUL-delimited output emits
        // one unbroken run instead, so there are no lines to append to and nothing may be added.
        if producer.words.iter().any(|t| match &t.kind {
            Kind::Word(w) => nul_delimited(&prog, w),
            _ => false,
        }) {
            continue;
        }
        // The pipeline's stdout must be what the model reads. A redirect anywhere in it, or a
        // stage that consumes its input as arguments, takes the answer somewhere else — and
        // appending the annotator BEFORE a redirect would write contracts into the agent's file.
        if pipe
            .iter()
            .any(|&i| stages[i].redirected || stages[i].consumes_args)
        {
            continue;
        }
        // Every downstream stage must leave a list of path-bearing lines for us to annotate.
        if pipe[1..]
            .iter()
            .any(|&i| match program_of(&stages[i].words) {
                Some((_, p)) => !line_safe(&p, &stages[i].words),
                None => true,
            })
        {
            continue;
        }
        // A producer whose lines are scoped by `<dir>:` headers needs MORE than line safety from
        // what follows: it needs the order kept and nothing dropped from the middle, or a header
        // ends up detached from the entries it scoped and each one resolves against the base.
        if header_scoped(&prog, &producer.words)
            && pipe[1..]
                .iter()
                .any(|&i| match program_of(&stages[i].words) {
                    Some((_, p)) => !ORDER_PRESERVING.contains(&p.as_str()),
                    None => true,
                })
        {
            continue;
        }

        // The span of the pipeline AS WRITTEN. Taken from token offsets so the agent's own
        // spacing, quoting and globs are reproduced byte for byte inside the parentheses.
        let (Some(from), Some(to)) = (
            pipe.iter().filter_map(|&i| stages[i].start).min(),
            pipe.iter().map(|&i| stages[i].end).max(),
        ) else {
            continue;
        };

        // The producer's argv, from its program token onward — `timeout 5 grep …` describes a
        // `grep` run, not a `timeout` one — so the annotator reads the flag rules that decide
        // which base a bare name resolves against rather than guessing them.
        // Forwarded as RAW SOURCE TEXT, not as re-quoted lexed words, so the shell performs the
        // SAME expansion for the annotator that it performs for the tool. Re-quoting turned
        // `ls "$d"` into the literal `'$d'`, which resolved to nothing, dropped the base back to
        // the cwd, and put a same-named cwd file's contract on the line.
        let argv: Vec<&str> = producer
            .words
            .iter()
            .filter(|t| t.start >= tok.start)
            .filter(|t| matches!(t.kind, Kind::Word(_)))
            .map(|t| &command[t.start..t.end])
            .collect();

        // Re-raise the status the command would have had. Without `pipefail` that is the stage
        // that was LAST before the annotator was appended, so `grep`'s exit 1 on no-match
        // survives and `ls | sort` still reports sort's. WITH `pipefail` bash takes the rightmost
        // FAILING stage instead, and a bare `exit ${PIPESTATUS[n]}` erased exactly the no-match
        // exit this is here to protect — `pipefail` may also have been set by an earlier command
        // in the agent's persistent shell, so it cannot be ruled out by reading this one.
        //
        // `PIPESTATUS` is captured FIRST: any later command, an assignment included, replaces it.
        // The subshell keeps `exit` from ending the agent's own shell, and wraps only the
        // pipeline, so a `cd` earlier in the command still persists.
        let n = pipe.len() - 1;
        edits.push((
            from,
            to,
            format!(
                "( {} | {annotator} {} ; __ps=(\"${{PIPESTATUS[@]}}\"); __rc=${{__ps[{n}]}}; \
                 [[ -o pipefail ]] && for __s in \"${{__ps[@]}}\"; do ((__s)) && __rc=$__s; \
                 done; exit $__rc )",
                &command[from..to],
                argv.join(" ")
            ),
        ));
        annotated.push(prog);
    }

    if edits.is_empty() {
        return None;
    }
    let mut out = command.to_string();
    // Right to left, so earlier spans stay valid.
    for (from, to, text) in edits.into_iter().rev() {
        out.replace_range(from..to, &text);
    }
    annotated.sort();
    annotated.dedup();
    Some((out, annotated))
}

/// Does this invocation print `<dir>:` block headers, making its output order load-bearing?
///
/// `ls` does so when recursing, and whenever it was given more than one operand. The operand
/// count is approximated by counting non-flag words — an over-count only ever refuses a rewrite,
/// which is free, while an under-count would let a wrong contract through.
fn header_scoped(prog: &str, words: &[&Token]) -> bool {
    if prog != "ls" {
        return false;
    }
    let args: Vec<&str> = words
        .iter()
        .skip(1)
        .filter_map(|t| match &t.kind {
            Kind::Word(w) => Some(w.as_str()),
            _ => None,
        })
        .collect();
    // `--recur` IS `--recursive` to getopt_long, and matching only the full spelling let an
    // abbreviation past the guard that the full spelling stops.
    let recursive = args.iter().any(|w| {
        *w == "--recursive"
            || (w.len() >= 5 && "--recursive".starts_with(*w))
            || (w.starts_with('-') && !w.starts_with("--") && w.contains('R'))
    });
    let operands: Vec<&&str> = args.iter().filter(|w| !w.starts_with('-')).collect();
    // A glob or a variable is ONE word HERE and many operands by the time the shell has run —
    // `ls */ | sort` reaches the annotator as a multi-directory listing whose headers `sort` has
    // already hoisted. Counting source words alone missed exactly that.
    let may_expand = operands
        .iter()
        .any(|w| w.contains(['*', '?', '[', '{', '$', '~']));
    recursive || may_expand || operands.len() > 1
}

/// Does this argument make the producer emit NUL-delimited records rather than lines?
fn nul_delimited(prog: &str, w: &str) -> bool {
    let long = matches!(
        w.split('=').next().unwrap_or(w),
        "--null" | "--null-data" | "--print0" | "--zero" | "--zero-terminated"
    );
    // `find`'s output-shaping predicates are words, not clusters, and each stops its lines being
    // plain paths: `-printf '%f'` prints BARE names from nested directories, which then resolve
    // against the cwd and take a same-named cwd file's contract; `-ls` prints a stat line;
    // `-fprint*` divert to a file; and `-printx` — a bfs predicate, which matters because bfs is
    // what `find` resolves to in a Claude Code session — ESCAPES whitespace and quotes, so
    // `./a\ b.rs` no longer names the file and the suffix scan falls back to a shorter name that
    // does. `-Z`/`-z` hide inside a grep cluster.
    long || (prog == "find"
        && matches!(
            w,
            "-print0"
                | "-printf"
                | "-printx"
                | "-fprintf"
                | "-fprint"
                | "-fprint0"
                | "-ls"
                | "-fls"
        ))
        || (w.starts_with('-') && !w.starts_with("--") && w.contains(['Z', 'z']))
}

struct Stage<'a> {
    words: Vec<&'a Token>,
    /// Byte offsets of this stage's FIRST and LAST tokens — redirects and their targets
    /// included, which a scan over `words` alone cannot see. A leading `2>/dev/null` left outside
    /// the parentheses is a bash syntax error; a trailing one silences the annotator too.
    start: Option<usize>,
    end: usize,
    /// This stage's stdout goes to the next one.
    piped_out: bool,
    /// This stage reads another stage's stdout.
    piped_in: bool,
    /// A `>`-family operator sends this stage's stdout somewhere other than the tool result.
    redirected: bool,
    /// The stage's output is consumed as arguments rather than read.
    consumes_args: bool,
}

fn split_stages<'a>(tokens: &'a [Token]) -> Vec<Stage<'a>> {
    let mut stages: Vec<Stage<'a>> = vec![new_stage()];
    let mut expect_target = false;
    // Did the previous token leave the command syntactically incomplete?
    let mut continues = false;
    for t in tokens {
        if let Kind::Op(op) = &t.kind {
            if op != "\n" {
                continues = matches!(op.as_str(), "|" | "|&" | "&&" | "||");
            }
        } else {
            continues = false;
        }
        match &t.kind {
            Kind::Op(op) if op == "|" || op == "|&" => {
                stages.last_mut().unwrap().piped_out = true;
                let mut next = new_stage();
                next.piped_in = true;
                stages.push(next);
            }
            // After `|`, `&&` or `||` bash treats a newline as a line CONTINUATION. Splitting on
            // it produced an empty stage, `program_of` returned None, and every multi-line
            // pipeline — `grep -rn foo . |` newline `head -20`, which agents write constantly —
            // was refused.
            Kind::Op(op) if op == "\n" && continues => {}
            Kind::Op(op) if matches!(op.as_str(), ";" | "&&" | "||" | "&" | "\n") => {
                stages.push(new_stage());
            }
            Kind::Op(op) => {
                // `2>` and `2>&1` touch stderr only; everything else in the family takes stdout
                // somewhere the model will not read.
                if !op.starts_with('2') && op.contains('>') {
                    stages.last_mut().unwrap().redirected = true;
                }
                // The word after ANY redirect names the destination, not an argument. Counting it
                // as one forwarded `/dev/null` to the annotator as `ls 2>/dev/null`'s operand.
                if op.contains('>') || op.contains('<') {
                    // EVERY redirect names a destination in the next word, `2>&1` included — its
                    // `1` lexes separately, and counting it as an argument made a directory named
                    // `1` the operand and rebased every name in the listing.
                    expect_target = true;
                    let s = stages.last_mut().unwrap();
                    s.start.get_or_insert(t.start);
                    s.end = t.end;
                }
            }
            Kind::Word(w) => {
                let s = stages.last_mut().unwrap();
                s.start.get_or_insert(t.start);
                s.end = t.end;
                if expect_target {
                    expect_target = false;
                    continue;
                }
                // All four spellings of find's run-a-program family, not just the two that
                // happened to be written down.
                if matches!(w.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
                    s.consumes_args = true;
                }
                s.words.push(t);
            }
        }
    }
    stages
}

fn new_stage<'a>() -> Stage<'a> {
    Stage {
        words: Vec::new(),
        start: None,
        end: 0,
        piped_out: false,
        piped_in: false,
        redirected: false,
        consumes_args: false,
    }
}

fn line_safe(prog: &str, words: &[&Token]) -> bool {
    if !LINE_SAFE.contains(&prog) {
        return false;
    }
    let flags: Vec<&str> = words
        .iter()
        .skip(1)
        .filter_map(|t| match &t.kind {
            Kind::Word(w) => Some(w.as_str()),
            _ => None,
        })
        .collect();
    // A short cluster like `-qc` hides its byte flag inside the cluster.
    let short_has = |c: char| {
        flags
            .iter()
            .any(|f| f.starts_with('-') && !f.starts_with("--") && f.contains(c))
    };
    // A stage can take stdout away with a FLAG rather than with `>`, and it can redefine the
    // record as NUL-terminated rather than newline-terminated. Either breaks a guarantee stated
    // in terms of lines reaching the model, and neither is specific to one program, so both are
    // refused before any per-program rule runs.
    let diverts_or_reframes = |f: &&str| -> bool {
        let name = f.split('=').next().unwrap_or(f);
        matches!(
            name,
            "-o" | "--output" | "-z" | "--zero-terminated" | "--null-data"
        ) || short_has('z')
    };
    if flags.iter().any(diverts_or_reframes) {
        return false;
    }

    match prog {
        // A byte count can cut a record in half.
        "head" | "tail" => !short_has('c') && !flags.iter().any(|f| f.starts_with("--bytes")),
        // `cat` is only identity without flags. `-n`/`-b` number the lines, and `-A`/`-e`/`-t`/
        // `-v`/`-E`/`-T` render invisible characters — each rewrites the text of the line the
        // annotator then has to read a path out of.
        "cat" => flags.is_empty(),
        // A filter may only DROP lines. `-c` emits a count — the very thing `wc` is refused for —
        // `-o` emits the matched fragment rather than the line, and `-l`/`-L` over a STREAM emit
        // the literal `(standard input)` rather than any path at all.
        "grep" => {
            !short_has('c')
                && !short_has('o')
                && !short_has('l')
                && !short_has('L')
                && !flags.iter().any(|f| {
                    f.starts_with("--count")
                        || f.starts_with("--only-matching")
                        || f.starts_with("--files-with")
                        || f.starts_with("--files-without")
                })
        }
        // `tac -s` / `nl -s` redefine the record separator, so "the unit is a line" stops holding.
        "tac" | "nl" => !flags
            .iter()
            .any(|f| f.starts_with("-s") || f.starts_with("--separator")),
        _ => true,
    }
}

/// The token that names the program a stage runs, and that name as written.
///
/// Only `nohup`, `nice`, `stdbuf`, `exec` and `timeout` are stepped over, and leading `VAR=value`
/// assignments are skipped. `command`, `sudo` and `env` deliberately are NOT: each changes how the
/// program resolves, and the caller who typed them is asking for exactly that. The name is
/// returned verbatim — no directory is stripped — so the caller can require a bare name.
fn program_of<'a>(words: &[&'a Token]) -> Option<(&'a Token, String)> {
    let mut i = 0;
    while i < words.len() {
        let Kind::Word(text) = &words[i].kind else {
            return None;
        };
        if is_assignment(text) {
            i += 1;
            continue;
        }
        if WRAPPERS.contains(&text.as_str()) {
            i += 1;
            continue;
        }
        if ARGFUL_WRAPPERS.contains(&text.as_str()) {
            i += 2;
            continue;
        }
        return Some((words[i], text.clone()));
    }
    None
}

fn is_assignment(w: &str) -> bool {
    match w.find('=') {
        None | Some(0) => false,
        Some(p) => {
            let name = &w[..p];
            name.starts_with(|c: char| c.is_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        }
    }
}
