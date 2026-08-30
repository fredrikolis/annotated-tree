// Concern: appends the annotator to a Bash pipeline whose stdout the model reads | Non-concern: lexing, the tool table, or annotating a line | IO: (command) -> rewrite + tools, or None

use super::lex::{lex, Kind, Token};
use super::map::shape_of;

fn wrapper_command() -> Option<String> {
    // Absolute: `PATH` may have changed, and failing to start gives the producer SIGPIPE (exit 141).
    let found = std::env::current_exe().ok()?;
    let path = found.to_string_lossy().into_owned();
    Some(format!(
        "'{}' bash-annotator --annotate-tool-output",
        path.replace('\'', "'\\''")
    ))
}

/// Stages that leave the output still a list of path-bearing lines.
const LINE_SAFE: &[&str] = &["head", "tail", "sort", "uniq", "cat", "nl", "tac", "grep"];

/// `LINE_SAFE` stages that also preserve order, dropping only from the END.
const ORDER_PRESERVING: &[&str] = &["cat", "head"];

/// `command`/`sudo`/`env`/`exec` deliberately excluded: each changes how the program resolves.
const WRAPPERS: &[&str] = &["nohup", "nice", "stdbuf"];
/// Same, but consumes one argument of its own first.
const ARGFUL_WRAPPERS: &[&str] = &["timeout"];

pub fn rewrite(command: &str) -> Option<(String, Vec<String>)> {
    let lexed = lex(command);
    if lexed.unmodellable() {
        return None;
    }

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

    // A subshell makes the pipeline ONE command; `${PIPESTATUS[i]}` inside it can't describe the stages.
    if command.contains("PIPESTATUS") {
        return None;
    }

    let annotator = wrapper_command()?;
    let stages = split_stages(&lexed.tokens);

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
        // NUL-delimited output arrives as one unbroken run: no lines to append to.
        if producer.words.iter().any(|t| match &t.kind {
            Kind::Word(w) => nul_delimited(&prog, w),
            _ => false,
        }) {
            continue;
        }
        if pipe
            .iter()
            .any(|&i| stages[i].redirected || stages[i].consumes_args)
        {
            continue;
        }
        if pipe[1..]
            .iter()
            .any(|&i| match program_of(&stages[i].words) {
                Some((_, p)) => !line_safe(&p, &stages[i].words),
                None => true,
            })
        {
            continue;
        }
        // Dropping or reordering a line detaches a `<dir>:` header from the entries it scoped.
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

        let (Some(from), Some(to)) = (
            pipe.iter().filter_map(|&i| stages[i].start).min(),
            pipe.iter().map(|&i| stages[i].end).max(),
        ) else {
            continue;
        };

        // Raw source text, not re-quoted: re-quoting once turned `ls "$d"` into literal `'$d'`.
        let argv: Vec<&str> = producer
            .words
            .iter()
            .filter(|t| t.start >= tok.start)
            .filter(|t| matches!(t.kind, Kind::Word(_)))
            .map(|t| &command[t.start..t.end])
            .collect();

        // `PIPESTATUS` is captured FIRST because any later command replaces it.
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
    // `--recur` IS `--recursive` to getopt_long.
    let recursive = args.iter().any(|w| {
        *w == "--recursive"
            || (w.len() >= 5 && "--recursive".starts_with(*w))
            || (w.starts_with('-') && !w.starts_with("--") && w.contains('R'))
    });
    let operands: Vec<&&str> = args.iter().filter(|w| !w.starts_with('-')).collect();
    let may_expand = operands
        .iter()
        .any(|w| w.contains(['*', '?', '[', '{', '$', '~']));
    recursive || may_expand || operands.len() > 1
}

fn nul_delimited(prog: &str, w: &str) -> bool {
    let long = matches!(
        w.split('=').next().unwrap_or(w),
        "--null" | "--null-data" | "--print0" | "--zero" | "--zero-terminated"
    );
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
    /// Includes redirects and their targets, which a scan over `words` alone cannot see.
    start: Option<usize>,
    end: usize,
    piped_out: bool,
    piped_in: bool,
    redirected: bool,
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
            // After `|`/`&&`/`||`, bash treats a newline as a line continuation, not a separator.
            Kind::Op(op) if op == "\n" && continues => {}
            Kind::Op(op) if matches!(op.as_str(), ";" | "&&" | "||" | "&" | "\n") => {
                stages.push(new_stage());
            }
            Kind::Op(op) => {
                // `2>`/`2>&1` touch stderr only; the rest of the `>` family divert stdout.
                if !op.starts_with('2') && op.contains('>') {
                    stages.last_mut().unwrap().redirected = true;
                }
                // `2>&1`'s `1` lexes separately and once made a directory named `1` the operand.
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
    let short_has = |c: char| {
        flags
            .iter()
            .any(|f| f.starts_with('-') && !f.starts_with("--") && f.contains(c))
    };
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
        "head" | "tail" => !short_has('c') && !flags.iter().any(|f| f.starts_with("--bytes")),
        "cat" => flags.is_empty(),
        // `-l`/`-L` over a stream emit the literal `(standard input)`, not a path.
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
        "tac" | "nl" => !flags
            .iter()
            .any(|f| f.starts_with("-s") || f.starts_with("--separator")),
        _ => true,
    }
}

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
