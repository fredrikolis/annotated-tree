// Python symbols: The Python grammar seam — binds tree-sitter-python to the shared engine via a vendored definition query. NOT concerned with query execution. | I/O: () -> (Language, query)

use tree_sitter::Language;

/// Vendored (no-network) query for top-level functions/classes and their methods.
pub const QUERY: &str = include_str!("queries/python.scm");

pub fn language() -> Language {
    tree_sitter_python::LANGUAGE.into()
}

#[cfg(test)]
mod tests {
    use crate::symbols::{for_language, Symbol, SymbolKind};

    fn extract(src: &str) -> Vec<Symbol> {
        for_language("python")
            .expect("python extractor")
            .extract(src)
    }

    fn has(syms: &[Symbol], kind: SymbolKind, name: &str) -> bool {
        syms.iter().any(|s| s.kind == kind && s.name == name)
    }

    #[test]
    fn extracts_functions_classes_and_methods() {
        let src = "def build():\n    pass\n\nclass Server:\n    def start(self):\n        pass\n";
        let syms = extract(src);
        assert!(has(&syms, SymbolKind::Function, "build"));
        assert!(has(&syms, SymbolKind::Class, "Server"));
        assert!(has(&syms, SymbolKind::Method, "start"));
    }

    #[test]
    fn ignores_functions_nested_inside_functions() {
        // Only module-level defs and class methods count as the outline — a helper
        // nested inside a function body is not a top-level definition.
        let syms = extract("def outer():\n    def inner():\n        pass\n");
        assert!(has(&syms, SymbolKind::Function, "outer"));
        assert!(!syms.iter().any(|s| s.name == "inner"));
    }

    #[test]
    fn reports_one_based_start_line() {
        let syms = extract("\n\ndef build():\n    pass\n");
        assert_eq!(syms[0].line, 3);
    }
}
