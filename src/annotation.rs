// Annotation: Extracts a file's first-line annotation and validates it against a language convention. NOT concerned with which files to visit. | I/O: (file head, Language) -> Option<String>

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
    // Lossy is fine: annotations are ASCII/UTF-8; a stray invalid byte in a
    // binary file just yields no match.
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Pure extraction over already-read text. Separated so it is trivially testable.
pub fn extract_from(text: &str, lang: &Language) -> Option<String> {
    if let Some(re) = &lang.pattern {
        let caps = re.captures(text)?;
        let group = caps.name("annotation").or_else(|| caps.get(1))?;
        return non_empty(group.as_str().trim());
    }

    let mut lines = text.lines();
    let mut current = lines.next()?;
    if current.starts_with("#!") {
        current = lines.next()?;
    }
    while current.trim().is_empty() {
        current = lines.next()?;
    }

    let first = current.trim_start();

    for delim in &lang.docstring {
        if let Some(rest) = first.strip_prefix(delim.as_str()) {
            let rest = rest.strip_suffix(delim.as_str()).unwrap_or(rest);
            return non_empty(rest.trim());
        }
    }

    if let Some((open, close)) = &lang.block {
        if let Some(rest) = first.strip_prefix(open.as_str()) {
            let content = rest.split(close.as_str()).next().unwrap_or(rest);
            return non_empty(content.trim());
        }
    }

    if let Some(token) = &lang.line {
        if let Some(rest) = first.strip_prefix(token.as_str()) {
            return non_empty(rest.trim());
        }
    }

    None
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// A strict-check failure for one file. `None` from [`validate`] means it passed.
pub fn validate(annotation: Option<&str>, lang: &Language) -> Option<String> {
    match annotation {
        None => Some("missing annotation".to_string()),
        Some(text) if lang.require.is_match(text) => None,
        Some(_) => Some(format!("annotation missing required '{}'", lang.hint)),
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
            require: Regex::new(r"\|\s*I/O:").unwrap(),
            hint: "| I/O:".into(),
        }
    }

    #[test]
    fn skips_shebang_then_reads_hash_comment() {
        let l = lang(Some("#"), None, &[]);
        let text = "#!/usr/bin/env python3\n# Role: does X. | I/O: (a) -> b\n";
        assert_eq!(
            extract_from(text, &l).unwrap(),
            "Role: does X. | I/O: (a) -> b"
        );
    }

    #[test]
    fn skips_blank_lines() {
        let l = lang(Some("//"), None, &[]);
        assert_eq!(extract_from("\n\n// hi\n", &l).unwrap(), "hi");
    }

    #[test]
    fn reads_single_line_docstring() {
        let l = lang(Some("#"), None, &["\"\"\""]);
        assert_eq!(
            extract_from("\"\"\"Schema: models.\"\"\"\n", &l).unwrap(),
            "Schema: models."
        );
    }

    #[test]
    fn reads_html_block_comment() {
        let l = lang(None, Some(("<!--", "-->")), &[]);
        assert_eq!(
            extract_from("<!-- Covers: x -->\n<div>\n", &l).unwrap(),
            "Covers: x"
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
    fn validate_distinguishes_pass_missing_and_nonconforming() {
        // Behaviour only: pass -> None, fail -> Some. The exact user-facing prose is
        // frozen once at the e2e level (tests/golden/strict_check.txt), so re-freezing
        // it here would be a redundant DbC-violating freeze on our own message.
        let l = lang(Some("#"), None, &[]);
        assert!(
            validate(Some("does X | I/O: a -> b"), &l).is_none(),
            "a conforming annotation passes"
        );
        assert!(
            validate(Some("just a comment"), &l).is_some(),
            "an annotation missing the required contract fails"
        );
        assert!(validate(None, &l).is_some(), "a missing annotation fails");
    }
}
