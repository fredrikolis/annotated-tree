// Concern: resolves and represents a directory's concern charter — the three-field line promoted onto the directory's own render row, sourced most-explicit-first (a `.annotation` breadcrumb, else the code entry file's annotation) | Non-concern: locating a file's first comment or grading vacuity (annotation.rs owns the one grammar) | IO: (dir, Config) -> Option<Charter>

use std::path::Path;

use serde::Serialize;

use crate::annotation;
use crate::config::Config;
use crate::walk::CHARTER_FILE;

/// A directory's charter: the same three keyed fields a file annotation carries, promoted onto
/// the directory row. Keyed (not one opaque string) so JSON consumers dispatch on each field,
/// mirroring how a file's annotation is the agent-navigable unit at file scale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Charter {
    pub concern: String,
    pub non_concern: String,
    pub io: String,
}

impl Charter {
    /// The charter as its canonical one-line render — the exact bare three-field shape a
    /// `.annotation` file holds and an entry file's annotation carries, so the rendered
    /// directory row reads identically to a file annotation (minus the comment marker).
    pub fn line(&self) -> String {
        format!(
            "Concern: {} | Non-concern: {} | IO: {}",
            self.concern, self.non_concern, self.io
        )
    }
}

/// A canonical, self-conforming charter line — the bare (marker-less) exemplar shown in a
/// `.annotation` strict-check diagnostic, the charter analog of a language's `example`. A test
/// proves it round-trips through [`annotation::analyze_charter`] as `Ok`, guarding against an
/// advertise-vs-enforce drift.
pub const EXAMPLE: &str =
    "Concern: owns request routing | Non-concern: business rules (services own them) | IO: (Request) -> Response";

/// Parse a bare three-field line into a [`Charter`], or `None` when it is not structurally the
/// format. Reuses the ONE annotation grammar ([`annotation::split_charter`]) — the only charter
/// parsing entry point — so `.annotation` bodies and promoted entry-file annotations flow
/// through the same splitter the file lint uses.
pub fn from_line(text: &str) -> Option<Charter> {
    let (concern, non_concern, io) = annotation::split_charter(text)?;
    Some(Charter {
        concern,
        non_concern,
        io,
    })
}

/// Entry-file basenames (under `src/`) for a Rust crate whose charter is its `lib.rs` (else
/// `main.rs`) annotation. Applies only when the directory holds a `Cargo.toml`. Bare basenames
/// are the single source of truth — the filesystem resolver joins them under `src/`, the model
/// resolver matches them against the built `src/` node — so the two paths cannot drift.
pub const CRATE_ENTRY_FILES: &[&str] = &["lib.rs", "main.rs"];

/// Entry-file candidates that are a DIRECT child of the directory, in most-specific order: a
/// Rust module (`mod.rs`), a Python package (`__init__.py`), a JS/TS package (`index.*`), a Go
/// package (`doc.go`). The directory's charter is promoted from the first that carries an
/// annotation. Each name self-identifies its ecosystem, so no manifest sniffing is needed.
pub const DIRECT_ENTRY_FILES: &[&str] = &[
    "mod.rs",
    "__init__.py",
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "doc.go",
];

/// Read a directory's `.annotation` breadcrumb, or `None` when absent/unreadable. Read DIRECTLY
/// (never through the code-file walk) so it resolves even though `.annotation` is dot-hidden and
/// excluded from the rendered tree — the metadata read the walk's display filters must not hide.
pub fn read_charter_file(abs_dir: &Path) -> Option<String> {
    std::fs::read_to_string(abs_dir.join(CHARTER_FILE)).ok()
}

/// Resolve `abs_dir`'s charter from the FILESYSTEM (the strict-check path, which holds no built
/// tree): `.annotation` breadcrumb first (its presence overrides, even if it fails to parse —
/// most-explicit-wins), else the promoted annotation of the code entry file. Re-reads the entry
/// file's head via [`annotation::extract`]; the model path instead reuses the already-extracted
/// `FileNode.annotation` (no re-parse). Both share [`from_line`] and the entry-file tables.
pub fn resolve_from_fs(abs_dir: &Path, config: &Config) -> Option<Charter> {
    // 1. `.annotation` breadcrumb — its mere presence is the resolution (a malformed body
    //    yields `None` here and is flagged by `--strict-check`; it never falls through).
    if let Some(content) = read_charter_file(abs_dir) {
        return from_line(&content);
    }
    // 2. Rust crate: promote src/lib.rs (else src/main.rs), only for a manifest-bearing crate.
    if abs_dir.join("Cargo.toml").is_file() {
        let src = abs_dir.join("src");
        if let Some(charter) = CRATE_ENTRY_FILES
            .iter()
            .find_map(|base| entry_annotation(&src.join(base), config))
            .and_then(|a| from_line(&a))
        {
            return Some(charter);
        }
    }
    // 3. Direct child entry file (module / package / index / doc).
    DIRECT_ENTRY_FILES
        .iter()
        .find_map(|name| entry_annotation(&abs_dir.join(name), config))
        .and_then(|a| from_line(&a))
}

/// The first-line annotation of an entry file at `path`, or `None` if it is absent, of an
/// unknown language, or carries no conforming comment — reusing the file-annotation extractor
/// (no charter-specific parsing).
fn entry_annotation(path: &Path, config: &Config) -> Option<String> {
    let lang = config.language_for_path(path)?;
    annotation::extract(path, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_line_splits_a_bare_three_field_charter() {
        // A bare (marker-less) line — a `.annotation` body — splits into the three keyed
        // fields via the ONE annotation grammar; the render line round-trips it verbatim.
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
        // Not the three-field shape ⇒ no charter (the render side stays silent; strict flags it).
        assert!(from_line("just a folder note").is_none());
    }

    #[test]
    fn advertised_example_is_self_conforming() {
        // The DbC guarantee against advertise-vs-enforce drift: the bare exemplar a malformed
        // `.annotation` diagnostic shows must itself pass the charter grammar it advertises.
        assert_eq!(
            annotation::analyze_charter(EXAMPLE),
            annotation::Outcome::Ok
        );
    }
}
