// Concern: serializes the canonical map as a versioned, machine-readable contract for external consumers (MCP server, editors) | Non-concern: building the map or human formatting | IO: (CodebaseMap) -> String

//! # Public JSON schema (version 1)
//!
//! The output is a stable, versioned contract other programs parse, so its shape
//! is documented here (the external-consumer exception to self-documenting code). The
//! exact same text is exposed at runtime via `--schema` and defined ONCE in
//! [`SCHEMA_DOC`] (an embedded file), so this rustdoc and the `--schema` output can
//! never drift apart:
//!
#![doc = concat!("```text\n", include_str!("json_schema.txt"), "```")]

use serde::Serialize;

use crate::graph::Warning;
use crate::model::{CodebaseMap, DirNode};

use super::Renderer;

/// Current schema version. Bump on any breaking change to the shape above.
const SCHEMA_VERSION: u32 = 1;

/// The human-readable wire schema (map document, sub-shapes, `warnings`, and the failure
/// error envelope) as text — the SAME string embedded in this module's rustdoc above. The
/// `--schema` flag prints it so an agent can fetch the contract programmatically; sourcing
/// both surfaces from this one embedded file keeps the advertised schema from drifting.
pub const SCHEMA_DOC: &str = include_str!("json_schema.txt");

pub struct JsonRenderer;

/// The versioned envelope. `roots`/`warnings` borrow the map, so serialization is
/// zero-copy over the canonical model. `warnings` is omitted when empty, consistent with
/// the schema's absent-key convention — so a clean run's output is byte-for-byte unchanged.
#[derive(Serialize)]
struct Document<'a> {
    schema: u32,
    roots: &'a [DirNode],
    /// Layer-0 annotation-coverage signal: present ONLY when some listed code file lacks an
    /// annotation, carrying a stable dispatch `code` plus the `{annotated, total}` counts an
    /// agent converges on. Omitted at full coverage (absent-key convention), so a
    /// fully-annotated repo's document is byte-for-byte unchanged — the structured
    /// counterpart of the text map's self-extinguishing coverage note.
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

/// `skip_serializing_if` predicate for the borrowed `warnings` slice (serde hands the
/// closure a reference to the field, i.e. `&&[Warning]`, which `<[_]>::is_empty` can't
/// take directly).
fn slice_is_empty<T>(s: &&[T]) -> bool {
    s.is_empty()
}

impl Renderer for JsonRenderer {
    fn render(&self, map: &CodebaseMap) -> String {
        let coverage = map.coverage();
        let document = Document {
            schema: SCHEMA_VERSION,
            roots: &map.roots,
            // Omitted at full coverage (byte-identical clean run), present with the stable
            // `annotations_incomplete` code when some listed file has no annotation.
            coverage: coverage.is_incomplete().then_some(CoverageReport {
                code: "annotations_incomplete",
                annotated: coverage.annotated,
                total: coverage.total,
            }),
            warnings: &map.warnings,
        };
        // The model is plain owned data with derived `Serialize`; serialization
        // cannot fail (DbC — we control both sides of this boundary).
        serde_json::to_string_pretty(&document).expect("canonical map serializes to JSON")
    }
}

/// The failure counterpart to [`Document`]: the same versioned envelope carrying an
/// `error` object instead of `roots`. Emitted to stdout on a failed `--format json` run
/// (and reused as the MCP tool-error payload) so an agent parses one dispatch `code`
/// rather than scraping prose off stderr. `path` is omitted (not null) when unknown,
/// consistent with the success schema's key-presence convention.
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

/// Serialize a structured error envelope (schema-1) for a failed run. `code` is a stable
/// key from [`crate::exit::code`]; `message` is human detail; `path` names the offender
/// when known. Both the CLI's `--format json` failure path and the MCP tool-error payload
/// call this, so the wire error shape lives in ONE place.
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
