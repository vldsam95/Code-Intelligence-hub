use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameworkSource {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameworkProject {
    pub files: Vec<FrameworkSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrameworkDetection {
    pub adapter: String,
    pub detected: bool,
    pub confidence: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrameworkRoute {
    pub adapter: String,
    pub method: String,
    pub path: String,
    pub handler: Option<String>,
    pub source_file: String,
    pub line: u32,
    pub confidence: f64,
    pub ambiguity_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameworkFact {
    pub adapter: String,
    pub kind: String,
    pub name: String,
    pub source_file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FrameworkAnalysis {
    pub status: String,
    pub detections: Vec<FrameworkDetection>,
    pub routes: Vec<FrameworkRoute>,
    pub entry_points: Vec<FrameworkFact>,
    pub container_bindings: Vec<FrameworkFact>,
    pub jobs_events_schedulers: Vec<FrameworkFact>,
    pub models_and_migrations: Vec<FrameworkFact>,
    pub discovered_test_commands: Vec<Vec<String>>,
    pub dynamic_boundaries: Vec<String>,
    pub risk_zones: Vec<String>,
    pub unknowns: Vec<String>,
}

/// Stable adapter boundary. New frameworks must implement this contract rather than adding
/// framework-specific branches to the structural indexer.
pub trait FrameworkAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection;
    fn extract_routes(&self, project: &FrameworkProject) -> Result<Vec<FrameworkRoute>>;
    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>>;
    fn extract_container_bindings(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>>;
    fn extract_jobs_events_schedulers(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>>;
    fn extract_models_and_migrations(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>>;
    fn discover_test_commands(&self, project: &FrameworkProject) -> Vec<Vec<String>>;
    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String>;
    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String>;
}

pub struct FrameworkRegistry {
    adapters: Vec<Box<dyn FrameworkAdapter>>,
}

impl Default for FrameworkRegistry {
    fn default() -> Self {
        Self {
            adapters: vec![
                Box::new(LaravelAdapter),
                Box::new(WordPressAdapter),
                Box::new(ExpressAdapter),
                Box::new(NextJsAdapter),
                Box::new(FastApiAdapter),
                Box::new(DjangoAdapter),
                Box::new(DockerComposeAdapter),
            ],
        }
    }
}

impl FrameworkRegistry {
    pub fn analyze(&self, project: &FrameworkProject) -> FrameworkAnalysis {
        let mut analysis = FrameworkAnalysis::default();
        for adapter in &self.adapters {
            let detection = adapter.detect(project);
            if !detection.detected {
                continue;
            }
            analysis.detections.push(detection);
            collect_result(
                adapter.extract_routes(project),
                &mut analysis.routes,
                &mut analysis.unknowns,
                adapter.name(),
                "routes",
            );
            collect_result(
                adapter.extract_entry_points(project),
                &mut analysis.entry_points,
                &mut analysis.unknowns,
                adapter.name(),
                "entry points",
            );
            collect_result(
                adapter.extract_container_bindings(project),
                &mut analysis.container_bindings,
                &mut analysis.unknowns,
                adapter.name(),
                "container bindings",
            );
            collect_result(
                adapter.extract_jobs_events_schedulers(project),
                &mut analysis.jobs_events_schedulers,
                &mut analysis.unknowns,
                adapter.name(),
                "jobs/events/schedulers",
            );
            collect_result(
                adapter.extract_models_and_migrations(project),
                &mut analysis.models_and_migrations,
                &mut analysis.unknowns,
                adapter.name(),
                "models/migrations",
            );
            analysis
                .discovered_test_commands
                .extend(adapter.discover_test_commands(project));
            analysis
                .dynamic_boundaries
                .extend(adapter.identify_dynamic_boundaries(project));
            analysis.risk_zones.extend(adapter.risk_zones(project));
        }
        sort_and_deduplicate(&mut analysis);
        analysis.status = if analysis.unknowns.is_empty() {
            "COMPLETE".into()
        } else {
            "PARTIAL".into()
        };
        analysis
    }
}

fn collect_result<T>(
    result: Result<Vec<T>>,
    target: &mut Vec<T>,
    unknowns: &mut Vec<String>,
    adapter: &str,
    capability: &str,
) {
    match result {
        Ok(items) => target.extend(items),
        Err(error) => unknowns.push(format!("{adapter} could not analyze {capability}: {error}")),
    }
}

fn sort_and_deduplicate(analysis: &mut FrameworkAnalysis) {
    analysis
        .detections
        .sort_by(|a, b| a.adapter.cmp(&b.adapter));
    analysis.routes.sort_by(|a, b| {
        (&a.source_file, a.line, &a.method, &a.path).cmp(&(
            &b.source_file,
            b.line,
            &b.method,
            &b.path,
        ))
    });
    analysis.routes.dedup_by(|a, b| {
        a.source_file == b.source_file
            && a.line == b.line
            && a.method == b.method
            && a.path == b.path
    });
    analysis.dynamic_boundaries.sort();
    analysis.dynamic_boundaries.dedup();
    analysis.risk_zones.sort();
    analysis.risk_zones.dedup();
    analysis.discovered_test_commands.sort();
    analysis.discovered_test_commands.dedup();
}

struct LaravelAdapter;
struct WordPressAdapter;
struct ExpressAdapter;
struct NextJsAdapter;
struct FastApiAdapter;
struct DjangoAdapter;
struct DockerComposeAdapter;

impl FrameworkAdapter for LaravelAdapter {
    fn name(&self) -> &'static str {
        "laravel"
    }

    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection {
        detect_from_markers(
            self.name(),
            project,
            &["artisan", "routes/web.php", "routes/api.php"],
            &["laravel/framework", "Illuminate\\"],
        )
    }

    fn extract_routes(&self, project: &FrameworkProject) -> Result<Vec<FrameworkRoute>> {
        regex_routes(
            self.name(),
            project,
            &Regex::new(
                r#"Route::(get|post|put|patch|delete|options|head)\s*\(\s*[\"']([^\"']+)[\"']\s*,?\s*([^\n;)]*)"#,
            )?,
            1,
            2,
            Some(3),
        )
    }

    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(path_facts(self.name(), project, "ENTRY_POINT", |path| {
            path == "artisan" || path.starts_with("routes/")
        }))
    }

    fn extract_container_bindings(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "CONTAINER_BINDING",
            &Regex::new(r"(?:bind|singleton|instance)\s*\(\s*([^,\n]+)")?,
            1,
        )
    }

    fn extract_jobs_events_schedulers(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "JOB_EVENT_SCHEDULE",
            &Regex::new(r"(?:dispatch|event|schedule|command)\s*\(\s*([^,\n)]+)")?,
            1,
        )
    }

    fn extract_models_and_migrations(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        Ok(path_facts(
            self.name(),
            project,
            "MODEL_OR_MIGRATION",
            |path| path.starts_with("app/Models/") || path.starts_with("database/migrations/"),
        ))
    }

    fn discover_test_commands(&self, project: &FrameworkProject) -> Vec<Vec<String>> {
        if self.detect(project).detected {
            vec![vec!["php".into(), "artisan".into(), "test".into()]]
        } else {
            vec![]
        }
    }

    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String> {
        dynamic_markers(project, &["app(", "resolve(", "__call(", "Route::fallback"])
    }

    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String> {
        matching_paths(
            project,
            &["routes/", "app/Http/Middleware/", "database/migrations/"],
        )
    }
}

impl FrameworkAdapter for WordPressAdapter {
    fn name(&self) -> &'static str {
        "wordpress"
    }

    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection {
        detect_from_markers(
            self.name(),
            project,
            &["wp-config.php", "wp-content/plugins/", "wp-content/themes/"],
            &["Plugin Name:", "add_action(", "add_filter("],
        )
    }

    fn extract_routes(&self, project: &FrameworkProject) -> Result<Vec<FrameworkRoute>> {
        let regex = Regex::new(
            r#"register_rest_route\s*\(\s*[\"']([^\"']+)[\"']\s*,\s*[\"']([^\"']+)[\"']"#,
        )?;
        let mut routes = vec![];
        for file in &project.files {
            for captures in regex.captures_iter(&file.content) {
                let whole = captures.get(0).expect("whole regex match");
                routes.push(FrameworkRoute {
                    adapter: self.name().into(),
                    method: "ANY".into(),
                    path: format!("/{}/{}", captures[1].trim_matches('/'), captures[2].trim_matches('/')),
                    handler: None,
                    source_file: file.path.clone(),
                    line: line_number(&file.content, whole.start()),
                    confidence: 0.86,
                    ambiguity_reason: Some(
                        "WordPress REST methods/callback may be declared in a dynamic options array."
                            .into(),
                    ),
                });
            }
        }
        Ok(routes)
    }

    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(project
            .files
            .iter()
            .filter(|file| file.content.contains("Plugin Name:"))
            .map(|file| {
                fact(
                    self.name(),
                    "ENTRY_POINT",
                    "plugin bootstrap",
                    &file.path,
                    1,
                )
            })
            .collect())
    }

    fn extract_container_bindings(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn extract_jobs_events_schedulers(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "HOOK_OR_CRON",
            &Regex::new(r#"(?:add_action|add_filter|wp_schedule_event)\s*\(\s*['\"]([^'\"]+)"#)?,
            1,
        )
    }

    fn extract_models_and_migrations(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn discover_test_commands(&self, project: &FrameworkProject) -> Vec<Vec<String>> {
        if has_file(project, "phpunit.xml") {
            vec![vec!["./vendor/bin/phpunit".into()]]
        } else {
            vec![]
        }
    }

    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String> {
        dynamic_markers(
            project,
            &["do_action(", "apply_filters(", "call_user_func("],
        )
    }

    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String> {
        matching_paths(project, &["wp-content/plugins/", "wp-content/themes/"])
    }
}

impl FrameworkAdapter for ExpressAdapter {
    fn name(&self) -> &'static str {
        "express"
    }

    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection {
        detect_from_markers(
            self.name(),
            project,
            &["package.json"],
            &[
                "\"express\"",
                "express()",
                "from 'express'",
                "from \"express\"",
            ],
        )
    }

    fn extract_routes(&self, project: &FrameworkProject) -> Result<Vec<FrameworkRoute>> {
        regex_routes(
            self.name(),
            project,
            &Regex::new(
                r#"(?:app|router)\.(get|post|put|patch|delete|options|head)\s*\(\s*[\"']([^\"']+)[\"']\s*,\s*([A-Za-z_$][A-Za-z0-9_$\.]*)"#,
            )?,
            1,
            2,
            Some(3),
        )
    }

    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(path_facts(self.name(), project, "ENTRY_POINT", |path| {
            [
                "server.js",
                "server.ts",
                "app.js",
                "app.ts",
                "index.js",
                "index.ts",
            ]
            .contains(&path)
        }))
    }

    fn extract_container_bindings(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn extract_jobs_events_schedulers(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "MIDDLEWARE",
            &Regex::new(r"(?:app|router)\.use\s*\(\s*([^,\n)]+)")?,
            1,
        )
    }

    fn extract_models_and_migrations(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        Ok(path_facts(
            self.name(),
            project,
            "MODEL_OR_MIGRATION",
            |path| path.contains("/models/") || path.contains("/migrations/"),
        ))
    }

    fn discover_test_commands(&self, project: &FrameworkProject) -> Vec<Vec<String>> {
        if has_file(project, "package.json") {
            vec![vec!["npm".into(), "test".into()]]
        } else {
            vec![]
        }
    }

    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String> {
        dynamic_markers(project, &["require(variable", "router[", "app["])
    }

    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String> {
        matching_paths(project, &["routes/", "middleware/", "migrations/"])
    }
}

impl FrameworkAdapter for NextJsAdapter {
    fn name(&self) -> &'static str {
        "nextjs"
    }

    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection {
        detect_from_markers(
            self.name(),
            project,
            &["next.config.js", "next.config.mjs", "next.config.ts"],
            &["\"next\"", "next/server", "next/navigation"],
        )
    }

    fn extract_routes(&self, project: &FrameworkProject) -> Result<Vec<FrameworkRoute>> {
        let mut routes = vec![];
        let method_regex = Regex::new(
            r"(?m)export\s+(?:async\s+)?function\s+(GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD)\b",
        )?;
        for file in &project.files {
            if let Some(path) = next_route_path(&file.path) {
                let methods: Vec<_> = method_regex
                    .captures_iter(&file.content)
                    .map(|captures| captures[1].to_string())
                    .collect();
                let methods = if methods.is_empty() {
                    vec!["PAGE".into()]
                } else {
                    methods
                };
                for method in methods {
                    routes.push(FrameworkRoute {
                        adapter: self.name().into(),
                        method,
                        path: path.clone(),
                        handler: None,
                        source_file: file.path.clone(),
                        line: 1,
                        confidence: 0.95,
                        ambiguity_reason: path.contains('[').then(|| {
                            "Next.js dynamic route parameter preserved in route template.".into()
                        }),
                    });
                }
            }
        }
        Ok(routes)
    }

    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(path_facts(self.name(), project, "ENTRY_POINT", |path| {
            next_route_path(path).is_some()
        }))
    }

    fn extract_container_bindings(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn extract_jobs_events_schedulers(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn extract_models_and_migrations(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn discover_test_commands(&self, project: &FrameworkProject) -> Vec<Vec<String>> {
        if has_file(project, "package.json") {
            vec![vec!["npm".into(), "test".into()]]
        } else {
            vec![]
        }
    }

    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String> {
        project
            .files
            .iter()
            .filter(|file| file.path.contains('[') && next_route_path(&file.path).is_some())
            .map(|file| format!("{}: dynamic filesystem route", file.path))
            .collect()
    }

    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String> {
        matching_paths(project, &["app/", "pages/api/", "middleware.ts"])
    }
}

impl FrameworkAdapter for FastApiAdapter {
    fn name(&self) -> &'static str {
        "fastapi"
    }

    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection {
        detect_from_markers(
            self.name(),
            project,
            &[],
            &[
                "from fastapi",
                "import fastapi",
                "FastAPI(",
                "@app.",
                "@router.",
            ],
        )
    }

    fn extract_routes(&self, project: &FrameworkProject) -> Result<Vec<FrameworkRoute>> {
        regex_routes(
            self.name(),
            project,
            &Regex::new(
                r#"@(?:app|router)\.(get|post|put|patch|delete|options|head)\s*\(\s*[\"']([^\"']+)[\"'][^\n]*\)\s*\n\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            )?,
            1,
            2,
            Some(3),
        )
    }

    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "ENTRY_POINT",
            &Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*=\s*FastAPI\s*\(")?,
            1,
        )
    }

    fn extract_container_bindings(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "DEPENDENCY",
            &Regex::new(r"Depends\s*\(\s*([A-Za-z_][A-Za-z0-9_\.]*)")?,
            1,
        )
    }

    fn extract_jobs_events_schedulers(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "BACKGROUND_TASK",
            &Regex::new(r"background_tasks\.add_task\s*\(\s*([A-Za-z_][A-Za-z0-9_\.]*)")?,
            1,
        )
    }

    fn extract_models_and_migrations(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn discover_test_commands(&self, project: &FrameworkProject) -> Vec<Vec<String>> {
        if project
            .files
            .iter()
            .any(|file| file.path.starts_with("test") || file.path.contains("/test"))
        {
            vec![vec!["pytest".into(), "-q".into()]]
        } else {
            vec![]
        }
    }

    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String> {
        dynamic_markers(project, &["add_api_route(", "include_router("])
    }

    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String> {
        matching_content_paths(project, &["@app.", "@router.", "Depends("])
    }
}

impl FrameworkAdapter for DjangoAdapter {
    fn name(&self) -> &'static str {
        "django"
    }

    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection {
        detect_from_markers(
            self.name(),
            project,
            &["manage.py"],
            &["from django", "import django", "urlpatterns"],
        )
    }

    fn extract_routes(&self, project: &FrameworkProject) -> Result<Vec<FrameworkRoute>> {
        let regex = Regex::new(
            r#"(?:path|re_path)\s*\(\s*[\"']([^\"']*)[\"']\s*,\s*([A-Za-z_][A-Za-z0-9_\.]*)"#,
        )?;
        let mut routes = vec![];
        for file in &project.files {
            for captures in regex.captures_iter(&file.content) {
                let whole = captures.get(0).expect("whole regex match");
                routes.push(FrameworkRoute {
                    adapter: self.name().into(),
                    method: "ANY".into(),
                    path: format!("/{}", captures[1].trim_start_matches('/')),
                    handler: Some(captures[2].into()),
                    source_file: file.path.clone(),
                    line: line_number(&file.content, whole.start()),
                    confidence: 0.9,
                    ambiguity_reason: Some(
                        "Django URL configuration does not statically declare the HTTP method."
                            .into(),
                    ),
                });
            }
        }
        Ok(routes)
    }

    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(path_facts(self.name(), project, "ENTRY_POINT", |path| {
            path == "manage.py" || path.ends_with("/urls.py") || path == "urls.py"
        }))
    }

    fn extract_container_bindings(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn extract_jobs_events_schedulers(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        regex_facts(
            self.name(),
            project,
            "SIGNAL_OR_TASK",
            &Regex::new(r"@receiver\s*\(\s*([A-Za-z_][A-Za-z0-9_\.]*)")?,
            1,
        )
    }

    fn extract_models_and_migrations(
        &self,
        project: &FrameworkProject,
    ) -> Result<Vec<FrameworkFact>> {
        Ok(path_facts(
            self.name(),
            project,
            "MODEL_OR_MIGRATION",
            |path| path.ends_with("/models.py") || path.contains("/migrations/"),
        ))
    }

    fn discover_test_commands(&self, project: &FrameworkProject) -> Vec<Vec<String>> {
        if has_file(project, "manage.py") {
            vec![vec!["python".into(), "manage.py".into(), "test".into()]]
        } else {
            vec![]
        }
    }

    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String> {
        dynamic_markers(project, &["include(", "import_string("])
    }

    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String> {
        matching_paths(project, &["urls.py", "settings.py", "/migrations/"])
    }
}

impl FrameworkAdapter for DockerComposeAdapter {
    fn name(&self) -> &'static str {
        "docker_compose"
    }

    fn detect(&self, project: &FrameworkProject) -> FrameworkDetection {
        detect_from_markers(
            self.name(),
            project,
            &[
                "docker-compose.yml",
                "docker-compose.yaml",
                "compose.yml",
                "compose.yaml",
            ],
            &["services:"],
        )
    }

    fn extract_routes(&self, _: &FrameworkProject) -> Result<Vec<FrameworkRoute>> {
        Ok(vec![])
    }

    fn extract_entry_points(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        let service = Regex::new(r"(?m)^  ([A-Za-z0-9_.-]+):\s*(?:#.*)?$")?;
        regex_facts(self.name(), project, "DOCKER_SERVICE", &service, 1)
    }

    fn extract_container_bindings(&self, project: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        self.extract_entry_points(project)
    }

    fn extract_jobs_events_schedulers(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn extract_models_and_migrations(&self, _: &FrameworkProject) -> Result<Vec<FrameworkFact>> {
        Ok(vec![])
    }

    fn discover_test_commands(&self, _: &FrameworkProject) -> Vec<Vec<String>> {
        vec![]
    }

    fn identify_dynamic_boundaries(&self, project: &FrameworkProject) -> Vec<String> {
        dynamic_markers(project, &["${", "extends:"])
    }

    fn risk_zones(&self, project: &FrameworkProject) -> Vec<String> {
        matching_paths(
            project,
            &[
                "docker-compose.yml",
                "docker-compose.yaml",
                "compose.yml",
                "compose.yaml",
            ],
        )
    }
}

fn detect_from_markers(
    adapter: &str,
    project: &FrameworkProject,
    path_markers: &[&str],
    content_markers: &[&str],
) -> FrameworkDetection {
    let mut reasons = BTreeSet::new();
    for file in &project.files {
        for marker in path_markers {
            if file.path == *marker || file.path.starts_with(marker) || file.path.ends_with(marker)
            {
                reasons.insert(format!("path marker: {marker}"));
            }
        }
        for marker in content_markers {
            if file.content.contains(marker) {
                reasons.insert(format!("content marker: {marker}"));
            }
        }
    }
    FrameworkDetection {
        adapter: adapter.into(),
        detected: !reasons.is_empty(),
        confidence: if reasons.len() >= 2 { 0.98 } else { 0.82 },
        reasons: reasons.into_iter().collect(),
    }
}

fn regex_routes(
    adapter: &str,
    project: &FrameworkProject,
    regex: &Regex,
    method_group: usize,
    path_group: usize,
    handler_group: Option<usize>,
) -> Result<Vec<FrameworkRoute>> {
    let mut routes = vec![];
    for file in &project.files {
        for captures in regex.captures_iter(&file.content) {
            let Some(method) = captures.get(method_group) else {
                continue;
            };
            let Some(path) = captures.get(path_group) else {
                continue;
            };
            let whole = captures.get(0).expect("whole regex match");
            routes.push(FrameworkRoute {
                adapter: adapter.into(),
                method: method.as_str().to_uppercase(),
                path: path.as_str().into(),
                handler: handler_group
                    .and_then(|group| captures.get(group))
                    .map(|value| clean_handler(value.as_str()))
                    .filter(|value| !value.is_empty()),
                source_file: file.path.clone(),
                line: line_number(&file.content, whole.start()),
                confidence: 0.95,
                ambiguity_reason: None,
            });
        }
    }
    Ok(routes)
}

fn regex_facts(
    adapter: &str,
    project: &FrameworkProject,
    kind: &str,
    regex: &Regex,
    name_group: usize,
) -> Result<Vec<FrameworkFact>> {
    let mut facts = vec![];
    for file in &project.files {
        for captures in regex.captures_iter(&file.content) {
            let Some(name) = captures.get(name_group) else {
                continue;
            };
            let whole = captures.get(0).expect("whole regex match");
            facts.push(fact(
                adapter,
                kind,
                name.as_str().trim(),
                &file.path,
                line_number(&file.content, whole.start()),
            ));
        }
    }
    Ok(facts)
}

fn fact(adapter: &str, kind: &str, name: &str, path: &str, line: u32) -> FrameworkFact {
    FrameworkFact {
        adapter: adapter.into(),
        kind: kind.into(),
        name: name.into(),
        source_file: path.into(),
        line,
    }
}

fn path_facts<F>(
    adapter: &str,
    project: &FrameworkProject,
    kind: &str,
    predicate: F,
) -> Vec<FrameworkFact>
where
    F: Fn(&str) -> bool,
{
    project
        .files
        .iter()
        .filter(|file| predicate(&file.path))
        .map(|file| fact(adapter, kind, &file.path, &file.path, 1))
        .collect()
}

fn line_number(source: &str, byte_offset: usize) -> u32 {
    source[..byte_offset]
        .bytes()
        .filter(|value| *value == b'\n')
        .count() as u32
        + 1
}

fn clean_handler(value: &str) -> String {
    value
        .trim()
        .trim_matches(['[', ']', '\'', '"'])
        .split([',', ')'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn has_file(project: &FrameworkProject, expected: &str) -> bool {
    project.files.iter().any(|file| file.path == expected)
}

fn matching_paths(project: &FrameworkProject, markers: &[&str]) -> Vec<String> {
    project
        .files
        .iter()
        .filter(|file| markers.iter().any(|marker| file.path.contains(marker)))
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn matching_content_paths(project: &FrameworkProject, markers: &[&str]) -> Vec<String> {
    project
        .files
        .iter()
        .filter(|file| markers.iter().any(|marker| file.content.contains(marker)))
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn dynamic_markers(project: &FrameworkProject, markers: &[&str]) -> Vec<String> {
    let mut results = BTreeSet::new();
    for file in &project.files {
        for marker in markers {
            for (index, line) in file.content.lines().enumerate() {
                if line.contains(marker) {
                    results.insert(format!("{}:{}: {}", file.path, index + 1, marker));
                }
            }
        }
    }
    results.into_iter().collect()
}

fn next_route_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    if let Some(relative) = normalized.strip_prefix("app/") {
        let route = relative
            .strip_suffix("/route.ts")
            .or_else(|| relative.strip_suffix("/route.tsx"))
            .or_else(|| relative.strip_suffix("/page.ts"))
            .or_else(|| relative.strip_suffix("/page.tsx"))?;
        return Some(format!("/{}", route.trim_matches('/')));
    }
    if let Some(relative) = normalized.strip_prefix("pages/") {
        let without_extension = source_stem(relative);
        let route = without_extension
            .strip_suffix("/index")
            .unwrap_or(&without_extension);
        return Some(format!("/{}", route.trim_matches('/')));
    }
    None
}

fn source_stem(path: &str) -> String {
    Path::new(path)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> FrameworkProject {
        FrameworkProject {
            files: files
                .iter()
                .map(|(path, content)| FrameworkSource {
                    path: (*path).into(),
                    content: (*content).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn fastapi_route_has_correct_method_path_and_no_duplicate() {
        let analysis = FrameworkRegistry::default().analyze(&project(&[(
            "app.py",
            "from fastapi import FastAPI\napp = FastAPI()\n@app.get(\"/health\")\ndef health(): pass",
        )]));
        assert_eq!(analysis.routes.len(), 1);
        assert_eq!(analysis.routes[0].adapter, "fastapi");
        assert_eq!(analysis.routes[0].method, "GET");
        assert_eq!(analysis.routes[0].path, "/health");
        assert_eq!(analysis.routes[0].handler.as_deref(), Some("health"));
    }

    #[test]
    fn django_route_does_not_confuse_path_with_method() {
        let analysis = FrameworkRegistry::default().analyze(&project(&[
            (
                "manage.py",
                "from django.core.management import execute_from_command_line",
            ),
            (
                "site/urls.py",
                "urlpatterns = [path(\"health/\", views.health)]",
            ),
        ]));
        let route = analysis
            .routes
            .iter()
            .find(|route| route.adapter == "django")
            .unwrap();
        assert_eq!(route.method, "ANY");
        assert_eq!(route.path, "/health/");
        assert_eq!(route.handler.as_deref(), Some("views.health"));
    }

    #[test]
    fn all_priority_adapters_are_detectable() {
        let analysis = FrameworkRegistry::default().analyze(&project(&[
            ("artisan", ""),
            (
                "composer.json",
                "{\"require\": {\"laravel/framework\": \"*\"}}",
            ),
            (
                "wp-content/plugins/example/plugin.php",
                "/* Plugin Name: Example */",
            ),
            (
                "package.json",
                "{\"dependencies\": {\"express\": \"*\", \"next\": \"*\"}}",
            ),
            ("next.config.js", "export default {}"),
            ("api.py", "from fastapi import FastAPI"),
            (
                "manage.py",
                "from django.core.management import execute_from_command_line",
            ),
            ("compose.yaml", "services:\n  api:\n    image: app"),
        ]));
        let detected: BTreeSet<_> = analysis
            .detections
            .iter()
            .map(|detection| detection.adapter.as_str())
            .collect();
        assert_eq!(
            detected,
            BTreeSet::from([
                "django",
                "docker_compose",
                "express",
                "fastapi",
                "laravel",
                "nextjs",
                "wordpress",
            ])
        );
    }

    #[test]
    fn next_dynamic_route_is_preserved_as_template() {
        let analysis = FrameworkRegistry::default().analyze(&project(&[
            ("next.config.js", "export default {}"),
            ("app/users/[id]/route.ts", "export async function GET() {}"),
        ]));
        let route = analysis.routes.first().unwrap();
        assert_eq!(route.path, "/users/[id]");
        assert!(route.ambiguity_reason.is_some());
        assert!(!analysis.dynamic_boundaries.is_empty());
    }
}
