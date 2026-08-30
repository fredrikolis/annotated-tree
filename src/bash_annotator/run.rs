// Concern: appends each printed path's contract to the line it appeared on, once per path | Non-concern: running the tool, or command eligibility | IO: (producer argv, stdin) -> those bytes, annotated

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use super::contracts::Contracts;
use super::map::{shape_of, Shape};

fn describable(p: &Path) -> bool {
    p.metadata().is_ok_and(|m| m.is_file() || m.is_dir())
}

/// A 29 MB `grep` result held 275 MB of keys unbounded.
const RESOLVE_CACHE_MAX: usize = 8192;

/// `producer` is the already-run tool's argv, forwarded so flag rules are read, not guessed.
pub fn annotate(producer: &[OsString], input: &mut impl BufRead, out: &mut impl Write) -> i32 {
    let Some((tool, args)) = producer.split_first() else {
        return 0;
    };
    let mut ann = Annotator::new(&tool.to_string_lossy(), args);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            // A failed write means the reader is gone (`| head -1` closed early).
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

/// THE INVARIANT: one line in, one line out.
struct Annotator {
    contracts: Contracts,
    base: PathBuf,
    current: Option<PathBuf>,
    seen: HashSet<PathBuf>,
    /// Keyed by (base directory, candidate): the same basename under `ls -R` names a different
    /// file per block.
    resolved: HashMap<(Option<PathBuf>, String), Option<PathBuf>>,
    /// Content follows the path here. True for `grep`; false for `ls`/`find`.
    content_lines: bool,
    prefixed: bool,
    /// The whole line is a path (`grep -l`).
    path_only: bool,
    block_headers: bool,
    multi_name: bool,
}

/// `--color` is absent: its argument is optional, so `ls --color docs` lists `docs`.
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

/// Without this, `--regexp`'s value in `grep --regexp PAT file` counted as a second operand.
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

/// EVERY long option each tool has: `getopt_long` accepts any unambiguous abbreviation.
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

fn long_matches(w: &str, name: &str, all: &[&str]) -> bool {
    if w == name {
        return true;
    }
    if !name.starts_with(w) || all.contains(&w) || w.len() < 3 {
        return false;
    }
    all.iter().filter(|o| o.starts_with(w)).count() == 1
}

fn value_long(tool: &str) -> &'static [&'static str] {
    match tool {
        "grep" => GREP_VALUE_LONG,
        "ls" => LS_VALUE_LONG,
        _ => &[],
    }
}

const LS_VALUE_SHORT: &str = "IwT";

/// Applying `ls`'s table here once read `-Ir` as `-I` plus attached value, losing the `r`.
const GREP_VALUE_SHORT: &str = "efmABCdD";

fn value_short(tool: &str) -> &'static str {
    match tool {
        "grep" => GREP_VALUE_SHORT,
        "ls" => LS_VALUE_SHORT,
        _ => "",
    }
}

/// Answered HERE, once: asked separately, `-Idist` once read as `-d`.
struct Cluster {
    letters: String,
    takes_next: bool,
}

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

struct Argv {
    options: Vec<String>,
    operands: Vec<PathBuf>,
}

/// Both halves must come from the SAME walk: separately, `grep -e -l -e beta .` once searched for `-l`.
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

        // ONE base: only `ls <onedir>` names entries relative to it.
        let lists_itself = has('d') || long("--directory");
        let dir_relative =
            tool == "ls" && !lists_itself && operands.len() == 1 && operands[0].is_dir();
        let recursive = has('r') || has('R') || long("--recursive");

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
        // `ls` resolves `--format`'s argument with XARGMATCH, so `acr` is `across`.
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
            // GNU grep prints `path:` only with >1 operand or when recursing into a directory.
            prefixed: !(has('h') || long("--no-filename"))
                && (has('H')
                    || long("--with-filename")
                    || operands.len() > 1
                    || (recursive && (operands.is_empty() || operands.iter().any(|o| o.is_dir())))),
            // `ls` prints `<dir>:` under -R, and also given >1 operand with a directory among them.
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

    fn contract_for(&mut self, text: &str) -> Option<String> {
        if self.multi_name {
            return None;
        }
        let subject = if self.content_lines {
            if !self.prefixed && !self.path_only {
                return None;
            }
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

    /// Scanned as suffixes of the line: a filename with spaces is one name. Cut at ` -> `.
    fn listed_path(&mut self, text: &str) -> Option<PathBuf> {
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

    /// Colon positions scanned in the LINE, not its first whitespace token.
    fn prefix_path(&mut self, text: &str) -> Option<PathBuf> {
        // `grep -l` prints the path alone, but only then: a match line can spell a filename too.
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

    /// Only a regular file or directory: a FIFO also `exists()`, but opening one never returns.
    fn resolve(&mut self, cand: &str) -> Option<PathBuf> {
        let key = (self.current.clone(), cand.to_string());
        if let Some(hit) = self.resolved.get(&key) {
            return hit.clone();
        }
        let p = Path::new(cand);
        let found = if p.is_absolute() {
            describable(p).then(|| p.to_path_buf())
        } else if let Some(block) = &self.current {
            // Inside a `<dir>:` block the header IS the scope, not a fallback to cwd.
            let j = block.join(p);
            describable(&j).then_some(j)
        } else {
            let j = self.base.join(p);
            describable(&j).then_some(j)
        };
        if self.resolved.len() < RESOLVE_CACHE_MAX {
            self.resolved.insert(key, found.clone());
        }
        found
    }
}
