// Concern: extracts a file's first-line annotation, checks its form and length, and whether an `.annotation` holds more | Non-concern: which files to visit | IO: (head, Language?, bound) -> Outcome

use std::path::Path;

use crate::config::Language;

/// A bounded head window that must hold any leading shebang and blank lines plus the first comment.
/// Bounded so a minified blob or huge data file never reads to EOF, and generous enough that a
/// blank-padded or long-banner header does not silently drop the annotation into a false failure.
const HEAD_BYTES: usize = 64 * 1024;

/// Read the annotation from `path` using `lang`'s rules. Returns the trimmed
/// annotation text, or `None` if the file has no conforming first-line comment.
pub fn extract(path: &Path, lang: &Language) -> Option<String> {
    let head = read_head(path)?;
    extract_from(&head, lang)
}

/// Read a first-line annotation from `path` WITHOUT knowing its comment marker — the escape
/// hatch for a file [`Config`](crate::config::Config) maps to no language (extensionless, or
/// an unrecognized extension), used when the caller has opted such files in. Returns the
/// trimmed annotation, or `None` if the first meaningful line does not carry the format.
pub fn extract_any(path: &Path) -> Option<String> {
    let head = read_head(path)?;
    extract_any_from(&head)
}

/// Marker-agnostic extraction over already-read text: locate the first meaningful line and, if it
/// carries the invariant `Concern:` opener, return the annotation from there on — leading marker
/// dropped, trailing block closer stripped. Keys on the format's fixed opener rather than a language
/// delimiter, so it reads the SAME three-field line from a file whose grammar is unknown.
pub fn extract_any_from(text: &str) -> Option<String> {
    let (_, line) = first_meaningful_line(text)?;
    let start = line.find(CONCERN_KEY)?;
    let mut annotation = line[start..].trim_end();
    for closer in BLOCK_CLOSERS {
        if let Some(stripped) = annotation.strip_suffix(closer) {
            annotation = stripped.trim_end();
            break;
        }
    }
    non_empty(annotation.trim())
}

/// Trailing block-comment closers stripped by [`extract_any_from`] when reading a marker-unknown
/// file — the closing halves of the block/docstring delimiters the built-in languages use, plus
/// a few common ones from languages the tool does not configure. Only a trailing match is
/// removed, so a closer appearing inside the annotation's own prose is untouched.
const BLOCK_CLOSERS: &[&str] = &["-->", "*/", "\"\"\"", "'''", "*)", "#}", "-}", "}}"];

/// The delimiter line opening and closing a YAML frontmatter block. Matched exactly (trailing
/// whitespace aside): an indented `---` is prose, and the block must sit at the very start of
/// the file, so a `---` further down stays a horizontal rule or a document separator.
const FRONTMATTER_FENCE: &str = "---";

/// The first line carrying real content, with its 1-based number: line 1, else past a `#!` shebang,
/// a closed YAML frontmatter block, or leading blanks. The ONE place that skip lives, so [`locate`]
/// and [`extract_any_from`] cannot drift. Frontmatter is skipped for a shebang's reason — line 1 is
/// spoken for by another contract, and requiring the annotation above it would exclude both.
fn first_meaningful_line(text: &str) -> Option<(usize, &str)> {
    let mut lines = text.lines();
    let mut line_no = 1usize;
    let mut current = lines.next()?;
    if current.starts_with("#!") {
        current = lines.next()?;
        line_no += 1;
    }
    if current.trim_end() == FRONTMATTER_FENCE {
        // Probe a clone: only a CLOSED block is a prefix, so a file merely opening with a horizontal rule is left where it was rather than swallowed to EOF.
        let mut probe = lines.clone();
        let mut probe_no = line_no;
        let closed = loop {
            match probe.next() {
                Some(line) => {
                    probe_no += 1;
                    if line.trim_end() == FRONTMATTER_FENCE {
                        break true;
                    }
                }
                None => break false,
            }
        };
        if closed {
            lines = probe;
            line_no = probe_no + 1;
            current = lines.next()?;
        }
    }
    while current.trim().is_empty() {
        current = lines.next()?;
        line_no += 1;
    }
    Some((line_no, current))
}

fn read_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEAD_BYTES];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(decode_head(&buf))
}

/// Decode a raw head window to text. Normalizes at this single read boundary:
/// lossy UTF-8 (a stray byte in a binary file just yields no match) and strips a
/// leading UTF-8 BOM so a BOM+shebang file isn't mis-read as lacking a first-line
/// shebang. Kept pure and separate so it is trivially testable.
fn decode_head(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}')
        .unwrap_or(text.as_ref())
        .to_string()
}

/// Where the annotation scan landed, with the real 1-based line number. Internal —
/// both the byte-identical [`extract_from`] wrapper and [`analyze`] are expressed
/// over it, so the extractor's behaviour and the strict-check diagnosis share ONE
/// scanner and can never drift.
enum Located {
    /// A comment was found and its content is non-empty; `line` is where it sits.
    Found { text: String, line: usize },
    /// The scan reached a real line (past shebang/blanks) that is not a conforming
    /// comment. `raw` is that line verbatim, so a diagnostic can hint "wrong marker".
    NoComment { line: usize, raw: String },
    /// The head held no usable line at all (empty / only shebang+blanks).
    Empty,
}

/// Scan `text` exactly like [`extract_from`] but track the real 1-based line the scan
/// lands on (line 1, +1 past a `#!` shebang, +1 per skipped blank) and capture the
/// landing line's raw content. This is the single scanner both the extractor and the
/// strict analyzer build on.
fn locate(text: &str, lang: &Language) -> Located {
    if let Some(re) = &lang.pattern {
        if let Some(caps) = re.captures(text) {
            if let Some(group) = caps.name("annotation").or_else(|| caps.get(1)) {
                if let Some(t) = non_empty(group.as_str().trim()) {
                    return Located::Found { text: t, line: 1 };
                }
            }
        }
        // Pattern-based languages carry no natural line for a regex match, so a miss is reported at line 1 with the first line as `raw` — a documented limitation.
        return match text.lines().next() {
            Some(raw) => Located::NoComment {
                line: 1,
                raw: raw.to_string(),
            },
            None => Located::Empty,
        };
    }

    let Some((line_no, current)) = first_meaningful_line(text) else {
        return Located::Empty;
    };

    let first = current.trim_start();

    // Each branch COMMITS once its opening delimiter matches: an empty-content comment is a landing, not a fall-through.
    for delim in &lang.docstring {
        if let Some(rest) = first.strip_prefix(delim.as_str()) {
            let rest = rest.strip_suffix(delim.as_str()).unwrap_or(rest);
            return found_or_no_comment(non_empty(rest.trim()), line_no, current);
        }
    }

    if let Some((open, close)) = &lang.block {
        if let Some(rest) = first.strip_prefix(open.as_str()) {
            let content = rest.split(close.as_str()).next().unwrap_or(rest);
            return found_or_no_comment(non_empty(content.trim()), line_no, current);
        }
    }

    if let Some(token) = &lang.line {
        if let Some(rest) = first.strip_prefix(token.as_str()) {
            return found_or_no_comment(non_empty(rest.trim()), line_no, current);
        }
    }

    Located::NoComment {
        line: line_no,
        raw: current.to_string(),
    }
}

fn found_or_no_comment(text: Option<String>, line: usize, raw: &str) -> Located {
    match text {
        Some(text) => Located::Found { text, line },
        None => Located::NoComment {
            line,
            raw: raw.to_string(),
        },
    }
}

/// Pure extraction over already-read text. A thin, byte-identical wrapper over
/// [`locate`]: a `Found` is the annotation, anything else is `None`.
pub fn extract_from(text: &str, lang: &Language) -> Option<String> {
    match locate(text, lang) {
        Located::Found { text, .. } => Some(text),
        Located::NoComment { .. } | Located::Empty => None,
    }
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// The three fixed fields of the one annotation format — the stable part tokens an agent
/// branches on (an absent, empty, or over-length part names one of these). Defined here, at
/// the checker that produces them, so [`crate::strict`]'s `Defect`/`Expected` and the embedded
/// annotation guide ([`crate::guide`]) all reference ONE source of truth and cannot drift.
pub(crate) const PART_CONCERN: &str = "concern";
pub(crate) const PART_NON_CONCERN: &str = "non_concern";
pub(crate) const PART_IO: &str = "io";

/// The human label for a part token (`concern` -> `Concern`) — the field name as it appears in the
/// annotation, so a diagnostic quotes what the author typed while the machine surface keeps the
/// snake_case token. The three tokens above are the only ones that exist and every caller passes
/// one, so anything else is a caller bug and fails loudly rather than leaking a token into prose.
pub(crate) fn part_label(part: &str) -> &'static str {
    match part {
        PART_CONCERN => "Concern",
        PART_NON_CONCERN => "Non-concern",
        PART_IO => "IO",
        other => unreachable!("part_label: `{other}` is not an annotation part token"),
    }
}

/// The structured verdict for one file's annotation, consumed by the strict layer to
/// build a rich, actionable diagnostic (language + marker + real line + example).
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A conforming annotation is present: all three fields, each non-empty and within any
    /// length bound the caller supplied.
    Ok,
    /// No conforming annotation at all. `raw` carries the offending non-comment /
    /// wrong-marker line when one was present (so the message can hint at it), or
    /// `None` for an empty / unreadable head.
    Missing { line: usize, raw: Option<String> },
    /// A comment is present but does not carry three non-empty fields — a key is absent, a field is
    /// empty after trimming, or the ` | ` structure is broken. `missing` names which keyed fields are
    /// absent or empty so an agent knows what to add; `actual` is the extracted text. `detail` is
    /// prose for the two cases `missing` alone reads wrong on, and `None` for a plainly absent key.
    Malformed {
        line: usize,
        actual: String,
        missing: Vec<&'static str>,
        detail: Option<String>,
    },
    /// All three fields are present and non-empty, but the annotation as a whole is longer than the
    /// bound the caller supplied. `length` is its size in Unicode scalar values. The bound is on the
    /// WHOLE annotation, not each field: what an agent pays for is the contract it ingests, and
    /// three fields each under a per-field bound can still add up to more than anyone wants.
    TooLong {
        line: usize,
        actual: String,
        length: usize,
        max: usize,
    },
}

/// Diagnose `text` against the one annotation format, bounding the WHOLE annotation at `max_len`
/// characters when a bound is supplied (`None` raises no length issue at any length). The
/// strict layer turns this into a message; [`extract`]/[`extract_from`] stay unchanged for
/// the tree renderer.
pub fn analyze(text: &str, lang: &Language, max_len: Option<usize>) -> Outcome {
    match locate(text, lang) {
        Located::Found { text, line } => check_found(text, line, max_len),
        Located::NoComment { line, raw } => Outcome::Missing {
            line,
            raw: Some(raw),
        },
        Located::Empty => Outcome::Missing { line: 1, raw: None },
    }
}

/// Check an already-located annotation body against the three-field format and the optional
/// whole-annotation bound — the shared tail of [`analyze`] and [`analyze_charter`]. ONE checker, so
/// a marker-bearing comment and a bare `.annotation` line are held to the same shape and cannot
/// drift. Structure outranks length: one outcome per input, and length only for a conforming line.
fn check_found(text: String, line: usize, max_len: Option<usize>) -> Outcome {
    match parse_fields(&text) {
        Some(fields) => {
            // A present-but-empty field is the same defect class as an absent one (CHECK1 admits both), so it is `Malformed` with that part named; `detail` carries the difference, since the key IS visible in `actual`.
            let empty = empty_parts(&fields);
            if !empty.is_empty() {
                let detail = Some(empty_detail(&empty));
                return Outcome::Malformed {
                    line,
                    actual: text,
                    missing: empty,
                    detail,
                };
            }
            if let Some(max) = max_len {
                let length = text.trim().chars().count();
                if length > max {
                    return Outcome::TooLong {
                        line,
                        actual: text,
                        length,
                        max,
                    };
                }
            }
            Outcome::Ok
        }
        // All three keys present yet unparseable means the ` | ` structure is broken or the keys are out of order, so NO part could be extracted: report all three, and say why.
        None => {
            let mut missing = absent_parts(&text);
            let mut detail = None;
            if missing.is_empty() {
                missing = vec![PART_CONCERN, PART_NON_CONCERN, PART_IO];
                detail = Some(BROKEN_STRUCTURE.to_string());
            }
            Outcome::Malformed {
                line,
                actual: text,
                missing,
                detail,
            }
        }
    }
}

/// Diagnose a bare (marker-less) `.annotation` body — the whole file IS the annotation — against the
/// SAME grammar and bound [`analyze`] applies after locating a comment. An empty body is `Missing`:
/// an empty opt-in file is a defect, not a silent no-op. Reuses [`check_found`], so a directory's
/// charter and a file's sidecar go through one checker, never a second parser.
pub fn analyze_charter(text: &str, max_len: Option<usize>) -> Outcome {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Outcome::Missing { line: 1, raw: None };
    }
    let outcome = check_found(trimmed.to_string(), 1, max_len);
    // An `.annotation` is the ONE place an annotation is written bare, so wrapping it in a marker is the easy mistake: it stays malformed either way, but "the ` | ` separators are missing" is the one diagnosis that is definitely false when they are plainly there.
    let Outcome::Malformed {
        line,
        actual,
        missing,
        detail,
    } = outcome
    else {
        return outcome;
    };
    let detail = match unwrapped_bare_line(&actual) {
        Some(wrapped) => Some(wrapped.detail()),
        None => detail,
    };
    Outcome::Malformed {
        line,
        actual,
        missing,
        detail,
    }
}

/// The 1-based line of the first non-whitespace content BELOW the annotation an `.annotation`
/// artifact holds, or `None` when it holds that one line and nothing else. Located by
/// [`first_meaningful_line`], so a body opening with blank lines is conforming. The rule is
/// "nothing but whitespace below that line", never "contains a newline" — every editor writes one.
pub(crate) fn content_past_first_line(body: &str) -> Option<usize> {
    let (annotation_line, _) = first_meaningful_line(body)?;
    body.lines()
        .enumerate()
        .skip(annotation_line)
        .find(|(_, line)| !line.trim().is_empty())
        .map(|(index, _)| index + 1)
}

/// A bare annotation line someone wrapped in a comment marker: the marker to remove, and the
/// conforming line underneath it.
pub(crate) struct Wrapped {
    opener: &'static str,
    closer: Option<&'static str>,
    /// The text between the markers — a line [`parse_fields`] accepts.
    pub(crate) bare: String,
}

impl Wrapped {
    /// The `detail` prose: name the marker to delete. Nothing else needs saying — the line
    /// inside it is already correct, which is exactly what makes this diagnosable.
    pub(crate) fn detail(&self) -> String {
        let markers = match self.closer {
            Some(closer) => format!("`{}` and `{closer}`", self.opener),
            None => format!("`{}`", self.opener),
        };
        format!(
            "a `.annotation` file holds a bare annotation line with no comment marker; remove \
             the {markers}"
        )
    }
}

/// The conforming annotation inside a comment-wrapped bare line, or `None` when `text` carries
/// no comment marker — or when what is inside one is still not the format, in which case
/// removing the marker is not the whole fix and the ordinary diagnosis stands.
pub(crate) fn unwrapped_bare_line(text: &str) -> Option<Wrapped> {
    let (opener, rest) = COMMENT_OPENERS
        .iter()
        .find_map(|open| text.strip_prefix(*open).map(|rest| (*open, rest)))?;
    let mut bare = rest.trim();
    let mut closer = None;
    for candidate in BLOCK_CLOSERS {
        if let Some(stripped) = bare.strip_suffix(candidate) {
            bare = stripped.trim_end();
            closer = Some(*candidate);
            break;
        }
    }
    parse_fields(bare)?;
    Some(Wrapped {
        opener,
        closer,
        bare: bare.to_string(),
    })
}

/// The comment openers a wrapped bare line is recognized by — exactly the four markers
/// `src/annotation-guide.md` teaches, since the mistake is copying one of those. `<!--` is
/// tested before `--` so the diagnostic quotes the whole marker the author typed.
const COMMENT_OPENERS: &[&str] = &["<!--", "//", "--", "#"];

/// The `detail` prose for a comment whose three keys are all present yet whose structure
/// defeats [`parse_fields`]. Without it a reader is told all three parts are absent while all
/// three keys are plainly visible in the offending line.
const BROKEN_STRUCTURE: &str = "the ` | ` field separators are missing or the keys are out of \
                                order, so no part could be extracted";

/// Split a bare three-field line into its values, or `None` when it is not structurally the format.
/// The render-side counterpart of [`analyze_charter`] — "render, don't reason": it only splits,
/// reusing the ONE grammar, and leaves every issue to `--strict-check`. Fed both an `.annotation`
/// body and an entry file's already-extracted line, so promotion needs no re-parse.
pub fn split_charter(text: &str) -> Option<(String, String, String)> {
    let fields = parse_fields(text.trim())?;
    Some((
        fields.concern.to_string(),
        fields.non_concern.to_string(),
        fields.io.to_string(),
    ))
}

/// The exact keys of the one format. `Concern:` opens the annotation; the other two are
/// matched with their leading ` | ` delimiter so a field's own freetext (a shell pipe, a
/// Rust closure `|x|`, SQL `||`) can never false-split — only ` | Non-concern:` / ` | IO:`
/// mark a real field boundary (belt-and-suspenders: the space-padded delimiter AND the key).
const CONCERN_KEY: &str = "Concern:";
const NON_CONCERN_SEP: &str = " | Non-concern:";
const IO_SEP: &str = " | IO:";

/// The three parsed field values (trimmed), borrowed from the annotation text.
struct Fields<'a> {
    concern: &'a str,
    non_concern: &'a str,
    io: &'a str,
}

/// Split a candidate annotation into its three fields, or `None` if it is not the format.
/// Marker-driven (find ` | Non-concern:` then ` | IO:` in order after a `Concern:` prefix)
/// so a bare `|` inside any field never mis-splits.
fn parse_fields(text: &str) -> Option<Fields<'_>> {
    let rest = text.strip_prefix(CONCERN_KEY)?;
    let nc_at = rest.find(NON_CONCERN_SEP)?;
    let concern = rest[..nc_at].trim();
    let after_nc = &rest[nc_at + NON_CONCERN_SEP.len()..];
    let io_at = after_nc.find(IO_SEP)?;
    let non_concern = after_nc[..io_at].trim();
    let io = after_nc[io_at + IO_SEP.len()..].trim();
    Some(Fields {
        concern,
        non_concern,
        io,
    })
}

/// Which of the three keyed fields are absent from a comment that failed to parse, by
/// presence of the key TEXT (this path never sees a field value, so it cannot judge
/// emptiness — [`empty_parts`] owns that). Case-sensitive: the keys are exact, and `Concern:`
/// is not a substring of `Non-concern:` (capital `C`), so the checks don't alias.
fn absent_parts(text: &str) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !text.contains(CONCERN_KEY) {
        missing.push(PART_CONCERN);
    }
    if !text.contains("Non-concern:") {
        missing.push(PART_NON_CONCERN);
    }
    if !text.contains("IO:") {
        missing.push(PART_IO);
    }
    missing
}

/// Which of the three fields are present but EMPTY after trimming, in the format's own
/// Concern -> Non-concern -> IO order. Every one of them, never just the first: the machine
/// `missing` list must not under-report what an agent has to fill in.
fn empty_parts(f: &Fields) -> Vec<&'static str> {
    part_values(f)
        .into_iter()
        .filter(|(value, _)| value.trim().is_empty())
        .map(|(_, part)| part)
        .collect()
}

/// The three parsed values paired with their stable part tokens, in the format's own order.
fn part_values<'a>(f: &Fields<'a>) -> [(&'a str, &'static str); 3] {
    paired(f.concern, f.non_concern, f.io)
}

/// Three field texts paired with their stable part tokens, in the format's own order — the ONE
/// place that pairing is written down. Every per-part list is built by iterating it (today, the
/// emptiness check's `missing` names), so a list can never order or label parts its own way.
fn paired<'a>(concern: &'a str, non_concern: &'a str, io: &'a str) -> [(&'a str, &'static str); 3] {
    [
        (concern, PART_CONCERN),
        (non_concern, PART_NON_CONCERN),
        (io, PART_IO),
    ]
}

/// The `detail` prose naming every present-but-empty field. Without it a reader is told
/// `concern` is missing while `Concern:` is plainly visible in the offending line.
fn empty_detail(parts: &[&'static str]) -> String {
    let labels: Vec<&str> = parts.iter().map(|p| part_label(p)).collect();
    let (noun, verb) = if labels.len() == 1 {
        ("field", "is")
    } else {
        ("fields", "are")
    };
    format!(
        "the {} {noun} {verb} present but empty",
        join_clauses(&labels)
    )
}

/// Join prose fragments as an English list: `A`, `A and B`, `A, B and C`. The ONE joiner the
/// empty-field prose uses, so two messages about the same three fields cannot punctuate
/// differently.
pub(crate) fn join_clauses<S: AsRef<str>>(items: &[S]) -> String {
    match items {
        [] => String::new(),
        [one] => one.as_ref().to_string(),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter().map(S::as_ref).collect::<Vec<_>>().join(", "),
            last.as_ref()
        ),
    }
}

/// A counted noun for prose: `1 character`, `21 characters` — the pluralization the
/// over-length diagnostic renders the annotation's length with.
pub(crate) fn counted(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The descriptive text of a candidate annotation to SEED a file-tailored suggestion — whatever the
/// file carries before its ` | Non-concern:` boundary, with a leading `Concern:` stripped. Empty
/// when there is only a delimiter with no lead-in. Cuts at the same separator and occurrence as
/// [`parse_fields`], so a bare ` | ` is prose, not a boundary, and never cuts here.
pub(crate) fn concern_seed(text: &str) -> &str {
    // Cut at whichever KEY comes first, not at `Non-concern` alone: a line missing that key (`Concern: x | IO: none`) otherwise seeds the whole remainder, and the suggestion comes back carrying a second `IO:` — a stub that passes the checker while saying nothing.
    let head = match [text.find(NON_CONCERN_SEP), text.find(IO_SEP)]
        .into_iter()
        .flatten()
        .min()
    {
        Some(at) => &text[..at],
        None => text,
    };
    let head = head.strip_prefix(CONCERN_KEY).unwrap_or(head);
    head.trim().trim_end_matches(['.', ',', ';', ':']).trim()
}

/// Diagnose the file at `path` by reading its bounded head, then [`analyze`] with the
/// caller's `max_len` bound. An unreadable file (open/read error) is reported as a missing
/// annotation with no `raw`, preserving the pre-existing "unreadable ⇒ missing" strict
/// behaviour.
pub fn analyze_file(path: &Path, lang: &Language, max_len: Option<usize>) -> Outcome {
    match read_head(path) {
        Some(head) => analyze(&head, lang, max_len),
        None => Outcome::Missing { line: 1, raw: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn lang(line: Option<&str>, block: Option<(&str, &str)>, docstring: &[&str]) -> Language {
        Language {
            name: "t".into(),
            line: line.map(String::from),
            block: block.map(|(a, b)| (a.to_string(), b.to_string())),
            docstring: docstring.iter().map(|s| s.to_string()).collect(),
            pattern: None,
        }
    }

    const OK: &str = "Concern: runs the loop | Non-concern: transport | IO: (Job) -> Result";

    #[test]
    fn skips_shebang_then_reads_hash_comment() {
        let l = lang(Some("#"), None, &[]);
        let text = format!("#!/usr/bin/env python3\n# {OK}\n");
        assert_eq!(extract_from(&text, &l).unwrap(), OK);
    }

    #[test]
    fn skips_node_shebang_reads_slash_comment() {
        let l = lang(Some("//"), None, &[]);
        let text = format!("#!/usr/bin/env node\n// {OK}\n");
        assert_eq!(extract_from(&text, &l).unwrap(), OK);
    }

    #[test]
    fn strips_leading_bom_before_shebang() {
        let l = lang(Some("#"), None, &[]);
        let head = decode_head(format!("\u{feff}#!/usr/bin/env bash\n# {OK}\n").as_bytes());
        assert_eq!(extract_from(&head, &l).unwrap(), OK);
    }

    #[test]
    fn skips_blank_lines() {
        let l = lang(Some("//"), None, &[]);
        assert_eq!(extract_from("\n\n// hi\n", &l).unwrap(), "hi");
    }

    #[test]
    fn reads_single_line_docstring() {
        let l = lang(Some("#"), None, &["\"\"\""]);
        let text = "\"\"\"Concern: models rows | Non-concern: I/O | IO: (row) -> Model\"\"\"\n";
        assert_eq!(
            extract_from(text, &l).unwrap(),
            "Concern: models rows | Non-concern: I/O | IO: (row) -> Model"
        );
    }

    #[test]
    fn reads_html_block_comment() {
        let l = lang(None, Some(("<!--", "-->")), &[]);
        assert_eq!(
            extract_from(
                "<!-- Concern: docs it | Non-concern: code | IO: none -->\n<div>\n",
                &l
            )
            .unwrap(),
            "Concern: docs it | Non-concern: code | IO: none"
        );
    }

    #[test]
    fn no_comment_returns_none() {
        let l = lang(Some("#"), None, &[]);
        assert!(extract_from("x = 1\n", &l).is_none());
    }

    #[test]
    fn pattern_escape_hatch_uses_named_group() {
        let mut l = lang(Some("//"), None, &[]);
        l.pattern = Some(Regex::new(r"(?m)^@doc\s+(?P<annotation>.*)$").unwrap());
        assert_eq!(
            extract_from("ignored\n@doc hello world\n", &l).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn extract_any_reads_the_format_regardless_of_marker() {
        let want = "Concern: a | Non-concern: b | IO: none";
        for text in [
            "# Concern: a | Non-concern: b | IO: none\n",
            "<!-- Concern: a | Non-concern: b | IO: none -->\n",
            "\"\"\"Concern: a | Non-concern: b | IO: none\"\"\"\n",
            "#!/usr/bin/env bash\n# Concern: a | Non-concern: b | IO: none\n",
            "\n\n;; Concern: a | Non-concern: b | IO: none\n",
        ] {
            assert_eq!(
                extract_any_from(text).as_deref(),
                Some(want),
                "marker-agnostic extraction failed for: {text:?}"
            );
        }
        assert!(extract_any_from("x = 1\n").is_none());
        assert!(extract_any_from("").is_none());
    }

    #[test]
    fn analyze_distinguishes_ok_missing_and_malformed() {
        // Assert on the `Outcome` variant, never the user-facing prose — that is frozen once, at the e2e level.
        let l = lang(Some("#"), None, &[]);
        assert_eq!(
            analyze(&format!("# {OK}\n"), &l, None),
            Outcome::Ok,
            "a fully-formed three-field annotation passes"
        );
        assert_eq!(
            analyze("# just a comment\n", &l, None),
            Outcome::Malformed {
                line: 1,
                actual: "just a comment".into(),
                missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO],
                detail: None,
            },
            "a comment that is not the format is Malformed, naming every absent key"
        );
        assert_eq!(
            analyze("x = 1\n", &l, None),
            Outcome::Missing {
                line: 1,
                raw: Some("x = 1".into()),
            },
            "a foreign first line is Missing with the raw line captured"
        );
    }

    #[test]
    fn malformed_names_only_the_absent_keys() {
        // A genuinely absent key carries no `detail`: the report already reads right without one.
        let l = lang(Some("//"), None, &[]);
        assert_eq!(
            analyze("// Concern: does X | IO: (a) -> b\n", &l, None),
            Outcome::Malformed {
                line: 1,
                actual: "Concern: does X | IO: (a) -> b".into(),
                missing: vec![PART_NON_CONCERN],
                detail: None,
            },
        );
    }

    #[test]
    fn an_empty_part_is_malformed_and_named() {
        // `detail` carries what `missing` alone cannot: the key IS visible in the offending line.
        let l = lang(Some("//"), None, &[]);
        match analyze(
            "// Concern:  | Non-concern: eviction | IO: (a) -> b\n",
            &l,
            None,
        ) {
            Outcome::Malformed {
                missing, detail, ..
            } => {
                assert_eq!(missing, vec![PART_CONCERN]);
                assert_eq!(
                    detail.as_deref(),
                    Some("the Concern field is present but empty")
                );
            }
            other => panic!("an empty Concern must be Malformed, got {other:?}"),
        }
    }

    #[test]
    fn every_empty_part_is_reported_not_just_the_first() {
        let l = lang(Some("//"), None, &[]);
        match analyze("// Concern:  | Non-concern:  | IO: \n", &l, None) {
            Outcome::Malformed {
                missing, detail, ..
            } => {
                assert_eq!(missing, vec![PART_CONCERN, PART_NON_CONCERN, PART_IO]);
                assert_eq!(
                    detail.as_deref(),
                    Some("the Concern, Non-concern and IO fields are present but empty")
                );
            }
            other => panic!("three empty fields must be Malformed, got {other:?}"),
        }
    }

    #[test]
    fn broken_structure_with_all_keys_present_reports_all_three() {
        // The parser needs the space-padded ` | ` delimiters, in order — hence an unpadded line and an out-of-order one.
        let l = lang(Some("//"), None, &[]);
        for line in [
            "Concern: a|Non-concern: b|IO: c",
            "Concern: a | IO: b | Non-concern: c",
        ] {
            match analyze(&format!("// {line}\n"), &l, None) {
                Outcome::Malformed {
                    missing, detail, ..
                } => {
                    assert_eq!(
                        missing,
                        vec![PART_CONCERN, PART_NON_CONCERN, PART_IO],
                        "no part is extractable from: {line}"
                    );
                    assert_eq!(detail.as_deref(), Some(BROKEN_STRUCTURE), "for: {line}");
                }
                other => panic!("expected Malformed for {line:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn form_only_checking_admits_any_wording() {
        let l = lang(Some("//"), None, &[]);
        for ok in [
            "// Concern: utils | Non-concern: none | IO: none\n",
            "// Concern: helpers | Non-concern: nothing | IO: (<inputs>) -> <outputs>\n",
            "// Concern: caches lookups | Non-concern: this file's own state | IO: TODO\n",
            "// Concern: <what it does> | Non-concern: <Y> | IO: (a) -> b\n",
            "// Concern: does X | Non-concern: storage | IO: (a) ->\n",
        ] {
            assert_eq!(
                analyze(ok, &l, None),
                Outcome::Ok,
                "a present, non-empty part passes on form alone: {ok}"
            );
        }
    }

    #[test]
    fn an_annotation_over_the_bound_is_too_long() {
        let l = lang(Some("//"), None, &[]);
        let at = "Concern: aaaa | Non-concern: b | IO: c";
        let over = "Concern: éééééé | Non-concern: b | IO: c";
        let bound = at.chars().count();
        assert_eq!(
            analyze(&format!("// {at}\n"), &l, Some(bound)),
            Outcome::Ok,
            "an annotation exactly at the bound passes"
        );
        assert_eq!(
            analyze(&format!("// {over}\n"), &l, None),
            Outcome::Ok,
            "no bound raises no length issue at any length"
        );
        match analyze(&format!("// {over}\n"), &l, Some(bound)) {
            Outcome::TooLong { length, max, .. } => {
                // Counted in scalar values, not bytes: `é` is two bytes and one character, so a byte count would fail a line that fits.
                assert_eq!(length, over.chars().count());
                assert!(
                    over.len() > over.chars().count(),
                    "the two counts differ here"
                );
                assert_eq!(max, bound);
            }
            other => panic!("expected TooLong, got {other:?}"),
        }
    }

    #[test]
    fn structure_outranks_length() {
        let l = lang(Some("//"), None, &[]);
        let long = "y".repeat(50);
        let line = format!("// Concern: {long} | Non-concern:  | IO: c\n");
        match analyze(&line, &l, Some(10)) {
            Outcome::Malformed { missing, .. } => {
                assert_eq!(missing, vec![PART_NON_CONCERN])
            }
            other => panic!("expected Malformed to outrank TooLong, got {other:?}"),
        }
    }

    #[test]
    fn a_quoted_separator_cannot_hide_length_from_a_whole_line_bound() {
        // A whole-line bound cannot be evaded by quoting a separator: however the keys divide the line, every character is still in it.
        let l = lang(Some("//"), None, &[]);
        for text in [
            // a separator quoted BEFORE the real key
            format!(
                "Concern: {}{NON_CONCERN_SEP} {}{IO_SEP} {} | Non-concern: b | IO: c",
                "q".repeat(150),
                "m".repeat(50),
                "t".repeat(77)
            ),
            // and quoted AFTER it, inside IO
            format!(
                "Concern: c1 | Non-concern: b | IO: {} | IO: {}",
                "m".repeat(100),
                "t".repeat(100)
            ),
            // a repeated Non-concern key
            format!(
                "Concern: c | Non-concern: {} | Non-concern: {} | IO: z",
                "x".repeat(100),
                "y".repeat(100)
            ),
        ] {
            let line = format!("// {text}\n");
            match analyze(&line, &l, Some(200)) {
                Outcome::TooLong { length, max, .. } => {
                    assert_eq!(length, text.chars().count(), "counts the line as written");
                    assert_eq!(max, 200);
                }
                other => panic!("expected TooLong on {text:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_bare_pipe_in_prose_never_false_splits() {
        // Why the separators carry their keys AND colons: a field's own freetext (a shell pipe, a Rust closure, SQL `||`) holds ` | ` without marking a boundary.
        let text = "Concern: pipes a | b through | x | y and ors a || b \
                    | Non-concern: storage | IO: (a) -> b";
        let f = parse_fields(text).unwrap();
        assert_eq!(f.concern, "pipes a | b through | x | y and ors a || b");
        assert_eq!(f.non_concern, "storage");
        assert_eq!(f.io, "(a) -> b");
        assert_eq!(
            concern_seed(text),
            "pipes a | b through | x | y and ors a || b"
        );
        let l = lang(Some("//"), None, &[]);
        assert_eq!(analyze(&format!("// {text}\n"), &l, Some(200)), Outcome::Ok);
    }

    #[test]
    fn a_closed_yaml_frontmatter_block_is_skipped_like_a_shebang() {
        let md = lang(None, Some(("<!--", "-->")), &[]);
        let ok = "Concern: the skill brief | Non-concern: running it | IO: none";
        assert_eq!(
            analyze(
                &format!("---\ndescription: d\n---\n<!-- {ok} -->\n"),
                &md,
                None
            ),
            Outcome::Ok,
            "frontmatter then the annotation is a conforming file"
        );
        assert_eq!(
            analyze(
                "---\ndescription: d\n---\n\n<!-- just a note -->\n",
                &md,
                None
            ),
            Outcome::Malformed {
                line: 5,
                actual: "just a note".into(),
                missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO],
                detail: None,
            },
            "the diagnostic line number counts the skipped block and blanks"
        );
        assert_eq!(
            analyze("---\nan unclosed opener\n", &md, None),
            Outcome::Missing {
                line: 1,
                raw: Some("---".into()),
            },
            "with no closing fence there is no block to skip"
        );
        // A `---` that is NOT at the start stays a horizontal rule: the annotation on line 1 is still read, and nothing below is a prefix.
        assert_eq!(
            extract_from(&format!("<!-- {ok} -->\n---\nbody\n---\n"), &md).as_deref(),
            Some(ok),
        );
        // Marker-agnostic reads share the ONE scanner, so they skip the block too.
        assert_eq!(
            extract_any_from(&format!("---\ndescription: d\n---\n# {ok}\n")).as_deref(),
            Some(ok),
        );
    }

    #[test]
    fn a_bare_line_wrapped_in_a_comment_marker_names_the_marker() {
        for (body, markers, bare) in [
            (
                "<!-- Concern: a | Non-concern: b | IO: none -->",
                "`<!--` and `-->`",
                "Concern: a | Non-concern: b | IO: none",
            ),
            (
                "# Concern: a | Non-concern: b | IO: none",
                "`#`",
                "Concern: a | Non-concern: b | IO: none",
            ),
        ] {
            match analyze_charter(body, None) {
                Outcome::Malformed {
                    missing, detail, ..
                } => {
                    assert_eq!(
                        missing,
                        vec![PART_CONCERN, PART_NON_CONCERN, PART_IO],
                        "the verdict is unchanged — no part is extractable from: {body}"
                    );
                    assert_eq!(
                        detail.as_deref(),
                        Some(
                            format!(
                                "a `.annotation` file holds a bare annotation line with no \
                                 comment marker; remove the {markers}"
                            )
                            .as_str()
                        ),
                        "for: {body}"
                    );
                }
                other => panic!("expected Malformed for {body:?}, got {other:?}"),
            }
            assert_eq!(unwrapped_bare_line(body).unwrap().bare, bare);
        }
        // A marker wrapping something that is STILL not the format is not the whole fix, so the ordinary diagnosis stands rather than a marker message that under-reports.
        assert!(unwrapped_bare_line("<!-- Concern: a | IO: b -->").is_none());
        match analyze_charter("<!-- Concern: a | IO: b -->", None) {
            Outcome::Malformed {
                missing, detail, ..
            } => {
                assert_eq!(missing, vec![PART_NON_CONCERN]);
                assert_eq!(detail, None);
            }
            other => panic!("expected Malformed, got {other:?}"),
        }
        // An unwrapped, conforming charter is untouched.
        assert_eq!(
            analyze_charter("Concern: a | Non-concern: b | IO: c", None),
            Outcome::Ok
        );
    }

    #[test]
    fn locate_reports_real_line_past_a_shebang() {
        // The annotation lives on line 2, so a malformed comment must be reported at line 2, never a hardcoded line 1.
        let l = lang(Some("#"), None, &[]);
        assert_eq!(
            analyze("#!/usr/bin/env bash\n# just a comment\n", &l, None),
            Outcome::Malformed {
                line: 2,
                actual: "just a comment".into(),
                missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO],
                detail: None,
            },
        );
    }

    #[test]
    fn content_past_first_line_ignores_trailing_whitespace_but_not_prose() {
        // The trap: a trailing newline is what every editor writes, and blank lines below it are still whitespace — neither is content.
        for clean in [
            "",
            "Concern: a | Non-concern: b | IO: none",
            "one line\n",
            "one line\n\n\n  \n",
        ] {
            assert_eq!(
                content_past_first_line(clean),
                None,
                "clean body: {clean:?}"
            );
        }
        assert_eq!(content_past_first_line("one line\nprose"), Some(2));
        // Keying on line 1 instead of the first meaningful one would report the annotation itself as stray content and tell the author to delete it.
        assert_eq!(content_past_first_line("\n\nConcern: a | IO: b\n"), None);
        assert_eq!(content_past_first_line("\n \tone line\n\nprose"), Some(4));
        assert_eq!(content_past_first_line("one line\n\nprose"), Some(3));
        assert_eq!(
            content_past_first_line("one line\n\n\nprose\nmore"),
            Some(4)
        );
    }
}
