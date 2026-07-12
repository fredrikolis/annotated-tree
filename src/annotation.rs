// Concern: extracts a file's first-line annotation and validates it against the three-field Concern/Non-concern/IO format | Non-concern: which files to visit | IO: (file head, Language) -> Option<String>

use std::path::Path;

use crate::config::Language;

/// Bytes read from the head of a file — a bounded window that must hold any
/// leading shebang/blank lines plus the first comment. Bounded (not a full read)
/// so a minified one-line blob or a huge data file never reads to EOF; generous
/// enough (64 KiB) that blank-padded or long-banner headers don't silently drop
/// the annotation and trip a false `--strict-check` failure.
const HEAD_BYTES: usize = 64 * 1024;

/// Read the annotation from `path` using `lang`'s rules. Returns the trimmed
/// annotation text, or `None` if the file has no conforming first-line comment.
pub fn extract(path: &Path, lang: &Language) -> Option<String> {
    let head = read_head(path)?;
    extract_from(&head, lang)
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
        // Pattern-based languages carry no natural line for a regex match, so a miss
        // is reported at line 1 with the first line as `raw` (a documented limitation).
        return match text.lines().next() {
            Some(raw) => Located::NoComment {
                line: 1,
                raw: raw.to_string(),
            },
            None => Located::Empty,
        };
    }

    let mut lines = text.lines();
    let mut line_no = 1usize;
    let Some(mut current) = lines.next() else {
        return Located::Empty;
    };
    if current.starts_with("#!") {
        let Some(next) = lines.next() else {
            return Located::Empty;
        };
        current = next;
        line_no += 1;
    }
    while current.trim().is_empty() {
        let Some(next) = lines.next() else {
            return Located::Empty;
        };
        current = next;
        line_no += 1;
    }

    let first = current.trim_start();

    // Each branch COMMITS once its opening delimiter matches (mirroring the original
    // `return non_empty(...)`): an empty-content comment is a landing, not a
    // fall-through, so `extract_from` stays byte-identical.
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
/// branches on (a missing/vacuous slot names one of these). Defined here, at the grader
/// that produces them, so [`crate::strict`]'s `Defect`/`Expected` and the embedded annotation
/// guide ([`crate::guide`]) all reference ONE source of truth and cannot drift.
pub(crate) const PART_CONCERN: &str = "concern";
pub(crate) const PART_NON_CONCERN: &str = "non_concern";
pub(crate) const PART_IO: &str = "io";

/// The structured verdict for one file's annotation, consumed by the strict layer to
/// build a rich, actionable diagnostic (language + marker + real line + example).
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A conforming, fully-formed annotation is present (all three fields, none hollow).
    Ok,
    /// No conforming annotation at all. `raw` carries the offending non-comment /
    /// wrong-marker line when one was present (so the message can hint at it), or
    /// `None` for an empty / unreadable head.
    Missing { line: usize, raw: Option<String> },
    /// A comment is present but is NOT the three-field `Concern: … | Non-concern: … |
    /// IO: …` shape (a key absent, or the ` | ` structure broken). `missing` names which
    /// of the three keyed fields are absent (by [`PART_CONCERN`] etc.) so an agent knows
    /// what to add. `actual` is the extracted comment text.
    Malformed {
        line: usize,
        actual: String,
        missing: Vec<&'static str>,
    },
    /// The three-field shape is present but a required slot is empty, a filler token, or
    /// an unfilled placeholder — a copied box-filling stub. FATAL: this is what makes a
    /// thoughtless suggestion-stub a *failing* state rather than merely a discouraged one.
    /// `slot` is the stable machine token for which field is hollow ([`PART_CONCERN`] |
    /// [`PART_NON_CONCERN`] | [`PART_IO`]) so an agent branches on it; `reason` is the
    /// human prose naming the same defect.
    Vacuous {
        line: usize,
        actual: String,
        slot: &'static str,
        reason: String,
    },
}

/// Diagnose `text` against the one annotation format. The strict layer turns this into a
/// message; [`extract`]/[`extract_from`] stay unchanged for the tree renderer.
pub fn analyze(text: &str, lang: &Language) -> Outcome {
    match locate(text, lang) {
        Located::Found { text, line } => grade_found(text, line),
        Located::NoComment { line, raw } => Outcome::Missing {
            line,
            raw: Some(raw),
        },
        Located::Empty => Outcome::Missing { line: 1, raw: None },
    }
}

/// Grade an already-located annotation body against the three-field format — the shared tail
/// of both [`analyze`] (which locates a file's first comment first) and [`analyze_charter`]
/// (whose whole input IS the body, no comment to locate). ONE grader, so a marker-bearing
/// comment and a bare `.annotation` line are held to the exact same shape and can never drift.
fn grade_found(text: String, line: usize) -> Outcome {
    match parse_fields(&text) {
        // Structurally the format: grade each slot for box-filling. A hollow slot is a
        // FATAL vacuous stub; all three substantive is a pass.
        Some(fields) => match grade(&fields) {
            None => Outcome::Ok,
            Some((slot, reason)) => Outcome::Vacuous {
                line,
                actual: text,
                slot,
                reason,
            },
        },
        // A comment, but not the three-field shape — name which keys are absent.
        None => {
            let missing = absent_parts(&text);
            Outcome::Malformed {
                line,
                actual: text,
                missing,
            }
        }
    }
}

/// Diagnose a bare (marker-less) `.annotation` file body — the whole file IS the annotation,
/// with no comment marker to strip — against the SAME three-field grammar [`analyze`] applies
/// after locating a file's first comment. An empty/whitespace body is `Missing` (an empty
/// opt-in file is a defect, not a silent no-op). Reuses [`grade_found`], so a directory's
/// charter is validated by the one grader, never a second parser.
pub fn analyze_charter(text: &str) -> Outcome {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Outcome::Missing { line: 1, raw: None };
    }
    grade_found(trimmed.to_string(), 1)
}

/// Split a bare three-field line into its `(concern, non_concern, io)` values, or `None` when
/// it is not structurally the format. The render-side counterpart of [`analyze_charter`]:
/// "render, don't reason" — it only splits (reusing the ONE [`parse_fields`] grammar), leaving
/// vacuity grading to `--strict-check`. Fed both a `.annotation` body and an entry file's
/// already-extracted annotation (both bare three-field lines), so promotion needs no re-parse.
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

/// Which of the three keyed fields a malformed comment is missing (by presence of the
/// key text). Case-sensitive: the keys are exact, and `Concern:` is not a substring of
/// `Non-concern:` (capital `C`), so the checks don't alias.
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

/// Grade the three fields, returning the FIRST hollow slot (its part token) and why, or
/// `None` when all three are substantive. Order is Concern, then Non-concern, then IO —
/// the file's primary claim first.
fn grade(f: &Fields) -> Option<(&'static str, String)> {
    if let Some(reason) = grade_prose(f.concern, "Concern") {
        return Some((PART_CONCERN, reason));
    }
    if let Some(reason) = grade_prose(f.non_concern, "Non-concern")
        .or_else(|| grade_non_concern_outward(f.non_concern))
    {
        return Some((PART_NON_CONCERN, reason));
    }
    if let Some(reason) = grade_io(f.io) {
        return Some((PART_IO, reason));
    }
    None
}

/// Filler words that mean the author never actually stated the concern/non-concern — a
/// box filled to satisfy the format, carrying no meaning. Matched case-insensitively
/// against the WHOLE trimmed field (never a substring), so a real statement mentioning
/// one of these words in passing never false-positives. Deliberate asymmetry: `none`
/// is filler HERE (a file must say what it does and does not own) but a blessed value
/// in `IO:` (a file may legitimately have no callable contract) — see [`grade_io`].
const PROSE_FILLER: &[&str] = &[
    "none",
    "n/a",
    "nothing",
    "everything",
    "anything",
    "misc",
    // Code-scaffolding non-words the guide names explicitly as failures (`Filler
    // ("utils", "helpers")`) — a file whose whole stated concern is "utils"/"helpers"
    // has not said what it does. Enforcing them keeps the guide's advertised FAILS
    // honest (no advertise-vs-enforce drift).
    "utils",
    "util",
    "helpers",
    "helper",
    "stuff",
    "the rest",
    "todo",
    "tbd",
    "...",
];

/// Grade a `Concern:` / `Non-concern:` field. `None` means substantive. Rejects empty, a
/// whole-slot bracket placeholder (`<…>` / `[…]` — a copied suggestion stub), and the
/// [`PROSE_FILLER`] tokens.
fn grade_prose(value: &str, name: &str) -> Option<String> {
    let display = value.trim();
    if display.is_empty() {
        return Some(format!("the {name} field is empty"));
    }
    // A whole-slot `<…>` / `[…]` is a copied suggestion placeholder. Checking the WHOLE
    // slot (not any embedded `<…>`) keeps a real field that happens to use generics from
    // tripping this.
    if (display.starts_with('<') && display.ends_with('>'))
        || (display.starts_with('[') && display.ends_with(']'))
    {
        return Some(format!(
            "the {name} field is an unfilled placeholder ('{display}')"
        ));
    }
    let norm = display
        .trim_end_matches(['.', ',', ';', ':'])
        .trim()
        .to_ascii_lowercase();
    if PROSE_FILLER.contains(&norm.as_str()) {
        return Some(format!(
            "the {name} field is filler, not a real statement ('{display}')"
        ));
    }
    None
}

/// Self-referential phrases that make a `Non-concern:` point INWARD at the file itself
/// instead of OUTWARD at a sibling — the non-answer the guide names ("the file's own
/// internals are non-answers"). Matched case-insensitively at WORD BOUNDARIES (so
/// "this file" never fires inside "this filesystem"), and ONLY phrases that EXPLICITLY name
/// the file. Two categories are deliberately excluded, because gating them would force the
/// tool to judge a referent — a semantic call it must not make (render, don't reason): a
/// bare word like "internals" (can name a SIBLING's internals — "caching internals
/// (CacheLayer owns it)"), and a reflexive pronoun like "itself" / "its own" (can refer to a
/// sibling — "each module owns its own", "React handles itself"). Applies to Non-concern
/// only; a `Concern:` describing the file's own job is exactly right.
const NON_CONCERN_INWARD: &[&str] = &["this file", "the file's own", "the file itself"];

/// Whether `hay` contains `phrase` bounded by non-alphanumeric edges (a whole-phrase match),
/// so "this file" matches in "this file's state" but NOT inside "this filesystem". `phrase`
/// is a lowercase ASCII literal from [`NON_CONCERN_INWARD`]; `hay` is already lowercased.
fn contains_bounded_phrase(hay: &str, phrase: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = hay[from..].find(phrase) {
        let start = from + rel;
        let end = start + phrase.len();
        let left_ok = hay[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let right_ok = hay[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Reject a `Non-concern:` that points at the file itself rather than a neighbour. `None`
/// means it points outward (substantive). Fires only on [`NON_CONCERN_INWARD`].
fn grade_non_concern_outward(value: &str) -> Option<String> {
    let hay = value.to_ascii_lowercase();
    for &marker in NON_CONCERN_INWARD {
        if contains_bounded_phrase(&hay, marker) {
            return Some(format!(
                "the Non-concern points inward ('{marker}'); name the sibling that owns the neighbouring concern, not this file's own parts"
            ));
        }
    }
    None
}

/// IO operand tokens that mean the author never filled the slot — the exact placeholders
/// the strict-check suggestion stub prints (`<inputs>`, `<outputs>`) plus obvious fillers.
/// Matched case-insensitively against a WHOLE trimmed operand, so a real type or generic
/// (`Job`, `Vec<Entry<K, V>>`, `Result`, `Option<String>`) never trips. NOTE bare `...`
/// is deliberately absent: `(...) -> void` / `() -> ...` are blessed "unspecified"
/// operands, only the bracketed `<...>` is a hole.
const IO_FILLER: &[&str] = &[
    "<inputs>",
    "<outputs>",
    "<in>",
    "<out>",
    "<...>",
    "todo",
    "tbd",
];

/// Grade the `IO:` field. `None` means substantive. `none` is the blessed "no callable
/// contract" value (first-class, not filler). Otherwise the operands (split on `->`) must
/// carry real types — the placeholders the suggestion stub prints fail.
fn grade_io(value: &str) -> Option<String> {
    let s = value.trim();
    if s.is_empty() {
        return Some("the IO field is empty".to_string());
    }
    // Blessed: the file-level analog of `void`/`()` — an honest "no callable contract".
    if s.eq_ignore_ascii_case("none") {
        return None;
    }
    io_operand_defect(s)
}

/// Whether the `IO:` operands are placeholder/filler box-filling, and why. Splits on the
/// `->` arrow into inputs and outputs and grades each; a contract with no arrow is graded
/// as one operand (never split a real type). `None` = substantive.
fn io_operand_defect(operands: &str) -> Option<String> {
    match operands.split_once("->") {
        Some((inputs, outputs)) => grade_operand(inputs.trim(), "IO input")
            .or_else(|| grade_operand(outputs.trim(), "IO output")),
        None => grade_operand(operands.trim(), "IO"),
    }
}

/// Grade one IO operand. A wrapping `(…)` is stripped first so a no-args `()` reads as the
/// deliberate empty it is (substantive, not a hole) and `(Job)` grades on its inner text.
/// Then a wholly-empty operand or a known placeholder/filler token fires; real types,
/// generics, and a bare `...` ("unspecified but present") pass.
fn grade_operand(operand: &str, name: &str) -> Option<String> {
    let inner = match operand.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(inner) => match inner.trim() {
            // `()` — an explicit no-args marker, not an unfilled hole.
            "" => return None,
            inner => inner,
        },
        None => operand,
    };
    if inner.is_empty() {
        return Some(format!("the {name} operand is empty"));
    }
    if IO_FILLER.contains(&inner.to_ascii_lowercase().as_str()) {
        return Some(format!(
            "the {name} operand is a placeholder, not a real type ('{inner}')"
        ));
    }
    None
}

/// The descriptive text of a candidate annotation to SEED a file-tailored strict-check
/// suggestion — whatever a file already carries before the first ` | `, with a leading
/// `Concern:` key stripped. Empty when the text is only a delimiter/contract with no
/// lead-in. Reuses the same key the parser splits on, so seed and grade stay consistent.
pub(crate) fn concern_seed(text: &str) -> &str {
    let head = text.split(" | ").next().unwrap_or(text);
    let head = head.strip_prefix(CONCERN_KEY).unwrap_or(head);
    head.trim().trim_end_matches(['.', ',', ';', ':']).trim()
}

/// Diagnose the file at `path` by reading its bounded head, then [`analyze`]. An
/// unreadable file (open/read error) is reported as a missing annotation with no
/// `raw`, preserving the pre-existing "unreadable ⇒ missing" strict behaviour.
pub fn analyze_file(path: &Path, lang: &Language) -> Outcome {
    match read_head(path) {
        Some(head) => analyze(&head, lang),
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
        // A BOM ahead of the shebang must not make line 1 look non-shebang and get
        // mis-read as the annotation. decode_head strips it at the read boundary.
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
    fn analyze_distinguishes_ok_missing_and_malformed() {
        // Behaviour only: assert on the `Outcome` variant, not the user-facing prose
        // (that is frozen once at the e2e level). A fully-formed three-field line is Ok;
        // a comment that is not the format is Malformed and names which keys are absent
        // and the real line; a foreign first line is Missing with the raw line captured.
        let l = lang(Some("#"), None, &[]);
        assert_eq!(
            analyze(&format!("# {OK}\n"), &l),
            Outcome::Ok,
            "a fully-formed three-field annotation passes"
        );
        assert_eq!(
            analyze("# just a comment\n", &l),
            Outcome::Malformed {
                line: 1,
                actual: "just a comment".into(),
                missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO],
            },
            "a comment that is not the format is Malformed, naming every absent key"
        );
        assert_eq!(
            analyze("x = 1\n", &l),
            Outcome::Missing {
                line: 1,
                raw: Some("x = 1".into()),
            },
            "a foreign first line is Missing with the raw line captured"
        );
    }

    #[test]
    fn malformed_names_only_the_absent_keys() {
        // A comment with two of three keys is Malformed, and `missing` names ONLY the
        // absent one — the machine delta an agent adds, not prose.
        let l = lang(Some("//"), None, &[]);
        assert_eq!(
            analyze("// Concern: does X | IO: (a) -> b\n", &l),
            Outcome::Malformed {
                line: 1,
                actual: "Concern: does X | IO: (a) -> b".into(),
                missing: vec![PART_NON_CONCERN],
            },
        );
    }

    #[test]
    fn vacuous_slots_are_a_fatal_box_fill() {
        // Past the three-field gate the grader rejects hollow slots. Behaviour only —
        // assert on the variant + which slot, not the exact reason prose.
        let l = lang(Some("//"), None, &[]);
        let cases = [
            (
                "Concern: <what it does> | Non-concern: storage | IO: (a) -> b",
                PART_CONCERN,
            ),
            (
                "Concern: caches | Non-concern: nothing | IO: (a) -> b",
                PART_NON_CONCERN,
            ),
            (
                "Concern: caches | Non-concern: none | IO: (a) -> b",
                PART_NON_CONCERN,
            ),
            (
                "Concern: caches | Non-concern: eviction | IO: (<inputs>) -> <outputs>",
                PART_IO,
            ),
            (
                "Concern: caches | Non-concern: eviction | IO: (Job) -> TODO",
                PART_IO,
            ),
            (
                "Concern:  | Non-concern: eviction | IO: (a) -> b",
                PART_CONCERN,
            ),
            // A: code-scaffolding filler the guide advertises as failing, now enforced —
            // symmetric across the two prose slots (Concern AND Non-concern).
            (
                "Concern: utils | Non-concern: eviction | IO: (a) -> b",
                PART_CONCERN,
            ),
            (
                "Concern: helpers | Non-concern: eviction | IO: (a) -> b",
                PART_CONCERN,
            ),
            (
                "Concern: caches | Non-concern: helpers | IO: (a) -> b",
                PART_NON_CONCERN,
            ),
            // B: a Non-concern pointing INWARD at the file itself is not a boundary — the
            // guide calls "the file's own internals" a non-answer; now gated.
            (
                "Concern: caches lookups | Non-concern: this file's own state | IO: (a) -> b",
                PART_NON_CONCERN,
            ),
            (
                "Concern: caches lookups | Non-concern: the file itself | IO: (a) -> b",
                PART_NON_CONCERN,
            ),
        ];
        for (line, slot) in cases {
            match analyze(&format!("// {line}\n"), &l) {
                Outcome::Vacuous { slot: got, .. } => {
                    assert_eq!(got, slot, "wrong slot for: {line}")
                }
                other => panic!("expected Vacuous({slot}) for {line:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn outward_non_concern_naming_a_siblings_internals_passes() {
        // B gates only unambiguous INWARD phrases. A Non-concern that names a SIBLING's
        // internals is a real boundary and must pass — gating a bare "internals" would force
        // the tool to judge whose internals (a semantic call it must not make).
        let l = lang(Some("//"), None, &[]);
        for ok in [
            // A sibling's internals — "internals" is not gated.
            "// Concern: caches lookups | Non-concern: caching internals (CacheLayer owns it) | IO: (Key) -> Value\n",
            // Reflexive pronouns whose referent is a SIBLING, not this file — must pass.
            "// Concern: re-exports the public API | Non-concern: implementing any component (each module owns its own) | IO: none\n",
            "// Concern: mounts the app | Non-concern: state management (React handles itself) | IO: (Element) -> void\n",
            // "this file" must not fire inside "this filesystem" (word-boundary match).
            "// Concern: resolves paths | Non-concern: this filesystem's mount logic (VfsLayer owns it) | IO: (Path) -> Resolved\n",
        ] {
            assert_eq!(
                analyze(ok, &l),
                Outcome::Ok,
                "outward boundary must pass, not read as self-reference: {ok}",
            );
        }
    }

    #[test]
    fn none_is_blessed_in_io_but_filler_in_prose() {
        // The deliberate asymmetry: `IO: none` PASSES (a file may have no contract), but
        // `Non-concern: none` FAILS (a file must state what it does not own).
        let l = lang(Some("//"), None, &[]);
        assert_eq!(
            analyze(
                "// Concern: re-exports the public API | Non-concern: implementing it | IO: none\n",
                &l
            ),
            Outcome::Ok,
            "IO: none is a first-class blessed value",
        );
        assert!(
            matches!(
                analyze(
                    "// Concern: re-exports the public API | Non-concern: none | IO: none\n",
                    &l
                ),
                Outcome::Vacuous {
                    slot: PART_NON_CONCERN,
                    ..
                }
            ),
            "Non-concern: none is filler and fails",
        );
    }

    #[test]
    fn real_and_no_args_io_operands_pass() {
        // False-positive guard: legitimate IO must NOT be flagged — real types, a no-args
        // `()`, an explicit `void`, generics, and the blessed unspecified `(...)`/`...`.
        let l = lang(Some("//"), None, &[]);
        for io in [
            "(Job) -> Result",
            "() -> void",
            "(Vec<Entry<K, V>>) -> Summary",
            "(files, Config) -> (report, exit_code)",
            "(...) -> void",
            "() -> ...",
            "none",
        ] {
            let line = format!("// Concern: runs it | Non-concern: storage | IO: {io}\n");
            assert_eq!(
                analyze(&line, &l),
                Outcome::Ok,
                "legitimate IO must pass: {io}"
            );
        }
    }

    #[test]
    fn locate_reports_real_line_past_a_shebang() {
        // The shebang trap the dogfood found: the annotation lives on line 2, and a
        // malformed comment must be reported at line 2, never a hardcoded line 1.
        let l = lang(Some("#"), None, &[]);
        assert_eq!(
            analyze("#!/usr/bin/env bash\n# just a comment\n", &l),
            Outcome::Malformed {
                line: 2,
                actual: "just a comment".into(),
                missing: vec![PART_CONCERN, PART_NON_CONCERN, PART_IO],
            },
        );
    }
}
