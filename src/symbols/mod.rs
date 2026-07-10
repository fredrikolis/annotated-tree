// Symbols: The definition-outline seam — plain `Symbol` data (always compiled) plus a feature-gated tree-sitter extractor registry keyed by the config language name. NOT concerned with walking, reading files, or rendering. | I/O: (source text, language name) -> [Symbol]

use serde::Serialize;

/// What a definition is. Plain data with NO tree-sitter dependency, so it compiles
/// in every build and keeps the JSON schema stable whether or not `symbols` is on.
/// The variants are only *constructed* by the feature-gated extractors, so in a
/// default build they are deliberately un-constructed (schema-stability, not dead).
#[cfg_attr(not(feature = "symbols"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Type,
}

/// One top-level definition extracted from a source file. Plain, serializable data
/// (no parser types leak here), so the JSON contract is identical across builds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    /// A short one-line signature (the definition's first line, whitespace-collapsed
    /// and truncated) — enough to identify the definition without its body.
    pub signature: String,
    /// 1-based line where the definition starts.
    pub line: u32,
}

/// Extracts the top-level definitions from a file's full source. One implementor per
/// grammar (composition behind a trait, not inheritance). The trait itself carries no
/// parser types, so it is defined in every build; only the implementations are gated.
pub trait SymbolExtractor {
    fn extract(&self, src: &str) -> Vec<Symbol>;
}

/// Resolve an extractor for a config `Language.name` ("python"/"rust"/"go"/
/// "typescript"), or `None` when the language has no grammar. Reuses the existing
/// config language registry rather than inventing a second language list.
///
/// This is the `symbols`-feature build: a real tree-sitter extractor per grammar.
#[cfg(feature = "symbols")]
pub fn for_language(name: &str) -> Option<Box<dyn SymbolExtractor>> {
    let query: QueryExtractor = match name {
        "python" => QueryExtractor::new(python::language(), python::QUERY),
        "rust" => QueryExtractor::new(rust::language(), rust::QUERY),
        "go" => QueryExtractor::new(go::language(), go::QUERY),
        "typescript" => QueryExtractor::new(typescript::language(), typescript::QUERY),
        _ => return None,
    };
    Some(Box::new(query))
}

/// Resolve an extractor by language name. This is the DEFAULT (no `symbols` feature)
/// build: always `None`, so the whole extraction path collapses to "no symbols" and
/// the rest of the tool compiles and behaves identically to a symbol-free build.
#[cfg(not(feature = "symbols"))]
pub fn for_language(_name: &str) -> Option<Box<dyn SymbolExtractor>> {
    None
}

#[cfg(feature = "symbols")]
mod engine;
#[cfg(feature = "symbols")]
mod go;
#[cfg(feature = "symbols")]
mod python;
#[cfg(feature = "symbols")]
mod rust;
#[cfg(feature = "symbols")]
mod typescript;

#[cfg(feature = "symbols")]
use engine::QueryExtractor;
