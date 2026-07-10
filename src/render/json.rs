// JSON renderer: Serializes the canonical map as a versioned, machine-readable contract for external consumers (MCP server, editors). NOT concerned with building the map or human formatting. | I/O: (CodebaseMap) -> String

//! # Public JSON schema (version 1)
//!
//! The output is a stable, versioned contract other programs parse, so its shape
//! is documented here (the external-consumer exception to self-documenting code):
//!
//! ```text
//! {
//!   "schema": 1,                     // integer; bumped only on a breaking change
//!   "roots": [ DirNode, … ]          // one entry per analyzed root directory
//! }
//!
//! DirNode  = {
//!   "name":   string,
//!   "dirs":   [ DirNode, … ],        // always present (possibly empty)
//!   "files":  [ FileNode, … ],       // always present (possibly empty)
//!   "deps":   DirDeps,               // OMITTED unless the dir holds a manifest
//!   "tokens": integer                // OMITTED unless --tokens (subtree estimate)
//! }
//!
//! FileNode = {
//!   "name":       string,
//!   "annotation": string,            // OMITTED when the file has no annotation
//!   "age_secs":   integer,           // OMITTED unless --age
//!   "tokens":     integer,           // OMITTED unless --tokens
//!   "symbols":    [ Symbol, … ]      // OMITTED unless --symbols (empty ⇒ omitted)
//! }
//!
//! Symbol   = {
//!   "kind":      string,             // function|method|class|struct|enum|trait|interface|type
//!   "name":      string,
//!   "signature": string,             // compact one-line signature
//!   "line":      integer             // 1-based start line
//! }
//!
//! DirDeps  = {
//!   "used_by":  [ string, … ],       // packages that depend on this one
//!   "internal": [ { "name": string, "resolved": bool }, … ],
//!   "external": [ string, … ]
//! }
//! ```
//!
//! Absent data is omitted rather than emitted as `null`, so the default-flags
//! output is deterministic (no timestamp/token noise) and consumers branch on
//! key-presence.

use serde::Serialize;

use crate::model::{CodebaseMap, DirNode};

use super::Renderer;

/// Current schema version. Bump on any breaking change to the shape above.
const SCHEMA_VERSION: u32 = 1;

pub struct JsonRenderer;

/// The versioned envelope. `roots` borrows the map's nodes, so serialization is
/// zero-copy over the canonical model.
#[derive(Serialize)]
struct Document<'a> {
    schema: u32,
    roots: &'a [DirNode],
}

impl Renderer for JsonRenderer {
    fn render(&self, map: &CodebaseMap) -> String {
        let document = Document {
            schema: SCHEMA_VERSION,
            roots: &map.roots,
        };
        // The model is plain owned data with derived `Serialize`; serialization
        // cannot fail (DbC — we control both sides of this boundary).
        serde_json::to_string_pretty(&document).expect("canonical map serializes to JSON")
    }
}
