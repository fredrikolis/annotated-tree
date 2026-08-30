// Concern: serializes the canonical map as the versioned, machine-readable JSON contract | Non-concern: building the map or human formatting | IO: (CodebaseMap) -> String

//! # Public JSON schema (version 1)
//! Defined ONCE in [`SCHEMA_DOC`], exposed via `--schema`, so this and that cannot drift apart:
#![doc = concat!("```text\n", include_str!("json_schema.txt"), "```")]

use serde::Serialize;

use crate::graph::Warning;
use crate::model::{CodebaseMap, DirNode};

use super::Renderer;

const SCHEMA_VERSION: u32 = 1;

/// Same string as this module's rustdoc above — one file, cannot drift.
pub const SCHEMA_DOC: &str = include_str!("json_schema.txt");

pub struct JsonRenderer;

#[derive(Serialize)]
struct Document<'a> {
    schema: u32,
    roots: &'a [DirNode],
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<CoverageReport>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    warnings: &'a [Warning],
}

#[derive(Serialize)]
struct CoverageReport {
    code: &'static str,
    annotated: u32,
    total: u32,
}

/// Serde hands the predicate `&&[T]`; `<[_]>::is_empty` cannot take it directly.
fn slice_is_empty<T>(s: &&[T]) -> bool {
    s.is_empty()
}

impl Renderer for JsonRenderer {
    fn render(&self, map: &CodebaseMap) -> String {
        let coverage = map.coverage();
        let document = Document {
            schema: SCHEMA_VERSION,
            roots: &map.roots,
            coverage: coverage.is_incomplete().then_some(CoverageReport {
                code: "annotations_incomplete",
                annotated: coverage.annotated,
                total: coverage.total,
            }),
            warnings: &map.warnings,
        };
        serde_json::to_string_pretty(&document).expect("canonical map serializes to JSON")
    }
}

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

pub fn render_error(code: &str, message: &str, path: Option<&str>) -> String {
    let document = ErrorDocument {
        schema: SCHEMA_VERSION,
        error: ErrorBody {
            code,
            message,
            path,
        },
    };
    serde_json::to_string_pretty(&document).expect("error envelope serializes to JSON")
}
