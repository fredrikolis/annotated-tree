// Symbols engine: The single grammar-agnostic query runner — parses source, runs a per-grammar `.scm` query, and turns kind-tagged captures into `Symbol`s. NOT concerned with which node kinds a grammar uses (that lives in the per-language query). | I/O: (source, Language, query) -> [Symbol]

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use super::{Symbol, SymbolKind};

/// Longest signature we keep; longer first-lines are truncated with an ellipsis so
/// the outline stays scannable and the text/JSON output bounded.
const MAX_SIGNATURE: usize = 80;

/// A data-driven extractor: one shared parse+query engine, specialized only by a
/// grammar's `Language` and its definition query. Adding a language is a new
/// `(language, query)` pair — no engine changes (Open/Closed).
pub struct QueryExtractor {
    language: Language,
    query_src: &'static str,
}

impl QueryExtractor {
    pub fn new(language: Language, query_src: &'static str) -> Self {
        QueryExtractor {
            language,
            query_src,
        }
    }
}

impl super::SymbolExtractor for QueryExtractor {
    /// Any parse/query failure yields `[]` (graceful, like a missing annotation) — a
    /// malformed source file never aborts the map.
    fn extract(&self, src: &str) -> Vec<Symbol> {
        run(&self.language, self.query_src, src).unwrap_or_default()
    }
}

/// The fallible core, kept separate so the trait method can swallow errors into `[]`.
/// A query names each definition node with a KIND capture (`@function`, `@struct`, …)
/// and the identifier with `@name`; the kind capture supplies the signature + line.
fn run(language: &Language, query_src: &str, src: &str) -> Option<Vec<Symbol>> {
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    let tree = parser.parse(src, None)?;
    let query = Query::new(language, query_src).ok()?;
    let names = query.capture_names();
    let bytes = src.as_bytes();

    let mut out = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), bytes);
    while let Some(m) = matches.next() {
        let mut kind: Option<(SymbolKind, u32, String)> = None;
        let mut name: Option<String> = None;
        for cap in m.captures {
            let cap_name = names[cap.index as usize];
            if cap_name == "name" {
                if let Ok(text) = cap.node.utf8_text(bytes) {
                    name = Some(text.to_string());
                }
            } else if let Some(k) = kind_from_capture(cap_name) {
                let line = cap.node.start_position().row as u32 + 1;
                let signature = first_line(cap.node.utf8_text(bytes).unwrap_or_default());
                kind = Some((k, line, signature));
            }
        }
        if let (Some((kind, line, signature)), Some(name)) = (kind, name) {
            out.push(Symbol {
                kind,
                name,
                signature,
                line,
            });
        }
    }

    // Query match order is not guaranteed source-order across patterns; sort by line
    // (then name) so the rendered outline is deterministic regardless of grammar.
    out.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.name.cmp(&b.name)));
    Some(out)
}

/// Map a query capture name to a symbol kind. Capture names ARE the vocabulary the
/// per-grammar `.scm` files share, so this is the one place kinds are enumerated.
fn kind_from_capture(name: &str) -> Option<SymbolKind> {
    Some(match name {
        "function" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "class" => SymbolKind::Class,
        "struct" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "trait" => SymbolKind::Trait,
        "interface" => SymbolKind::Interface,
        "type" => SymbolKind::Type,
        _ => return None,
    })
}

/// Reduce a definition node's text to a compact one-line signature: first line only,
/// whitespace collapsed, a trailing `{`/`:` block-opener dropped, then length-capped.
fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed
        .trim_end_matches(['{', ':', ';'])
        .trim_end()
        .to_string();
    if trimmed.chars().count() > MAX_SIGNATURE {
        let cut: String = trimmed.chars().take(MAX_SIGNATURE - 1).collect();
        format!("{cut}…")
    } else {
        trimmed
    }
}
