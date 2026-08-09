//! AST analysis — tree-sitter-backed when the `ast` feature is enabled,
//! regex fallback otherwise.
//!
//! T3 of the zero-node-dependency plan. The `ast` feature is OFF by default
//! because tree-sitter grammars compile C and add ~30s to a clean build.
//! Enable with: `cargo build --features ruflo-cli/ast`.
//!
//! Either path returns the same `AstAnalysis` shape so callers don't branch.

use serde_json::{json, Value};

#[derive(Debug, Clone, Default)]
pub struct AstAnalysis {
    pub language: String,
    pub functions: Vec<String>,
    pub classes: Vec<String>,
    pub structs: Vec<String>,
    pub imports: Vec<String>,
    pub loc: usize,
}

impl AstAnalysis {
    pub fn to_json(&self) -> Value {
        json!({
            "language": self.language,
            "functions": self.functions,
            "classes": self.classes,
            "structs": self.structs,
            "imports": self.imports,
            "loc": self.loc,
            "backend": if cfg!(feature = "ast") { "tree-sitter" } else { "regex" },
        })
    }
}

/// Analyze a source file. Dispatches to tree-sitter (feature `ast`) or regex.
pub fn analyze_source(path: &str, source: &str) -> AstAnalysis {
    let lang = detect_language(path);
    #[cfg(feature = "ast")]
    {
        if let Some(result) = analyze_treesitter(path, source, &lang) {
            return result;
        }
    }
    analyze_regex(source, &lang)
}

pub fn detect_language(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => "rust".into(),
        "ts" | "mts" | "cts" => "typescript".into(),
        "tsx" => "tsx".into(),
        "js" | "mjs" | "cjs" => "javascript".into(),
        "jsx" => "jsx".into(),
        "py" => "python".into(),
        "go" => "go".into(),
        "java" => "java".into(),
        "c" | "h" => "c".into(),
        "cpp" | "cc" | "cxx" | "hpp" => "cpp".into(),
        _ => "unknown".into(),
    }
}

// ---- tree-sitter path (feature-gated) ---------------------------------------

#[cfg(feature = "ast")]
fn analyze_treesitter(path: &str, source: &str, lang: &str) -> Option<AstAnalysis> {
    use tree_sitter::{Language, Parser};

    let language: Language = match lang {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "typescript" | "tsx" => {
            // tree-sitter-typescript exposes both TS and TSX.
            if lang == "tsx" {
                tree_sitter_typescript::LANGUAGE_TSX.into()
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
        }
        _ => return None, // no grammar compiled for this language
    };

    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();

    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut structs = Vec::new();
    let mut imports = Vec::new();

    // Walk all named nodes once, classify by kind.
    let mut cursor = root.walk();
    for node in root.named_descendants() {
        let kind = node.kind();
        match kind {
            // Rust
            "function_item" | "function_signature_item" => {
                if let Some(name) = node.child_by_field_name("name") {
                    if let Ok(n) = name.utf8_text(source.as_bytes()) {
                        functions.push(n.to_string());
                    }
                }
            }
            "struct_item" => {
                if let Some(name) = node.child_by_field_name("name") {
                    if let Ok(n) = name.utf8_text(source.as_bytes()) {
                        structs.push(n.to_string());
                    }
                }
            }
            "trait_item" | "enum_item" | "impl_item" => {
                if let Some(name) = node.child_by_field_name("name") {
                    if let Ok(n) = name.utf8_text(source.as_bytes()) {
                        classes.push(n.to_string());
                    }
                }
            }
            "use_declaration" => {
                if let Ok(t) = node.utf8_text(source.as_bytes()) {
                    imports.push(t.trim().to_string());
                }
            }
            // TypeScript / JavaScript
            "function_declaration" | "method_definition" | "arrow_function" => {
                if let Some(name) = node.child_by_field_name("name") {
                    if let Ok(n) = name.utf8_text(source.as_bytes()) {
                        functions.push(n.to_string());
                    }
                }
            }
            "class_declaration" | "interface_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    if let Ok(n) = name.utf8_text(source.as_bytes()) {
                        classes.push(n.to_string());
                    }
                }
            }
            "import_statement" => {
                if let Ok(t) = node.utf8_text(source.as_bytes()) {
                    imports.push(t.trim().to_string());
                }
            }
            _ => {}
        }
    }
    drop(cursor);
    drop(language);

    let loc = source.lines().count();
    Some(AstAnalysis {
        language: lang.to_string(),
        functions,
        classes,
        structs,
        imports,
        loc,
    })
}

// ---- regex fallback (always available) --------------------------------------

fn analyze_regex(source: &str, lang: &str) -> AstAnalysis {
    use regex::Regex;
    let mut classes = Vec::new();
    let mut structs = Vec::new();

    let func_re = match lang {
        "rust" => Regex::new(r"(?m)^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)").unwrap(),
        "typescript" | "tsx" | "javascript" | "jsx" => {
            Regex::new(r"\b(?:function|const)\s+(\w+)\s*(?:=|\()").unwrap()
        }
        "python" => Regex::new(r"(?m)^\s*def\s+(\w+)").unwrap(),
        "go" => Regex::new(r"(?m)^\s*func\s+(?:\([^)]*\)\s+)?(\w+)").unwrap(),
        _ => Regex::new(r"\bfn\s+(\w+)").unwrap(),
    };
    let functions: Vec<String> = func_re
        .captures_iter(source)
        .map(|c| c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())
        .filter(|s| !s.is_empty())
        .collect();

    let mut imports: Vec<String> = Vec::new();
    let import_re = match lang {
        "rust" => Regex::new(r"(?m)^\s*use\s+([\w:]+)").unwrap(),
        "typescript" | "tsx" | "javascript" | "jsx" => {
            Regex::new(r#"(?:import\s+[^'"]+\s+from\s+|require\s*\(\s*)['"]([^'"]+)['"]"#).unwrap()
        }
        "python" => Regex::new(r"(?m)^\s*(?:from\s+[\w.]+\s+)?import\s+([\w.]+)").unwrap(),
        "go" => Regex::new(r#""([\w./]+)""#).unwrap(),
        _ => Regex::new(r"import\s+(\w+)").unwrap(),
    };
    let imports: Vec<String> = import_re
        .captures_iter(source)
        .map(|c| c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())
        .filter(|s| !s.is_empty())
        .collect();

    if lang == "rust" {
        let struct_re = Regex::new(r"(?m)^\s*(?:pub\s+)?struct\s+(\w+)").unwrap();
        let enum_re = Regex::new(r"(?m)^\s*(?:pub\s+)?(?:enum|trait)\s+(\w+)").unwrap();
        for c in struct_re.captures_iter(source) {
            if let Some(m) = c.get(1) {
                structs.push(m.as_str().to_string());
            }
        }
        for c in enum_re.captures_iter(source) {
            if let Some(m) = c.get(1) {
                classes.push(m.as_str().to_string());
            }
        }
    } else if matches!(lang, "typescript" | "tsx" | "javascript" | "jsx") {
        let class_re = Regex::new(r"\b(?:class|interface)\s+(\w+)").unwrap();
        for c in class_re.captures_iter(source) {
            if let Some(m) = c.get(1) {
                classes.push(m.as_str().to_string());
            }
        }
    }

    let loc = source.lines().count();
    AstAnalysis {
        language: lang.to_string(),
        functions,
        classes,
        structs,
        imports,
        loc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_extracts_rust_symbols() {
        let src = r#"
use std::collections::HashMap;

pub struct Foo { x: i32 }

trait Bar { fn baz(&self); }

pub fn add(a: i32, b: i32) -> i32 { a + b }
"#;
        let analysis = analyze_source("test.rs", src);
        assert_eq!(analysis.language, "rust");
        assert!(analysis.functions.contains(&"add".to_string()));
        assert!(analysis.structs.contains(&"Foo".to_string()));
        assert!(analysis.classes.contains(&"Bar".to_string()));
        assert!(analysis.imports.iter().any(|i| i.contains("HashMap")));
    }

    #[test]
    fn regex_extracts_typescript_symbols() {
        let src = r#"
import { foo } from './bar';

class Baz { method() {} }

function quux(x: number): number { return x; }
"#;
        let analysis = analyze_source("test.ts", src);
        assert_eq!(analysis.language, "typescript");
        assert!(analysis.functions.contains(&"quux".to_string()));
        assert!(analysis.classes.contains(&"Baz".to_string()));
    }

    #[test]
    fn detects_language_by_extension() {
        assert_eq!(detect_language("foo.rs"), "rust");
        assert_eq!(detect_language("foo.ts"), "typescript");
        assert_eq!(detect_language("foo.tsx"), "tsx");
        assert_eq!(detect_language("foo.py"), "python");
        assert_eq!(detect_language("foo.unknown"), "unknown");
    }

    #[test]
    fn json_shape_consistent() {
        let src = "fn main() {}";
        let analysis = analyze_source("x.rs", src);
        let j = analysis.to_json();
        assert!(j["functions"].is_array());
        assert!(j["loc"].is_number());
        let backend = j["backend"].as_str().unwrap_or("");
        assert!(backend == "tree-sitter" || backend == "regex");
    }
}
