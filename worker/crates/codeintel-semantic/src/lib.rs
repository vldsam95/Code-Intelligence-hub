use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

const SUPPORTED_LANGUAGES: [&str; 6] = [
    "php",
    "javascript",
    "typescript",
    "python",
    "csharp",
    "rust",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceUnit {
    pub path: String,
    pub language: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticSymbol {
    pub id: String,
    pub qualified_name: String,
    pub display_name: String,
    pub file_path: String,
    pub kind: String,
    pub language: String,
    pub start_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallSite {
    pub caller_id: String,
    pub target_expression: String,
    pub file_path: String,
    pub language: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocation {
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub symbol_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedTarget {
    pub symbol_id: String,
    pub confidence: f64,
    pub ambiguity_reason: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallResolution {
    pub targets: Vec<ResolvedTarget>,
    pub status: String,
    pub dynamic_boundary: bool,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceResult {
    pub file_path: String,
    pub line: u32,
    pub caller_id: String,
    pub target_expression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeResolution {
    pub status: String,
    pub type_name: Option<String>,
    pub confidence_millis: u16,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticCoverage {
    pub status: String,
    pub supported_languages: Vec<String>,
    pub analyzed_files: usize,
    pub failed_files: Vec<String>,
    pub resolved_calls: usize,
    pub ambiguous_calls: usize,
    pub unresolved_calls: usize,
    pub external_or_unindexed_calls: usize,
    pub unresolved_files: Vec<String>,
    pub external_or_unindexed_files: Vec<String>,
    pub ambiguous_files: Vec<String>,
    pub dynamic_boundaries: Vec<String>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolverDetection {
    pub available: bool,
    pub implementation: String,
    pub supported_languages: Vec<String>,
    pub external_process_required: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SemanticProject {
    pub units: Vec<SourceUnit>,
    pub symbols: Vec<SemanticSymbol>,
    pub calls: Vec<CallSite>,
    pub failed_files: Vec<String>,
}

/// Stable semantic contract for embedded or lazy external implementations.
/// Implementations must preserve ambiguity and must never invent one exact target.
pub trait SemanticResolver {
    fn detect(&self) -> ResolverDetection;
    fn resolve_definition(&self, location: &SourceLocation) -> CallResolution;
    fn find_references(&self, symbol_id: &str) -> Vec<ReferenceResult>;
    fn resolve_type(&self, location: &SourceLocation) -> TypeResolution;
    fn resolve_call_target(&self, call_site: &CallSite) -> CallResolution;
    fn coverage(&self) -> SemanticCoverage;
    fn shutdown(&mut self) -> Result<()>;
}

#[derive(Debug, Clone)]
struct ImportBinding {
    alias: String,
    imported_name: Option<String>,
    module: String,
}

/// Low-infrastructure V1 resolver. It combines AST-produced declarations/call sites with
/// language import/namespace information and local type annotations. Dynamic boundaries are
/// reported explicitly. An external LSP can later implement the same trait and remains lazy.
pub struct EmbeddedHybridResolver {
    project: SemanticProject,
    symbols_by_name: HashMap<String, Vec<SemanticSymbol>>,
    symbols_by_id: HashMap<String, SemanticSymbol>,
    imports_by_file: HashMap<String, Vec<ImportBinding>>,
    resolutions: BTreeMap<(String, u32, String), CallResolution>,
    coverage: SemanticCoverage,
}

impl EmbeddedHybridResolver {
    pub fn analyze(project: SemanticProject) -> Result<Self> {
        let mut symbols_by_name: HashMap<String, Vec<SemanticSymbol>> = HashMap::new();
        let mut symbols_by_id = HashMap::new();
        for symbol in &project.symbols {
            symbols_by_name
                .entry(normalize_identifier(&symbol.display_name))
                .or_default()
                .push(symbol.clone());
            symbols_by_id.insert(symbol.id.clone(), symbol.clone());
        }

        let mut imports_by_file = HashMap::new();
        let mut supported_languages = BTreeSet::new();
        for unit in &project.units {
            if SUPPORTED_LANGUAGES.contains(&unit.language.as_str()) {
                supported_languages.insert(unit.language.clone());
            }
            imports_by_file.insert(unit.path.clone(), extract_imports(unit)?);
        }

        let mut resolver = Self {
            project,
            symbols_by_name,
            symbols_by_id,
            imports_by_file,
            resolutions: BTreeMap::new(),
            coverage: SemanticCoverage {
                status: String::new(),
                supported_languages: supported_languages.into_iter().collect(),
                ..SemanticCoverage::default()
            },
        };
        resolver.coverage.analyzed_files = resolver.project.units.len();
        resolver.coverage.failed_files = resolver.project.failed_files.clone();
        resolver.compute_resolutions();
        Ok(resolver)
    }

    fn compute_resolutions(&mut self) {
        let mut unresolved_files = BTreeSet::new();
        let mut external_or_unindexed_files = BTreeSet::new();
        let mut ambiguous_files = BTreeSet::new();
        let mut dynamic_boundaries = BTreeSet::new();
        for call in &self.project.calls {
            let resolution = self.resolve_call(call);
            match resolution.status.as_str() {
                "RESOLVED" => self.coverage.resolved_calls += 1,
                "AMBIGUOUS" => {
                    self.coverage.ambiguous_calls += 1;
                    ambiguous_files.insert(call.file_path.clone());
                }
                "EXTERNAL_OR_UNINDEXED" => {
                    self.coverage.external_or_unindexed_calls += 1;
                    external_or_unindexed_files.insert(call.file_path.clone());
                }
                _ => {
                    self.coverage.unresolved_calls += 1;
                    unresolved_files.insert(call.file_path.clone());
                }
            }
            if resolution.dynamic_boundary {
                dynamic_boundaries.insert(format!(
                    "{}:{}:{}",
                    call.file_path, call.line, call.target_expression
                ));
            }
            self.resolutions.insert(call_key(call), resolution);
        }
        self.coverage.unresolved_files = unresolved_files.into_iter().collect();
        self.coverage.external_or_unindexed_files =
            external_or_unindexed_files.into_iter().collect();
        self.coverage.ambiguous_files = ambiguous_files.into_iter().collect();
        self.coverage.dynamic_boundaries = dynamic_boundaries.into_iter().collect();
        self.coverage.status = if self.coverage.analyzed_files == 0 {
            "NOT_APPLICABLE".into()
        } else if self.coverage.failed_files.is_empty() {
            "COMPLETE".into()
        } else {
            "PARTIAL".into()
        };
        if !self.coverage.unresolved_files.is_empty() {
            self.coverage.unknowns.push(
                "Some call targets remain unresolved; absence claims in those files are not proven."
                    .into(),
            );
        }
        if !self.coverage.ambiguous_files.is_empty() {
            self.coverage
                .unknowns
                .push("Ambiguous targets were preserved instead of selecting one target.".into());
        }
        if !self.coverage.external_or_unindexed_files.is_empty() {
            self.coverage.unknowns.push(
                "Calls without a local declaration were preserved as external-or-unindexed; they cannot support absence claims."
                    .into(),
            );
        }
    }

    fn resolve_call(&self, call: &CallSite) -> CallResolution {
        let expression = call.target_expression.trim();
        if is_dynamic_expression(expression, &call.language) {
            return CallResolution {
                targets: vec![],
                status: "UNKNOWN".into(),
                dynamic_boundary: true,
                unknowns: vec![
                    "Dynamic dispatch requires runtime or external semantic evidence.".into(),
                ],
            };
        }
        let target_name = terminal_identifier(expression);
        if target_name.is_empty() {
            return unresolved("Call target has no stable identifier.", false);
        }
        let candidates = self
            .symbols_by_name
            .get(&normalize_identifier(target_name))
            .cloned()
            .unwrap_or_default();
        if candidates.is_empty() {
            return CallResolution {
                targets: vec![],
                status: "EXTERNAL_OR_UNINDEXED".into(),
                dynamic_boundary: false,
                unknowns: vec![
                    "No local declaration exists; the target may be an external dependency or outside indexed scope."
                        .into(),
                ],
            };
        }

        let same_file: Vec<_> = candidates
            .iter()
            .filter(|symbol| symbol.file_path == call.file_path)
            .cloned()
            .collect();
        if !same_file.is_empty() {
            return targets_to_resolution(same_file, 0.97, "same-file declaration");
        }

        let imports = self
            .imports_by_file
            .get(&call.file_path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let root_name = root_identifier(expression);
        let matching_imports: Vec<_> = imports
            .iter()
            .filter(|binding| {
                normalize_identifier(&binding.alias) == normalize_identifier(target_name)
                    || normalize_identifier(&binding.alias) == normalize_identifier(root_name)
                    || (call.language == "csharp" && binding.alias == "*")
            })
            .collect();
        if !matching_imports.is_empty() {
            let imported: Vec<_> = candidates
                .iter()
                .filter(|symbol| {
                    matching_imports.iter().any(|binding| {
                        import_matches_symbol(binding, symbol, expression, &call.file_path)
                    })
                })
                .cloned()
                .collect();
            if !imported.is_empty() {
                return targets_to_resolution(imported, 0.94, "resolved import/namespace binding");
            }
        }

        unresolved(
            "No unique declaration was proven by local scope or an import binding.",
            false,
        )
    }

    fn unit(&self, path: &str) -> Option<&SourceUnit> {
        self.project.units.iter().find(|unit| unit.path == path)
    }
}

impl SemanticResolver for EmbeddedHybridResolver {
    fn detect(&self) -> ResolverDetection {
        ResolverDetection {
            available: self.coverage.analyzed_files > 0,
            implementation: "embedded_hybrid_v1".into(),
            supported_languages: SUPPORTED_LANGUAGES
                .iter()
                .map(|value| (*value).into())
                .collect(),
            external_process_required: false,
        }
    }

    fn resolve_definition(&self, location: &SourceLocation) -> CallResolution {
        let Some(hint) = location.symbol_hint.as_deref() else {
            return unresolved("Definition lookup requires a symbol hint.", false);
        };
        if let Some(symbol) = self.symbols_by_id.get(hint) {
            return targets_to_resolution(
                vec![symbol.clone()],
                1.0,
                "exact semantic symbol identifier",
            );
        }
        self.resolve_call_target(&CallSite {
            caller_id: format!("location:{}:{}", location.file_path, location.line),
            target_expression: hint.into(),
            file_path: location.file_path.clone(),
            language: self
                .unit(&location.file_path)
                .map(|unit| unit.language.clone())
                .unwrap_or_default(),
            line: location.line,
        })
    }

    fn find_references(&self, symbol_id: &str) -> Vec<ReferenceResult> {
        self.project
            .calls
            .iter()
            .filter(|call| {
                self.resolutions
                    .get(&call_key(call))
                    .is_some_and(|resolution| {
                        resolution
                            .targets
                            .iter()
                            .any(|target| target.symbol_id == symbol_id)
                    })
            })
            .map(|call| ReferenceResult {
                file_path: call.file_path.clone(),
                line: call.line,
                caller_id: call.caller_id.clone(),
                target_expression: call.target_expression.clone(),
            })
            .collect()
    }

    fn resolve_type(&self, location: &SourceLocation) -> TypeResolution {
        let Some(hint) = location.symbol_hint.as_deref() else {
            return unknown_type("Type lookup requires a symbol hint.");
        };
        let Some(unit) = self.unit(&location.file_path) else {
            return unknown_type("Source file is outside the analyzed semantic scope.");
        };
        let escaped = regex::escape(hint);
        let patterns = match unit.language.as_str() {
            "python" => vec![format!(
                r"(?m)\b{escaped}\s*:\s*([A-Za-z_][A-Za-z0-9_\.\[\], ]*)"
            )],
            "typescript" | "javascript" => vec![format!(
                r"(?m)\b{escaped}\s*:\s*([A-Za-z_][A-Za-z0-9_<>,\[\]\.| ]*)"
            )],
            "php" => vec![format!(
                r"(?m)([A-Za-z_\\][A-Za-z0-9_\\|?]*)\s+\${escaped}\b"
            )],
            "csharp" => vec![format!(
                r"(?m)\b([A-Za-z_][A-Za-z0-9_\.<>\[\],?]*)\s+{escaped}\b"
            )],
            "rust" => vec![format!(
                r"(?m)\b{escaped}\s*:\s*([A-Za-z_][A-Za-z0-9_:<>\[\], ]*)"
            )],
            _ => vec![],
        };
        for pattern in patterns {
            if let Ok(regex) = Regex::new(&pattern) {
                if let Some(captures) = regex.captures(&unit.content) {
                    if let Some(value) = captures.get(1) {
                        return TypeResolution {
                            status: "RESOLVED".into(),
                            type_name: Some(value.as_str().trim().into()),
                            confidence_millis: 900,
                            reason: "explicit local type annotation".into(),
                        };
                    }
                }
            }
        }
        unknown_type("No explicit supported type annotation was found.")
    }

    fn resolve_call_target(&self, call_site: &CallSite) -> CallResolution {
        self.resolutions
            .get(&call_key(call_site))
            .cloned()
            .unwrap_or_else(|| self.resolve_call(call_site))
    }

    fn coverage(&self) -> SemanticCoverage {
        self.coverage.clone()
    }

    fn shutdown(&mut self) -> Result<()> {
        self.resolutions.clear();
        Ok(())
    }
}

fn call_key(call: &CallSite) -> (String, u32, String) {
    (
        call.file_path.clone(),
        call.line,
        call.target_expression.clone(),
    )
}

fn targets_to_resolution(
    mut symbols: Vec<SemanticSymbol>,
    confidence: f64,
    reason: &str,
) -> CallResolution {
    symbols.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    symbols.dedup_by(|a, b| a.id == b.id);
    let ambiguity = (symbols.len() > 1)
        .then(|| format!("{} declarations match the semantic evidence", symbols.len()));
    CallResolution {
        status: if ambiguity.is_some() {
            "AMBIGUOUS".into()
        } else {
            "RESOLVED".into()
        },
        targets: symbols
            .into_iter()
            .map(|symbol| ResolvedTarget {
                symbol_id: symbol.id,
                confidence: if ambiguity.is_some() {
                    confidence.min(0.69)
                } else {
                    confidence
                },
                ambiguity_reason: ambiguity.clone(),
                reason: reason.into(),
            })
            .collect(),
        dynamic_boundary: false,
        unknowns: ambiguity.into_iter().collect(),
    }
}

fn unresolved(reason: &str, dynamic_boundary: bool) -> CallResolution {
    CallResolution {
        targets: vec![],
        status: "UNKNOWN".into(),
        dynamic_boundary,
        unknowns: vec![reason.into()],
    }
}

fn unknown_type(reason: &str) -> TypeResolution {
    TypeResolution {
        status: "UNKNOWN".into(),
        type_name: None,
        confidence_millis: 0,
        reason: reason.into(),
    }
}

fn normalize_identifier(value: &str) -> String {
    value
        .trim_matches(|character: char| !character.is_alphanumeric() && character != '_')
        .to_lowercase()
}

fn terminal_identifier(expression: &str) -> &str {
    expression
        .rsplit(['.', ':', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(expression)
        .trim_matches(|character: char| !character.is_alphanumeric() && character != '_')
}

fn root_identifier(expression: &str) -> &str {
    expression
        .split(['.', ':', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(expression)
        .trim_matches(|character: char| !character.is_alphanumeric() && character != '_')
}

fn is_dynamic_expression(expression: &str, language: &str) -> bool {
    let lower = expression.to_lowercase();
    if language == "csharp" {
        return lower.contains("reflection")
            || lower.contains("getmethod")
            || lower.contains("getproperty")
            || lower.contains("getfield")
            || lower.contains("dynamicinvoke");
    }
    lower.contains("getattr")
        || lower.contains("call_user_func")
        || lower.contains("reflection")
        || lower.contains('[')
        || lower.contains("${")
        || expression.trim_start().starts_with('$')
}

fn extract_imports(unit: &SourceUnit) -> Result<Vec<ImportBinding>> {
    let mut bindings = vec![];
    match unit.language.as_str() {
        "python" => extract_python_imports(&unit.content, &mut bindings)?,
        "javascript" | "typescript" => extract_js_imports(&unit.content, &mut bindings)?,
        "php" => extract_php_imports(&unit.content, &mut bindings)?,
        "csharp" => extract_csharp_imports(&unit.content, &mut bindings)?,
        _ => {}
    }
    Ok(bindings)
}

fn extract_python_imports(source: &str, bindings: &mut Vec<ImportBinding>) -> Result<()> {
    let from = Regex::new(r"^\s*from\s+([A-Za-z0-9_\.]+)\s+import\s+(.+?)\s*$")?;
    let direct =
        Regex::new(r"^\s*import\s+([A-Za-z0-9_\.]+)(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?\s*$")?;
    for line in source.lines() {
        if let Some(captures) = from.captures(line) {
            let module = captures[1].to_string();
            for item in captures[2].split(',') {
                let mut parts = item.split_whitespace();
                let Some(imported) = parts.next() else {
                    continue;
                };
                let alias = if parts.next() == Some("as") {
                    parts.next().unwrap_or(imported)
                } else {
                    imported
                };
                bindings.push(ImportBinding {
                    alias: alias.into(),
                    imported_name: Some(imported.into()),
                    module: module.clone(),
                });
            }
        } else if let Some(captures) = direct.captures(line) {
            let module = captures[1].to_string();
            let alias = captures
                .get(2)
                .map(|value| value.as_str())
                .unwrap_or_else(|| module.rsplit('.').next().unwrap_or(&module));
            bindings.push(ImportBinding {
                alias: alias.into(),
                imported_name: None,
                module,
            });
        }
    }
    Ok(())
}

fn extract_js_imports(source: &str, bindings: &mut Vec<ImportBinding>) -> Result<()> {
    let named = Regex::new(r#"^\s*import\s*\{([^}]+)\}\s*from\s*[\"']([^\"']+)[\"']"#)?;
    let default =
        Regex::new(r#"^\s*import\s+([A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+[\"']([^\"']+)[\"']"#)?;
    let require = Regex::new(
        r#"^\s*(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*require\s*\(\s*[\"']([^\"']+)[\"']"#,
    )?;
    for line in source.lines() {
        if let Some(captures) = named.captures(line) {
            for item in captures[1].split(',') {
                let mut parts = item.split_whitespace();
                let Some(imported) = parts.next() else {
                    continue;
                };
                let alias = if parts.next() == Some("as") {
                    parts.next().unwrap_or(imported)
                } else {
                    imported
                };
                bindings.push(ImportBinding {
                    alias: alias.into(),
                    imported_name: Some(imported.into()),
                    module: captures[2].into(),
                });
            }
        } else if let Some(captures) = default.captures(line) {
            bindings.push(ImportBinding {
                alias: captures[1].into(),
                imported_name: None,
                module: captures[2].into(),
            });
        } else if let Some(captures) = require.captures(line) {
            bindings.push(ImportBinding {
                alias: captures[1].into(),
                imported_name: None,
                module: captures[2].into(),
            });
        }
    }
    Ok(())
}

fn extract_php_imports(source: &str, bindings: &mut Vec<ImportBinding>) -> Result<()> {
    let uses = Regex::new(
        r"^\s*use\s+([A-Za-z_\\][A-Za-z0-9_\\]*)(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?\s*;",
    )?;
    for line in source.lines() {
        if let Some(captures) = uses.captures(line) {
            let module = captures[1].to_string();
            let alias = captures
                .get(2)
                .map(|value| value.as_str())
                .unwrap_or_else(|| module.rsplit('\\').next().unwrap_or(&module));
            bindings.push(ImportBinding {
                alias: alias.into(),
                imported_name: None,
                module,
            });
        }
    }
    Ok(())
}

fn extract_csharp_imports(source: &str, bindings: &mut Vec<ImportBinding>) -> Result<()> {
    let alias = Regex::new(
        r"^\s*(?:global\s+)?using\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([A-Za-z_][A-Za-z0-9_\.]*)\s*;",
    )?;
    let namespace_import =
        Regex::new(r"^\s*(?:global\s+)?using\s+(?:static\s+)?([A-Za-z_][A-Za-z0-9_\.]*)\s*;")?;
    let namespace = Regex::new(r"^\s*namespace\s+([A-Za-z_][A-Za-z0-9_\.]*)\s*(?:;|\{)")?;
    for line in source.lines() {
        if let Some(captures) = alias.captures(line) {
            bindings.push(ImportBinding {
                alias: captures[1].into(),
                imported_name: None,
                module: captures[2].into(),
            });
        } else if let Some(captures) = namespace_import.captures(line) {
            bindings.push(ImportBinding {
                alias: "*".into(),
                imported_name: None,
                module: captures[1].into(),
            });
        } else if let Some(captures) = namespace.captures(line) {
            bindings.push(ImportBinding {
                alias: "*".into(),
                imported_name: None,
                module: captures[1].into(),
            });
        }
    }
    Ok(())
}

fn import_matches_symbol(
    binding: &ImportBinding,
    symbol: &SemanticSymbol,
    expression: &str,
    caller_file: &str,
) -> bool {
    if let Some(imported) = binding.imported_name.as_deref() {
        if normalize_identifier(imported) != normalize_identifier(&symbol.display_name) {
            return false;
        }
    } else if expression.contains(['.', ':']) {
        let terminal = terminal_identifier(expression);
        if normalize_identifier(terminal) != normalize_identifier(&symbol.display_name) {
            return false;
        }
    }
    module_matches_file(&binding.module, &symbol.file_path, caller_file)
        || symbol
            .qualified_name
            .to_lowercase()
            .contains(&binding.module.replace(['.', '\\'], "::").to_lowercase())
}

fn module_matches_file(module: &str, candidate_file: &str, caller_file: &str) -> bool {
    let module = module.trim_matches(['\'', '"']);
    let candidate = without_source_extension(candidate_file).replace('\\', "/");
    if module.starts_with('.') {
        let parent = Path::new(caller_file)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let joined = lexical_normalize(&parent.join(module));
        return candidate == joined || candidate == format!("{joined}/index");
    }
    let normalized_module = module
        .replace(['.', '\\'], "/")
        .trim_matches('/')
        .to_string();
    candidate == normalized_module
        || candidate.ends_with(&format!("/{normalized_module}"))
        || candidate.ends_with(&format!("/{normalized_module}/index"))
}

fn lexical_normalize(path: &Path) -> String {
    let mut parts: Vec<String> = vec![];
    for component in path.components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    without_source_extension(&PathBuf::from_iter(parts).to_string_lossy()).replace('\\', "/")
}

fn without_source_extension(path: &str) -> String {
    for extension in [
        ".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".py", ".pyi", ".php", ".cs",
    ] {
        if let Some(value) = path.strip_suffix(extension) {
            return value.into();
        }
    }
    path.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(id: &str, name: &str, path: &str) -> SemanticSymbol {
        SemanticSymbol {
            id: id.into(),
            qualified_name: format!("{}::{name}", path.replace('/', "::")),
            display_name: name.into(),
            file_path: path.into(),
            kind: "FUNCTION".into(),
            language: "python".into(),
            start_line: 1,
        }
    }

    #[test]
    fn resolves_same_file_definition_without_name_only_guessing() {
        let call = CallSite {
            caller_id: "main".into(),
            target_expression: "helper".into(),
            file_path: "app.py".into(),
            language: "python".into(),
            line: 4,
        };
        let resolver = EmbeddedHybridResolver::analyze(SemanticProject {
            units: vec![SourceUnit {
                path: "app.py".into(),
                language: "python".into(),
                content: "def helper(): pass\ndef main(): return helper()".into(),
            }],
            symbols: vec![symbol("helper", "helper", "app.py")],
            calls: vec![call.clone()],
            failed_files: vec![],
        })
        .unwrap();
        let result = resolver.resolve_call_target(&call);
        assert_eq!(result.status, "RESOLVED");
        assert_eq!(result.targets[0].symbol_id, "helper");
        assert!(result.targets[0].ambiguity_reason.is_none());
    }

    #[test]
    fn resolves_python_import_and_preserves_ambiguity() {
        let call = CallSite {
            caller_id: "main".into(),
            target_expression: "helper".into(),
            file_path: "app.py".into(),
            language: "python".into(),
            line: 3,
        };
        let resolver = EmbeddedHybridResolver::analyze(SemanticProject {
            units: vec![
                SourceUnit {
                    path: "app.py".into(),
                    language: "python".into(),
                    content: "from services import helper\nhelper()".into(),
                },
                SourceUnit {
                    path: "services.py".into(),
                    language: "python".into(),
                    content: "def helper(): pass".into(),
                },
            ],
            symbols: vec![
                symbol("one", "helper", "services.py"),
                symbol("two", "helper", "pkg/services.py"),
            ],
            calls: vec![call.clone()],
            failed_files: vec![],
        })
        .unwrap();
        let result = resolver.resolve_call_target(&call);
        assert_eq!(result.status, "AMBIGUOUS");
        assert_eq!(result.targets.len(), 2);
        assert!(result
            .targets
            .iter()
            .all(|target| target.ambiguity_reason.is_some()));
    }

    #[test]
    fn dynamic_dispatch_remains_unknown() {
        let call = CallSite {
            caller_id: "main".into(),
            target_expression: "getattr(service, action)".into(),
            file_path: "app.py".into(),
            language: "python".into(),
            line: 2,
        };
        let resolver = EmbeddedHybridResolver::analyze(SemanticProject {
            units: vec![SourceUnit {
                path: "app.py".into(),
                language: "python".into(),
                content: "getattr(service, action)()".into(),
            }],
            symbols: vec![],
            calls: vec![call.clone()],
            failed_files: vec![],
        })
        .unwrap();
        let result = resolver.resolve_call_target(&call);
        assert_eq!(result.status, "UNKNOWN");
        assert!(result.dynamic_boundary);
        assert_eq!(resolver.coverage().unresolved_calls, 1);
    }

    #[test]
    fn csharp_indexer_access_is_not_dynamic_dispatch() {
        let call = CallSite {
            caller_id: "reader".into(),
            target_expression: "row[\"Name\"].ToString".into(),
            file_path: "Reader.cs".into(),
            language: "csharp".into(),
            line: 4,
        };
        let resolver = EmbeddedHybridResolver::analyze(SemanticProject {
            units: vec![SourceUnit {
                path: "Reader.cs".into(),
                language: "csharp".into(),
                content: "class Reader { string Read(dynamic row) => row[\"Name\"].ToString(); }"
                    .into(),
            }],
            calls: vec![call.clone()],
            ..SemanticProject::default()
        })
        .unwrap();

        let result = resolver.resolve_call_target(&call);
        assert_eq!(result.status, "EXTERNAL_OR_UNINDEXED");
        assert!(!result.dynamic_boundary);
        assert!(resolver.coverage().dynamic_boundaries.is_empty());
        assert_eq!(resolver.coverage().external_or_unindexed_calls, 1);
    }

    #[test]
    fn resolves_explicit_type_annotations_only() {
        let resolver = EmbeddedHybridResolver::analyze(SemanticProject {
            units: vec![SourceUnit {
                path: "app.ts".into(),
                language: "typescript".into(),
                content: "const service: PaymentService = build();".into(),
            }],
            ..SemanticProject::default()
        })
        .unwrap();
        let result = resolver.resolve_type(&SourceLocation {
            file_path: "app.ts".into(),
            line: 1,
            column: 6,
            symbol_hint: Some("service".into()),
        });
        assert_eq!(result.status, "RESOLVED");
        assert_eq!(result.type_name.as_deref(), Some("PaymentService"));
    }

    #[test]
    fn resolves_csharp_namespace_import_and_type_annotation() {
        let call = CallSite {
            caller_id: "program-main".into(),
            target_expression: "GreetingService".into(),
            file_path: "App/Program.cs".into(),
            language: "csharp".into(),
            line: 8,
        };
        let resolver = EmbeddedHybridResolver::analyze(SemanticProject {
            units: vec![
                SourceUnit {
                    path: "App/Program.cs".into(),
                    language: "csharp".into(),
                    content: "using Demo.Services;\nnamespace Demo.App { class Program { void Main() { GreetingService service = new GreetingService(); } } }".into(),
                },
                SourceUnit {
                    path: "Services/GreetingService.cs".into(),
                    language: "csharp".into(),
                    content: "namespace Demo.Services { public class GreetingService {} }".into(),
                },
            ],
            symbols: vec![SemanticSymbol {
                id: "greeting-service".into(),
                qualified_name: "Services::GreetingService.cs::Demo::Services::GreetingService".into(),
                display_name: "GreetingService".into(),
                file_path: "Services/GreetingService.cs".into(),
                kind: "CLASS".into(),
                language: "csharp".into(),
                start_line: 1,
            }],
            calls: vec![call.clone()],
            failed_files: vec![],
        })
        .unwrap();

        let resolved = resolver.resolve_call_target(&call);
        assert_eq!(resolved.status, "RESOLVED");
        assert_eq!(resolved.targets[0].symbol_id, "greeting-service");
        let resolved_type = resolver.resolve_type(&SourceLocation {
            file_path: "App/Program.cs".into(),
            line: 8,
            column: 0,
            symbol_hint: Some("service".into()),
        });
        assert_eq!(resolved_type.status, "RESOLVED");
        assert_eq!(resolved_type.type_name.as_deref(), Some("GreetingService"));
    }
}
