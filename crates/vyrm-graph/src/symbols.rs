//! Symbol and dependency extraction.
//!
//! Extraction is parser-based. Each file is parsed with its language's
//! tree-sitter grammar and occurrences are read from the syntax tree by query,
//! so a declaration inside a comment or string literal never produces an
//! occurrence, and a declaration split across lines is attributed to the line
//! that names it.
//!
//! The v1 extractor was line-based. It was replaced when measured routing
//! precision (5.61x on a 1,616-file repository) fell short of published results
//! for tree-sitter-based repository maps (~10x). The swap's measured outcome,
//! 2026-08-10, same queries: the lines-to-read ratio was unchanged (5.61x),
//! while the distinct-definition count fell from 6,521 to 5,773 — the 748
//! removed names were locals and commented declarations the line scan had
//! promoted to definers — and full-index cost rose from 334 ms to 4,752 ms,
//! absorbed by incremental refresh. The precision gap therefore lies in
//! ranking and query classes without declaration sites, not in extraction.
//!
//! Grammars are the tree-sitter organization's own crates (Rust, TypeScript,
//! JavaScript, Python) plus the tree-sitter-grammars organization's Svelte
//! grammar. A Svelte file is parsed twice: the Svelte grammar locates its
//! `script_element` contents, which are then parsed as TypeScript with the
//! original line numbering preserved.
//!
//! Every routed result carries the evidence that produced it, so a wrong route
//! is inspectable rather than opaque.

use crate::profile::Language;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator as _};

/// How a name appears in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The file declares the name.
    Definition,
    /// The file mentions the name without declaring it.
    Reference,
    /// The file imports from another module.
    Import,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub name: String,
    pub role: Role,
    pub line: usize,
}

/// A compiled grammar and the query that reads occurrences from its trees.
///
/// Compiled once per process: query compilation is the expensive step, and the
/// result is immutable. Parsers are per-call, because `Parser` is stateful.
struct Grammar {
    language: tree_sitter::Language,
    query: Query,
}

impl Grammar {
    /// Compiles at first use. A grammar/query mismatch is a defect in this
    /// module's constants, not a runtime condition, so it panics rather than
    /// degrading extraction silently.
    fn compile(language: tree_sitter::Language, source: &str) -> Grammar {
        let query = Query::new(&language, source)
            .unwrap_or_else(|e| panic!("extraction query does not match its grammar: {e}"));
        Grammar { language, query }
    }
}

/// Capture names shared by every query. `definition` and `import` map onto
/// [`Role`]; `script` marks embedded source that is re-parsed.
const CAPTURE_DEFINITION: &str = "definition";
const CAPTURE_IMPORT: &str = "import";

const RUST_QUERY: &str = r#"
(function_item name: (identifier) @definition)
(function_signature_item name: (identifier) @definition)
(struct_item name: (type_identifier) @definition)
(enum_item name: (type_identifier) @definition)
(union_item name: (type_identifier) @definition)
(trait_item name: (type_identifier) @definition)
(type_item name: (type_identifier) @definition)
(const_item name: (identifier) @definition)
(static_item name: (identifier) @definition)
(mod_item name: (identifier) @definition)
(macro_definition name: (identifier) @definition)
(impl_item type: (type_identifier) @definition)
(impl_item type: (generic_type type: (type_identifier) @definition))
(impl_item type: (scoped_type_identifier name: (type_identifier) @definition))
(use_declaration argument: (_) @import)
"#;

/// Shared by the TypeScript and TSX grammars, whose node names agree even
/// though their parse rules do not.
const TYPESCRIPT_QUERY: &str = r#"
(function_declaration name: (identifier) @definition)
(generator_function_declaration name: (identifier) @definition)
(class_declaration name: (type_identifier) @definition)
(abstract_class_declaration name: (type_identifier) @definition)
(interface_declaration name: (type_identifier) @definition)
(type_alias_declaration name: (type_identifier) @definition)
(enum_declaration name: (identifier) @definition)
(method_definition name: (property_identifier) @definition)
(program (lexical_declaration (variable_declarator name: (identifier) @definition)))
(program (variable_declaration (variable_declarator name: (identifier) @definition)))
(export_statement declaration: (lexical_declaration (variable_declarator name: (identifier) @definition)))
(export_statement declaration: (variable_declaration (variable_declarator name: (identifier) @definition)))
(import_statement source: (string (string_fragment) @import))
(export_statement source: (string (string_fragment) @import))
"#;

const JAVASCRIPT_QUERY: &str = r#"
(function_declaration name: (identifier) @definition)
(generator_function_declaration name: (identifier) @definition)
(class_declaration name: (identifier) @definition)
(method_definition name: (property_identifier) @definition)
(program (lexical_declaration (variable_declarator name: (identifier) @definition)))
(program (variable_declaration (variable_declarator name: (identifier) @definition)))
(export_statement declaration: (lexical_declaration (variable_declarator name: (identifier) @definition)))
(export_statement declaration: (variable_declaration (variable_declarator name: (identifier) @definition)))
(import_statement source: (string (string_fragment) @import))
(export_statement source: (string (string_fragment) @import))
"#;

const PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @definition)
(class_definition name: (identifier) @definition)
(module (expression_statement (assignment left: (identifier) @definition)))
(import_statement name: (dotted_name) @import)
(import_statement name: (aliased_import name: (dotted_name) @import))
(import_from_statement module_name: (dotted_name) @import)
(import_from_statement module_name: (relative_import) @import)
"#;

/// The Svelte grammar parses the component's markup; declarations live in the
/// script elements, whose raw text is re-parsed as TypeScript.
const SVELTE_QUERY: &str = r#"
(script_element (raw_text) @script)
"#;

fn rust_grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(|| Grammar::compile(tree_sitter_rust::LANGUAGE.into(), RUST_QUERY))
}

fn typescript_grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(|| {
        Grammar::compile(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), TYPESCRIPT_QUERY)
    })
}

fn tsx_grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(|| {
        Grammar::compile(tree_sitter_typescript::LANGUAGE_TSX.into(), TYPESCRIPT_QUERY)
    })
}

fn javascript_grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR
        .get_or_init(|| Grammar::compile(tree_sitter_javascript::LANGUAGE.into(), JAVASCRIPT_QUERY))
}

fn python_grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(|| Grammar::compile(tree_sitter_python::LANGUAGE.into(), PYTHON_QUERY))
}

fn svelte_grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(|| Grammar::compile(tree_sitter_svelte_ng::LANGUAGE.into(), SVELTE_QUERY))
}

/// Extracts declarations and imports from one file's text.
///
/// Output is ordered by line, then name, then role, and deduplicated, so equal
/// inputs produce identical output — grounding (`SPEC.md` §8.3) compares
/// occurrence lists structurally.
pub fn extract(text: &str, language: Language) -> Vec<Occurrence> {
    let mut out = Vec::new();
    match language {
        Language::Rust => extract_with(rust_grammar(), text, 0, &mut out),
        Language::TypeScript => extract_with(typescript_grammar(), text, 0, &mut out),
        Language::Tsx => extract_with(tsx_grammar(), text, 0, &mut out),
        Language::JavaScript => extract_with(javascript_grammar(), text, 0, &mut out),
        Language::Python => extract_with(python_grammar(), text, 0, &mut out),
        Language::Svelte => extract_svelte(text, &mut out),
        Language::Unknown => {}
    }
    out.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.role.cmp(&b.role))
    });
    out.dedup();
    out
}

/// Parses `source` with `grammar` and appends every captured occurrence.
///
/// `line_offset` is the number of lines preceding `source` in its enclosing
/// document; zero for a whole file, nonzero for embedded script text.
fn extract_with(grammar: &'static Grammar, source: &str, line_offset: usize, out: &mut Vec<Occurrence>) {
    let mut parser = Parser::new();
    if parser.set_language(&grammar.language).is_err() {
        return;
    }
    let Some(tree) = parser.parse(source, None) else {
        return;
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&grammar.query, tree.root_node(), source.as_bytes());
    while let Some(matched) = matches.next() {
        for capture in matched.captures {
            let role = match grammar.query.capture_names()[capture.index as usize] {
                CAPTURE_DEFINITION => Role::Definition,
                CAPTURE_IMPORT => Role::Import,
                _ => continue,
            };
            let Ok(name) = capture.node.utf8_text(source.as_bytes()) else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            out.push(Occurrence {
                name: name.to_string(),
                role,
                line: capture.node.start_position().row + 1 + line_offset,
            });
        }
    }
}

/// Locates each `script_element`'s raw text with the Svelte grammar and parses
/// it as TypeScript, preserving the file's line numbering.
///
/// The TypeScript grammar covers both `lang="ts"` and plain JavaScript script
/// blocks; Svelte templates contain no JSX, so the ambiguity that forces the
/// TS/TSX split elsewhere does not arise here.
fn extract_svelte(source: &str, out: &mut Vec<Occurrence>) {
    let svelte = svelte_grammar();
    let mut parser = Parser::new();
    if parser.set_language(&svelte.language).is_err() {
        return;
    }
    let Some(tree) = parser.parse(source, None) else {
        return;
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&svelte.query, tree.root_node(), source.as_bytes());
    while let Some(matched) = matches.next() {
        for capture in matched.captures {
            let Ok(script) = capture.node.utf8_text(source.as_bytes()) else {
                continue;
            };
            extract_with(typescript_grammar(), script, capture.node.start_position().row, out);
        }
    }
}

/// Case-insensitive whole-word occurrences of `needle`, as references.
pub fn references(text: &str, needle: &str) -> Vec<usize> {
    let lowered = needle.to_lowercase();
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            line.to_lowercase()
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|word| word == lowered)
        })
        .map(|(index, _)| index + 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definitions(text: &str, language: Language) -> Vec<String> {
        extract(text, language)
            .into_iter()
            .filter(|o| o.role == Role::Definition)
            .map(|o| o.name)
            .collect()
    }

    #[test]
    fn rust_declarations_are_found_with_and_without_visibility() {
        let text = "pub fn claim_key() {}\nfn helper() {}\npub struct Store;\nimpl ClaimSource for Store {}";
        let names = definitions(text, Language::Rust);
        assert!(names.contains(&"claim_key".to_string()));
        assert!(
            names.contains(&"helper".to_string()),
            "a private item is still a definition site"
        );
        assert!(names.contains(&"Store".to_string()));
    }

    #[test]
    fn rust_imports_are_captured() {
        let found = extract("use vyrm_core::key::claim_key;", Language::Rust);
        assert!(found.iter().any(|o| o.role == Role::Import && o.name.contains("vyrm_core")));
    }

    #[test]
    fn a_multi_line_declaration_is_attributed_to_its_naming_line() {
        let text = "pub fn spread(\n    a: u64,\n    b: u64,\n) -> u64 {\n    a + b\n}";
        let found = extract(text, Language::Rust);
        let spread = found
            .iter()
            .find(|o| o.name == "spread" && o.role == Role::Definition)
            .expect("multi-line declaration missed");
        assert_eq!(spread.line, 1);
    }

    #[test]
    fn ecmascript_exports_and_imports_are_captured() {
        let text = "export function route() {}\nimport { Store } from './store';";
        let found = extract(text, Language::TypeScript);
        assert!(found.iter().any(|o| o.role == Role::Definition && o.name == "route"));
        assert!(found.iter().any(|o| o.role == Role::Import && o.name == "./store"));
    }

    #[test]
    fn tsx_components_parse_despite_jsx_syntax() {
        let text = "export function Panel() {\n  return <div className=\"x\">hi</div>;\n}";
        let names = definitions(text, Language::Tsx);
        assert!(names.contains(&"Panel".to_string()), "JSX broke extraction: {names:?}");
    }

    #[test]
    fn python_definitions_and_imports_are_captured() {
        let text = "import os\nfrom collections import OrderedDict\n\nVERSION = 1\n\nclass Router:\n    def route(self):\n        pass\n";
        let found = extract(text, Language::Python);
        let names = definitions(text, Language::Python);
        assert!(names.contains(&"Router".to_string()));
        assert!(names.contains(&"route".to_string()));
        assert!(names.contains(&"VERSION".to_string()), "module-level assignment missed");
        assert!(found.iter().any(|o| o.role == Role::Import && o.name == "os"));
        assert!(found.iter().any(|o| o.role == Role::Import && o.name == "collections"));
    }

    #[test]
    fn svelte_script_declarations_carry_file_line_numbers() {
        let text = "<script lang=\"ts\">\n  export let title: string;\n  function toggle() {}\n</script>\n\n<button on:click={toggle}>{title}</button>\n";
        let found = extract(text, Language::Svelte);
        let toggle = found
            .iter()
            .find(|o| o.name == "toggle" && o.role == Role::Definition)
            .expect("script-block declaration missed");
        assert_eq!(toggle.line, 3, "line number not offset to the enclosing file");
    }

    #[test]
    fn comments_do_not_produce_declarations() {
        let found = extract("// pub fn not_real() {}", Language::Rust);
        assert!(found.is_empty(), "a commented declaration was indexed: {found:?}");
    }

    #[test]
    fn string_literals_do_not_produce_declarations() {
        let text = "fn real() {\n    let s = \"pub fn fake() {}\";\n}";
        let names = definitions(text, Language::Rust);
        assert!(names.contains(&"real".to_string()));
        assert!(!names.contains(&"fake".to_string()), "a declaration inside a string was indexed");
    }

    #[test]
    fn references_match_whole_words_only() {
        let text = "let claim_key = 1;\nlet claim_keyring = 2;";
        assert_eq!(references(text, "claim_key"), vec![1]);
    }
}
