// Concern: appends each printed path's contract to the line it appeared on, once per path | Non-concern: running the tool, or command eligibility | IO: (producer argv, stdin) -> those bytes, annotated

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::contracts::Contracts;
use crate::map::{shape_of, Shape};

/// Is this something we can read a contract from without risking a block?
///
/// `metadata()` follows symlinks, so a dangling link is excluded here too, and a FIFO or device
/// never reaches the open in `contracts.rs`.
fn describable(p: &Path) -> bool {
    p.metadata().is_ok_and(|m| m.is_file() || m.is_dir())
}

/// Bound on the resolution memo.
///
/// Every candidate string a line offers becomes a key, so an unbounded map retains a copy of the
/// output: a 29 MB `grep` result held 275 MB of keys. Past this many entries the cache stops
/// growing and resolution simply costs a `stat` again — slower, never wrong.
const RESOLVE_CACHE_MAX: usize = 8192;

/// Annotate `input` — the output of the pipeline whose producer was `producer` — onto `out`.
///
/// The tool is NOT run here. It has already run, as the shell's own `grep`/`find`/`ls`, and this
/// reads what it printed. `producer` is that tool's argv, forwarded by the rewrite so the flag
/// rules below (which base a bare name resolves against, whether the layout puts one name on a
/// line) are read rather than guessed.
pub fn annotate(producer: &[OsString], input: &mut impl BufRead, out: &mut impl Write) -> i32 {
    let Some((tool, args)) = producer.split_first() else {
        return 0;
    };
    let mut ann = Annotator::new(&tool.to_string_lossy(), args);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        // `read_until` keeps the delimiter, which is how a final line with NO newline stays that
        // way: `split` would drop the distinction and `writeln!` would invent one.
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            // A failed write means the reader is gone (`| head -1` upstream of us closed early).
            Ok(_) => {
                if ann.line(&buf, out).is_err() {
                    break;
                }
            }
        }
    }
    let _ = out.flush();
    0
}

/// Appends contracts to a tool's output.
///
/// THE INVARIANT: one line in, one line out. A contract is appended to the line its path appeared
/// on and never printed on a line of its own, so a downstream `head -20`, `tail`, `wc -l`, `sort`
/// or `uniq` — every modifier whose smallest unit is a line — means exactly what it meant without
/// this wrapper.
struct Annotator {
    contracts: Contracts,
    /// The ONE directory a bare name is relative to. `ls <onedir>` names entries relative to that
    /// directory; every other shape prints paths that already carry their operand, so offering a
    /// second base would let a same-named file elsewhere win.
    base: PathBuf,
    /// Set by a `<dir>:` header line and searched first, which is how `ls -R` and multi-operand
    /// listings resolve without this knowing either exists.
    current: Option<PathBuf>,
    seen: HashSet<PathBuf>,
    /// Keyed by (base directory, candidate): the same basename under `ls -R` means a different
    /// file in each block, and a bare-candidate key made every later block reuse the first hit.
    resolved: HashMap<(Option<PathBuf>, String), Option<PathBuf>>,
    /// The tool prints FILE CONTENT after its path, so anything past the leading path is a match,
    /// not a subject. True for `grep`; false for `ls` and `find`, which put the path at one end.
    content_lines: bool,
    /// The tool prints a `path:` prefix on its content lines. When it does not — a single file
    /// operand, or `-h` — a line has NO subject and must carry no contract.
    prefixed: bool,
    /// The whole line is a path, because the invocation asked for paths only (`grep -l`). Without
    /// this, every content line was tried as a path: a match line reading `beta.rs` took that
    /// file's contract, and each attempt left a copy of the line in the memo.
    path_only: bool,
    /// The invocation prints `<dir>:` block headers (`ls -R`). Without this, ANY line ending in a
    /// colon that named a directory rescoped resolution — a file called `docs:` put `docs/x.rs`'s
    /// contract on a later line about `./x.rs`.
    block_headers: bool,
    /// The layout puts SEVERAL names on one line (`ls -C`, `-x`, `-m`), so no line has a single
    /// subject and none can carry a contract.
    multi_name: bool,
}

/// `ls` options that consume the NEXT argument. Knowing which flags take a value is far less than
/// knowing what flags mean, and it is the minimum needed to tell an operand from a flag's value:
/// `ls -I sub` lists the working directory while EXCLUDING `sub`.
///
/// `--color` is deliberately absent: its argument is OPTIONAL, and `getopt_long` never takes a
/// separate token for one, so `ls --color docs` lists `docs`.
const LS_VALUE_LONG: &[&str] = &[
    "--ignore",
    "--hide",
    "--width",
    "--tabsize",
    "--sort",
    "--time",
    "--block-size",
    "--format",
    "--quoting-style",
    "--time-style",
    "--indicator-style",
];

/// `grep`'s long options that consume the NEXT argv entry. Without this the pattern in
/// `grep --regexp PAT file` was left unconsumed and counted as a second operand, so a single-file
/// search looked multi-file, believed it printed `path:` prefixes, and put the pattern's namesake
/// contract on a line of file content.
const GREP_VALUE_LONG: &[&str] = &[
    "--regexp",
    "--file",
    "--max-count",
    "--after-context",
    "--before-context",
    "--context",
    "--binary-files",
    "--devices",
    "--directories",
    "--exclude",
    "--exclude-dir",
    "--exclude-from",
    "--include",
    "--label",
    "--group-separator",
];

/// EVERY long option each tool has, not only the ones consulted below.
///
/// `getopt_long` accepts any UNAMBIGUOUS abbreviation, so a guard that compares full spellings is
/// evaded by `ls --recur`. Resolving an abbreviation needs the whole option set, for two reasons:
/// an abbreviation is only unambiguous with respect to all of them, and a candidate that is
/// itself another option's full spelling is THAT option, never an abbreviation of a longer one.
/// Missing the second is what made `grep --file P onefile` — `--file` being the long spelling of
/// `-f` — read as `--files-with-matches` and hand a content line another file's contract.
const LS_LONG: &[&str] = &[
    "--all",
    "--almost-all",
    "--author",
    "--escape",
    "--block-size",
    "--ignore-backups",
    "--color",
    "--directory",
    "--dired",
    "--classify",
    "--file-type",
    "--format",
    "--full-time",
    "--group-directories-first",
    "--no-group",
    "--human-readable",
    "--si",
    "--dereference",
    "--dereference-command-line",
    "--dereference-command-line-symlink-to-dir",
    "--hide",
    "--hyperlink",
    "--indicator-style",
    "--inode",
    "--ignore",
    "--kibibytes",
    "--literal",
    "--numeric-uid-gid",
    "--hide-control-chars",
    "--show-control-chars",
    "--quote-name",
    "--quoting-style",
    "--reverse",
    "--recursive",
    "--size",
    "--sort",
    "--time",
    "--time-style",
    "--tabsize",
    "--width",
    "--context",
    "--zero",
    "--version",
    "--help",
];

const GREP_LONG: &[&str] = &[
    "--extended-regexp",
    "--fixed-strings",
    "--basic-regexp",
    "--perl-regexp",
    "--regexp",
    "--file",
    "--ignore-case",
    "--no-ignore-case",
    "--word-regexp",
    "--line-regexp",
    "--null-data",
    "--no-messages",
    "--invert-match",
    "--version",
    "--help",
    "--max-count",
    "--byte-offset",
    "--line-number",
    "--line-buffered",
    "--with-filename",
    "--no-filename",
    "--label",
    "--only-matching",
    "--quiet",
    "--silent",
    "--binary-files",
    "--text",
    "--directories",
    "--devices",
    "--recursive",
    "--dereference-recursive",
    "--include",
    "--exclude",
    "--exclude-from",
    "--exclude-dir",
    "--files-without-match",
    "--files-with-matches",
    "--count",
    "--initial-tab",
    "--null",
    "--before-context",
    "--after-context",
    "--context",
    "--group-separator",
    "--no-group-separator",
    "--color",
    "--colour",
    "--binary",
];

fn long_names(tool: &str) -> &'static [&'static str] {
    match tool {
        "grep" => GREP_LONG,
        "ls" => LS_LONG,
        _ => &[],
    }
}

/// Does the argv entry `w` resolve to the option `name`, abbreviations included?
fn long_matches(w: &str, name: &str, all: &[&str]) -> bool {
    if w == name {
        return true;
    }
    // An exact spelling of some OTHER option is that option, never an abbreviation.
    if !name.starts_with(w) || all.contains(&w) || w.len() < 3 {
        return false;
    }
    all.iter().filter(|o| o.starts_with(w)).count() == 1
}

/// Which long options take a value depends on the TOOL, exactly as the short ones do.
fn value_long(tool: &str) -> &'static [&'static str] {
    match tool {
        "grep" => GREP_VALUE_LONG,
        "ls" => LS_VALUE_LONG,
        _ => &[],
    }
}

/// Short options that consume an argument.
const LS_VALUE_SHORT: &str = "IwT";

/// The same for `grep`. Applying `ls`'s table to `grep` mis-parsed clusters both ways: `-Ir` read
/// as `-I` + an attached value, losing the `r` and every contract with it, while `-e`/`-f` were
/// not seen to take a value at all.
const GREP_VALUE_SHORT: &str = "efmABCdD";

/// Which short options take a value depends on the TOOL. `find` has none of this shape — its
/// predicates are long-form (`-name x`), handled by the operand walk below.
fn value_short(tool: &str) -> &'static str {
    match tool {
        "grep" => GREP_VALUE_SHORT,
        "ls" => LS_VALUE_SHORT,
        _ => "",
    }
}

/// What a short-option cluster means. Both questions the caller can ask about a cluster are
/// answered HERE, once: asking them separately produced two different rules and a wrong-contract
/// defect from each (`-Idist` read as `-d`; `-Iw docs` consumed `docs`).
struct Cluster {
    /// The option letters, stopping before any attached value.
    letters: String,
    /// The cluster ends with a value option, so the NEXT argv entry is its value.
    takes_next: bool,
}

/// Walk a `-abc`-style token left to right. The FIRST value option ends the option letters —
/// everything after it in the same token is that option's attached value, not more options.
fn cluster_of_for(word: &str, value_letters: &str) -> Cluster {
    let mut letters = String::new();
    let mut rest = word.chars().skip(1);
    let mut takes_next = false;
    while let Some(c) = rest.next() {
        letters.push(c);
        if !value_letters.is_empty() && value_letters.contains(c) {
            // `-I pat` takes the next entry; `-Ipat` carries its value already.
            takes_next = rest.next().is_none();
            break;
        }
    }
    Cluster {
        letters,
        takes_next,
    }
}

/// The argv entries that name something to operate on, rather than flags or their values.
///
/// For `grep` the FIRST non-flag entry is the PATTERN, not a file. Counting it as one — which
/// `p.exists()` alone does whenever the search term happens to name a path, as in
/// `grep src Makefile` — made a single-file invocation look like a multi-file one, so its content
/// lines were scanned for a `path:` prefix that grep never printed. `-e`/`-f` supply the pattern
/// separately, and then every non-flag entry really is a file.
struct Argv {
    /// Only the entries that are OPTIONS — never an option's value, never an operand.
    options: Vec<String>,
    /// Only the entries that name something to operate on.
    operands: Vec<PathBuf>,
}

/// Split an argv once, into options and operands.
///
/// Both halves must come from the SAME walk. Deriving them separately meant a flag's value was
/// re-read as an option — `grep -e -l -e beta .` searches for the literal `-l` and switched on
/// path-only mode — and, for `grep`, that the PATTERN was counted as a file: `grep src Makefile`
/// then looked like a multi-file search and believed it printed `path:` prefixes it never did.
/// `-e`/`-f` and their long spellings supply the pattern separately, and then every non-flag
/// entry really is a file.
fn parse_argv(tool: &str, args: &[OsString]) -> Argv {
    let values = value_short(tool);
    let longs = value_long(tool);
    let words: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    let mut options: Vec<String> = Vec::new();
    let mut operands: Vec<PathBuf> = Vec::new();
    let mut skip_next = false;
    // First pass collects the options, which is what decides whether the pattern is positional.
    for w in &words {
        if skip_next {
            skip_next = false;
            continue;
        }
        if w.starts_with("--") {
            let name = w.split('=').next().unwrap_or(w);
            skip_next = longs.contains(&name) && !w.contains('=');
            options.push(w.clone());
        } else if w.starts_with('-') && w.len() > 1 {
            skip_next = cluster_of_for(w, values).takes_next;
            options.push(w.clone());
        }
    }
    let pattern_is_positional = tool == "grep"
        && !options.iter().any(|w| {
            w.starts_with("--regexp")
                || w.starts_with("--file")
                || (!w.starts_with("--") && cluster_of_for(w, values).letters.contains(['e', 'f']))
        });
    let mut pattern_seen = !pattern_is_positional;

    skip_next = false;
    for w in &words {
        if skip_next {
            skip_next = false;
            continue;
        }
        if w.starts_with("--") {
            let name = w.split('=').next().unwrap_or(w);
            skip_next = longs.contains(&name) && !w.contains('=');
            continue;
        }
        if w.starts_with('-') && w.len() > 1 {
            skip_next = cluster_of_for(w, values).takes_next;
            continue;
        }
        if !pattern_seen {
            pattern_seen = true;
            continue;
        }
        let p = PathBuf::from(w);
        if p.exists() {
            operands.push(p);
        }
    }
    Argv { options, operands }
}

impl Annotator {
    fn new(tool: &str, args: &[OsString]) -> Self {
        let Argv { options, operands } = parse_argv(tool, args);
        // Does any single-dash cluster carry this option letter? Only the LETTERS are searched,
        // never an attached value, so `-Idist` is not read as `-d`.
        let values = value_short(tool);
        let has = |c: char| {
            options
                .iter()
                .any(|w| !w.starts_with("--") && cluster_of_for(w, values).letters.contains(c))
        };
        let names = long_names(tool);
        let long = |name: &str| {
            options
                .iter()
                .any(|w| long_matches(w.split('=').next().unwrap_or(w), name, names))
        };

        // ONE base, chosen by how this tool prints. `ls <onedir>` names entries relative to that
        // directory and nothing else does — `grep`/`find` paths already contain the operand, and
        // `ls FILE DIR` mixes a cwd-relative file with a header-scoped block. Offering both bases
        // to one resolver is what put another file's contract on the line.
        let lists_itself = has('d') || long("--directory");
        let dir_relative =
            tool == "ls" && !lists_itself && operands.len() == 1 && operands[0].is_dir();
        let recursive = has('r') || has('R') || long("--recursive");

        // `--format=` spells the column layouts that `-C`/`-x`/`-m` select.
        // A long option's value may be ATTACHED (`--format=across`) or the NEXT argv entry
        // (`--format across`). Comparing only the attached spelling let a column layout through,
        // and one file's contract was then appended to a line naming five.
        let long_value = |name: &str| -> Option<String> {
            let mut want_next = false;
            for a in args {
                let w = a.to_string_lossy();
                if want_next {
                    return Some(w.into_owned());
                }
                match w.split_once('=') {
                    Some((opt, v)) if long_matches(opt, name, names) => return Some(v.to_string()),
                    _ => want_next = long_matches(&w, name, names),
                }
            }
            None
        };
        // `ls` resolves `--format`'s ARGUMENT with XARGMATCH, so `acr` is `across` just as
        // `--form` is `--format`. Both spellings select a column layout, and both were missed.
        let format = long_value("--format").unwrap_or_default();
        let format_is = |v: &str| !format.is_empty() && v.starts_with(format.as_str());

        Annotator {
            contracts: Contracts::new(),
            base: if dir_relative {
                operands[0].clone()
            } else {
                PathBuf::from(".")
            },
            current: None,
            seen: HashSet::new(),
            resolved: HashMap::new(),
            content_lines: shape_of(tool) == Some(Shape::Prefixed),
            path_only: has('l')
                || has('L')
                || long("--files-with-matches")
                || long("--files-without-match"),
            // Does this `grep` print a `path:` prefix at all? It does not for a single file
            // operand, and `-h` suppresses it outright. Without knowing, every colon in a line
            // was tried as a prefix, so a line of PROSE naming a file took that file's contract.
            // GNU grep prints the prefix when it has MORE THAN ONE operand, or when it recurses
            // into a directory — `grep -r pat onefile` prints none. Keying on `-r` alone made a
            // single-file search look prefixed, so every colon in a content line became a
            // candidate path and a line of prose took another file's contract.
            prefixed: !(has('h') || long("--no-filename"))
                && (has('H')
                    || long("--with-filename")
                    || operands.len() > 1
                    || (recursive && (operands.is_empty() || operands.iter().any(|o| o.is_dir())))),
            // `ls` prints a `<dir>:` header under -R, AND whenever it was given more than one
            // operand with a directory among them. Keying only on -R left `ls docs src` with its
            // headers unconsumed, so every bare name resolved against the cwd and took a
            // same-named cwd file's contract.
            block_headers: tool == "ls"
                && (has('R')
                    || long("--recursive")
                    || (operands.len() > 1 && operands.iter().any(|o| o.is_dir()))),
            multi_name: tool == "ls"
                && (has('C')
                    || has('x')
                    || has('m')
                    || format_is("across")
                    || format_is("horizontal")
                    || format_is("commas")
                    || format_is("vertical")),
        }
    }

    /// One input line, one output line.
    ///
    /// The tool's own bytes are written back untouched — decoded only to look for paths, never to
    /// re-encode — so a path that is not valid UTF-8 survives verbatim.
    fn line(&mut self, raw: &[u8], out: &mut impl Write) -> std::io::Result<()> {
        let terminated = raw.last() == Some(&b'\n');
        let body = if terminated {
            &raw[..raw.len() - 1]
        } else {
            raw
        };
        let text = String::from_utf8_lossy(body);

        if let Some(dir) = text.strip_suffix(':').filter(|_| self.block_headers) {
            if !dir.is_empty() && Path::new(dir).is_dir() {
                self.current = Some(PathBuf::from(dir));
                return out.write_all(raw);
            }
        }
        let contract = self.contract_for(text.trim_end_matches('\r'));
        // A CRLF line ends with `\r`; appending after it would make a terminal overwrite the line.
        let (content, cr) = match body.last() {
            Some(&b'\r') => (&body[..body.len() - 1], &b"\r"[..]),
            _ => (body, &b""[..]),
        };
        out.write_all(content)?;
        if let Some(c) = contract {
            // ONE LINE IN, ONE LINE OUT is this tool's cardinal invariant, and it must hold even
            // when the contract itself is not one line. A directory's charter is read from a whole
            // `.annotation` file, so prose written under the charter line lands inside the `IO`
            // field, newline and all — and pasting that verbatim split one output line into two,
            // silently costing `| head -20` a path and `| wc -l` its count.
            let c = c.split(['\n', '\r']).next().unwrap_or_default();
            if !c.is_empty() {
                write!(out, "  # {c}")?;
            }
        }
        out.write_all(cr)?;
        if terminated {
            out.write_all(b"\n")?;
        }
        Ok(())
    }

    /// The contract of the path this line is ABOUT, if it has not been described already.
    ///
    /// A line names its subject at one END or the other — `path`, `path:line:text`,
    /// `-rw-r--r-- … path`, `123 path`. A path appearing anywhere else is part of a match's
    /// CONTENT, and describing it would put one file's contract on a line about another.
    ///
    /// A content-printing tool names its subject only as a `path:` prefix, so a line without one
    /// — `grep PAT onefile`, `grep -rh` — carries NO contract. Guessing the subject from argv was
    /// tried and removed: a pattern or a flag's value that happened to name a file then became the
    /// subject of every line.
    fn contract_for(&mut self, text: &str) -> Option<String> {
        if self.multi_name {
            return None;
        }
        let subject = if self.content_lines {
            if !self.prefixed && !self.path_only {
                return None;
            }
            // Only a printed `path:` prefix names a subject; nothing else on a content line does.
            self.prefix_path(text)
        } else {
            self.listed_path(text)
        };
        let path = subject?;
        if !self.seen.insert(path.clone()) {
            return None;
        }
        self.contracts.describe(&path)
    }

    /// The path a listing tool named on this line.
    ///
    /// Scanned as SUFFIXES OF THE ORIGINAL LINE, from each token boundary, longest first — a
    /// filename containing spaces is one name, and `ls -l` puts it last. Slicing rather than
    /// re-joining tokens is load-bearing: joining with a single space silently loses a name with
    /// two consecutive spaces or a tab in it. A symlink line is cut at ` -> ` so the link, not its
    /// target, is described.
    fn listed_path(&mut self, text: &str) -> Option<PathBuf> {
        // `ls -l`/`-s` open a listing with a `total N` summary that is about no file. Left to the
        // suffix scan, `N` resolved against a numerically-named file — and because `seen` then
        // counted that file as described, its own line later carried nothing.
        if let Some(n) = text.strip_prefix("total ") {
            if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
        }
        let line = match text.find(" -> ") {
            Some(i) => &text[..i],
            None => text,
        };
        let mut after_space = true;
        let mut starts: Vec<usize> = Vec::new();
        for (i, c) in line.char_indices() {
            if c.is_whitespace() {
                after_space = true;
            } else {
                if after_space {
                    starts.push(i);
                }
                after_space = false;
            }
        }
        let mut found: Option<PathBuf> = None;
        for start in starts {
            let candidate = line[start..].trim_end();
            if candidate.is_empty() {
                continue;
            }
            if let Some(p) = self.resolve(candidate) {
                if found.is_none() {
                    found = Some(p);
                }
                break;
            }
        }
        found
    }

    /// The path a content-printing tool named as this line's `path:` prefix.
    ///
    /// The colon positions are scanned in the LINE, not in its first whitespace token, so a prefix
    /// that itself contains spaces still resolves. A prefix is always followed by content in the
    /// same field and always names a file, so a trailing colon (`docs:` in a Makefile, `src:` in
    /// YAML) is content, and a directory is never a prefix.
    fn prefix_path(&mut self, text: &str) -> Option<PathBuf> {
        // `grep -l` prints the path alone — but ONLY then. Trying every line as a whole made a
        // match line that happened to spell a filename take that file's contract, contradicting
        // the rule stated above, and made the memo retain a copy of the entire output.
        if self.path_only {
            let whole = text.trim_end();
            if let Some(p) = self.resolve(whole).filter(|p| p.is_file()) {
                return Some(p);
            }
        }
        for (i, _) in text.match_indices(':') {
            if i == 0 || i + 1 >= text.len() {
                continue;
            }
            if let Some(p) = self.resolve(&text[..i]).filter(|p| p.is_file()) {
                return Some(p);
            }
        }
        None
    }

    /// A path this line might be about, if it names something we can safely describe.
    ///
    /// ONLY a regular file or a directory. A FIFO, socket or character device also `exists()`, and
    /// describing one means opening it to read its first line — which never returns. `ls` in a
    /// directory holding a FIFO hung until the agent's whole tool call timed out, losing output
    /// that had already been produced.
    fn resolve(&mut self, cand: &str) -> Option<PathBuf> {
        let key = (self.current.clone(), cand.to_string());
        if let Some(hit) = self.resolved.get(&key) {
            return hit.clone();
        }
        let p = Path::new(cand);
        let found = if p.is_absolute() {
            describable(p).then(|| p.to_path_buf())
        } else if let Some(block) = &self.current {
            // Inside a `<dir>:` block the header IS the scope. Falling back to the cwd when an
            // entry did not resolve there — a dangling symlink is enough — silently handed the
            // line a same-named cwd file's contract.
            let j = block.join(p);
            describable(&j).then_some(j)
        } else {
            let j = self.base.join(p);
            describable(&j).then_some(j)
        };
        // Past the bound, resolution still works — it just stops being memoised. Growing without
        // limit meant retaining a copy of the tool's whole output.
        if self.resolved.len() < RESOLVE_CACHE_MAX {
            self.resolved.insert(key, found.clone());
        }
        found
    }
}
