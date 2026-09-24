// Concern: reads a file's annotation, locates the line wholly given to one, checks form, length and `.annotation` contents | Non-concern: which files to visit | IO: (head, lang, bound) -> Outcome, line

use std::path::Path;

use crate::config::Language;

/// Bounded so a huge or minified file never reads to EOF; generous enough for a long header.
const HEAD_BYTES: usize = 64 * 1024;

pub fn extract(path: &Path, lang: &Language) -> Option<String> {
    let head = read_head(path)?;
    extract_from(&head, lang)
}

/// Escape hatch for a file [`Config`](crate::config::Config) maps to no language, used when
/// the caller has opted such files in.
pub fn extract_any(path: &Path) -> Option<String> {
    let head = read_head(path)?;
    extract_any_from(&head)
}

/// A position is reportable only when the opener starts the line and the closer ends it —
/// an Annotation is a span, not a line, and can open/close mid-line or across lines.
pub(crate) fn sole_annotation_line(text: &str, lang: &Language) -> Option<usize> {
    // A `pattern` language has no reportable line — `locate` always reports line 1 for it.
    if lang.pattern.is_some() || !matches!(analyze(text, lang, None), Outcome::Ok) {
        return None;
    }
    let annotation = extract_from(text, lang)?;
    let (line_no, raw) = first_meaningful_line(text, lang.frontmatter_prefix)?;
    let line = raw.trim();

    let body = lang.docstring.iter().find_map(|delim| line.strip_prefix(delim.as_str()).and_then(|rest| rest.strip_suffix(delim.as_str()))).or_else(|| lang.block.as_ref().and_then(|(open, close)| line.strip_prefix(open.as_str()).and_then(|rest| rest.strip_suffix(close.as_str())))).or_else(|| lang.line.as_deref().and_then(|m| line.strip_prefix(m)))?;

    (body.trim() == annotation).then_some(line_no - 1)
}

/// Keys on the format's fixed `Concern:` opener rather than a language delimiter, so it reads
/// the same line from a file whose comment grammar is unknown.
pub fn extract_any_from(text: &str) -> Option<String> {
    let (_, line) = first_meaningful_line(text, true)?;
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

/// Only a trailing match is removed, so a closer inside the annotation's own prose is untouched.
const BLOCK_CLOSERS: &[&str] = &["-->", "*/", "\"\"\"", "'''", "*)", "#}", "-}", "}}"];

/// Must sit at the very start of the file — a `---` further down stays a horizontal rule.
const FRONTMATTER_FENCE: &str = "---";

fn first_meaningful_line(text: &str, frontmatter_prefix: bool) -> Option<(usize, &str)> {
    let mut lines = text.lines();
    let mut line_no = 1usize;
    let mut current = lines.next()?;
    if current.starts_with("#!") {
        current = lines.next()?;
        line_no += 1;
    }
    if frontmatter_prefix && current.trim_end() == FRONTMATTER_FENCE {
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
    text.strip_prefix('\u{feff}').unwrap_or(text.as_ref()).to_string()
}

/// Shared by [`extract_from`] and [`analyze`] so they can never drift apart.
enum Located {
    Found {
        text: String,
        line: usize,
    },
    /// `raw` is the offending line verbatim, so a diagnostic can hint "wrong marker".
    NoComment {
        line: usize,
        raw: String,
    },
    Empty,
}

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
            Some(raw) => Located::NoComment { line: 1, raw: raw.to_string() },
            None => Located::Empty,
        };
    }

    let Some((line_no, current)) = first_meaningful_line(text, lang.frontmatter_prefix) else {
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

    Located::NoComment { line: line_no, raw: current.to_string() }
}

fn found_or_no_comment(text: Option<String>, line: usize, raw: &str) -> Located {
    match text {
        Some(text) => Located::Found { text, line },
        None => Located::NoComment { line, raw: raw.to_string() },
    }
}

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

pub(crate) const PART_CONCERN: &str = "concern";
pub(crate) const PART_NON_CONCERN: &str = "non_concern";
pub(crate) const PART_IO: &str = "io";

pub(crate) fn part_label(part: &str) -> &'static str {
    match part {
        PART_CONCERN => "Concern",
        PART_NON_CONCERN => "Non-concern",
        PART_IO => "IO",
        other => unreachable!("part_label: `{other}` is not an annotation part token"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// `raw` carries the offending line when one was present, `None` for an empty/unreadable head.
    Missing {
        line: usize,
        raw: Option<String>,
    },
    /// `missing` names absent/empty keyed fields; `detail` is prose for the cases `missing`
    /// alone reads wrong on.
    Malformed {
        line: usize,
        actual: String,
        missing: Vec<&'static str>,
        detail: Option<String>,
    },
    /// The bound is on the WHOLE annotation, not each field — three fields each under a
    /// per-field bound can still add up to more than anyone wants.
    TooLong {
        line: usize,
        actual: String,
        length: usize,
        max: usize,
    },
}

pub fn analyze(text: &str, lang: &Language, max_len: Option<usize>) -> Outcome {
    match locate(text, lang) {
        Located::Found { text, line } => check_found(text, line, max_len),
        Located::NoComment { line, raw } => Outcome::Missing { line, raw: Some(raw) },
        Located::Empty => Outcome::Missing { line: 1, raw: None },
    }
}

/// Shared tail of [`analyze`] and [`analyze_charter`]. Structure outranks length.
fn check_found(text: String, line: usize, max_len: Option<usize>) -> Outcome {
    match parse_fields(&text) {
        Some(fields) => {
            let empty = empty_parts(&fields);
            if !empty.is_empty() {
                let detail = Some(empty_detail(&empty));
                return Outcome::Malformed { line, actual: text, missing: empty, detail };
            }
            if let Some(max) = max_len {
                let length = text.trim().chars().count();
                if length > max {
                    return Outcome::TooLong { line, actual: text, length, max };
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
            Outcome::Malformed { line, actual: text, missing, detail }
        }
    }
}

/// An empty body is `Missing`: an empty opt-in file is a defect, not a silent no-op.
pub fn analyze_charter(text: &str, max_len: Option<usize>) -> Outcome {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Outcome::Missing { line: 1, raw: None };
    }
    let outcome = check_found(trimmed.to_string(), 1, max_len);
    // Wrapping a bare `.annotation` line in a comment marker is a common mistake worth naming.
    let Outcome::Malformed { line, actual, missing, detail } = outcome else {
        return outcome;
    };
    let detail = match unwrapped_bare_line(&actual) {
        Some(wrapped) => Some(wrapped.detail()),
        None => detail,
    };
    Outcome::Malformed { line, actual, missing, detail }
}

/// The rule is "nothing but whitespace below that line", never "contains a newline" — every
/// editor writes one.
pub(crate) fn content_past_first_line(body: &str) -> Option<usize> {
    let (annotation_line, _) = first_meaningful_line(body, true)?;
    body.lines().enumerate().skip(annotation_line).find(|(_, line)| !line.trim().is_empty()).map(|(index, _)| index + 1)
}

/// A bare annotation line someone wrapped in a comment marker.
pub(crate) struct Wrapped {
    opener: &'static str,
    closer: Option<&'static str>,
    pub(crate) bare: String,
}

impl Wrapped {
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

/// `None` also when what is inside the marker is still not the format — removing the marker
/// would not be the whole fix, so the ordinary diagnosis stands instead.
pub(crate) fn unwrapped_bare_line(text: &str) -> Option<Wrapped> {
    let (opener, rest) = COMMENT_OPENERS.iter().find_map(|open| text.strip_prefix(*open).map(|rest| (*open, rest)))?;
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
    Some(Wrapped { opener, closer, bare: bare.to_string() })
}

/// `<!--` is tested before `--` so the diagnostic quotes the whole marker the author typed.
const COMMENT_OPENERS: &[&str] = &["<!--", "//", "--", "#"];

const BROKEN_STRUCTURE: &str = "the ` | ` field separators are missing or the keys are out of \
                                order, so no part could be extracted";

pub fn split_charter(text: &str) -> Option<(String, String, String)> {
    let fields = parse_fields(text.trim())?;
    Some((fields.concern.to_string(), fields.non_concern.to_string(), fields.io.to_string()))
}

/// The other two are matched with their leading ` | ` delimiter so a field's own freetext (a
/// shell pipe, a Rust closure `|x|`, SQL `||`) can never false-split.
const CONCERN_KEY: &str = "Concern:";
const NON_CONCERN_SEP: &str = " | Non-concern:";
const IO_SEP: &str = " | IO:";

struct Fields<'a> {
    concern: &'a str,
    non_concern: &'a str,
    io: &'a str,
}

fn parse_fields(text: &str) -> Option<Fields<'_>> {
    let rest = text.strip_prefix(CONCERN_KEY)?;
    let nc_at = rest.find(NON_CONCERN_SEP)?;
    let concern = rest[..nc_at].trim();
    let after_nc = &rest[nc_at + NON_CONCERN_SEP.len()..];
    let io_at = after_nc.find(IO_SEP)?;
    let non_concern = after_nc[..io_at].trim();
    let io = after_nc[io_at + IO_SEP.len()..].trim();
    Some(Fields { concern, non_concern, io })
}

/// Case-sensitive: `Concern:` is not a substring of `Non-concern:` (capital `C`).
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
    part_values(f).into_iter().filter(|(value, _)| value.trim().is_empty()).map(|(_, part)| part).collect()
}

fn part_values<'a>(f: &Fields<'a>) -> [(&'a str, &'static str); 3] {
    paired(f.concern, f.non_concern, f.io)
}

fn paired<'a>(concern: &'a str, non_concern: &'a str, io: &'a str) -> [(&'a str, &'static str); 3] {
    [(concern, PART_CONCERN), (non_concern, PART_NON_CONCERN), (io, PART_IO)]
}

fn empty_detail(parts: &[&'static str]) -> String {
    let labels: Vec<&str> = parts.iter().map(|p| part_label(p)).collect();
    let (noun, verb) = if labels.len() == 1 { ("field", "is") } else { ("fields", "are") };
    format!("the {} {noun} {verb} present but empty", join_clauses(&labels))
}

/// `A`, `A and B`, `A, B and C`.
pub(crate) fn join_clauses<S: AsRef<str>>(items: &[S]) -> String {
    match items {
        [] => String::new(),
        [one] => one.as_ref().to_string(),
        [rest @ .., last] => format!("{} and {}", rest.iter().map(S::as_ref).collect::<Vec<_>>().join(", "), last.as_ref()),
    }
}

/// `1 character`, `21 characters`.
pub(crate) fn counted(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Cuts at whichever key comes first — a line missing `Non-concern:` must still cut before `IO:`.
pub(crate) fn concern_seed(text: &str) -> &str {
    let head = match [text.find(NON_CONCERN_SEP), text.find(IO_SEP)].into_iter().flatten().min() {
        Some(at) => &text[..at],
        None => text,
    };
    let head = head.strip_prefix(CONCERN_KEY).unwrap_or(head);
    head.trim().trim_end_matches(['.', ',', ';', ':']).trim()
}

/// An unreadable file is reported as a missing annotation with no `raw`.
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
        Language { name: "t".into(), interpreters: Vec::new(), frontmatter_prefix: true, line: line.map(String::from), block: block.map(|(a, b)| (a.to_string(), b.to_string())), docstring: docstring.iter().map(|s| s.to_string()).collect(), pattern: None }
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
        assert_eq!(extract_from(text, &l).unwrap(), "Concern: models rows | Non-concern: I/O | IO: (row) -> Model");
    }

    #[test]
    fn reads_html_block_comment() {
        let l = lang(None, Some(("<!--", "-->")), &[]);
        assert_eq!(extract_from("<!-- Concern: docs it | Non-concern: code | IO: none -->\n<div>\n", &l).unwrap(), "Concern: docs it | Non-concern: code | IO: none");
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
        assert_eq!(extract_from("ignored\n@doc hello world\n", &l).unwrap(), "hello world");
    }

    #[test]
    fn extract_any_reads_the_format_regardless_of_marker() {
        let want = "Concern: a | Non-concern: b | IO: none";
        for text in ["# Concern: a | Non-concern: b | IO: none\n", "<!-- Concern: a | Non-concern: b | IO: none -->\n", "\"\"\"Concern: a | Non-concern: b | IO: none\"\"\"\n", "#!/usr/bin/env bash\n# Concern: a | Non-concern: b | IO: none\n", "\n\n;; Concern: a | Non-concern: b | IO: none\n"] {
            assert_eq!(extract_any_from(text).as_deref(), Some(want), "marker-agnostic extraction failed for: {text:?}");
        }
        assert!(extract_any_from("x = 1\n").is_none());
        assert!(extract_any_from("").is_none());
    }

    #[test]
    fn analyze_distinguishes_ok_missing_and_malformed() {
        let l = lang(Some("#"), None, &[]);
        assert_eq!(analyze(&format!("# {OK}\n"), &l, None), Outcome::Ok, "a fully-formed three-field annotation passes");
        assert_eq!(analyze("# just a comment\n", &l, None), Outcome::Malformed { line: 1, actual: "just a comment".into(), missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO], detail: None }, "a comment that is not the format is Malformed, naming every absent key");
        assert_eq!(analyze("x = 1\n", &l, None), Outcome::Missing { line: 1, raw: Some("x = 1".into()) }, "a foreign first line is Missing with the raw line captured");
    }

    #[test]
    fn malformed_names_only_the_absent_keys() {
        let l = lang(Some("//"), None, &[]);
        assert_eq!(analyze("// Concern: does X | IO: (a) -> b\n", &l, None), Outcome::Malformed { line: 1, actual: "Concern: does X | IO: (a) -> b".into(), missing: vec![PART_NON_CONCERN], detail: None },);
    }

    #[test]
    fn an_empty_part_is_malformed_and_named() {
        let l = lang(Some("//"), None, &[]);
        match analyze("// Concern:  | Non-concern: eviction | IO: (a) -> b\n", &l, None) {
            Outcome::Malformed { missing, detail, .. } => {
                assert_eq!(missing, vec![PART_CONCERN]);
                assert_eq!(detail.as_deref(), Some("the Concern field is present but empty"));
            }
            other => panic!("an empty Concern must be Malformed, got {other:?}"),
        }
    }

    #[test]
    fn every_empty_part_is_reported_not_just_the_first() {
        let l = lang(Some("//"), None, &[]);
        match analyze("// Concern:  | Non-concern:  | IO: \n", &l, None) {
            Outcome::Malformed { missing, detail, .. } => {
                assert_eq!(missing, vec![PART_CONCERN, PART_NON_CONCERN, PART_IO]);
                assert_eq!(detail.as_deref(), Some("the Concern, Non-concern and IO fields are present but empty"));
            }
            other => panic!("three empty fields must be Malformed, got {other:?}"),
        }
    }

    #[test]
    fn broken_structure_with_all_keys_present_reports_all_three() {
        let l = lang(Some("//"), None, &[]);
        for line in ["Concern: a|Non-concern: b|IO: c", "Concern: a | IO: b | Non-concern: c"] {
            match analyze(&format!("// {line}\n"), &l, None) {
                Outcome::Malformed { missing, detail, .. } => {
                    assert_eq!(missing, vec![PART_CONCERN, PART_NON_CONCERN, PART_IO], "no part is extractable from: {line}");
                    assert_eq!(detail.as_deref(), Some(BROKEN_STRUCTURE), "for: {line}");
                }
                other => panic!("expected Malformed for {line:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn form_only_checking_admits_any_wording() {
        let l = lang(Some("//"), None, &[]);
        for ok in ["// Concern: utils | Non-concern: none | IO: none\n", "// Concern: helpers | Non-concern: nothing | IO: (<inputs>) -> <outputs>\n", "// Concern: caches lookups | Non-concern: this file's own state | IO: TODO\n", "// Concern: <what it does> | Non-concern: <Y> | IO: (a) -> b\n", "// Concern: does X | Non-concern: storage | IO: (a) ->\n"] {
            assert_eq!(analyze(ok, &l, None), Outcome::Ok, "a present, non-empty part passes on form alone: {ok}");
        }
    }

    #[test]
    fn an_annotation_over_the_bound_is_too_long() {
        let l = lang(Some("//"), None, &[]);
        let at = "Concern: aaaa | Non-concern: b | IO: c";
        let over = "Concern: éééééé | Non-concern: b | IO: c";
        let bound = at.chars().count();
        assert_eq!(analyze(&format!("// {at}\n"), &l, Some(bound)), Outcome::Ok, "an annotation exactly at the bound passes");
        assert_eq!(analyze(&format!("// {over}\n"), &l, None), Outcome::Ok, "no bound raises no length issue at any length");
        match analyze(&format!("// {over}\n"), &l, Some(bound)) {
            Outcome::TooLong { length, max, .. } => {
                // Counted in scalar values, not bytes: `é` is two bytes and one character, so a byte count would fail a line that fits.
                assert_eq!(length, over.chars().count());
                assert!(over.len() > over.chars().count(), "the two counts differ here");
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
        let l = lang(Some("//"), None, &[]);
        for text in [format!("Concern: {}{NON_CONCERN_SEP} {}{IO_SEP} {} | Non-concern: b | IO: c", "q".repeat(150), "m".repeat(50), "t".repeat(77)), format!("Concern: c1 | Non-concern: b | IO: {} | IO: {}", "m".repeat(100), "t".repeat(100)), format!("Concern: c | Non-concern: {} | Non-concern: {} | IO: z", "x".repeat(100), "y".repeat(100))] {
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
        let text = "Concern: pipes a | b through | x | y and ors a || b \
                    | Non-concern: storage | IO: (a) -> b";
        let f = parse_fields(text).unwrap();
        assert_eq!(f.concern, "pipes a | b through | x | y and ors a || b");
        assert_eq!(f.non_concern, "storage");
        assert_eq!(f.io, "(a) -> b");
        assert_eq!(concern_seed(text), "pipes a | b through | x | y and ors a || b");
        let l = lang(Some("//"), None, &[]);
        assert_eq!(analyze(&format!("// {text}\n"), &l, Some(200)), Outcome::Ok);
    }

    #[test]
    fn a_closed_yaml_frontmatter_block_is_skipped_like_a_shebang() {
        let md = lang(None, Some(("<!--", "-->")), &[]);
        let ok = "Concern: the skill brief | Non-concern: running it | IO: none";
        assert_eq!(analyze(&format!("---\ndescription: d\n---\n<!-- {ok} -->\n"), &md, None), Outcome::Ok, "frontmatter then the annotation is a conforming file");
        assert_eq!(analyze("---\ndescription: d\n---\n\n<!-- just a note -->\n", &md, None), Outcome::Malformed { line: 5, actual: "just a note".into(), missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO], detail: None }, "the diagnostic line number counts the skipped block and blanks");
        assert_eq!(analyze("---\nan unclosed opener\n", &md, None), Outcome::Missing { line: 1, raw: Some("---".into()) }, "with no closing fence there is no block to skip");
        assert_eq!(extract_from(&format!("<!-- {ok} -->\n---\nbody\n---\n"), &md).as_deref(), Some(ok),);
        assert_eq!(extract_any_from(&format!("---\ndescription: d\n---\n# {ok}\n")).as_deref(), Some(ok),);
    }

    #[test]
    fn a_bare_line_wrapped_in_a_comment_marker_names_the_marker() {
        for (body, markers, bare) in [("<!-- Concern: a | Non-concern: b | IO: none -->", "`<!--` and `-->`", "Concern: a | Non-concern: b | IO: none"), ("# Concern: a | Non-concern: b | IO: none", "`#`", "Concern: a | Non-concern: b | IO: none")] {
            match analyze_charter(body, None) {
                Outcome::Malformed { missing, detail, .. } => {
                    assert_eq!(missing, vec![PART_CONCERN, PART_NON_CONCERN, PART_IO], "the verdict is unchanged — no part is extractable from: {body}");
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
        assert!(unwrapped_bare_line("<!-- Concern: a | IO: b -->").is_none());
        match analyze_charter("<!-- Concern: a | IO: b -->", None) {
            Outcome::Malformed { missing, detail, .. } => {
                assert_eq!(missing, vec![PART_NON_CONCERN]);
                assert_eq!(detail, None);
            }
            other => panic!("expected Malformed, got {other:?}"),
        }
        assert_eq!(analyze_charter("Concern: a | Non-concern: b | IO: c", None), Outcome::Ok);
    }

    #[test]
    fn locate_reports_real_line_past_a_shebang() {
        let l = lang(Some("#"), None, &[]);
        assert_eq!(analyze("#!/usr/bin/env bash\n# just a comment\n", &l, None), Outcome::Malformed { line: 2, actual: "just a comment".into(), missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO], detail: None },);
    }

    #[test]
    fn content_past_first_line_ignores_trailing_whitespace_but_not_prose() {
        // The trap: a trailing newline is what every editor writes, and blank lines below it are still whitespace — neither is content.
        for clean in ["", "Concern: a | Non-concern: b | IO: none", "one line\n", "one line\n\n\n  \n"] {
            assert_eq!(content_past_first_line(clean), None, "clean body: {clean:?}");
        }
        assert_eq!(content_past_first_line("one line\nprose"), Some(2));
        // Keying on line 1 instead of the first meaningful one would report the annotation itself as stray content and tell the author to delete it.
        assert_eq!(content_past_first_line("\n\nConcern: a | IO: b\n"), None);
        assert_eq!(content_past_first_line("\n \tone line\n\nprose"), Some(4));
        assert_eq!(content_past_first_line("one line\n\nprose"), Some(3));
        assert_eq!(content_past_first_line("one line\n\n\nprose\nmore"), Some(4));
    }
}
