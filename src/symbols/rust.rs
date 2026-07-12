// Concern: the Rust grammar seam — binds tree-sitter-rust to the shared engine via a vendored definition query | Non-concern: query execution | IO: () -> (Language, query)

use tree_sitter::Language;

/// Vendored (no-network) query for top-level fns/structs/enums/traits and impl methods.
pub const QUERY: &str = include_str!("queries/rust.scm");

pub fn language() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

#[cfg(test)]
mod tests {
    use crate::symbols::{for_language, Symbol, SymbolKind};

    fn extract(src: &str) -> Vec<Symbol> {
        for_language("rust").expect("rust extractor").extract(src)
    }

    fn has(syms: &[Symbol], kind: SymbolKind, name: &str) -> bool {
        syms.iter().any(|s| s.kind == kind && s.name == name)
    }

    #[test]
    fn extracts_items_and_impl_methods() {
        let src = "pub struct Engine;\npub enum State { Idle }\npub trait Runnable {}\n\
                   pub fn build() {}\nimpl Engine { pub fn run(&self) {} }\n";
        let syms = extract(src);
        assert!(has(&syms, SymbolKind::Struct, "Engine"));
        assert!(has(&syms, SymbolKind::Enum, "State"));
        assert!(has(&syms, SymbolKind::Trait, "Runnable"));
        assert!(has(&syms, SymbolKind::Function, "build"));
        assert!(has(&syms, SymbolKind::Method, "run"));
    }

    #[test]
    fn ignores_functions_nested_inside_functions() {
        let syms = extract("fn outer() {\n    fn inner() {}\n}\n");
        assert!(has(&syms, SymbolKind::Function, "outer"));
        assert!(!syms.iter().any(|s| s.name == "inner"));
    }

    #[test]
    fn signature_is_the_first_line_without_the_brace() {
        let syms = extract("pub fn build(config: &str) -> Engine {\n    todo!()\n}\n");
        assert_eq!(syms[0].signature, "pub fn build(config: &str) -> Engine");
    }
}
