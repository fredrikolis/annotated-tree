// Concern: serializes the canonical map as the versioned, machine-readable JSON contract | Non-concern: building the map or human formatting | IO: (CodebaseMap) -> String

//! # Public JSON schema (version 1)
//! The output is a stable, versioned contract other programs parse, so its shape is documented
//! here — the external-consumer exception to self-documenting code. That text is exposed at
//! runtime via `--schema` and defined ONCE in [`SCHEMA_DOC`], so the two cannot drift apart:
#![doc = concat!("```text\n", include_str!("json_schema.txt"), "```")]

use serde::Serialize;

use crate::graph::Warning;
use crate::model::{CodebaseMap, DirNode};

use super::Renderer;

/// Current schema version. Bump on any breaking change to the shape above.
const SCHEMA_VERSION: u32 = 1;

/// The wire schema as text — the SAME string embedded in this module's rustdoc above, so the
/// rustdoc and what `--schema` prints are sourced from one file and cannot drift.
pub const SCHEMA_DOC: &str = include_str!("json_schema.txt");

pub struct JsonRenderer;

/// The versioned envelope. `roots`/`warnings` borrow the map, so serialization is zero-copy, and
/// `warnings` is omitted when empty — a clean run's output stays byte-for-byte unchanged.
#[derive(Serialize)]
struct Document<'a> {
    schema: u32,
    roots: &'a [DirNode],
    /// Present ONLY when some listed code file lacks an annotation, so a fully-annotated repo's
    /// document is byte-for-byte unchanged — the structured counterpart of the text map's note.
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<CoverageReport>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    warnings: &'a [Warning],
}

/// The envelope's `coverage` object: a code file with no annotation is invisible to an agent
/// reading the tree, so an incomplete map reports how many of its files carry one. `code` is
/// a stable dispatch key (like the error envelope's); the counts are the same `Coverage`
/// [`crate::model::CodebaseMap::coverage`] renders in the text footer.
#[derive(Serialize)]
struct CoverageReport {
    code: &'static str,
    annotated: u32,
    total: u32,
}

/// Serde hands the predicate `&&[Warning]`, which `<[_]>::is_empty` cannot take directly.
fn slice_is_empty<T>(s: &&[T]) -> bool {
    s.is_empty()
}

impl Renderer for JsonRenderer {
    fn render(&self, map: &CodebaseMap) -> String {
        let coverage = map.coverage();
        let document = Document {
            schema: SCHEMA_VERSION,
            roots: &map.roots,
            // Omitted at full coverage (byte-identical clean run), present with the stable `annotations_incomplete` code when some listed file has no annotation.
            coverage: coverage.is_incomplete().then_some(CoverageReport {
                code: "annotations_incomplete",
                annotated: coverage.annotated,
                total: coverage.total,
            }),
            warnings: &map.warnings,
        };
        // The model is plain owned data with derived `Serialize`; serialization cannot fail (DbC — we control both sides of this boundary).
        serde_json::to_string_pretty(&document).expect("canonical map serializes to JSON")
    }
}

/// The failure counterpart to [`Document`]: the same envelope carrying an `error` object instead of
/// `roots`, so an agent parses one dispatch `code` rather than scraping prose off stderr. `path` is
/// omitted rather than null when unknown, per the success schema's key-presence convention.
#[derive(Serialize)]
struct ErrorDocument<'a> {
    schema: u32,
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
}

/// Serialize a structured error envelope for a failed run. Every failure path that emits
/// `--format json` calls this, so the wire error shape lives in ONE place.
pub fn render_error(code: &str, message: &str, path: Option<&str>) -> String {
    let document = ErrorDocument {
        schema: SCHEMA_VERSION,
        error: ErrorBody {
            code,
            message,
            path,
        },
    };
    // Plain borrowed data with derived `Serialize`; serialization cannot fail (DbC).
    serde_json::to_string_pretty(&document).expect("error envelope serializes to JSON")
}
