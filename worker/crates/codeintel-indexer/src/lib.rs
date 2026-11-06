use anyhow::{anyhow, Context, Result};
use codeintel_core::{cache_content, safe_read, Policy, SourceRevision};
use codeintel_frameworks::{
    FrameworkAnalysis, FrameworkFact, FrameworkProject, FrameworkRegistry, FrameworkSource,
};
use codeintel_graph::{
    atomic_publish, edge, staging_path, Coverage, EdgeEvidence, GraphBuilder, GraphNode,
    NodeProvenance,
};
use codeintel_semantic::{
    CallSite, EmbeddedHybridResolver, SemanticProject, SemanticResolver, SemanticSymbol, SourceUnit,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Node, Parser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexResult {
    pub source_revision_id: String,
    pub state: String,
    pub graph_path: PathBuf,
    pub node_count: usize,
    pub edge_count: usize,
    pub coverage: Coverage,
    pub volatile_files: Vec<String>,
}

#[derive(Debug, Clone)]
struct PendingCall {
    caller_id: String,
    target_name: String,
    file_path: String,
    language: String,
    line: u32,
}

#[derive(Debug, Clone)]
struct PendingRoute {
    route_id: String,
    handler_name: Option<String>,
    file_path: String,
    line: u32,
    confidence: f64,
    ambiguity_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedLanguage {
    Php,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    CSharp,
    Rust,
}

impl SupportedLanguage {
    fn name(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::JavaScript => "javascript",
            Self::TypeScript | Self::Tsx => "typescript",
            Self::Python => "python",
            Self::CSharp => "csharp",
            Self::Rust => "rust",
        }
    }

    fn grammar(self) -> Language {
        match self {
            Self::Php => tree_sitter_php::language(),
            Self::JavaScript => tree_sitter_javascript::language(),
            Self::TypeScript => tree_sitter_typescript::language_typescript(),
            Self::Tsx => tree_sitter_typescript::language_tsx(),
            Self::Python => tree_sitter_python::language(),
            Self::CSharp => tree_sitter_c_sharp::language(),
            Self::Rust => tree_sitter_rust::language(),
        }
    }
}

pub fn index_project(
    root: &Path,
    revision: &SourceRevision,
    _policy: &Policy,
    graph_path: &Path,
    content_cache: &Path,
) -> Result<IndexResult> {
    if !revision.volatile_files.is_empty() {
        return Err(anyhow!(
            "Cannot publish CURRENT graph while files are volatile: {}",
            revision.volatile_files.join(", ")
        ));
    }
    let staging = staging_path(graph_path);
    let mut builder = GraphBuilder::create(&staging)?;
    let mut nodes: Vec<GraphNode> = vec![];
    let mut contains_edges: Vec<(String, String, String)> = vec![];
    let mut pending_calls = vec![];
    let mut pending_routes = vec![];
    let mut semantic_units = vec![];
    let mut framework_files = vec![];
    let mut failed_files = vec![];
    let mut supported = BTreeSet::new();
    let mut unsupported = BTreeSet::new();
    let project_identity =
        canonical_node_identity("project", "", "meta", "PROJECT", "project", 0, 0, 0, 0);
    let project_node = GraphNode {
        id: node_id(&project_identity),
        kind: "PROJECT".into(),
        qualified_name: "project".into(),
        display_name: "project".into(),
        file_path: "".into(),
        start_line: 0,
        start_column: 0,
        end_line: 0,
        end_column: 0,
        signature: None,
        language: "meta".into(),
        exported: false,
        entry_point: true,
        logical_root: "project".into(),
        canonical_identity: project_identity,
        provenance: vec![provenance("STATIC_EXACT", "indexer:project")],
    };
    nodes.push(project_node.clone());
    let tx = builder.begin()?;
    for entry in &revision.files {
        tx.add_inventory(entry)?;
        let language = language_for_path(&entry.logical_path);
        if language.is_none() {
            if is_framework_candidate(&entry.logical_path) && entry.size <= 1024 * 1024 {
                let absolute = root.join(&entry.logical_path);
                match safe_read(&absolute, 3) {
                    Ok((bytes, _)) => match std::str::from_utf8(&bytes) {
                        Ok(source) => {
                            cache_content(content_cache, &entry.content_sha256, &bytes)?;
                            tx.add_file_content(&entry.logical_path, "config", source)?;
                            let file_id = append_file_node(
                                &entry.logical_path,
                                "config",
                                source,
                                &project_node,
                                &mut nodes,
                                &mut contains_edges,
                            );
                            framework_files.push(FrameworkSource {
                                path: entry.logical_path.clone(),
                                content: source.into(),
                            });
                            if file_id.is_empty() {
                                failed_files.push(format!(
                                    "{}: failed to create framework config file node",
                                    entry.logical_path
                                ));
                            }
                        }
                        Err(_) => failed_files.push(format!(
                            "{}: framework config is not UTF-8",
                            entry.logical_path
                        )),
                    },
                    Err(_) => failed_files.push(format!(
                        "{}: volatile during framework config read",
                        entry.logical_path
                    )),
                }
            }
            if let Some(extension) = Path::new(&entry.logical_path)
                .extension()
                .and_then(|value| value.to_str())
            {
                if is_probable_source_extension(extension) {
                    unsupported.insert(extension.to_string());
                }
            }
            continue;
        }
        let language = language.unwrap();
        supported.insert(language.name().to_string());
        if entry.size > 4 * 1024 * 1024 {
            failed_files.push(format!(
                "{}: source file exceeds 4 MiB parser cap",
                entry.logical_path
            ));
            continue;
        }
        let absolute = root.join(&entry.logical_path);
        let Ok((bytes, _)) = safe_read(&absolute, 3) else {
            failed_files.push(format!("{}: volatile during parse", entry.logical_path));
            continue;
        };
        cache_content(content_cache, &entry.content_sha256, &bytes)?;
        let Ok(source) = std::str::from_utf8(&bytes) else {
            failed_files.push(format!("{}: source is not UTF-8", entry.logical_path));
            continue;
        };
        tx.add_file_content(&entry.logical_path, language.name(), source)?;
        let file_id = append_file_node(
            &entry.logical_path,
            language.name(),
            source,
            &project_node,
            &mut nodes,
            &mut contains_edges,
        );
        framework_files.push(FrameworkSource {
            path: entry.logical_path.clone(),
            content: source.into(),
        });
        let mut parser = Parser::new();
        parser
            .set_language(language.grammar())
            .with_context(|| format!("load {} grammar", language.name()))?;
        let Some(tree) = parser.parse(source, None) else {
            failed_files.push(format!("{}: parser returned no tree", entry.logical_path));
            continue;
        };
        semantic_units.push(SourceUnit {
            path: entry.logical_path.clone(),
            language: language.name().into(),
            content: source.into(),
        });
        if tree.root_node().has_error() {
            let detail = first_syntax_issue(tree.root_node())
                .map(|node| {
                    let start = node.start_position();
                    format!("{} at {}:{}", node.kind(), start.row + 1, start.column + 1)
                })
                .unwrap_or_else(|| "unknown parser error range".into());
            failed_files.push(format!("{}: {detail}", entry.logical_path));
        }
        extract_tree(
            tree.root_node(),
            source.as_bytes(),
            language,
            &entry.logical_path,
            &file_id,
            None,
            &mut nodes,
            &mut contains_edges,
            &mut pending_calls,
        );
    }
    let framework_analysis = FrameworkRegistry::default().analyze(&FrameworkProject {
        files: framework_files,
    });
    append_framework_analysis(
        &framework_analysis,
        &mut nodes,
        &mut contains_edges,
        &mut pending_routes,
    );
    let nodes = reconcile_nodes(nodes)?;
    for node in &nodes {
        tx.add_node(node)?;
    }
    let semantic_project = SemanticProject {
        units: semantic_units,
        symbols: nodes
            .iter()
            .filter(|node| !matches!(node.kind.as_str(), "FILE" | "PROJECT" | "ROUTE"))
            .map(|node| SemanticSymbol {
                id: node.id.clone(),
                qualified_name: node.qualified_name.clone(),
                display_name: node.display_name.clone(),
                file_path: node.file_path.clone(),
                kind: node.kind.clone(),
                language: node.language.clone(),
                start_line: node.start_line,
            })
            .collect(),
        calls: pending_calls
            .iter()
            .map(|call| CallSite {
                caller_id: call.caller_id.clone(),
                target_expression: call.target_name.clone(),
                file_path: call.file_path.clone(),
                language: call.language.clone(),
                line: call.line,
            })
            .collect(),
        failed_files: failed_files.clone(),
    };
    let semantic_resolver = EmbeddedHybridResolver::analyze(semantic_project)?;
    let semantic_coverage = semantic_resolver.coverage();
    let mut symbols_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for node in &nodes {
        if !matches!(node.kind.as_str(), "FILE" | "PROJECT" | "ROUTE") {
            symbols_by_name
                .entry(node.display_name.to_lowercase())
                .or_default()
                .push(node.id.clone());
        }
    }
    let mut edge_count = 0;
    let mut emitted_contains_edges = BTreeSet::new();
    for (source, target, file) in contains_edges {
        if !emitted_contains_edges.insert((source.clone(), target.clone(), file.clone())) {
            continue;
        }
        tx.add_edge(&edge(EdgeEvidence {
            source_id: &source,
            target_id: &target,
            kind: "CONTAINS",
            origin: "STATIC_EXACT",
            confidence: 1.0,
            source_file: &file,
            source_range: "0",
            revision: &revision.source_revision_id,
            ambiguity_reason: None,
        }))?;
        edge_count += 1;
    }
    for call in pending_calls {
        let call_site = CallSite {
            caller_id: call.caller_id.clone(),
            target_expression: call.target_name.clone(),
            file_path: call.file_path.clone(),
            language: call.language.clone(),
            line: call.line,
        };
        let semantic = semantic_resolver.resolve_call_target(&call_site);
        if !semantic.targets.is_empty() {
            for target in semantic.targets {
                tx.add_edge(&edge(EdgeEvidence {
                    source_id: &call.caller_id,
                    target_id: &target.symbol_id,
                    kind: "CALLS",
                    origin: "SEMANTIC_RESOLVED",
                    confidence: target.confidence,
                    source_file: &call.file_path,
                    source_range: &call.line.to_string(),
                    revision: &revision.source_revision_id,
                    ambiguity_reason: target.ambiguity_reason,
                }))?;
                edge_count += 1;
            }
            continue;
        }
        let candidates = symbols_by_name
            .get(&normalize_target_name(&call.target_name))
            .cloned()
            .unwrap_or_default();
        for target in &candidates {
            let ambiguity = (candidates.len() > 1)
                .then(|| format!("{} symbols share the target name", candidates.len()));
            tx.add_edge(&edge(EdgeEvidence {
                source_id: &call.caller_id,
                target_id: target,
                kind: "CALLS",
                origin: "STATIC_HEURISTIC",
                confidence: if candidates.len() == 1 { 0.78 } else { 0.45 },
                source_file: &call.file_path,
                source_range: &call.line.to_string(),
                revision: &revision.source_revision_id,
                ambiguity_reason: ambiguity,
            }))?;
            edge_count += 1;
        }
    }
    for route in pending_routes {
        if let Some(handler) = route.handler_name {
            let candidates = symbols_by_name
                .get(&normalize_target_name(&handler))
                .cloned()
                .unwrap_or_default();
            for target in &candidates {
                tx.add_edge(&edge(EdgeEvidence {
                    source_id: &route.route_id,
                    target_id: target,
                    kind: "ROUTES_TO",
                    origin: "FRAMEWORK_RESOLVED",
                    confidence: if candidates.len() == 1 {
                        route.confidence
                    } else {
                        route.confidence.min(0.55)
                    },
                    source_file: &route.file_path,
                    source_range: &route.line.to_string(),
                    revision: &revision.source_revision_id,
                    ambiguity_reason: if candidates.len() > 1 {
                        Some("route handler name is ambiguous".into())
                    } else {
                        route.ambiguity_reason.clone()
                    },
                }))?;
                edge_count += 1;
            }
        }
    }
    tx.commit()?;
    let coverage = Coverage {
        supported_languages: supported.into_iter().collect(),
        unsupported_languages: unsupported.into_iter().collect(),
        parsed_files: revision.files.len().saturating_sub(failed_files.len()),
        failed_files,
        semantic: semantic_coverage.status,
        framework: framework_analysis.status,
        semantic_resolved_calls: semantic_coverage.resolved_calls,
        semantic_ambiguous_calls: semantic_coverage.ambiguous_calls,
        semantic_unresolved_calls: semantic_coverage.unresolved_calls,
        semantic_external_or_unindexed_calls: semantic_coverage.external_or_unindexed_calls,
        semantic_unresolved_files: semantic_coverage.unresolved_files,
        semantic_external_or_unindexed_files: semantic_coverage.external_or_unindexed_files,
        semantic_ambiguous_files: semantic_coverage.ambiguous_files,
        dynamic_boundaries: semantic_coverage
            .dynamic_boundaries
            .into_iter()
            .chain(framework_analysis.dynamic_boundaries.iter().cloned())
            .collect(),
        semantic_unknowns: semantic_coverage.unknowns,
        framework_adapters: framework_analysis
            .detections
            .iter()
            .map(|detection| detection.adapter.clone())
            .collect(),
        framework_unknowns: framework_analysis
            .unknowns
            .into_iter()
            .chain(framework_analysis.dynamic_boundaries)
            .collect(),
    };
    let node_count = nodes.len();
    builder.validate(revision, &coverage)?;
    atomic_publish(&staging, graph_path)?;
    Ok(IndexResult {
        source_revision_id: revision.source_revision_id.clone(),
        state: "CURRENT".into(),
        graph_path: graph_path.to_path_buf(),
        node_count,
        edge_count,
        coverage,
        volatile_files: revision.volatile_files.clone(),
    })
}

fn first_syntax_issue(node: Node<'_>) -> Option<Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    let mut cursor = node.walk();
    let issue = node.children(&mut cursor).find_map(first_syntax_issue);
    issue
}

#[allow(clippy::too_many_arguments)]
fn extract_tree(
    node: Node<'_>,
    source: &[u8],
    language: SupportedLanguage,
    file_path: &str,
    file_id: &str,
    scope: Option<(String, String)>,
    nodes: &mut Vec<GraphNode>,
    contains_edges: &mut Vec<(String, String, String)>,
    pending_calls: &mut Vec<PendingCall>,
) {
    let kind = declaration_kind(language, node.kind());
    let mut next_scope = scope.clone();
    if let Some(graph_kind) = kind {
        if let Some(name_node) = declaration_name_node(node) {
            if let Ok(name) = name_node.utf8_text(source) {
                let qualified = if let Some((_, parent_name)) = scope.as_ref() {
                    format!("{parent_name}::{name}")
                } else {
                    format!("{}::{name}", file_path.replace('/', "::"))
                };
                let start = node.start_position();
                let end = node.end_position();
                let canonical_identity = canonical_node_identity(
                    "primary",
                    file_path,
                    language.name(),
                    graph_kind,
                    &qualified,
                    start.row as u32 + 1,
                    start.column as u32,
                    end.row as u32 + 1,
                    end.column as u32,
                );
                let id = node_id(&canonical_identity);
                let signature = source
                    .get(node.start_byte()..node.end_byte().min(node.start_byte() + 300))
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .map(|text| text.lines().next().unwrap_or("").trim().to_string());
                let graph_node = GraphNode {
                    id: id.clone(),
                    kind: graph_kind.into(),
                    qualified_name: qualified.clone(),
                    display_name: name.to_string(),
                    file_path: file_path.into(),
                    start_line: start.row as u32 + 1,
                    start_column: start.column as u32,
                    end_line: end.row as u32 + 1,
                    end_column: end.column as u32,
                    signature,
                    language: language.name().into(),
                    exported: looks_exported(language, node, source),
                    entry_point: is_test_path(file_path) && graph_kind == "FUNCTION",
                    logical_root: "primary".into(),
                    canonical_identity,
                    provenance: vec![provenance(
                        "STATIC_EXACT",
                        &format!("tree-sitter:{}", language.name()),
                    )],
                };
                let parent = scope
                    .as_ref()
                    .map(|(id, _)| id.clone())
                    .unwrap_or_else(|| file_id.to_string());
                contains_edges.push((parent, id.clone(), file_path.into()));
                nodes.push(graph_node);
                if matches!(
                    graph_kind,
                    "CLASS" | "INTERFACE" | "TRAIT" | "FUNCTION" | "METHOD" | "NAMESPACE"
                ) {
                    next_scope = Some((id, qualified));
                }
            }
        }
    }
    if is_call_kind(node.kind()) {
        if let Some(target_node) = call_target_node(node) {
            if let Ok(target_name) = target_node.utf8_text(source) {
                let caller = scope.as_ref().map(|(id, _)| id.as_str()).unwrap_or(file_id);
                pending_calls.push(PendingCall {
                    caller_id: caller.into(),
                    target_name: target_name.into(),
                    file_path: file_path.into(),
                    language: language.name().into(),
                    line: node.start_position().row as u32 + 1,
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_tree(
            child,
            source,
            language,
            file_path,
            file_id,
            next_scope.clone(),
            nodes,
            contains_edges,
            pending_calls,
        );
    }
}

fn declaration_kind(language: SupportedLanguage, kind: &str) -> Option<&'static str> {
    match (language, kind) {
        (_, "class_declaration" | "class_definition") => Some("CLASS"),
        (_, "interface_declaration") => Some("INTERFACE"),
        (SupportedLanguage::CSharp, "struct_declaration") => Some("STRUCT"),
        (SupportedLanguage::CSharp, "record_declaration") => Some("CLASS"),
        (SupportedLanguage::CSharp, "enum_declaration") => Some("ENUM"),
        (SupportedLanguage::CSharp, "delegate_declaration") => Some("DELEGATE"),
        (SupportedLanguage::CSharp, "property_declaration") => Some("PROPERTY"),
        (SupportedLanguage::CSharp, "constructor_declaration") => Some("CONSTRUCTOR"),
        (SupportedLanguage::Rust, "struct_item") => Some("STRUCT"),
        (SupportedLanguage::Rust, "enum_item") => Some("ENUM"),
        (SupportedLanguage::Rust, "trait_item") => Some("TRAIT"),
        (SupportedLanguage::Rust, "impl_item") => Some("IMPL"),
        (SupportedLanguage::Rust, "function_item") => Some("FUNCTION"),
        (SupportedLanguage::Rust, "mod_item") => Some("MODULE"),
        (SupportedLanguage::Rust, "type_item") => Some("TYPE"),
        (SupportedLanguage::Php, "trait_declaration") => Some("TRAIT"),
        (_, "function_declaration" | "function_definition") => Some("FUNCTION"),
        (_, "method_definition" | "method_declaration") => Some("METHOD"),
        (SupportedLanguage::TypeScript | SupportedLanguage::Tsx, "type_alias_declaration") => {
            Some("TYPE")
        }
        (_, "namespace_definition" | "namespace_declaration") => Some("NAMESPACE"),
        _ => None,
    }
}

fn declaration_name_node(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
        .or_else(|| {
            let mut cursor = node.walk();
            let found = node.children(&mut cursor).find(|child| {
                matches!(
                    child.kind(),
                    "identifier" | "name" | "type_identifier" | "namespace_name"
                )
            });
            found
        })
}

fn call_target_node(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("function")
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.child_by_field_name("callable"))
        .or_else(|| node.child_by_field_name("type"))
}

fn is_call_kind(kind: &str) -> bool {
    matches!(
        kind,
        "call"
            | "call_expression"
            | "function_call_expression"
            | "member_call_expression"
            | "invocation_expression"
            | "object_creation_expression"
    )
}

fn looks_exported(language: SupportedLanguage, node: Node<'_>, source: &[u8]) -> bool {
    let start = node.start_byte().saturating_sub(30);
    let end = node.end_byte().min(node.start_byte() + 180);
    let text = std::str::from_utf8(&source[start..end])
        .unwrap_or("")
        .to_lowercase();
    match language {
        SupportedLanguage::JavaScript | SupportedLanguage::TypeScript | SupportedLanguage::Tsx => {
            text.contains("export ")
        }
        SupportedLanguage::Php => text.contains("public "),
        SupportedLanguage::CSharp => {
            text.contains("public ") || text.contains("protected ") || text.contains("internal ")
        }
        SupportedLanguage::Rust => text.contains("pub "),
        SupportedLanguage::Python => declaration_name_node(node)
            .and_then(|name| name.utf8_text(source).ok())
            .map(|name| !name.starts_with('_'))
            .unwrap_or(false),
    }
}

fn append_file_node(
    file_path: &str,
    language: &str,
    source: &str,
    project_node: &GraphNode,
    nodes: &mut Vec<GraphNode>,
    contains_edges: &mut Vec<(String, String, String)>,
) -> String {
    let canonical_identity = canonical_node_identity(
        "primary",
        file_path,
        language,
        "FILE",
        file_path,
        1,
        0,
        source.lines().count().max(1) as u32,
        0,
    );
    let file_id = node_id(&canonical_identity);
    nodes.push(GraphNode {
        id: file_id.clone(),
        kind: "FILE".into(),
        qualified_name: file_path.into(),
        display_name: Path::new(file_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        file_path: file_path.into(),
        start_line: 1,
        start_column: 0,
        end_line: source.lines().count().max(1) as u32,
        end_column: 0,
        signature: None,
        language: language.into(),
        exported: false,
        entry_point: file_is_entry_point(file_path),
        logical_root: "primary".into(),
        canonical_identity,
        provenance: vec![provenance("STATIC_EXACT", "inventory")],
    });
    contains_edges.push((project_node.id.clone(), file_id.clone(), file_path.into()));
    file_id
}

fn append_framework_analysis(
    analysis: &FrameworkAnalysis,
    nodes: &mut Vec<GraphNode>,
    contains_edges: &mut Vec<(String, String, String)>,
    pending_routes: &mut Vec<PendingRoute>,
) {
    for route in &analysis.routes {
        let qualified = format!("{} {}", route.method, route.path);
        let canonical_identity = canonical_node_identity(
            "primary",
            &route.source_file,
            &format!("framework:{}", route.adapter),
            "ROUTE",
            &format!("{}::{qualified}", route.adapter),
            route.line,
            0,
            route.line,
            0,
        );
        let id = node_id(&canonical_identity);
        nodes.push(GraphNode {
            id: id.clone(),
            kind: "ROUTE".into(),
            qualified_name: qualified.clone(),
            display_name: qualified,
            file_path: route.source_file.clone(),
            start_line: route.line,
            start_column: 0,
            end_line: route.line,
            end_column: 0,
            signature: None,
            language: format!("framework:{}", route.adapter),
            exported: true,
            entry_point: true,
            logical_root: "primary".into(),
            canonical_identity,
            provenance: vec![provenance(
                "FRAMEWORK_RESOLVED",
                &format!("framework:{}", route.adapter),
            )],
        });
        if let Some(file_id) = file_node_id(nodes, &route.source_file) {
            contains_edges.push((file_id, id.clone(), route.source_file.clone()));
        }
        pending_routes.push(PendingRoute {
            route_id: id,
            handler_name: route.handler.clone(),
            file_path: route.source_file.clone(),
            line: route.line,
            confidence: route.confidence,
            ambiguity_reason: route.ambiguity_reason.clone(),
        });
    }

    for fact in analysis
        .entry_points
        .iter()
        .chain(analysis.container_bindings.iter())
        .chain(analysis.jobs_events_schedulers.iter())
        .chain(analysis.models_and_migrations.iter())
    {
        append_framework_fact(fact, nodes, contains_edges);
    }
}

fn append_framework_fact(
    fact: &FrameworkFact,
    nodes: &mut Vec<GraphNode>,
    contains_edges: &mut Vec<(String, String, String)>,
) {
    let graph_kind = match fact.kind.as_str() {
        "DOCKER_SERVICE" => "DOCKER_SERVICE",
        "MIDDLEWARE" | "DEPENDENCY" | "CONTAINER_BINDING" => "MIDDLEWARE",
        "MODEL_OR_MIGRATION" => "DATABASE_MODEL",
        "HOOK_OR_CRON" | "JOB_EVENT_SCHEDULE" | "SIGNAL_OR_TASK" | "BACKGROUND_TASK" => "JOB",
        _ => "MODULE",
    };
    let qualified = format!("{}::{}::{}", fact.adapter, fact.kind, fact.name);
    let canonical_identity = canonical_node_identity(
        "primary",
        &fact.source_file,
        &format!("framework:{}", fact.adapter),
        graph_kind,
        &qualified,
        fact.line,
        0,
        fact.line,
        0,
    );
    let id = node_id(&canonical_identity);
    nodes.push(GraphNode {
        id: id.clone(),
        kind: graph_kind.into(),
        qualified_name: qualified,
        display_name: fact.name.clone(),
        file_path: fact.source_file.clone(),
        start_line: fact.line,
        start_column: 0,
        end_line: fact.line,
        end_column: 0,
        signature: None,
        language: format!("framework:{}", fact.adapter),
        exported: true,
        entry_point: fact.kind == "ENTRY_POINT" || fact.kind == "DOCKER_SERVICE",
        logical_root: "primary".into(),
        canonical_identity,
        provenance: vec![provenance(
            "FRAMEWORK_RESOLVED",
            &format!("framework:{}", fact.adapter),
        )],
    });
    if let Some(file_id) = file_node_id(nodes, &fact.source_file) {
        contains_edges.push((file_id, id, fact.source_file.clone()));
    }
}

fn file_node_id(nodes: &[GraphNode], file_path: &str) -> Option<String> {
    nodes
        .iter()
        .find(|node| node.kind == "FILE" && node.file_path == file_path)
        .map(|node| node.id.clone())
}

fn is_framework_candidate(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        "artisan",
        "composer.json",
        "package.json",
        "manage.py",
        "wp-config.php",
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
        "next.config.js",
        "next.config.mjs",
        "next.config.ts",
    ]
    .iter()
    .any(|candidate| lower == *candidate || lower.ends_with(&format!("/{candidate}")))
        || lower.starts_with("routes/")
        || lower.starts_with("wp-content/plugins/")
        || lower.starts_with("wp-content/themes/")
}

fn language_for_path(path: &str) -> Option<SupportedLanguage> {
    match Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "php" => Some(SupportedLanguage::Php),
        "js" | "jsx" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
        "ts" | "mts" | "cts" => Some(SupportedLanguage::TypeScript),
        "tsx" => Some(SupportedLanguage::Tsx),
        "py" | "pyi" => Some(SupportedLanguage::Python),
        "cs" => Some(SupportedLanguage::CSharp),
        "rs" => Some(SupportedLanguage::Rust),
        _ => None,
    }
}

fn is_probable_source_extension(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "rb" | "go"
            | "java"
            | "kt"
            | "kts"
            | "cs"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "rs"
            | "swift"
            | "scala"
            | "dart"
            | "lua"
    )
}

fn file_is_entry_point(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with("main.py")
        || lower.ends_with("index.php")
        || lower.ends_with("server.js")
        || lower.ends_with("server.ts")
        || lower.ends_with("program.cs")
        || lower.contains("/routes/")
        || lower.starts_with("routes/")
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("/test")
        || lower.starts_with("test")
        || lower.contains("__tests__")
        || lower.ends_with("_test.py")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".spec.ts")
}

fn normalize_target_name(name: &str) -> String {
    name.rsplit(['.', ':', '\\'])
        .next()
        .unwrap_or(name)
        .trim_matches(|value: char| !value.is_alphanumeric() && value != '_')
        .to_lowercase()
}

/// Versioned, deterministic identity for graph nodes.  The length-delimited components make
/// paths, roots, namespaces and source ranges unambiguous; the full SHA-256 digest keeps the
/// persisted primary key bounded without dropping the contextual dimensions that caused v1.2.0
/// collisions.
#[allow(clippy::too_many_arguments)] // Identity fields are deliberately explicit at every producer.
fn canonical_node_identity(
    logical_root: &str,
    file_path: &str,
    language: &str,
    kind: &str,
    qualified_name: &str,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
) -> String {
    fn part(label: &str, value: &str) -> String {
        format!("{label}={}:{}", value.len(), value)
    }

    [
        "codeintel-node-v2".into(),
        part("root", logical_root),
        part("path", file_path),
        part("language", language),
        part("kind", kind),
        part("qualified", qualified_name),
        format!("range={start_line}:{start_column}-{end_line}:{end_column}"),
    ]
    .join("|")
}

fn node_id(canonical_identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_identity.as_bytes());
    format!("node:v2:{}", hex_string(&hasher.finalize()))
}

fn provenance(origin: &str, producer: &str) -> NodeProvenance {
    NodeProvenance {
        origin: origin.into(),
        producer: producer.into(),
    }
}

fn reconcile_nodes(nodes: Vec<GraphNode>) -> Result<Vec<GraphNode>> {
    let mut unique: Vec<GraphNode> = Vec::with_capacity(nodes.len());
    let mut index_by_id = HashMap::<String, usize>::new();
    for candidate in nodes {
        if let Some(index) = index_by_id.get(&candidate.id).copied() {
            let existing = &mut unique[index];
            if existing.canonical_identity != candidate.canonical_identity {
                return Err(anyhow!(
                    "NODE_ID_COLLISION before graph publication: node_id={} existing_candidate={existing:?} incoming_candidate={candidate:?}",
                    candidate.id,
                ));
            }
            for producer in candidate.provenance {
                if !existing.provenance.contains(&producer) {
                    existing.provenance.push(producer);
                }
            }
            continue;
        }
        index_by_id.insert(candidate.id.clone(), unique.len());
        unique.push(candidate);
    }
    Ok(unique)
}

fn hex_string(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0xf) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeintel_core::{compute_source_revision, Policy};
    use codeintel_graph::GraphStore;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn indexes_python_without_git() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("app.py"),
            "def helper():\n    return 1\n\ndef main():\n    return helper()\n",
        )
        .unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        let result = index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();
        assert_eq!(result.state, "CURRENT");
        let store = GraphStore::open(&graph).unwrap();
        let found = store.find_symbol("helper", 10).unwrap();
        assert_eq!(found.items.len(), 1);
        let evidence = store.edge_evidence(&found.items[0].id, 10).unwrap();
        assert!(evidence
            .items
            .iter()
            .any(|edge| edge.kind == "CALLS" && edge.origin == "SEMANTIC_RESOLVED"));
    }

    #[test]
    fn indexes_csharp_declarations_and_calls() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Program.cs"),
            "namespace Demo { public class Worker { public void Run() {} } public class Program { public static void Main() { var worker = new Worker(); worker.Run(); } } }",
        )
        .unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        let result = index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();

        assert!(result
            .coverage
            .supported_languages
            .contains(&"csharp".into()));
        assert!(!result.coverage.unsupported_languages.contains(&"cs".into()));
        let store = GraphStore::open(&graph).unwrap();
        let worker = store.find_symbol("Worker", 10).unwrap();
        assert!(worker
            .items
            .iter()
            .any(|node| node.kind == "CLASS" && node.display_name == "Worker"));
        let run = store.find_symbol("Run", 10).unwrap();
        let run_method = run
            .items
            .iter()
            .find(|node| node.kind == "METHOD" && node.display_name == "Run")
            .unwrap();
        let evidence = store.edge_evidence(&run_method.id, 20).unwrap();
        assert!(evidence
            .items
            .iter()
            .any(|edge| edge.kind == "CALLS" && edge.origin == "SEMANTIC_RESOLVED"));
    }

    #[test]
    fn indexes_rust_declarations_and_same_file_calls() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("main.rs"),
            "pub fn reserve() -> u32 { 0 }\nfn charge() -> u32 { reserve() }\nfn main() { let _ = charge(); }\n",
        )
        .unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        let result = index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();
        assert!(result.coverage.supported_languages.contains(&"rust".into()));
        assert!(!result.coverage.unsupported_languages.contains(&"rs".into()));
        let store = GraphStore::open(&graph).unwrap();
        let reserve = store.find_symbol("reserve", 10).unwrap();
        assert!(reserve
            .items
            .iter()
            .any(|node| node.kind == "FUNCTION" && node.display_name == "reserve"));
        let evidence = store.edge_evidence(&reserve.items[0].id, 20).unwrap();
        assert!(evidence
            .items
            .iter()
            .any(|edge| edge.kind == "CALLS" && edge.origin == "SEMANTIC_RESOLVED"));
    }

    #[test]
    fn indexes_fastapi_route_once() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("app.py"),
            "@app.get(\"/health\")\ndef health():\n    return {\"ok\": True}\n",
        )
        .unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        let result = index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();
        assert_eq!(result.state, "CURRENT");
        let store = GraphStore::open(&graph).unwrap();
        let routes = store.find_symbol("GET /health", 10).unwrap();
        assert_eq!(routes.items.len(), 1);
        let candidates = store.dead_code_candidates(20).unwrap();
        assert!(!candidates
            .iter()
            .any(|candidate| candidate["symbol"].as_str() == Some("app.py::health")));
    }

    #[test]
    fn indexes_django_route_with_correct_group_semantics() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("site")).unwrap();
        fs::write(
            dir.path().join("manage.py"),
            "from django.core.management import execute_from_command_line\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("site/urls.py"),
            "from django.urls import path\nfrom . import views\nurlpatterns = [path(\"health/\", views.health)]\n",
        )
        .unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();
        let store = GraphStore::open(&graph).unwrap();
        let route = store.find_symbol("ANY /health/", 10).unwrap();
        assert_eq!(route.items.len(), 1);
        assert!(store
            .coverage()
            .unwrap()
            .framework_adapters
            .contains(&"django".into()));
    }

    #[test]
    fn indexes_docker_compose_service_without_treating_yaml_as_deep_language() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("compose.yaml"),
            "services:\n  api:\n    image: example/api\n",
        )
        .unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();
        let store = GraphStore::open(&graph).unwrap();
        let service = store.find_symbol("api", 10).unwrap();
        assert!(service
            .items
            .iter()
            .any(|node| node.kind == "DOCKER_SERVICE"));
        let coverage = store.coverage().unwrap();
        assert_eq!(coverage.semantic, "NOT_APPLICABLE");
        assert!(coverage
            .framework_adapters
            .contains(&"docker_compose".into()));
    }

    #[test]
    fn regression_php_namespaces_with_same_function_name_get_distinct_nodes() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("handlers.php"),
            "<?php\nnamespace DrillCenter\\PublicApi;\nfunction handle() { return 1; }\nnamespace DrillCenter\\Admin;\nfunction handle() { return 2; }\n",
        )
        .unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();

        let store = GraphStore::open(&graph).unwrap();
        let functions = store
            .graph_query(Some("FUNCTION"), Some("handlers.php"), 20)
            .unwrap()
            .items;
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].display_name, "handle");
        assert_eq!(functions[1].display_name, "handle");
        assert_ne!(functions[0].id, functions[1].id);
        assert_ne!(
            functions[0].canonical_identity,
            functions[1].canonical_identity
        );
    }

    #[test]
    fn collision_matrix_distinguishes_files_classes_roots_kinds_and_ranges() {
        let a = canonical_node_identity(
            "primary", "a.php", "php", "FUNCTION", "helper", 10, 0, 10, 20,
        );
        let other_file = canonical_node_identity(
            "primary", "b.php", "php", "FUNCTION", "helper", 10, 0, 10, 20,
        );
        let other_class = canonical_node_identity(
            "primary",
            "a.php",
            "php",
            "METHOD",
            "OtherClass::helper",
            10,
            0,
            10,
            20,
        );
        let other_root = canonical_node_identity(
            "related:shared",
            "a.php",
            "php",
            "FUNCTION",
            "helper",
            10,
            0,
            10,
            20,
        );
        let other_kind_same_range =
            canonical_node_identity("primary", "a.php", "php", "TYPE", "helper", 10, 0, 10, 20);
        let other_range = canonical_node_identity(
            "primary", "a.php", "php", "FUNCTION", "helper", 11, 0, 11, 20,
        );
        let ids = [
            node_id(&a),
            node_id(&other_file),
            node_id(&other_class),
            node_id(&other_root),
            node_id(&other_kind_same_range),
            node_id(&other_range),
        ];
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            ids.len()
        );
    }

    #[test]
    fn same_logical_node_is_explicitly_merged_with_all_provenance() {
        let identity = canonical_node_identity(
            "primary",
            "app.py",
            "python",
            "ROUTE",
            "GET /health",
            1,
            0,
            1,
            0,
        );
        let mut parser = GraphNode {
            id: node_id(&identity),
            kind: "ROUTE".into(),
            qualified_name: "GET /health".into(),
            display_name: "GET /health".into(),
            file_path: "app.py".into(),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
            signature: None,
            language: "python".into(),
            exported: true,
            entry_point: true,
            logical_root: "primary".into(),
            canonical_identity: identity,
            provenance: vec![provenance("STATIC_EXACT", "tree-sitter:python")],
        };
        let mut framework = parser.clone();
        framework.provenance = vec![provenance("FRAMEWORK_RESOLVED", "framework:fastapi")];
        let merged = reconcile_nodes(vec![parser.clone(), framework]).unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].provenance.len(), 2);
        parser
            .provenance
            .push(provenance("FRAMEWORK_RESOLVED", "framework:fastapi"));
        assert_eq!(merged[0].provenance, parser.provenance);
    }

    #[test]
    fn deterministic_rebuild_and_retry_keep_same_node_ids() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("lib")).unwrap();
        fs::write(dir.path().join("lib/a.py"), "def helper():\n    return 1\n").unwrap();
        fs::write(dir.path().join("lib/b.py"), "def helper():\n    return 2\n").unwrap();
        let revision = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        let graph = dir.path().join("data/graph.sqlite3");
        for _ in 0..2 {
            index_project(
                dir.path(),
                &revision,
                &Policy::default(),
                &graph,
                &dir.path().join("cache"),
            )
            .unwrap();
        }
        let first = GraphStore::open(&graph)
            .unwrap()
            .graph_query(Some("FUNCTION"), None, 20)
            .unwrap()
            .items
            .into_iter()
            .map(|node| node.id)
            .collect::<Vec<_>>();
        index_project(
            dir.path(),
            &revision,
            &Policy::default(),
            &graph,
            &dir.path().join("cache"),
        )
        .unwrap();
        let second = GraphStore::open(&graph)
            .unwrap()
            .graph_query(Some("FUNCTION"), None, 20)
            .unwrap()
            .items
            .into_iter()
            .map(|node| node.id)
            .collect::<Vec<_>>();
        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
    }
}
