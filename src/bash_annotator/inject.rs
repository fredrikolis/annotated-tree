// Concern: appends the annotator to a Bash pipeline whose stdout the model reads | Non-concern: lexing, the tool table, or annotating a line | IO: (command) -> rewrite + tools, or None

use super::lex::{lex, Kind, Token};
use super::map::shape_of;

/// How to spell the annotator so the rewritten command can actually run: `current_exe()` plus the
/// verb, since the annotator is this same binary. `None` is effectively unreachable — it needs
/// `current_exe()` itself to fail — and the caller then treats it as "leave the command alone",
/// which stays the correct answer for a process that cannot locate itself.
fn wrapper_command() -> Option<String> {
    // ALWAYS absolute: the command may itself have changed `PATH`, and since the annotator is the READER of the pipe, failing to start it gives the producer SIGPIPE — no output at all, exit 141.
    let found = std::env::current_exe().ok()?;
    let path = found.to_string_lossy().into_owned();
    Some(format!(
        "'{}' bash-annotator --annotate-tool-output",
        path.replace('\'', "'\\''")
    ))
}

/// Downstream stages that leave the output still a list of path-bearing lines. The annotator is
/// the LAST stage, so these see the tool's RAW output; each emits a subset, a reordering or a
/// re-numbering of what it read. `wc` and `head -c` are excluded (a count, and a cut record),
/// `sed` because it rewrites line TEXT, and `grep` is present only as a filter.
const LINE_SAFE: &[&str] = &["head", "tail", "sort", "uniq", "cat", "nl", "tac", "grep"];

/// The subset of `LINE_SAFE` that emits its input's lines IN ORDER, dropping only from the END.
/// `ls -R` names entries relative to the last `<dir>:` header printed, so reordering or dropping
/// detaches a header from its block. Order alone is not enough — the TEXT must survive too, which
/// is why `nl` and `cat -n` are out: a numbered line stops looking like a header.
const ORDER_PRESERVING: &[&str] = &["cat", "head"];

/// Stepped over when looking for the program a stage actually runs. `command`, `sudo` and `env` are
/// deliberately NOT here — each changes how the program resolves, so rewriting one inverts the
/// caller's intent or produces a command that exits 127. `exec` is absent too: it REPLACES the
/// shell, so inside our subshell a following `echo` would start running.
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

    // A compound command's redirect and pipe belong to the whole construct, but `split_stages` has no notion of nesting — so `done > out.txt` would rewrite the BODY and write contracts into the agent's file. Command position only: `grep -rn for src` stays a search.
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

    // A subshell makes the pipeline ONE command to the parent, so `${PIPESTATUS[i]}` would describe the subshell, not the stages. `$?` survives; the array cannot.
    if command.contains("PIPESTATUS") {
        return None;
    }

    let annotator = wrapper_command()?;
    let stages = split_stages(&lexed.tokens);

    // The pipeline, not the stage, is the unit: the annotator is appended to its END, so every stage inside it sees the tool's raw output.
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
        // The annotator frames records by NEWLINE, so NUL-delimited output arrives as one unbroken run with no lines to append to.
        if producer.words.iter().any(|t| match &t.kind {
            Kind::Word(w) => nul_delimited(&prog, w),
            _ => false,
        }) {
            continue;
        }
        // The pipeline's stdout must be what the model reads, and appending the annotator BEFORE a redirect would write contracts into the agent's file.
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
        // Header-scoped output needs MORE than line safety downstream: drop or reorder a line and a `<dir>:` header detaches from the entries it scoped, which then resolve against the base.
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

        // Taken from token offsets so the agent's own spacing, quoting and globs are reproduced byte for byte inside the parentheses.
        let (Some(from), Some(to)) = (
            pipe.iter().filter_map(|&i| stages[i].start).min(),
            pipe.iter().map(|&i| stages[i].end).max(),
        ) else {
            continue;
        };

        // From the program token onward (`timeout 5 grep …` describes a `grep` run) and forwarded as RAW SOURCE TEXT, so the shell expands it identically for the annotator — re-quoting turned `ls "$d"` into a literal `'$d'` that resolved to nothing and put a cwd file's contract on the line.
        let argv: Vec<&str> = producer
            .words
            .iter()
            .filter(|t| t.start >= tok.start)
            .filter(|t| matches!(t.kind, Kind::Word(_)))
            .map(|t| &command[t.start..t.end])
            .collect();

        // Re-raise the status the command would have had: `pipefail` may have been set by an earlier command in the agent's persistent shell, so it cannot be ruled out by reading this one. `PIPESTATUS` is captured FIRST because any later command replaces it.
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

/// Does this invocation print `<dir>:` block headers, making its output order load-bearing? `ls`
/// does when recursing, and whenever given more than one operand. The operand count is approximated
/// by counting non-flag words: an over-count only refuses a rewrite, which is free, while an
/// under-count would let a wrong contract through.
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
    // `--recur` IS `--recursive` to getopt_long, so matching only the full spelling lets an abbreviation past the guard.
    let recursive = args.iter().any(|w| {
        *w == "--recursive"
            || (w.len() >= 5 && "--recursive".starts_with(*w))
            || (w.starts_with('-') && !w.starts_with("--") && w.contains('R'))
    });
    let operands: Vec<&&str> = args.iter().filter(|w| !w.starts_with('-')).collect();
    // A glob or variable is ONE word here and many operands once the shell has run: `ls */ | sort` arrives as a multi-directory listing whose headers `sort` already hoisted.
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
    // `find`'s output-shaping predicates are words, not clusters, and each stops its lines being plain paths — `-printf '%f'` prints BARE names, `-ls` a stat line, `-printx` escapes whitespace. `-Z`/`-z` hide inside a grep cluster.
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
            // After `|`, `&&` or `||` bash treats a newline as a line CONTINUATION; splitting on it yields an empty stage and refuses every multi-line pipeline.
            Kind::Op(op) if op == "\n" && continues => {}
            Kind::Op(op) if matches!(op.as_str(), ";" | "&&" | "||" | "&" | "\n") => {
                stages.push(new_stage());
            }
            Kind::Op(op) => {
                // `2>` and `2>&1` touch stderr only; everything else in the family takes stdout somewhere the model will not read.
                if !op.starts_with('2') && op.contains('>') {
                    stages.last_mut().unwrap().redirected = true;
                }
                // The word after ANY redirect names a destination, not an argument — `2>&1` included, whose `1` lexes separately and once made a directory named `1` the operand.
                if op.contains('>') || op.contains('<') {
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
                // All four spellings of find's run-a-program family.
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
    // A stage can divert stdout with a FLAG rather than `>`, or reframe the record as NUL-terminated — neither is program-specific, so both are refused before any per-program rule runs.
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
        // `cat` is identity only without flags: `-n`/`-b` number the lines and `-A`/`-e`/`-t`/`-v`/`-E`/`-T` render invisible characters, each rewriting the text a path must be read out of.
        "cat" => flags.is_empty(),
        // A filter may only DROP lines: `-c` emits a count, `-o` the matched fragment, and `-l`/`-L` over a STREAM emit the literal `(standard input)` rather than any path.
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

/// The token that names the program a stage runs, and that name as written. Only `nohup`, `nice`,
/// `stdbuf`, `exec` and `timeout` are stepped over, and leading `VAR=value` assignments skipped.
/// `command`, `sudo` and `env` deliberately are NOT: each changes how the program resolves, and the
/// caller who typed them is asking for that. The name is returned verbatim, no directory stripped.
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
