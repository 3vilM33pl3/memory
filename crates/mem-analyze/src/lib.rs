use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, TreeCursor};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzerLanguage {
    Rust,
    TypeScript,
    JavaScript,
    Python,
}

impl AnalyzerLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Module,
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Class,
    Interface,
    Variable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolFact {
    pub id: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub kind: SymbolKind,
    pub name: String,
    pub qualified_name: Option<String>,
    pub span: Span,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportFact {
    pub id: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub import_text: String,
    pub target: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceFact {
    pub id: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub reference_text: String,
    pub enclosing_symbol: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallFact {
    pub id: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub callee_text: String,
    pub caller_symbol: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestLinkFact {
    pub id: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub test_name: String,
    pub target_symbol: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalyzerSummary {
    pub analyzer: String,
    pub files_seen: usize,
    pub files_parsed: usize,
    pub symbol_count: usize,
    pub import_count: usize,
    pub reference_count: usize,
    pub call_count: usize,
    pub test_link_count: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisReport {
    pub enabled_analyzers: Vec<String>,
    pub summaries: Vec<AnalyzerSummary>,
    pub symbols: Vec<SymbolFact>,
    pub imports: Vec<ImportFact>,
    pub references: Vec<ReferenceFact>,
    pub calls: Vec<CallFact>,
    pub test_links: Vec<TestLinkFact>,
}

pub fn analyze_repository(
    repo_root: &Path,
    tracked_paths: &[String],
    enabled_analyzers: &[String],
) -> Result<AnalysisReport> {
    let mut report = AnalysisReport {
        enabled_analyzers: enabled_analyzers.to_vec(),
        ..AnalysisReport::default()
    };

    let mut summaries = BTreeMap::<String, AnalyzerSummary>::new();
    let mut enabled = enabled_analyzers
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    enabled.sort();
    enabled.dedup();

    for analyzer in &enabled {
        summaries.insert(
            analyzer.clone(),
            AnalyzerSummary {
                analyzer: analyzer.clone(),
                ..AnalyzerSummary::default()
            },
        );
    }

    for analyzer in &enabled {
        if !matches!(analyzer.as_str(), "rust" | "typescript" | "python") {
            summaries
                .entry(analyzer.clone())
                .or_default()
                .warnings
                .push(format!("unsupported analyzer: {analyzer}"));
        }
    }

    for path in tracked_paths {
        let Some((parser_kind, language)) = parser_for_path(path, &enabled) else {
            continue;
        };
        let Some(summary) = summaries.get_mut(parser_kind) else {
            continue;
        };
        summary.files_seen += 1;
        let full_path = repo_root.join(path);
        let source = match fs::read_to_string(&full_path) {
            Ok(content) => content,
            Err(error) => {
                summary
                    .errors
                    .push(format!("{}: {}", path, error));
                continue;
            }
        };
        let analysis = match analyze_source(path, &source, language.clone()) {
            Ok(analysis) => analysis,
            Err(error) => {
                summary.errors.push(format!("{}: {}", path, error));
                continue;
            }
        };
        summary.files_parsed += 1;
        summary.symbol_count += analysis.symbols.len();
        summary.import_count += analysis.imports.len();
        summary.reference_count += analysis.references.len();
        summary.call_count += analysis.calls.len();
        summary.test_link_count += analysis.test_links.len();
        report.symbols.extend(analysis.symbols);
        report.imports.extend(analysis.imports);
        report.references.extend(analysis.references);
        report.calls.extend(analysis.calls);
        report.test_links.extend(analysis.test_links);
    }

    report.summaries = summaries.into_values().collect();
    Ok(report)
}

struct FileAnalysis {
    symbols: Vec<SymbolFact>,
    imports: Vec<ImportFact>,
    references: Vec<ReferenceFact>,
    calls: Vec<CallFact>,
    test_links: Vec<TestLinkFact>,
}

fn analyze_source(path: &str, source: &str, language: AnalyzerLanguage) -> Result<FileAnalysis> {
    let mut parser = Parser::new();
    match language {
        AnalyzerLanguage::Rust => parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .context("configure rust parser")?,
        AnalyzerLanguage::TypeScript => parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .context("configure typescript parser")?,
        AnalyzerLanguage::JavaScript => parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .context("configure javascript parser")?,
        AnalyzerLanguage::Python => parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .context("configure python parser")?,
    }

    let tree = parser.parse(source, None).context("parse source")?;
    let root = tree.root_node();
    let mut analysis = FileAnalysis {
        symbols: Vec::new(),
        imports: Vec::new(),
        references: Vec::new(),
        calls: Vec::new(),
        test_links: Vec::new(),
    };
    let mut symbol_stack = Vec::<String>::new();
    walk_tree(
        root,
        source,
        path,
        &language,
        &mut symbol_stack,
        &mut analysis,
    );
    Ok(analysis)
}

fn parser_for_path(path: &str, enabled: &[String]) -> Option<(&'static str, AnalyzerLanguage)> {
    let ext = Path::new(path).extension().and_then(|ext| ext.to_str())?;
    match ext {
        "rs" if enabled.iter().any(|item| item == "rust") => Some(("rust", AnalyzerLanguage::Rust)),
        "ts" | "tsx" if enabled.iter().any(|item| item == "typescript") => {
            Some(("typescript", AnalyzerLanguage::TypeScript))
        }
        "js" | "jsx" | "mjs" | "cjs" if enabled.iter().any(|item| item == "typescript") => {
            Some(("typescript", AnalyzerLanguage::JavaScript))
        }
        "py" if enabled.iter().any(|item| item == "python") => Some(("python", AnalyzerLanguage::Python)),
        _ => None,
    }
}

fn walk_tree(
    node: Node<'_>,
    source: &str,
    path: &str,
    language: &AnalyzerLanguage,
    symbol_stack: &mut Vec<String>,
    analysis: &mut FileAnalysis,
) {
    let kind = node.kind();
    let push_symbol = extract_symbol(node, source, path, language, symbol_stack);
    if let Some(symbol) = &push_symbol {
        symbol_stack.push(
            symbol
                .qualified_name
                .clone()
                .unwrap_or_else(|| symbol.name.clone()),
        );
        analysis.symbols.push(symbol.clone());
    }

    if let Some(import_fact) = extract_import(node, source, path, language) {
        analysis.imports.push(import_fact);
    }
    if let Some(call_fact) = extract_call(node, source, path, language, symbol_stack.last()) {
        analysis.calls.push(call_fact);
    }
    if let Some(reference_fact) =
        extract_reference(node, source, path, language, symbol_stack.last(), kind)
    {
        analysis.references.push(reference_fact);
    }
    if let Some(test_link) = extract_test_link(node, source, path, language, symbol_stack.last()) {
        analysis.test_links.push(test_link);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree(child, source, path, language, symbol_stack, analysis);
    }

    if push_symbol.is_some() {
        symbol_stack.pop();
    }
}

fn extract_symbol(
    node: Node<'_>,
    source: &str,
    path: &str,
    language: &AnalyzerLanguage,
    symbol_stack: &[String],
) -> Option<SymbolFact> {
    let (kind, name_node_kind) = match (language, node.kind()) {
        (AnalyzerLanguage::Rust, "function_item") => (SymbolKind::Function, "identifier"),
        (AnalyzerLanguage::Rust, "struct_item") => (SymbolKind::Struct, "type_identifier"),
        (AnalyzerLanguage::Rust, "enum_item") => (SymbolKind::Enum, "type_identifier"),
        (AnalyzerLanguage::Rust, "trait_item") => (SymbolKind::Trait, "type_identifier"),
        (AnalyzerLanguage::Rust, "impl_item") => (SymbolKind::Module, "type_identifier"),
        (AnalyzerLanguage::TypeScript, "function_declaration")
        | (AnalyzerLanguage::JavaScript, "function_declaration") => (SymbolKind::Function, "identifier"),
        (AnalyzerLanguage::TypeScript, "class_declaration")
        | (AnalyzerLanguage::JavaScript, "class_declaration") => (SymbolKind::Class, "type_identifier"),
        (AnalyzerLanguage::TypeScript, "method_definition")
        | (AnalyzerLanguage::JavaScript, "method_definition") => (SymbolKind::Method, "property_identifier"),
        (AnalyzerLanguage::TypeScript, "interface_declaration") => (SymbolKind::Interface, "type_identifier"),
        (AnalyzerLanguage::TypeScript, "lexical_declaration")
        | (AnalyzerLanguage::JavaScript, "lexical_declaration")
        | (AnalyzerLanguage::TypeScript, "variable_declaration")
        | (AnalyzerLanguage::JavaScript, "variable_declaration") => (SymbolKind::Variable, "identifier"),
        (AnalyzerLanguage::Python, "function_definition") => (SymbolKind::Function, "identifier"),
        (AnalyzerLanguage::Python, "class_definition") => (SymbolKind::Class, "identifier"),
        _ => return None,
    };

    let name = child_text_by_kind(node, name_node_kind, source)
        .or_else(|| child_text_by_kind(node, "property_identifier", source))
        .or_else(|| child_text_by_kind(node, "identifier", source))?;
    let qualified_name = if symbol_stack.is_empty() {
        Some(name.clone())
    } else {
        Some(format!("{}::{}", symbol_stack.join("::"), name))
    };
    Some(SymbolFact {
        id: fact_id(path, kind_name(kind.clone()), &name, node),
        language: language.clone(),
        file_path: path.to_string(),
        kind: kind.clone(),
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        span: span(node),
        display: qualified_name.unwrap_or(name),
    })
}

fn extract_import(
    node: Node<'_>,
    source: &str,
    path: &str,
    language: &AnalyzerLanguage,
) -> Option<ImportFact> {
    let import_text = match (language, node.kind()) {
        (AnalyzerLanguage::Rust, "use_declaration")
        | (AnalyzerLanguage::TypeScript, "import_statement")
        | (AnalyzerLanguage::JavaScript, "import_statement")
        | (AnalyzerLanguage::Python, "import_statement")
        | (AnalyzerLanguage::Python, "import_from_statement") => node_text(node, source),
        _ => return None,
    }?;
    Some(ImportFact {
        id: fact_id(path, "import", &import_text, node),
        language: language.clone(),
        file_path: path.to_string(),
        target: extract_import_target(language, &import_text),
        import_text,
        span: span(node),
    })
}

fn extract_call(
    node: Node<'_>,
    source: &str,
    path: &str,
    language: &AnalyzerLanguage,
    caller_symbol: Option<&String>,
) -> Option<CallFact> {
    let function_node = match (language, node.kind()) {
        (AnalyzerLanguage::Rust, "call_expression")
        | (AnalyzerLanguage::TypeScript, "call_expression")
        | (AnalyzerLanguage::JavaScript, "call_expression")
        | (AnalyzerLanguage::Python, "call") => node.child_by_field_name("function")?,
        _ => return None,
    };
    let callee_text = node_text(function_node, source)?;
    Some(CallFact {
        id: fact_id(path, "call", &callee_text, node),
        language: language.clone(),
        file_path: path.to_string(),
        callee_text,
        caller_symbol: caller_symbol.cloned(),
        span: span(node),
    })
}

fn extract_reference(
    node: Node<'_>,
    source: &str,
    path: &str,
    language: &AnalyzerLanguage,
    caller_symbol: Option<&String>,
    kind: &str,
) -> Option<ReferenceFact> {
    let is_identifier = match language {
        AnalyzerLanguage::Rust => matches!(kind, "identifier" | "type_identifier"),
        AnalyzerLanguage::TypeScript | AnalyzerLanguage::JavaScript => {
            matches!(kind, "identifier" | "property_identifier" | "type_identifier")
        }
        AnalyzerLanguage::Python => kind == "identifier",
    };
    if !is_identifier {
        return None;
    }
    let reference_text = node_text(node, source)?;
    if reference_text.is_empty() {
        return None;
    }
    Some(ReferenceFact {
        id: fact_id(path, "reference", &reference_text, node),
        language: language.clone(),
        file_path: path.to_string(),
        reference_text,
        enclosing_symbol: caller_symbol.cloned(),
        span: span(node),
    })
}

fn extract_test_link(
    node: Node<'_>,
    source: &str,
    path: &str,
    language: &AnalyzerLanguage,
    enclosing_symbol: Option<&String>,
) -> Option<TestLinkFact> {
    match language {
        AnalyzerLanguage::Rust => {
            if node.kind() == "attribute_item" {
                let attr = node_text(node, source)?;
                if attr.contains("#[test]") {
                    return Some(TestLinkFact {
                        id: fact_id(path, "test", &attr, node),
                        language: language.clone(),
                        file_path: path.to_string(),
                        test_name: attr,
                        target_symbol: enclosing_symbol.cloned(),
                        span: span(node),
                    });
                }
            }
        }
        AnalyzerLanguage::TypeScript | AnalyzerLanguage::JavaScript => {
            if node.kind() == "call_expression" {
                let callee = node.child_by_field_name("function")?;
                let callee_text = node_text(callee, source)?;
                if matches!(callee_text.as_str(), "describe" | "it" | "test") {
                    return Some(TestLinkFact {
                        id: fact_id(path, "test", &callee_text, node),
                        language: language.clone(),
                        file_path: path.to_string(),
                        test_name: callee_text,
                        target_symbol: enclosing_symbol.cloned(),
                        span: span(node),
                    });
                }
            }
        }
        AnalyzerLanguage::Python => {
            if node.kind() == "function_definition" {
                let name = child_text_by_kind(node, "identifier", source)?;
                if name.starts_with("test_") {
                    return Some(TestLinkFact {
                        id: fact_id(path, "test", &name, node),
                        language: language.clone(),
                        file_path: path.to_string(),
                        test_name: name,
                        target_symbol: enclosing_symbol.cloned(),
                        span: span(node),
                    });
                }
            }
        }
    }
    None
}

fn extract_import_target(language: &AnalyzerLanguage, import_text: &str) -> Option<String> {
    match language {
        AnalyzerLanguage::Rust => import_text
            .strip_prefix("use ")
            .map(|value| value.trim_end_matches(';').trim().to_string()),
        AnalyzerLanguage::TypeScript | AnalyzerLanguage::JavaScript => import_text
            .split("from")
            .nth(1)
            .map(|value| value.trim().trim_matches(';').trim_matches('\'').trim_matches('"').to_string())
            .or_else(|| {
                import_text
                    .strip_prefix("import ")
                    .map(|value| value.trim().trim_matches(';').to_string())
            }),
        AnalyzerLanguage::Python => {
            if let Some(rest) = import_text.strip_prefix("from ") {
                rest.split_whitespace().next().map(ToOwned::to_owned)
            } else {
                import_text.strip_prefix("import ").map(|value| value.trim().to_string())
            }
        }
    }
}

fn fact_id(path: &str, prefix: &str, name: &str, node: Node<'_>) -> String {
    format!(
        "{}:{}:{}:{}-{}",
        path,
        prefix,
        sanitize(name),
        node.start_byte(),
        node.end_byte()
    )
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Module => "module",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::Variable => "variable",
    }
}

fn span(node: Node<'_>) -> Span {
    let start = node.start_position();
    let end = node.end_position();
    Span {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: start.row + 1,
        end_line: end.row + 1,
    }
}

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes())
        .ok()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn child_text_by_kind(node: Node<'_>, expected_kind: &str, source: &str) -> Option<String> {
    let mut cursor: TreeCursor<'_> = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == expected_kind)
        .and_then(|child| node_text(child, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_analysis_extracts_symbols_imports_calls_and_tests() {
        let src = r#"
            use crate::db::Pool;
            struct App;
            fn run(pool: Pool) { helper(); }
            fn helper() {}
            #[test]
            fn smoke() { run(todo!()); }
        "#;
        let analysis = analyze_source("src/lib.rs", src, AnalyzerLanguage::Rust).unwrap();
        assert!(analysis.symbols.iter().any(|s| s.name == "run"));
        assert!(analysis.imports.iter().any(|i| i.import_text.contains("use crate::db::Pool")));
        assert!(analysis.calls.iter().any(|c| c.callee_text.contains("helper")));
        assert!(!analysis.test_links.is_empty());
    }

    #[test]
    fn ts_analysis_extracts_symbols_imports_and_test_calls() {
        let src = r#"
            import { boot } from './boot';
            export function run() { boot(); }
            test('smoke', () => run());
        "#;
        let analysis = analyze_source("web/app.ts", src, AnalyzerLanguage::TypeScript).unwrap();
        assert!(analysis.symbols.iter().any(|s| s.name == "run"));
        assert!(analysis.imports.iter().any(|i| i.import_text.contains("import { boot }")));
        assert!(analysis.calls.iter().any(|c| c.callee_text.contains("boot")));
        assert!(!analysis.test_links.is_empty());
    }

    #[test]
    fn python_analysis_extracts_symbols_imports_calls_and_tests() {
        let src = r#"
import os

class App:
    def run(self):
        helper()

def helper():
    return os.getcwd()

def test_smoke():
    helper()
"#;
        let analysis = analyze_source("app.py", src, AnalyzerLanguage::Python).unwrap();
        assert!(analysis.symbols.iter().any(|s| s.name == "App"));
        assert!(analysis.imports.iter().any(|i| i.import_text.contains("import os")));
        assert!(analysis.calls.iter().any(|c| c.callee_text.contains("helper")));
        assert!(!analysis.test_links.is_empty());
    }
}
