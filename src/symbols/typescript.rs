// TypeScript symbols: The TypeScript grammar seam — binds tree-sitter-typescript to the shared engine via a vendored definition query. NOT concerned with query execution. | I/O: () -> (Language, query)

use tree_sitter::Language;

/// Vendored (no-network) query for top-level (and exported) functions/classes/
/// interfaces/type aliases plus class methods. Covers `.ts`/`.tsx`/`.js`/`.jsx`.
pub const QUERY: &str = include_str!("queries/typescript.scm");

pub fn language() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

#[cfg(test)]
mod tests {
    use crate::symbols::{for_language, Symbol, SymbolKind};

    fn extract(src: &str) -> Vec<Symbol> {
        for_language("typescript")
            .expect("typescript extractor")
            .extract(src)
    }

    fn has(syms: &[Symbol], kind: SymbolKind, name: &str) -> bool {
        syms.iter().any(|s| s.kind == kind && s.name == name)
    }

    #[test]
    fn extracts_exported_and_plain_declarations() {
        let src = "export function render() {}\ninterface Props { id: string }\n\
                   export type Handler = () => void;\n\
                   export class Widget { mount() {} }\n";
        let syms = extract(src);
        assert!(has(&syms, SymbolKind::Function, "render"));
        assert!(has(&syms, SymbolKind::Interface, "Props"));
        assert!(has(&syms, SymbolKind::Type, "Handler"));
        assert!(has(&syms, SymbolKind::Class, "Widget"));
        assert!(has(&syms, SymbolKind::Method, "mount"));
    }

    #[test]
    fn export_keeps_the_declaration_name_not_the_export_wrapper() {
        // An `export` wrapper must not shadow the inner declaration's identity.
        let syms = extract("export function build() {}\n");
        assert!(has(&syms, SymbolKind::Function, "build"));
    }
}
