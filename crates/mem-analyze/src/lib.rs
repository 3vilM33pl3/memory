use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Parser, TreeCursor};

pub const ANALYZER_VERSION: &str = "mem-analyze-v2";
pub const RESOLUTION_STRATEGY_VERSION: &str = "code-graph-resolution-v1";

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

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Variable => "variable",
        }
    }
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
    #[serde(default)]
    pub stable_identity: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub kind: SymbolKind,
    pub name: String,
    pub qualified_name: Option<String>,
    pub span: Span,
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
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
    #[serde(default = "default_analyzer_version")]
    pub analyzer_version: String,
    pub enabled_analyzers: Vec<String>,
    pub summaries: Vec<AnalyzerSummary>,
    pub symbols: Vec<SymbolFact>,
    pub imports: Vec<ImportFact>,
    pub references: Vec<ReferenceFact>,
    pub calls: Vec<CallFact>,
    pub test_links: Vec<TestLinkFact>,
}

fn default_analyzer_version() -> String {
    ANALYZER_VERSION.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Import,
    Call,
    Reference,
    TestLink,
}

impl ReferenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Call => "call",
            Self::Reference => "reference",
            Self::TestLink => "test_link",
        }
    }

    pub fn graph_edge_kind(&self) -> &'static str {
        match self {
            Self::Import => "imports",
            Self::Call => "calls",
            Self::Reference => "references",
            Self::TestLink => "tested_by",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Resolved,
    Unresolved,
    Ambiguous,
}

impl ResolutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedCodeSymbol {
    pub fact_id: String,
    pub stable_identity: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub kind: SymbolKind,
    pub name: String,
    pub qualified_name: Option<String>,
    pub span: Span,
    pub display: String,
    pub source_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedCodeReference {
    pub fact_id: String,
    pub kind: ReferenceKind,
    pub graph_edge_kind: String,
    pub language: AnalyzerLanguage,
    pub file_path: String,
    pub source_symbol_identity: Option<String>,
    pub target_symbol_identity: Option<String>,
    pub source_text: Option<String>,
    pub target_text: String,
    pub span: Span,
    pub resolution_status: ResolutionStatus,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedAnalysisReport {
    pub analyzer_version: String,
    pub resolution_strategy_version: String,
    pub symbols: Vec<ResolvedCodeSymbol>,
    pub references: Vec<ResolvedCodeReference>,
}

pub fn analyze_repository(
    repo_root: &Path,
    tracked_paths: &[String],
    enabled_analyzers: &[String],
) -> Result<AnalysisReport> {
    let mut report = AnalysisReport {
        analyzer_version: ANALYZER_VERSION.to_string(),
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
                summary.errors.push(format!("{}: {}", path, error));
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

pub fn resolve_analysis(report: &AnalysisReport) -> ResolvedAnalysisReport {
    let mut symbol_index = SymbolIndex::default();
    for symbol in &report.symbols {
        symbol_index.insert(symbol);
    }

    let symbols = report
        .symbols
        .iter()
        .map(|symbol| ResolvedCodeSymbol {
            fact_id: symbol.id.clone(),
            stable_identity: stable_symbol_identity(symbol),
            language: symbol.language.clone(),
            file_path: symbol.file_path.clone(),
            kind: symbol.kind.clone(),
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            span: symbol.span.clone(),
            display: symbol.display.clone(),
            source_hash: symbol.source_hash.clone(),
        })
        .collect();

    let mut references = Vec::new();
    references.extend(report.imports.iter().map(|fact| {
        let target_text = fact
            .target
            .clone()
            .unwrap_or_else(|| fact.import_text.clone());
        build_resolved_reference(
            &symbol_index,
            &fact.id,
            ReferenceKind::Import,
            &fact.language,
            &fact.file_path,
            None,
            None,
            &target_text,
            &fact.span,
        )
    }));
    references.extend(report.calls.iter().map(|fact| {
        build_resolved_reference(
            &symbol_index,
            &fact.id,
            ReferenceKind::Call,
            &fact.language,
            &fact.file_path,
            fact.caller_symbol.as_deref(),
            fact.caller_symbol.as_deref(),
            &fact.callee_text,
            &fact.span,
        )
    }));
    references.extend(report.references.iter().map(|fact| {
        build_resolved_reference(
            &symbol_index,
            &fact.id,
            ReferenceKind::Reference,
            &fact.language,
            &fact.file_path,
            fact.enclosing_symbol.as_deref(),
            fact.enclosing_symbol.as_deref(),
            &fact.reference_text,
            &fact.span,
        )
    }));
    references.extend(report.test_links.iter().map(|fact| {
        build_resolved_reference(
            &symbol_index,
            &fact.id,
            ReferenceKind::TestLink,
            &fact.language,
            &fact.file_path,
            Some(&fact.test_name),
            Some(&fact.test_name),
            fact.target_symbol.as_deref().unwrap_or(&fact.test_name),
            &fact.span,
        )
    }));

    ResolvedAnalysisReport {
        analyzer_version: default_analyzer_version(),
        resolution_strategy_version: RESOLUTION_STRATEGY_VERSION.to_string(),
        symbols,
        references,
    }
}

#[derive(Default)]
struct SymbolIndex<'a> {
    by_name: BTreeMap<String, Vec<&'a SymbolFact>>,
    by_qualified_name: BTreeMap<String, Vec<&'a SymbolFact>>,
}

impl<'a> SymbolIndex<'a> {
    fn insert(&mut self, symbol: &'a SymbolFact) {
        self.by_name
            .entry(symbol.name.clone())
            .or_default()
            .push(symbol);
        if let Some(qualified_name) = &symbol.qualified_name {
            self.by_qualified_name
                .entry(qualified_name.clone())
                .or_default()
                .push(symbol);
        }
    }

    fn resolve(&self, language: &AnalyzerLanguage, file_path: &str, text: &str) -> Resolution {
        let normalized = normalize_target_text(text);
        if normalized.is_empty() {
            return Resolution::unresolved();
        }
        if let Some(matches) = self.by_qualified_name.get(&normalized) {
            return choose_symbol(language, file_path, matches, 1.0);
        }
        let simple_name = normalized
            .rsplit([':', '.', '/'])
            .find(|part| !part.is_empty())
            .unwrap_or(&normalized);
        if let Some(matches) = self.by_name.get(simple_name) {
            return choose_symbol(language, file_path, matches, 0.7);
        }
        Resolution::unresolved()
    }
}

struct Resolution {
    status: ResolutionStatus,
    target_symbol_identity: Option<String>,
    confidence: f32,
}

impl Resolution {
    fn unresolved() -> Self {
        Self {
            status: ResolutionStatus::Unresolved,
            target_symbol_identity: None,
            confidence: 0.0,
        }
    }
}

fn choose_symbol(
    language: &AnalyzerLanguage,
    file_path: &str,
    matches: &[&SymbolFact],
    base_confidence: f32,
) -> Resolution {
    let language_matches = matches
        .iter()
        .copied()
        .filter(|symbol| &symbol.language == language)
        .collect::<Vec<_>>();
    let candidates = if language_matches.is_empty() {
        matches.to_vec()
    } else {
        language_matches
    };
    let same_file = candidates
        .iter()
        .copied()
        .filter(|symbol| symbol.file_path == file_path)
        .collect::<Vec<_>>();
    let narrowed = if same_file.is_empty() {
        candidates
    } else {
        same_file
    };
    if narrowed.len() == 1 {
        Resolution {
            status: ResolutionStatus::Resolved,
            target_symbol_identity: Some(stable_symbol_identity(narrowed[0])),
            confidence: base_confidence,
        }
    } else {
        Resolution {
            status: ResolutionStatus::Ambiguous,
            target_symbol_identity: None,
            confidence: 0.0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_resolved_reference(
    index: &SymbolIndex<'_>,
    fact_id: &str,
    kind: ReferenceKind,
    language: &AnalyzerLanguage,
    file_path: &str,
    source_symbol_text: Option<&str>,
    source_text: Option<&str>,
    target_text: &str,
    span: &Span,
) -> ResolvedCodeReference {
    let source_symbol_identity = source_symbol_text.and_then(|text| {
        let resolution = index.resolve(language, file_path, text);
        if resolution.status == ResolutionStatus::Resolved {
            resolution.target_symbol_identity
        } else {
            None
        }
    });
    let resolution = index.resolve(language, file_path, target_text);
    ResolvedCodeReference {
        fact_id: fact_id.to_string(),
        graph_edge_kind: kind.graph_edge_kind().to_string(),
        kind,
        language: language.clone(),
        file_path: file_path.to_string(),
        source_symbol_identity,
        target_symbol_identity: resolution.target_symbol_identity,
        source_text: source_text.map(ToOwned::to_owned),
        target_text: target_text.to_string(),
        span: span.clone(),
        resolution_status: resolution.status,
        confidence: resolution.confidence,
    }
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
        "py" if enabled.iter().any(|item| item == "python") => {
            Some(("python", AnalyzerLanguage::Python))
        }
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
        | (AnalyzerLanguage::JavaScript, "function_declaration") => {
            (SymbolKind::Function, "identifier")
        }
        (AnalyzerLanguage::TypeScript, "class_declaration")
        | (AnalyzerLanguage::JavaScript, "class_declaration") => {
            (SymbolKind::Class, "type_identifier")
        }
        (AnalyzerLanguage::TypeScript, "method_definition")
        | (AnalyzerLanguage::JavaScript, "method_definition") => {
            (SymbolKind::Method, "property_identifier")
        }
        (AnalyzerLanguage::TypeScript, "interface_declaration") => {
            (SymbolKind::Interface, "type_identifier")
        }
        (AnalyzerLanguage::TypeScript, "lexical_declaration")
        | (AnalyzerLanguage::JavaScript, "lexical_declaration")
        | (AnalyzerLanguage::TypeScript, "variable_declaration")
        | (AnalyzerLanguage::JavaScript, "variable_declaration") => {
            (SymbolKind::Variable, "identifier")
        }
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
        id: fact_id(path, kind.as_str(), &name, node),
        stable_identity: symbol_identity(
            path,
            language,
            &kind,
            qualified_name.as_deref().unwrap_or(&name),
            &span(node),
        ),
        language: language.clone(),
        file_path: path.to_string(),
        kind: kind.clone(),
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        span: span(node),
        display: qualified_name.unwrap_or(name),
        source_hash: node_text(node, source).map(|text| content_hash(&text)),
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
            matches!(
                kind,
                "identifier" | "property_identifier" | "type_identifier"
            )
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
            .map(|value| {
                value
                    .trim()
                    .trim_matches(';')
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string()
            })
            .or_else(|| {
                import_text
                    .strip_prefix("import ")
                    .map(|value| value.trim().trim_matches(';').to_string())
            }),
        AnalyzerLanguage::Python => {
            if let Some(rest) = import_text.strip_prefix("from ") {
                rest.split_whitespace().next().map(ToOwned::to_owned)
            } else {
                import_text
                    .strip_prefix("import ")
                    .map(|value| value.trim().to_string())
            }
        }
    }
}

fn stable_symbol_identity(symbol: &SymbolFact) -> String {
    if !symbol.stable_identity.trim().is_empty() {
        return symbol.stable_identity.clone();
    }
    symbol_identity(
        &symbol.file_path,
        &symbol.language,
        &symbol.kind,
        symbol.qualified_name.as_deref().unwrap_or(&symbol.name),
        &symbol.span,
    )
}

fn symbol_identity(
    path: &str,
    language: &AnalyzerLanguage,
    kind: &SymbolKind,
    qualified_name: &str,
    span: &Span,
) -> String {
    format!(
        "{}:{}:{}:{}:{}-{}",
        language.as_str(),
        path,
        kind.as_str(),
        qualified_name,
        span.start_line,
        span.end_line
    )
}

fn normalize_target_text(text: &str) -> String {
    text.trim()
        .trim_matches(';')
        .trim_matches('\'')
        .trim_matches('"')
        .trim_start_matches("crate::")
        .trim_start_matches("self::")
        .trim()
        .to_string()
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
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
        let analysis = analyze_source("src/lib.rs", src, AnalyzerLanguage::Rust)
            .expect("analyze Rust fixture");
        assert!(analysis.symbols.iter().any(|s| s.name == "run"));
        assert!(
            analysis
                .imports
                .iter()
                .any(|i| i.import_text.contains("use crate::db::Pool"))
        );
        assert!(
            analysis
                .calls
                .iter()
                .any(|c| c.callee_text.contains("helper"))
        );
        assert!(!analysis.test_links.is_empty());
        assert!(
            analysis
                .symbols
                .iter()
                .all(|s| !s.stable_identity.is_empty())
        );
    }

    #[test]
    fn ts_analysis_extracts_symbols_imports_and_test_calls() {
        let src = r#"
            import { boot } from './boot';
            export function run() { boot(); }
            test('smoke', () => run());
        "#;
        let analysis = analyze_source("web/app.ts", src, AnalyzerLanguage::TypeScript)
            .expect("analyze TypeScript fixture");
        assert!(analysis.symbols.iter().any(|s| s.name == "run"));
        assert!(
            analysis
                .imports
                .iter()
                .any(|i| i.import_text.contains("import { boot }"))
        );
        assert!(
            analysis
                .calls
                .iter()
                .any(|c| c.callee_text.contains("boot"))
        );
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
        let analysis = analyze_source("app.py", src, AnalyzerLanguage::Python)
            .expect("analyze Python fixture");
        assert!(analysis.symbols.iter().any(|s| s.name == "App"));
        assert!(
            analysis
                .imports
                .iter()
                .any(|i| i.import_text.contains("import os"))
        );
        assert!(
            analysis
                .calls
                .iter()
                .any(|c| c.callee_text.contains("helper"))
        );
        assert!(!analysis.test_links.is_empty());
    }

    #[test]
    fn resolver_marks_local_calls_as_resolved() {
        let src = r#"
            fn run() { helper(); }
            fn helper() {}
        "#;
        let analysis = analyze_source("src/lib.rs", src, AnalyzerLanguage::Rust)
            .expect("analyze Rust fixture");
        let report = AnalysisReport {
            analyzer_version: ANALYZER_VERSION.to_string(),
            symbols: analysis.symbols,
            calls: analysis.calls,
            ..AnalysisReport::default()
        };
        let resolved = resolve_analysis(&report);
        assert!(resolved.references.iter().any(|reference| {
            reference.kind == ReferenceKind::Call
                && reference.target_text == "helper"
                && reference.resolution_status == ResolutionStatus::Resolved
                && reference.target_symbol_identity.is_some()
        }));
    }

    #[test]
    fn resolver_marks_duplicate_names_as_ambiguous() {
        let span = Span {
            start_byte: 0,
            end_byte: 1,
            start_line: 1,
            end_line: 1,
        };
        let report = AnalysisReport {
            analyzer_version: ANALYZER_VERSION.to_string(),
            symbols: vec![
                SymbolFact {
                    id: "a".to_string(),
                    stable_identity: "rust:src/a.rs:function:helper:1-1".to_string(),
                    language: AnalyzerLanguage::Rust,
                    file_path: "src/a.rs".to_string(),
                    kind: SymbolKind::Function,
                    name: "helper".to_string(),
                    qualified_name: Some("a::helper".to_string()),
                    span: span.clone(),
                    display: "a::helper".to_string(),
                    source_hash: None,
                },
                SymbolFact {
                    id: "b".to_string(),
                    stable_identity: "rust:src/b.rs:function:helper:1-1".to_string(),
                    language: AnalyzerLanguage::Rust,
                    file_path: "src/b.rs".to_string(),
                    kind: SymbolKind::Function,
                    name: "helper".to_string(),
                    qualified_name: Some("b::helper".to_string()),
                    span: span.clone(),
                    display: "b::helper".to_string(),
                    source_hash: None,
                },
            ],
            calls: vec![CallFact {
                id: "call".to_string(),
                language: AnalyzerLanguage::Rust,
                file_path: "src/c.rs".to_string(),
                callee_text: "helper".to_string(),
                caller_symbol: None,
                span,
            }],
            ..AnalysisReport::default()
        };
        let resolved = resolve_analysis(&report);
        assert_eq!(
            resolved.references[0].resolution_status,
            ResolutionStatus::Ambiguous
        );
    }
}
