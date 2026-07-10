// Go symbols: The Go grammar seam — binds tree-sitter-go to the shared engine via a vendored definition query. NOT concerned with query execution. | I/O: () -> (Language, query)

use tree_sitter::Language;

/// Vendored (no-network) query for top-level funcs/methods and type declarations.
pub const QUERY: &str = include_str!("queries/go.scm");

pub fn language() -> Language {
    tree_sitter_go::LANGUAGE.into()
}

#[cfg(test)]
mod tests {
    use crate::symbols::{for_language, Symbol, SymbolKind};

    fn extract(src: &str) -> Vec<Symbol> {
        for_language("go").expect("go extractor").extract(src)
    }

    fn has(syms: &[Symbol], kind: SymbolKind, name: &str) -> bool {
        syms.iter().any(|s| s.kind == kind && s.name == name)
    }

    #[test]
    fn extracts_funcs_methods_and_types() {
        let src = "package main\n\ntype Handler struct { name string }\n\
                   func New() *Handler { return nil }\n\
                   func (h *Handler) Serve() error { return nil }\n";
        let syms = extract(src);
        assert!(has(&syms, SymbolKind::Type, "Handler"));
        assert!(has(&syms, SymbolKind::Function, "New"));
        assert!(has(&syms, SymbolKind::Method, "Serve"));
    }

    #[test]
    fn package_clause_is_not_a_symbol() {
        let syms = extract("package main\n\nfunc main() {}\n");
        assert_eq!(syms.len(), 1);
        assert!(has(&syms, SymbolKind::Function, "main"));
    }
}
