// Concern: resolves and represents a directory's concern charter — the three-field line on its render row | Non-concern: a file's first comment, or a line's form | IO: (dir, Config) -> Option<Charter>

use std::path::Path;

use serde::Serialize;

use crate::annotation;
use crate::config::Config;
use crate::walk::CHARTER_FILE;

/// Keyed (not one opaque string) so JSON consumers dispatch on each field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Charter {
    pub concern: String,
    pub non_concern: String,
    pub io: String,
}

impl Charter {
    pub fn line(&self) -> String {
        format!(
            "Concern: {} | Non-concern: {} | IO: {}",
            self.concern, self.non_concern, self.io
        )
    }
}

/// The bare exemplar shown in a `.annotation` strict-check diagnostic.
pub const EXAMPLE: &str =
    "Concern: owns request routing | Non-concern: business rules (services own them) | IO: (Request) -> Response";

pub fn from_line(text: &str) -> Option<Charter> {
    let (concern, non_concern, io) = annotation::split_charter(text)?;
    Some(Charter {
        concern,
        non_concern,
        io,
    })
}

/// `.annotation` is ONE bare line; prose below resolves to no charter, not a silent guess.
pub fn from_file_body(body: &str) -> Option<Charter> {
    annotation::content_past_first_line(body)
        .is_none()
        .then(|| from_line(body))
        .flatten()
}

pub const CRATE_ENTRY_FILES: &[&str] = &["lib.rs", "main.rs"];

/// Most-specific first.
pub const DIRECT_ENTRY_FILES: &[&str] = &[
    "mod.rs",
    "__init__.py",
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "doc.go",
];

/// Read DIRECTLY, never through the code-file walk — `.annotation` is dot-hidden.
pub fn read_charter_file(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(CHARTER_FILE)).ok()
}

/// `.annotation` breadcrumb first — its presence overrides even if it fails to parse.
pub fn resolve_from_fs(dir: &Path, config: &Config) -> Option<Charter> {
    if let Some(content) = read_charter_file(dir) {
        return from_file_body(&content);
    }
    if dir.join("Cargo.toml").is_file() {
        let src = dir.join("src");
        if let Some(charter) = CRATE_ENTRY_FILES
            .iter()
            .find_map(|base| entry_annotation(&src.join(base), config))
            .and_then(|a| from_line(&a))
        {
            return Some(charter);
        }
    }
    DIRECT_ENTRY_FILES
        .iter()
        .find_map(|name| entry_annotation(&dir.join(name), config))
        .and_then(|a| from_line(&a))
}

fn entry_annotation(path: &Path, config: &Config) -> Option<String> {
    let lang = config.language_for_path(path)?;
    annotation::extract(path, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_line_splits_a_bare_three_field_charter() {
        let c = from_line("Concern: owns the API | Non-concern: storage (db owns it) | IO: none")
            .expect("a valid three-field line parses");
        assert_eq!(c.concern, "owns the API");
        assert_eq!(c.non_concern, "storage (db owns it)");
        assert_eq!(c.io, "none");
        assert_eq!(
            c.line(),
            "Concern: owns the API | Non-concern: storage (db owns it) | IO: none"
        );
    }

    #[test]
    fn from_line_rejects_a_non_charter_line() {
        assert!(from_line("just a folder note").is_none());
    }

    #[test]
    fn advertised_example_is_self_conforming() {
        assert_eq!(
            annotation::analyze_charter(EXAMPLE, None),
            annotation::Outcome::Ok
        );
    }
}
