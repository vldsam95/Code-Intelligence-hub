use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use codeintel_core::{
    atomic_write, compute_source_revision, discover_project, ensure_project_dirs, load_project,
    project_paths, ChangeRun, ChangeRunStatus, Policy, ProjectManifest, ProjectPaths,
    SourceRevision, PRODUCT_VERSION,
};
use codeintel_graph::{Coverage, GraphStore, RuntimeEdgeObservation, RuntimeIngest};
use codeintel_indexer::{index_project, IndexResult};
use codeintel_quality::{
    analyze_dataflow, correlate_security_scenarios, ingest_runtime_trace, quality_gate,
    run_security_scenarios, verify_change,
};
use codeintel_review::{
    abandon_change_run, close_change_run, preflight_change, review_change, PreflightRequest,
};
use fs2::FileExt;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const DEFAULT_SOCKET: &str = "/run/code-intelligence/worker.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub schema_version: u32,
    pub hub_url: Option<String>,
    pub worker_id: Option<Uuid>,
    pub host_id: Uuid,
    pub host_name: String,
    pub credential_file: PathBuf,
    #[serde(default)]
    pub host_access_credential_file: Option<PathBuf>,
    #[serde(default = "default_socket")]
    pub socket_path: PathBuf,
    #[serde(default = "default_long_poll")]
    pub long_poll_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LocalHubBinding {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    hub_url: Option<String>,
    #[serde(default)]
    provisioning_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalRegistry {
    pub projects: BTreeMap<Uuid, RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub project_id: Uuid,
    pub instance_id: Uuid,
    pub root: PathBuf,
    pub current_source_revision_id: Option<String>,
    pub last_indexed_at: Option<DateTime<Utc>>,
    pub last_quality_revision_id: Option<String>,
    pub last_quality_verdict: Option<String>,
    #[serde(default = "default_registration_state")]
    pub hub_registration: String,
    #[serde(default)]
    pub initialized_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub first_registered_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_successful_hub_sync: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_hub_error: Option<String>,
    #[serde(default)]
    pub last_index_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub request_id: Uuid,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub request_id: Uuid,
    pub ok: bool,
    pub result: Option<Value>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

pub struct ProjectLock {
    file: File,
}

impl ProjectLock {
    pub fn acquire(paths: &ProjectPaths) -> Result<Self> {
        fs::create_dir_all(&paths.locks_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(paths.locks_dir.join("analysis.lock"))?;
        file.try_lock_exclusive()
            .map_err(|_| anyhow!("IN_USE: another analysis mutation is active"))?;
        Ok(Self { file })
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

pub fn config_path() -> PathBuf {
    std::env::var_os("CODEINTEL_WORKER_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/code-intelligence/worker.yaml"))
}

pub fn registry_path() -> PathBuf {
    codeintel_core::data_root().join("registry.json")
}

pub fn load_config() -> Result<WorkerConfig> {
    let path = config_path();
    serde_yaml::from_slice(&fs::read(&path).with_context(|| format!("read {}", path.display()))?)
        .context("parse Worker config")
}

pub fn save_config(config: &WorkerConfig) -> Result<()> {
    atomic_write(&config_path(), serde_yaml::to_string(config)?.as_bytes())
}

fn host_access_path(config: &WorkerConfig) -> PathBuf {
    config
        .host_access_credential_file
        .clone()
        .unwrap_or_else(|| {
            codeintel_core::data_root()
                .join("credentials")
                .join("host-access")
        })
}

fn local_binding_path(root: &Path) -> PathBuf {
    root.join(".code-intel/local/hub-binding.yaml")
}

fn project_access_path(root: &Path) -> PathBuf {
    root.join(".code-intel/local/project-access.token")
}

pub fn configure_project_hub_binding(
    root: &Path,
    hub_url: Option<&str>,
    mode: &str,
    provisioning_allowed: bool,
    project_access: Option<&str>,
) -> Result<()> {
    let local = root.join(".code-intel/local");
    fs::create_dir_all(&local)?;
    #[cfg(unix)]
    fs::set_permissions(&local, fs::Permissions::from_mode(0o700))?;
    atomic_write(
        &local_binding_path(root),
        serde_yaml::to_string(&LocalHubBinding {
            mode: mode.into(),
            hub_url: hub_url.map(str::to_string),
            provisioning_allowed,
        })?
        .as_bytes(),
    )?;
    #[cfg(unix)]
    fs::set_permissions(local_binding_path(root), fs::Permissions::from_mode(0o600))?;
    if let Some(token) = project_access {
        atomic_write(&project_access_path(root), token.as_bytes())?;
        #[cfg(unix)]
        fs::set_permissions(project_access_path(root), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn clear_project_hub_secrets(root: &Path) -> Result<()> {
    let local = root.join(".code-intel/local");
    if local.exists() {
        for name in ["project-access.token", "hub-binding.yaml", "instance.json"] {
            let path = local.join(name);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

pub fn load_registry() -> Result<LocalRegistry> {
    let path = registry_path();
    if !path.is_file() {
        return Ok(LocalRegistry::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn save_registry(registry: &LocalRegistry) -> Result<()> {
    mutate_registry(|current| {
        *current = registry.clone();
    })
}

fn mutate_registry<T>(change: impl FnOnce(&mut LocalRegistry) -> T) -> Result<T> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.lock_exclusive()?;
    let mut registry = if path.is_file() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        LocalRegistry::default()
    };
    let result = change(&mut registry);
    atomic_write(&path, &serde_json::to_vec_pretty(&registry)?)?;
    Ok(result)
}

pub fn open_project(
    path: &Path,
) -> Result<(ProjectManifest, Policy, RegistryEntry, SourceRevision)> {
    let root = discover_project(path)?;
    let (manifest, policy) = load_project(&root)?;
    let revision = compute_source_revision(&root, &policy)?;
    let initial_registration = if local_binding_path(&root).is_file() {
        "PENDING"
    } else {
        "LOCAL_ONLY"
    };
    let entry = mutate_registry(|registry| {
        let entry = registry
            .projects
            .entry(manifest.project_id)
            .or_insert_with(|| RegistryEntry {
                project_id: manifest.project_id,
                instance_id: Uuid::new_v4(),
                root: root.clone(),
                current_source_revision_id: None,
                last_indexed_at: None,
                last_quality_revision_id: None,
                last_quality_verdict: None,
                hub_registration: initial_registration.into(),
                initialized_at: Some(Utc::now()),
                first_registered_at: None,
                last_successful_hub_sync: None,
                last_hub_error: None,
                last_index_error: None,
            });
        entry.root = root.clone();
        entry.current_source_revision_id = Some(revision.source_revision_id.clone());
        entry.clone()
    })?;
    let paths = project_paths(&root, &manifest, entry.instance_id);
    ensure_project_dirs(&paths)?;
    Ok((manifest, policy, entry, revision))
}

pub fn refresh_index(
    root: &Path,
    manifest: &ProjectManifest,
    policy: &Policy,
    entry: &RegistryEntry,
    revision: &SourceRevision,
) -> Result<IndexResult> {
    refresh_index_with_mode(root, manifest, policy, entry, revision, false)
}

/// Rebuild even when the source revision is unchanged. This is used after an analyzer upgrade;
/// the previous CURRENT graph stays available until its staged replacement validates.
pub fn rebuild_index(
    root: &Path,
    manifest: &ProjectManifest,
    policy: &Policy,
    entry: &RegistryEntry,
    revision: &SourceRevision,
) -> Result<IndexResult> {
    refresh_index_with_mode(root, manifest, policy, entry, revision, true)
}

fn refresh_index_with_mode(
    root: &Path,
    manifest: &ProjectManifest,
    policy: &Policy,
    entry: &RegistryEntry,
    revision: &SourceRevision,
    force: bool,
) -> Result<IndexResult> {
    let paths = project_paths(root, manifest, entry.instance_id);
    ensure_project_dirs(&paths)?;
    let _lock = ProjectLock::acquire(&paths)?;
    if let Ok(store) = GraphStore::open(&paths.graph_path) {
        if !force && store.revision()? == revision.source_revision_id {
            return Ok(IndexResult {
                source_revision_id: revision.source_revision_id.clone(),
                state: "CURRENT".into(),
                graph_path: paths.graph_path,
                node_count: 0,
                edge_count: 0,
                coverage: store.coverage()?,
                volatile_files: revision.volatile_files.clone(),
            });
        }
    }
    let result = match index_project(
        root,
        revision,
        policy,
        &paths.graph_path,
        &paths.content_cache,
    ) {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            mutate_registry(|registry| {
                if let Some(current) = registry.projects.get_mut(&manifest.project_id) {
                    current.last_index_error = Some(message.chars().take(2_000).collect());
                }
            })?;
            return Err(error.context("graph build failed; no staged graph was published"));
        }
    };
    mutate_registry(|registry| {
        if let Some(current) = registry.projects.get_mut(&manifest.project_id) {
            current.current_source_revision_id = Some(revision.source_revision_id.clone());
            current.last_indexed_at = Some(Utc::now());
            current.last_index_error = None;
        }
    })?;
    spool_index_report(manifest, entry, revision, &result, &paths)?;
    Ok(result)
}

pub fn handle_request(request: &RpcRequest) -> Result<Value> {
    match request.method.as_str() {
        "open_project" => {
            let path = required_path(&request.params)?;
            let (manifest, policy, entry, revision) = open_project(&path)?;
            let index = refresh_index(&entry.root, &manifest, &policy, &entry, &revision)?;
            let paths = project_paths(&entry.root, &manifest, entry.instance_id);
            Ok(json!({
                "project_id": manifest.project_id,
                "parent_project_id": manifest.parent_project_id,
                "instance_id": entry.instance_id,
                "project_root": entry.root,
                "current_source_revision_id": revision.source_revision_id,
                "index_state": index.state,
                "readiness": if index.coverage.failed_files.is_empty() { "READY" } else { "PARTIAL" },
                "coverage": bounded_coverage(&index.coverage),
                "wiki_mode": manifest.wiki.mode,
                "wiki": {
                    "mode": manifest.wiki.mode,
                    "binding_status": wiki_status_for_root(&manifest, &entry.root),
                },
                "hub_registration": entry.hub_registration,
                "hub_sync": local_hub_sync(&entry, &paths)?,
                "workspace_mode": manifest.workspace.mode,
            }))
        }
        "get_project_context" => {
            let (manifest, _policy, entry, revision, paths) = context(&request.params)?;
            let store = GraphStore::open(&paths.graph_path)?;
            let coverage = store.coverage()?;
            let architecture = architecture_summary(&store, &manifest, &revision)?;
            Ok(json!({
                "project_id": manifest.project_id,
                "instance_id": entry.instance_id,
                "name": manifest.name,
                "slug": manifest.slug,
                "current_source_revision_id": revision.source_revision_id,
                "wiki": manifest.wiki,
                "workspace": manifest.workspace,
                "coverage": bounded_coverage(&coverage),
                "architecture": architecture,
                "unknowns": if coverage.absence_scope_complete() { Vec::<String>::new() } else {
                    vec!["Coverage is partial. Absence claims remain unproven.".to_string()]
                }
            }))
        }
        "understand" => {
            let (manifest, _policy, entry, revision, paths) = context(&request.params)?;
            let intent = request
                .params
                .get("intent")
                .and_then(Value::as_str)
                .unwrap_or("*");
            let store = GraphStore::open(&paths.graph_path)?;
            let result = if intent == "*" {
                json!(architecture_summary(&store, &manifest, &revision)?)
            } else {
                serde_json::to_value(store.search(intent, 20)?)?
            };
            Ok(json!({
                "summary": format!("Bounded understanding context for {intent}"),
                "source_revision_id": revision.source_revision_id,
                "result": result,
                "next_recommended_action": "Inspect the actual current source ranges before editing.",
                "context_handle": format!("ctx_{}", Uuid::new_v4()),
                "project_id": manifest.project_id,
                "instance_id": entry.instance_id,
            }))
        }
        "find_code" => {
            let (_manifest, _policy, _entry, revision, paths) = context(&request.params)?;
            let store = GraphStore::open(&paths.graph_path)?;
            let query = request
                .params
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("query is required"))?;
            let operation = request
                .params
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or("symbol");
            let result = match operation {
                "symbol" => serde_json::to_value(store.find_symbol(query, 50)?)?,
                "search" => serde_json::to_value(store.search(query, 30)?)?,
                "callers" | "callees" => {
                    let found = store.find_symbol(query, 10)?;
                    if let Some(symbol) = found.items.first() {
                        serde_json::to_value(store.neighbors(&symbol.id, operation, 50)?)?
                    } else {
                        serde_json::to_value(found)?
                    }
                }
                _ => return Err(anyhow!("unsupported find_code operation")),
            };
            Ok(json!({"source_revision_id": revision.source_revision_id, "result": result}))
        }
        "preflight_change" => {
            let (manifest, policy, entry, _revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let task = request
                .params
                .get("intent")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("intent is required"))?;
            let planned_files: Vec<String> = request
                .params
                .get("planned_files")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let result = preflight_change(PreflightRequest {
                root: &entry.root,
                manifest: &manifest,
                policy: &policy,
                paths: &paths,
                instance_id: entry.instance_id,
                task_summary: task,
                planned_files: &planned_files,
                agent_client: request
                    .params
                    .get("agent_client")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })?;
            spool_change_run(&manifest, &entry, &result.change_run, &paths)?;
            Ok(json!({
                "change_run": result.change_run,
                "current_revision": {
                    "source_revision_id": result.current_revision.source_revision_id,
                    "policy_hash": result.current_revision.policy_hash,
                    "file_count": result.current_revision.files.len(),
                    "volatile_files": result.current_revision.volatile_files,
                },
                "planned_files": result.planned_files,
                "baseline_recommended": result.baseline_recommended,
                "warnings": result.warnings,
            }))
        }
        "review_change" => {
            let (manifest, policy, entry, revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let change_run_id = required_uuid(&request.params, "change_run_id")?;
            let result = review_change(&entry.root, &policy, &paths, change_run_id)?;
            spool_report(
                &manifest,
                &entry,
                &revision,
                "review",
                json!({
                    "evidence_id": Uuid::new_v4(),
                    "producer": "codeintel-review",
                    "producer_version": PRODUCT_VERSION,
                    "origin": "STATIC_EXACT",
                    "confidence": 0.9,
                    "coverage": {"bounded": true},
                    "unknowns": result.known_unknowns,
                    "policy_hash": revision.policy_hash,
                    "summary": result.change_summary,
                    "change_run_id": change_run_id,
                    "findings": result.findings,
                }),
                &paths,
            )?;
            Ok(serde_json::to_value(result)?)
        }
        "verify_change" => {
            let (manifest, policy, entry, revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let change_run_id = required_uuid(&request.params, "change_run_id")?;
            let run = load_change_run(&paths, change_run_id)?;
            let risk = run.risk_after.as_ref().unwrap_or(&run.risk_before).clone();
            let result = verify_change(
                &entry.root,
                &policy,
                &paths,
                change_run_id,
                &revision.source_revision_id,
                &risk,
                "POST_CHANGE",
            )?;
            spool_report(
                &manifest,
                &entry,
                &revision,
                "tests",
                json!({
                    "evidence_id": result.verification_id,
                    "producer": "codeintel-quality",
                    "producer_version": PRODUCT_VERSION,
                    "origin": "TEST_OBSERVED",
                    "confidence": 1.0,
                    "coverage": {"required_checks": result.required_checks, "completed_checks": result.completed_checks},
                    "unknowns": result.blocking_unknowns,
                    "policy_hash": revision.policy_hash,
                    "summary": format!("{} commands and {} smoke checks executed", result.commands.len(), result.smoke.len()),
                    "change_run_id": change_run_id,
                }),
                &paths,
            )?;
            Ok(serde_json::to_value(result)?)
        }
        "quality_gate" => {
            let (manifest, policy, entry, revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let change_run_id = required_uuid(&request.params, "change_run_id")?;
            let review_path = paths
                .reports_dir
                .join(format!("review-{change_run_id}.json"));
            let blocking_findings = if review_path.is_file() {
                let value: Value = serde_json::from_slice(&fs::read(review_path)?)?;
                value["findings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|finding| finding["blocking"].as_bool() == Some(true))
                    .map(|finding| {
                        finding["why"]
                            .as_str()
                            .unwrap_or("blocking finding")
                            .to_string()
                    })
                    .collect()
            } else {
                vec!["Review bundle is missing.".into()]
            };
            let report = quality_gate(
                &entry.root,
                &policy,
                &paths,
                change_run_id,
                blocking_findings,
            )?;
            update_registry_quality(&manifest, &report.source_revision_id, &report.verdict)?;
            update_portable_last_verified(&entry.root, &report)?;
            spool_report(
                &manifest,
                &entry,
                &revision,
                "quality",
                json!({
                    "quality_run_id": report.quality_run_id,
                    "project_id": manifest.project_id,
                    "source_revision_id": report.source_revision_id,
                    "change_run_id": change_run_id,
                    "verdict": report.verdict,
                    "currentness": report.currentness,
                    "risk_score": report.risk_score,
                    "required_checks": report.required_checks,
                    "completed_checks": report.completed_checks,
                    "failed_checks": report.failed_checks,
                    "blocking_unknowns": report.blocking_unknowns,
                    "warnings": report.warnings,
                    "policy_hash": report.policy_hash,
                    "graph_state": "CURRENT",
                }),
                &paths,
            )?;
            if matches!(report.verdict.as_str(), "PASS" | "PASS_WITH_WARNINGS") {
                close_change_run(&paths, change_run_id, ChangeRunStatus::Gated)?;
            }
            Ok(serde_json::to_value(report)?)
        }
        "security_scenarios" => {
            let (_manifest, policy, _entry, revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let change_run_id = required_uuid(&request.params, "change_run_id")?;
            let _run = load_change_run(&paths, change_run_id)?;
            let action = request
                .params
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("validate");
            if !matches!(action, "inventory" | "validate" | "run" | "correlate") {
                return Err(anyhow!(
                    "action must be inventory, validate, run or correlate"
                ));
            }
            if action == "inventory" {
                let graph = GraphStore::open(&paths.graph_path)?;
                let page = request
                    .params
                    .get("page")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as usize;
                let limit = request
                    .params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(25) as usize;
                let inventory = graph.endpoint_inventory(page, limit)?;
                let inventory_incomplete = inventory["remaining"].as_u64().unwrap_or(0) > 0;
                return Ok(json!({
                    "source_revision_id": revision.source_revision_id,
                    "inventory": inventory,
                    "unknowns": if inventory_incomplete {
                        vec!["Endpoint inventory may be incomplete because the graph response is bounded."]
                    } else {
                        vec![]
                    },
                }));
            }
            let selected: Vec<String> = request
                .params
                .get("scenario_ids")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if action == "correlate" {
                return Ok(serde_json::to_value(correlate_security_scenarios(
                    &paths,
                    change_run_id,
                    &revision.source_revision_id,
                    &policy.security_scenarios,
                    &selected,
                )?)?);
            }
            let report = run_security_scenarios(
                &paths,
                change_run_id,
                &revision.source_revision_id,
                &policy.security_scenarios,
                &selected,
                action == "run",
            )?;
            Ok(serde_json::to_value(report)?)
        }
        "abandon_change_run" => {
            let (_manifest, _policy, _entry, _revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let change_run_id = required_uuid(&request.params, "change_run_id")?;
            let reason = request
                .params
                .get("reason")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("reason is required"))?;
            Ok(serde_json::to_value(abandon_change_run(
                &paths,
                change_run_id,
                reason,
            )?)?)
        }
        "project_status" => {
            let path = required_path(&request.params)?;
            let (manifest, policy, entry, revision) = open_project(&path)?;
            let paths = project_paths(&entry.root, &manifest, entry.instance_id);
            ensure_project_dirs(&paths)?;
            let stale = GraphStore::open(&paths.graph_path)
                .and_then(|store| store.revision())
                .ok()
                .as_deref()
                != Some(&revision.source_revision_id);
            let refresh_error = stale
                .then(|| refresh_index(&entry.root, &manifest, &policy, &entry, &revision))
                .transpose()
                .err()
                .map(|error| error.to_string());
            let registry = load_registry()?;
            let current = registry.projects.get(&entry.project_id).unwrap_or(&entry);
            let graph = GraphStore::open(&paths.graph_path);
            let graph_revision = graph.as_ref().ok().and_then(|store| store.revision().ok());
            let graph_current = graph_revision.as_deref() == Some(&revision.source_revision_id);
            let currentness = match &current.last_quality_revision_id {
                Some(quality) if quality == &revision.source_revision_id => "CURRENT",
                Some(_) => "UNVERIFIED_CHANGE",
                None => "UNVERIFIED_CHANGE",
            };
            Ok(json!({
                "project_id": entry.project_id,
                "instance_id": entry.instance_id,
                "current_source_revision_id": revision.source_revision_id,
                "currentness": currentness,
                "index_state": if graph_current { "CURRENT" } else if refresh_error.is_some() || current.last_index_error.is_some() { "FAILED" } else { "STALE" },
                "graph_revision": graph_revision,
                "last_quality_gate": current.last_quality_verdict,
                "last_quality_revision_id": current.last_quality_revision_id,
                "active_change_run": codeintel_review::active_change_run(&paths)?,
                "hub_registration": entry.hub_registration,
                "hub_sync": local_hub_sync(current, &paths)?,
                "project_access": if project_access_path(&entry.root).is_file() { "PRESENT" } else { "MISSING" },
                "local_intelligence": if graph_current { "READY" } else if refresh_error.is_some() || current.last_index_error.is_some() { "INVALID" } else { "STALE" },
                "last_index_error": refresh_error.or_else(|| current.last_index_error.clone()),
                "last_successful_hub_sync": current.last_successful_hub_sync,
                "last_hub_error": current.last_hub_error,
                "wiki": {
                    "mode": manifest.wiki.mode,
                    "binding_status": wiki_status_for_root(&manifest, &entry.root),
                },
            }))
        }
        "advanced.coverage" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let coverage = GraphStore::open(&paths.graph_path)?.coverage()?;
            Ok(json!({
                "source_revision_id": revision.source_revision_id,
                "coverage": bounded_coverage(&coverage)
            }))
        }
        "advanced.graph_query" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let store = GraphStore::open(&paths.graph_path)?;
            let result = store.graph_query(
                request.params.get("kind").and_then(Value::as_str),
                request.params.get("file_path").and_then(Value::as_str),
                request.params["limit"].as_u64().unwrap_or(50) as usize,
            )?;
            Ok(json!({"source_revision_id": revision.source_revision_id, "result": result}))
        }
        "advanced.graph_visualization" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let store = GraphStore::open(&paths.graph_path)?;
            let result = store.visualization(
                request.params["max_nodes"].as_u64().unwrap_or(220) as usize,
                request.params["max_edges"].as_u64().unwrap_or(420) as usize,
            )?;
            Ok(json!({"source_revision_id": revision.source_revision_id, "result": result}))
        }
        "sync_hub" => Ok(sync_hub_now()),
        "advanced.edge_evidence" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let symbol_id = required_string(&request.params, "symbol_id")?;
            let result = GraphStore::open(&paths.graph_path)?.edge_evidence(
                symbol_id,
                request.params["limit"].as_u64().unwrap_or(50) as usize,
            )?;
            Ok(json!({"source_revision_id": revision.source_revision_id, "result": result}))
        }
        "advanced.find_path" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let source = required_string(&request.params, "source_symbol_id")?;
            let sink = required_string(&request.params, "sink_symbol_id")?;
            let store = GraphStore::open(&paths.graph_path)?;
            let path = store.find_path(
                source,
                sink,
                request.params["max_depth"]
                    .as_u64()
                    .unwrap_or(12)
                    .clamp(1, 50) as usize,
            )?;
            let coverage = store.coverage()?;
            Ok(json!({
                "source_revision_id": revision.source_revision_id,
                "path": path,
                "absence_proven": path.is_empty() && coverage.absence_scope_complete(),
                "coverage": coverage,
                "unknowns": if path.is_empty() && !coverage.absence_scope_complete() {
                    vec!["No graph path is not proof that no application flow exists."]
                } else { Vec::<&str>::new() }
            }))
        }
        "advanced.dead_code_audit" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let candidates = GraphStore::open(&paths.graph_path)?.dead_code_candidates(100)?;
            Ok(json!({
                "source_revision_id": revision.source_revision_id,
                "candidates": candidates,
                "auto_delete_allowed": false,
                "warning": "Candidates require actual source, framework/config, tests/runtime, review and gate."
            }))
        }
        "advanced.runtime_ingest" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let change_run_id = required_uuid(&request.params, "change_run_id")?;
            let input = PathBuf::from(
                request
                    .params
                    .get("input_path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("input_path is required"))?,
            );
            let workload = request
                .params
                .get("workload")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("workload is required"))?;
            let adapter = request
                .params
                .get("adapter")
                .and_then(Value::as_str)
                .unwrap_or("generic_trace_ingest");
            if !matches!(adapter, "generic_trace_ingest" | "http_trace") {
                return Err(anyhow!("unsupported runtime ingest adapter"));
            }
            let result = ingest_runtime_trace(
                &paths,
                change_run_id,
                &revision.source_revision_id,
                workload,
                adapter,
                &input,
            )?;
            let runtime_edges: Vec<_> = result
                .edges
                .iter()
                .map(|edge| RuntimeEdgeObservation {
                    source_symbol: edge.source_symbol.clone(),
                    target_symbol: edge.target_symbol.clone(),
                    kind: edge.kind.clone(),
                    count: edge.count,
                })
                .collect();
            let mut graph = GraphStore::open(&paths.graph_path)?;
            graph.ingest_runtime_edges(RuntimeIngest {
                runtime_run_id: &result.runtime_run_id.to_string(),
                source_revision_id: &result.source_revision_id,
                workload: &result.workload,
                observed_at: &result.observed_at.to_rfc3339(),
                coverage_scope: &serde_json::to_string(&result.coverage_scope)?,
                adapter: &result.adapter,
                adapter_version: &result.adapter_version,
                edges: &runtime_edges,
            })?;
            Ok(serde_json::to_value(result)?)
        }
        "advanced.dataflow" => {
            let (_, _, _, revision, paths) = context(&request.params)?;
            let _lock = ProjectLock::acquire(&paths)?;
            let change_run_id = required_uuid(&request.params, "change_run_id")?;
            let source = required_string(&request.params, "source")?;
            let sink = required_string(&request.params, "sink")?;
            let result = analyze_dataflow(
                &paths,
                change_run_id,
                &revision.source_revision_id,
                source,
                sink,
                request.params["declared_scope_complete"]
                    .as_bool()
                    .unwrap_or(false),
            )?;
            Ok(serde_json::to_value(result)?)
        }
        _ => Err(anyhow!("Unknown Worker method: {}", request.method)),
    }
}

fn bounded_coverage(coverage: &Coverage) -> Value {
    const SAMPLE_LIMIT: usize = 5;
    fn sample(values: &[String]) -> Vec<String> {
        values
            .iter()
            .take(SAMPLE_LIMIT)
            .map(|value| {
                let truncated = value.chars().count() > 240;
                let mut bounded: String = value.chars().take(240).collect();
                if truncated {
                    bounded.push('…');
                }
                bounded
            })
            .collect()
    }
    json!({
        "supported_languages": coverage.supported_languages,
        "unsupported_languages": sample(&coverage.unsupported_languages),
        "parsed_files": coverage.parsed_files,
        "failed_files": sample(&coverage.failed_files),
        "semantic": coverage.semantic,
        "framework": coverage.framework,
        "semantic_resolved_calls": coverage.semantic_resolved_calls,
        "semantic_ambiguous_calls": coverage.semantic_ambiguous_calls,
        "semantic_unresolved_calls": coverage.semantic_unresolved_calls,
        "semantic_external_or_unindexed_calls": coverage.semantic_external_or_unindexed_calls,
        "semantic_unresolved_files": sample(&coverage.semantic_unresolved_files),
        "semantic_external_or_unindexed_files": sample(&coverage.semantic_external_or_unindexed_files),
        "semantic_ambiguous_files": sample(&coverage.semantic_ambiguous_files),
        "dynamic_boundaries": sample(&coverage.dynamic_boundaries),
        "semantic_unknowns": sample(&coverage.semantic_unknowns),
        "framework_adapters": coverage.framework_adapters,
        "framework_unknowns": sample(&coverage.framework_unknowns),
        "absence_scope_complete": coverage.absence_scope_complete(),
        "items_remaining": {
            "unsupported_languages": coverage.unsupported_languages.len().saturating_sub(SAMPLE_LIMIT),
            "failed_files": coverage.failed_files.len().saturating_sub(SAMPLE_LIMIT),
            "semantic_unresolved_files": coverage.semantic_unresolved_files.len().saturating_sub(SAMPLE_LIMIT),
            "semantic_external_or_unindexed_files": coverage.semantic_external_or_unindexed_files.len().saturating_sub(SAMPLE_LIMIT),
            "semantic_ambiguous_files": coverage.semantic_ambiguous_files.len().saturating_sub(SAMPLE_LIMIT),
            "dynamic_boundaries": coverage.dynamic_boundaries.len().saturating_sub(SAMPLE_LIMIT),
            "semantic_unknowns": coverage.semantic_unknowns.len().saturating_sub(SAMPLE_LIMIT),
            "framework_unknowns": coverage.framework_unknowns.len().saturating_sub(SAMPLE_LIMIT),
        }
    })
}

pub fn send_local_request(socket: &Path, request: &RpcRequest) -> Result<Value> {
    let mut stream =
        UnixStream::connect(socket).with_context(|| format!("connect {}", socket.display()))?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let response: RpcResponse = serde_json::from_str(&line)?;
    if response.ok {
        Ok(response.result.unwrap_or(Value::Null))
    } else {
        let error = response.error.unwrap();
        Err(anyhow!("{}: {}", error.code, error.message))
    }
}

pub fn run_daemon(config: WorkerConfig) -> Result<()> {
    if let Some(parent) = config.socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if config.socket_path.exists() {
        fs::remove_file(&config.socket_path)?;
    }
    let listener = UnixListener::bind(&config.socket_path)?;
    fs::set_permissions(&config.socket_path, fs::Permissions::from_mode(0o660))?;
    listener.set_nonblocking(true)?;
    let hub_config = config.clone();
    thread::spawn(move || {
        if let Err(error) = hub_loop(&hub_config) {
            eprintln!(
                "{}",
                json!({"level":"ERROR","event_code":"HUB_LOOP_STOPPED","message":error.to_string()})
            );
        }
    });
    let watcher_config = config.clone();
    thread::spawn(move || {
        if let Err(error) = watch_projects(&watcher_config) {
            eprintln!(
                "{}",
                json!({"level":"ERROR","event_code":"WATCHER_STOPPED","message":error.to_string()})
            );
        }
    });
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                thread::spawn(move || {
                    if let Err(error) = serve_stream(stream) {
                        eprintln!(
                            "{}",
                            json!({"level":"WARN","event_code":"LOCAL_RPC_ERROR","message":error.to_string()})
                        );
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn serve_stream(mut stream: UnixStream) -> Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let request: RpcRequest = serde_json::from_str(&line)?;
    let response = match std::panic::catch_unwind(|| handle_request(&request)) {
        Ok(Ok(result)) => RpcResponse {
            request_id: request.request_id,
            ok: true,
            result: Some(result),
            error: None,
        },
        Ok(Err(error)) => RpcResponse {
            request_id: request.request_id,
            ok: false,
            result: None,
            error: Some(RpcError {
                code: classify_error(&error.to_string()).into(),
                message: error.to_string(),
                retryable: error.to_string().contains("IN_USE"),
            }),
        },
        Err(_) => RpcResponse {
            request_id: request.request_id,
            ok: false,
            result: None,
            error: Some(RpcError {
                code: "ANALYZER_PANIC_RECOVERED".into(),
                message: "An analysis task panicked; Worker remained available.".into(),
                retryable: true,
            }),
        },
    };
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    Ok(())
}

fn watch_projects(_config: &WorkerConfig) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })?;
    let mut watched = HashSet::new();
    for entry in load_registry()?.projects.values() {
        if entry.root.is_dir() {
            watcher.watch(&entry.root, RecursiveMode::Recursive)?;
            watched.insert(entry.root.clone());
        }
    }
    let mut last_event = Instant::now();
    let mut dirty = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(Ok(_)) => {
                last_event = Instant::now();
                dirty = true;
            }
            Ok(Err(error)) => {
                eprintln!(
                    "{}",
                    json!({"level":"WARN","event_code":"WATCH_ERROR","message":error.to_string()})
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout)
                if dirty && last_event.elapsed() >= Duration::from_millis(400) =>
            {
                dirty = false;
                let registry = load_registry()?;
                for entry in registry.projects.values() {
                    if let Ok((manifest, policy)) = load_project(&entry.root) {
                        if let Ok(revision) = compute_source_revision(&entry.root, &policy) {
                            if entry.current_source_revision_id.as_deref()
                                != Some(&revision.source_revision_id)
                            {
                                let _ = refresh_index(
                                    &entry.root,
                                    &manifest,
                                    &policy,
                                    entry,
                                    &revision,
                                );
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("watch channel closed"))
            }
        }
        for entry in load_registry()?.projects.values() {
            if entry.root.is_dir() && watched.insert(entry.root.clone()) {
                watcher.watch(&entry.root, RecursiveMode::Recursive)?;
            }
        }
    }
}

fn hub_loop(config: &WorkerConfig) -> Result<()> {
    let Some(hub_url) = config.hub_url.as_ref() else {
        loop {
            thread::sleep(Duration::from_secs(30));
        }
    };
    let credential = fs::read_to_string(&config.credential_file)
        .context("read Worker credential")?
        .trim()
        .to_string();
    let worker_id = config
        .worker_id
        .ok_or_else(|| anyhow!("Worker is not enrolled"))?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(35))
        .build()?;
    let mut backoff = 1_u64;
    loop {
        let result = (|| -> Result<()> {
            sync_hub_once(config, &client, hub_url, &credential, worker_id)?;
            let response = client
                .get(format!("{hub_url}/api/v1/workers/{worker_id}/jobs/next"))
                .bearer_auth(&credential)
                .send()?;
            if response.status().as_u16() == 204 {
                return Ok(());
            }
            let payload: Value = response.error_for_status()?.json()?;
            if let Some(job) = payload.get("job") {
                execute_remote_job(&client, hub_url, &credential, worker_id, job)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => backoff = 1,
            Err(error) => {
                let _ = mark_all_hub_errors(&error.to_string());
                eprintln!(
                    "{}",
                    json!({"level":"WARN","event_code":"HUB_SYNC_RETRY","message":error.to_string(),"backoff_sec":backoff})
                );
                thread::sleep(Duration::from_secs(backoff));
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

fn ensure_host_access(
    config: &WorkerConfig,
    client: &Client,
    hub_url: &str,
    worker_credential: &str,
    worker_id: Uuid,
) -> Result<Option<String>> {
    let path = host_access_path(config);
    if let Ok(value) = fs::read_to_string(&path) {
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let token = value.trim().to_string();
        let recently_refreshed = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(300));
        if !token.is_empty() && recently_refreshed {
            return Ok(Some(token));
        }
    }
    let response = client
        .post(format!(
            "{hub_url}/api/v1/workers/{worker_id}/host-access/bootstrap"
        ))
        .bearer_auth(worker_credential)
        .send()?;
    if response.status().as_u16() == 403 {
        return Ok(None);
    }
    let payload: Value = response.error_for_status()?.json()?;
    let Some(raw) = payload.get("host_access_token").and_then(Value::as_str) else {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(None);
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(&path, raw.as_bytes())?;
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(Some(raw.to_string()))
}

fn sync_hub_once(
    config: &WorkerConfig,
    client: &Client,
    hub_url: &str,
    credential: &str,
    worker_id: Uuid,
) -> Result<()> {
    let host_access = ensure_host_access(config, client, hub_url, credential, worker_id)?;
    register_remote_projects(
        config,
        client,
        hub_url,
        credential,
        host_access.as_deref(),
        worker_id,
        config.host_id,
    )?;
    sync_spool(client, hub_url, credential)?;
    let registry = load_registry()?;
    let instances: Vec<Value> = registry
        .projects
        .values()
        .map(|entry| {
            json!({
                "instance_id": entry.instance_id,
                "source_revision_id": entry.current_source_revision_id,
                "status": "ONLINE"
            })
        })
        .collect();
    client
        .post(format!("{hub_url}/api/v1/workers/{worker_id}/heartbeat"))
        .bearer_auth(credential)
        .json(&json!({
            "worker_version": PRODUCT_VERSION,
            "resource_summary": resource_summary(),
            "project_instances": instances
        }))
        .send()?
        .error_for_status()?;
    let now = Utc::now();
    mutate_registry(|registry| {
        for entry in registry.projects.values_mut() {
            entry.last_successful_hub_sync = Some(now);
            entry.last_hub_error = None;
        }
    })?;
    Ok(())
}

/// One-shot synchronization used by `codeintel init`. Failure is returned as structured state
/// and never turns a successful local initialization into an error.
pub fn sync_hub_now() -> Value {
    let config = match load_config() {
        Ok(value) => value,
        Err(error) => {
            return json!({
                "registration": "LOCAL_ONLY",
                "sync": "LOCAL_ONLY",
                "automatic_retry": false,
                "reason": error.to_string(),
            })
        }
    };
    let Some(hub_url) = config.hub_url.as_deref() else {
        return json!({
            "registration": "LOCAL_ONLY",
            "sync": "LOCAL_ONLY",
            "automatic_retry": false,
            "reason": "Worker is not enrolled with a Hub",
        });
    };
    let credential = match fs::read_to_string(&config.credential_file) {
        Ok(value) => value.trim().to_string(),
        Err(error) => {
            return json!({
                "registration": "PENDING",
                "sync": "OFFLINE",
                "automatic_retry": true,
                "reason": error.to_string(),
            })
        }
    };
    let Some(worker_id) = config.worker_id else {
        return json!({
            "registration": "PENDING",
            "sync": "OFFLINE",
            "automatic_retry": false,
            "reason": "Worker is not enrolled",
        });
    };
    let client = match Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(value) => value,
        Err(error) => {
            return json!({"registration":"PENDING","sync":"OFFLINE","automatic_retry":true,"reason":error.to_string()})
        }
    };
    match sync_hub_once(&config, &client, hub_url, &credential, worker_id) {
        Ok(()) => {
            let states: Vec<String> = load_registry()
                .map(|registry| {
                    registry
                        .projects
                        .values()
                        .map(|entry| entry.hub_registration.clone())
                        .collect()
                })
                .unwrap_or_default();
            let registration = if states.iter().all(|state| state == "REGISTERED") {
                "REGISTERED"
            } else if states.iter().any(|state| state == "AUTH_FAILED") {
                "AUTH_FAILED"
            } else if states.iter().any(|state| state == "PENDING") {
                "PENDING"
            } else {
                "LOCAL_ONLY"
            };
            json!({
                "registration": registration,
                "sync": if registration == "REGISTERED" { "CURRENT" } else { registration },
                "automatic_retry": true,
            })
        }
        Err(error) => {
            let _ = mark_all_hub_errors(&error.to_string());
            json!({
                "registration": "PENDING",
                "sync": "OFFLINE",
                "automatic_retry": true,
                "reason": error.to_string(),
            })
        }
    }
}

fn update_hub_state(
    project_id: Uuid,
    registration: &str,
    synced_at: Option<DateTime<Utc>>,
    error: Option<String>,
) -> Result<()> {
    mutate_registry(|registry| {
        if let Some(entry) = registry.projects.get_mut(&project_id) {
            entry.hub_registration = registration.into();
            if registration == "REGISTERED" && entry.first_registered_at.is_none() {
                entry.first_registered_at = synced_at;
            }
            if let Some(value) = synced_at {
                entry.last_successful_hub_sync = Some(value);
            }
            entry.last_hub_error = error;
        }
    })
}

fn mark_all_hub_errors(message: &str) -> Result<()> {
    mutate_registry(|registry| {
        for entry in registry.projects.values_mut() {
            if entry.hub_registration != "REGISTERED" {
                entry.hub_registration = "PENDING".into();
            }
            entry.last_hub_error = Some(message.chars().take(500).collect());
        }
    })
}

fn register_remote_projects(
    config: &WorkerConfig,
    client: &Client,
    hub_url: &str,
    worker_credential: &str,
    host_access: Option<&str>,
    worker_id: Uuid,
    host_id: Uuid,
) -> Result<()> {
    let registry = load_registry()?;
    for entry in registry.projects.values() {
        let (manifest, policy) = load_project(&entry.root)?;
        let binding: LocalHubBinding = fs::read(local_binding_path(&entry.root))
            .ok()
            .and_then(|raw| serde_yaml::from_slice(&raw).ok())
            .unwrap_or_default();
        let project_access = fs::read_to_string(project_access_path(&entry.root))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let using_host_access = entry.hub_registration != "REGISTERED"
            && project_access.is_none()
            && binding.provisioning_allowed;
        let registration_credential = if entry.hub_registration == "REGISTERED" {
            Some(worker_credential)
        } else if let Some(value) = project_access.as_deref() {
            Some(value)
        } else if binding.provisioning_allowed {
            host_access
        } else {
            None
        };
        let Some(registration_credential) = registration_credential else {
            update_hub_state(
                entry.project_id,
                "LOCAL_ONLY",
                None,
                Some("No Project Access binding; local intelligence remains available".into()),
            )?;
            continue;
        };
        let revision = compute_source_revision(&entry.root, &policy)?;
        let coverage =
            GraphStore::open(&project_paths(&entry.root, &manifest, entry.instance_id).graph_path)
                .and_then(|store| store.coverage())
                .unwrap_or_default();
        let response = client
            .post(format!(
                "{hub_url}/api/v1/workers/{worker_id}/projects/register"
            ))
            .bearer_auth(registration_credential)
            .header(
                "Idempotency-Key",
                format!("{}:{}", entry.instance_id, revision.source_revision_id),
            )
            .json(&json!({
                "project_id": manifest.project_id,
                "slug": manifest.slug,
                "name": manifest.name,
                "instance_id": entry.instance_id,
                "host_id": host_id,
                "primary_path": entry.root,
                "related_roots": manifest.source.related_roots,
                "capabilities": {
                    "languages": coverage.supported_languages,
                    "frameworks": coverage.framework_adapters,
                    "semantic": coverage.semantic,
                    "framework": coverage.framework,
                },
                "wiki": {
                    "mode": manifest.wiki.mode,
                    "wiki_id": manifest.wiki.wiki_id,
                    "local_path": manifest.wiki.local_path,
                    "binding_status": wiki_status_for_root(&manifest, &entry.root),
                },
                "index_status": "CURRENT",
                "worker_version": PRODUCT_VERSION,
                "policy_schema_version": 1,
                "project_manifest_schema_version": manifest.schema_version,
                "initialized_at": entry.initialized_at,
                "revision": {
                    "revision_id": revision.source_revision_id,
                    "project_id": manifest.project_id,
                    "policy_hash": revision.policy_hash,
                    "indexer_version": revision.indexer_version,
                    "root_mapping_version": revision.root_mapping_version,
                    "file_count": revision.files.len(),
                    "inventory_hash": revision.inventory_hash,
                    "git_commit": Value::Null,
                    "git_dirty": Value::Null
                }
            }))
            .send()?;
        if matches!(response.status().as_u16(), 401 | 403) {
            let state = if project_access.is_some() {
                "AUTH_FAILED"
            } else if using_host_access {
                let path = host_access_path(config);
                if path.exists() {
                    fs::remove_file(path)?;
                }
                "PENDING"
            } else {
                "LOCAL_ONLY"
            };
            update_hub_state(
                entry.project_id,
                state,
                None,
                Some(format!("Hub authorization rejected: {}", response.status())),
            )?;
            continue;
        }
        let payload: Value = response.error_for_status()?.json()?;
        if let Some(raw) = payload
            .get("project_access")
            .and_then(|value| value.get("token"))
            .and_then(Value::as_str)
        {
            configure_project_hub_binding(
                &entry.root,
                Some(hub_url),
                "CONNECTED",
                false,
                Some(raw),
            )?;
        } else if project_access.is_some() || entry.hub_registration == "REGISTERED" {
            configure_project_hub_binding(
                &entry.root,
                Some(hub_url),
                "CONNECTED",
                false,
                project_access.as_deref(),
            )?;
        }
        if payload
            .get("project_access")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            == Some("MISSING")
        {
            let token_path = project_access_path(&entry.root);
            if token_path.exists() {
                fs::remove_file(token_path)?;
            }
            configure_project_hub_binding(
                &entry.root,
                Some(hub_url),
                "CONNECTED_NO_PROJECT_ACCESS",
                false,
                None,
            )?;
        }
        update_hub_state(entry.project_id, "REGISTERED", Some(Utc::now()), None)?;
    }
    Ok(())
}

fn execute_remote_job(
    client: &Client,
    hub_url: &str,
    credential: &str,
    worker_id: Uuid,
    job: &Value,
) -> Result<()> {
    let request_id = required_string(job, "request_id")?;
    client
        .post(format!(
            "{hub_url}/api/v1/workers/{worker_id}/remote-analysis/{request_id}/claim"
        ))
        .bearer_auth(credential)
        .send()?
        .error_for_status()?;
    let project_id = Uuid::parse_str(required_string(job, "project_id")?)?;
    let registry = load_registry()?;
    let entry = registry
        .projects
        .get(&project_id)
        .ok_or_else(|| anyhow!("Project is not registered on this Worker"))?;
    let query_type = required_string(job, "query_type")?;
    let mut params = job.get("query").cloned().unwrap_or_else(|| json!({}));
    params["path"] = json!(entry.root);
    let local_method = match query_type {
        "project_status" | "quality_status" => "project_status",
        "architecture_refresh" => "get_project_context",
        "find_symbol_metadata" => "find_code",
        "callers" | "callees" => {
            params["operation"] = json!(query_type);
            "find_code"
        }
        "impact" | "preflight_preview" => "understand",
        "coverage" => "advanced.coverage",
        "graph_visualization" => "advanced.graph_visualization",
        _ => return Err(anyhow!("Remote query type is not allowed")),
    };
    let result = handle_request(&RpcRequest {
        request_id: Uuid::new_v4(),
        method: local_method.into(),
        params,
    })?;
    // The local advanced tool returns revision metadata around its result. The Hub's
    // ephemeral graph contract intentionally accepts only the bounded graph itself.
    let structured_result = remote_structured_result(query_type, result)?;
    let current = compute_source_revision(&entry.root, &load_project(&entry.root)?.1)?;
    client
        .post(format!(
            "{hub_url}/api/v1/remote-analysis/{request_id}/result"
        ))
        .bearer_auth(credential)
        .json(&json!({
            "result_id": Uuid::new_v4(),
            "project_id": project_id,
            "instance_id": entry.instance_id,
            "source_revision_id": current.source_revision_id,
            "freshness": "CURRENT",
            "summary": format!("Live structured {query_type} result"),
            "structured_result": structured_result,
            "coverage": {"source_bodies_returned": false},
            "unknowns": []
        }))
        .send()?
        .error_for_status()?;
    Ok(())
}

fn remote_structured_result(query_type: &str, result: Value) -> Result<Value> {
    if query_type == "graph_visualization" {
        return result
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("Graph visualization result is missing its bounded payload"));
    }
    Ok(result)
}

fn sync_spool(client: &Client, hub_url: &str, credential: &str) -> Result<()> {
    let registry = load_registry()?;
    for entry in registry.projects.values() {
        if entry.hub_registration != "REGISTERED" {
            continue;
        }
        let (manifest, _) = load_project(&entry.root)?;
        let paths = project_paths(&entry.root, &manifest, entry.instance_id);
        if !paths.spool_dir.is_dir() {
            continue;
        }
        let mut pending_runs: Vec<_> = paths
            .spool_dir
            .read_dir()?
            .filter_map(|item| item.ok())
            .filter(|item| {
                item.path().extension().and_then(|value| value.to_str()) == Some("pending")
            })
            .collect();
        pending_runs.sort_by_key(|item| item.file_name());
        for item in pending_runs {
            let envelope: Value = serde_json::from_slice(&fs::read(item.path())?)?;
            if envelope.get("kind").and_then(Value::as_str) != Some("change_run") {
                return Err(anyhow!("unknown pending spool object"));
            }
            let run = envelope
                .get("payload")
                .ok_or_else(|| anyhow!("pending ChangeRun payload missing"))?;
            let payload = json!({
                "change_run_id": run["change_run_id"],
                "project_id": run["project_id"],
                "agent_client": run["agent_client"],
                "agent_session_id": run["agent_session_id"],
                "task_summary": run["task_summary"],
                "before_source_revision_id": run["before_source_revision_id"],
                "risk_before": run["risk_before"]["risk_score"],
                "required_checks": run["required_checks"]
            });
            let response = client
                .post(format!(
                    "{hub_url}/api/v1/instances/{}/change-runs",
                    entry.instance_id
                ))
                .bearer_auth(credential)
                .header(
                    "Idempotency-Key",
                    run["change_run_id"].as_str().unwrap_or("missing"),
                )
                .json(&payload)
                .send()?;
            if response.status().is_success() {
                fs::remove_file(item.path())?;
            } else {
                return Err(anyhow!(
                    "Hub rejected pending ChangeRun with status {}",
                    response.status()
                ));
            }
        }
        let mut files: Vec<_> = paths
            .spool_dir
            .read_dir()?
            .filter_map(|item| item.ok())
            .filter(|item| item.path().extension().and_then(|value| value.to_str()) == Some("json"))
            .collect();
        files.sort_by_key(|item| item.file_name());
        for item in files {
            let envelope: Value = serde_json::from_slice(&fs::read(item.path())?)?;
            let report_type = envelope["report_type"]
                .as_str()
                .ok_or_else(|| anyhow!("spool report_type missing"))?;
            let response = client
                .post(format!(
                    "{hub_url}/api/v1/instances/{}/reports/{report_type}",
                    entry.instance_id
                ))
                .bearer_auth(credential)
                .header(
                    "Idempotency-Key",
                    envelope["message_id"].as_str().unwrap_or("missing"),
                )
                .json(&envelope)
                .send()?;
            if response.status().is_success() {
                fs::remove_file(item.path())?;
            } else if response.status().as_u16() == 409 && envelope.get("revision").is_none() {
                let status = response.status();
                let reason = response.text().unwrap_or_else(|_| "Conflict".into());
                let rejected = paths.spool_dir.join("rejected");
                fs::create_dir_all(&rejected)?;
                let target = rejected.join(item.file_name());
                fs::rename(item.path(), &target)?;
                atomic_write(
                    &target.with_extension("reason"),
                    format!("Legacy report retained after Hub {status}: {reason}\n").as_bytes(),
                )?;
                eprintln!(
                    "{}",
                    json!({
                        "level":"WARN",
                        "event_code":"LEGACY_SPOOL_QUARANTINED",
                        "message":"A pre-revision-metadata report was retained locally after Hub conflict.",
                        "file":target.file_name().map(|value| value.to_string_lossy().to_string()),
                    })
                );
            } else {
                return Err(anyhow!(
                    "Hub rejected spool report with status {}",
                    response.status()
                ));
            }
        }
    }
    Ok(())
}

fn spool_index_report(
    manifest: &ProjectManifest,
    entry: &RegistryEntry,
    revision: &SourceRevision,
    index: &IndexResult,
    paths: &ProjectPaths,
) -> Result<()> {
    spool_report(
        manifest,
        entry,
        revision,
        "index",
        json!({
            "evidence_id": Uuid::new_v4(),
            "index_snapshot_id": Uuid::new_v4(),
            "producer": "codeintel-indexer",
            "producer_version": PRODUCT_VERSION,
            "origin": "STATIC_EXACT",
            "confidence": 1.0,
            "coverage": index.coverage,
            "unsupported_areas": index.coverage.unsupported_languages,
            "unknowns": index.coverage.failed_files,
            "policy_hash": revision.policy_hash,
            "summary": format!("Indexed {} nodes and {} edges", index.node_count, index.edge_count),
            "state": index.state,
            "db_size_bytes": fs::metadata(&paths.graph_path).map(|m| m.len()).unwrap_or(0),
        }),
        paths,
    )
}

fn spool_change_run(
    manifest: &ProjectManifest,
    entry: &RegistryEntry,
    run: &ChangeRun,
    paths: &ProjectPaths,
) -> Result<()> {
    atomic_write(
        &paths
            .spool_dir
            .join(format!("change-run-{}.pending", run.change_run_id)),
        &serde_json::to_vec_pretty(&json!({
            "kind": "change_run",
            "project_id": manifest.project_id,
            "instance_id": entry.instance_id,
            "payload": run,
        }))?,
    )
}

fn spool_report(
    manifest: &ProjectManifest,
    entry: &RegistryEntry,
    revision: &SourceRevision,
    report_type: &str,
    payload: Value,
    paths: &ProjectPaths,
) -> Result<()> {
    let message_id = Uuid::new_v4();
    let envelope = json!({
        "schema_version": 1,
        "message_id": message_id,
        "sent_at": Utc::now(),
        "project_id": manifest.project_id,
        "instance_id": entry.instance_id,
        "source_revision_id": revision.source_revision_id,
        "revision": {
            "revision_id": revision.source_revision_id,
            "project_id": manifest.project_id,
            "policy_hash": revision.policy_hash,
            "indexer_version": revision.indexer_version,
            "root_mapping_version": revision.root_mapping_version,
            "file_count": revision.files.len(),
            "inventory_hash": revision.inventory_hash,
            "git_commit": Value::Null,
            "git_dirty": Value::Null,
        },
        "report_type": report_type,
        "payload": payload,
    });
    atomic_write(
        &paths.spool_dir.join(format!(
            "{}-{}.json",
            Utc::now().timestamp_millis(),
            message_id
        )),
        &serde_json::to_vec_pretty(&envelope)?,
    )
}

fn spool_has_pending(paths: &ProjectPaths) -> Result<bool> {
    if !paths.spool_dir.is_dir() {
        return Ok(false);
    }
    Ok(paths
        .spool_dir
        .read_dir()?
        .filter_map(|item| item.ok())
        .any(|item| {
            item.file_type().map(|kind| kind.is_file()).unwrap_or(false)
                && matches!(
                    item.path().extension().and_then(|value| value.to_str()),
                    Some("json" | "pending")
                )
        }))
}

fn local_hub_sync(entry: &RegistryEntry, paths: &ProjectPaths) -> Result<&'static str> {
    if load_config()
        .ok()
        .and_then(|config| config.hub_url)
        .is_none()
    {
        return Ok("LOCAL_ONLY");
    }
    if entry.hub_registration == "LOCAL_ONLY" {
        return Ok("LOCAL_ONLY");
    }
    if entry.hub_registration == "AUTH_FAILED" {
        return Ok("AUTH_FAILED");
    }
    if entry.last_hub_error.is_some() {
        return Ok("OFFLINE");
    }
    if entry.hub_registration != "REGISTERED" || spool_has_pending(paths)? {
        return Ok("PENDING");
    }
    let recent = entry
        .last_successful_hub_sync
        .map(|value| Utc::now().signed_duration_since(value).num_seconds() <= 90)
        .unwrap_or(false);
    Ok(if recent { "CURRENT" } else { "OFFLINE" })
}

fn wiki_status_for_root(manifest: &ProjectManifest, root: &Path) -> String {
    match manifest.wiki.mode.as_str() {
        "none" => manifest.wiki.binding_status.clone(),
        "local" => {
            if manifest
                .wiki
                .local_path
                .as_ref()
                .map(|path| root.join(path).is_dir())
                .unwrap_or(false)
            {
                "BOUND".into()
            } else {
                "UNAVAILABLE".into()
            }
        }
        "remote" if manifest.wiki.wiki_id.is_some() => "BOUND".into(),
        "remote" => "UNRESOLVED".into(),
        _ => "UNRESOLVED".into(),
    }
}

fn context(
    params: &Value,
) -> Result<(
    ProjectManifest,
    Policy,
    RegistryEntry,
    SourceRevision,
    ProjectPaths,
)> {
    let path = required_path(params)?;
    let (manifest, policy, entry, revision) = open_project(&path)?;
    let paths = project_paths(&entry.root, &manifest, entry.instance_id);
    ensure_project_dirs(&paths)?;
    if GraphStore::open(&paths.graph_path)
        .and_then(|store| store.revision())
        .ok()
        .as_deref()
        != Some(&revision.source_revision_id)
    {
        refresh_index(&entry.root, &manifest, &policy, &entry, &revision)?;
    }
    Ok((manifest, policy, entry, revision, paths))
}

fn architecture_summary(
    store: &GraphStore,
    manifest: &ProjectManifest,
    revision: &SourceRevision,
) -> Result<Value> {
    let routes =
        store
            .find_symbol("ROUTE", 20)
            .unwrap_or_else(|_| codeintel_graph::QueryResponse {
                summary: "No route metadata".into(),
                items: vec![],
                coverage: store.coverage().unwrap(),
                absence_proven: false,
                unknowns: vec!["Route extraction may be partial.".into()],
                context_handle: None,
                items_remaining: 0,
            });
    Ok(json!({
        "summary": format!("{} at revision {}", manifest.name, &revision.source_revision_id[..12]),
        "flow_view": [
            "HTTP Routes → Controllers/Handlers → Services",
            "Background Jobs/Events → Application Services",
            "Application Services → Database / External Services"
        ],
        "route_count_visible": routes.items.len(),
        "coverage": store.coverage()?,
        "note": "Architecture is derived evidence; inspect actual source for exact current behavior."
    }))
}

fn load_change_run(paths: &ProjectPaths, id: Uuid) -> Result<ChangeRun> {
    Ok(serde_json::from_slice(&fs::read(
        paths.reports_dir.join(format!("change-run-{id}.json")),
    )?)?)
}

fn update_registry_quality(
    manifest: &ProjectManifest,
    revision: &str,
    verdict: &str,
) -> Result<()> {
    mutate_registry(|registry| {
        if let Some(entry) = registry.projects.get_mut(&manifest.project_id) {
            entry.last_quality_revision_id = Some(revision.into());
            entry.last_quality_verdict = Some(verdict.into());
        }
    })
}

/// Keep only bounded, revision-pinned verification metadata beside the source. The whole
/// `.code-intel/` directory is excluded from source inventory, so this write cannot invalidate the
/// gate it records. It is portable and safe for Git; reports, graph data and secrets stay local.
fn update_portable_last_verified(
    root: &Path,
    report: &codeintel_quality::QualityReport,
) -> Result<()> {
    let path = root.join(".code-intel/context/LAST_VERIFIED.json");
    let payload = json!({
        "schema_version": 1,
        "source_revision_id": report.source_revision_id,
        "quality_gate": report.verdict,
        "currentness": report.currentness,
        "risk_score": report.risk_score,
        "completed_checks": report.completed_checks,
        "failed_checks": report.failed_checks,
        "blocking_unknown_count": report.blocking_unknowns.len(),
        "warning_count": report.warnings.len(),
        "policy_hash": report.policy_hash,
        "produced_at": report.completed_at,
        "producer": format!("codeintel-quality/{}", PRODUCT_VERSION),
        "note": "Portable metadata only. Rebuild the local graph and match source_revision_id before treating it as current."
    });
    atomic_write(&path, &serde_json::to_vec_pretty(&payload)?)
}

fn resource_summary() -> Value {
    let rss_mb = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|value| {
            value
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u64>().ok())
        })
        .map(|kb| kb / 1024);
    json!({
        "rss_mb": rss_mb,
        "heavy_jobs": 0,
        "lsp_processes": 0,
        "version": PRODUCT_VERSION,
    })
}

fn required_path(params: &Value) -> Result<PathBuf> {
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| anyhow!("path is required"))?;
    Ok(path)
}

fn required_uuid(params: &Value, key: &str) -> Result<Uuid> {
    Ok(Uuid::parse_str(required_string(params, key)?)?)
}

fn required_string<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{key} is required"))
}

fn classify_error(message: &str) -> &str {
    if message.contains("IN_USE") {
        "IN_USE"
    } else if message.contains("SOURCE_DRIFT") {
        "SOURCE_DRIFT"
    } else if message.contains("No .code-intel") {
        "PROJECT_NOT_FOUND"
    } else if message.contains("not CURRENT") {
        "GRAPH_STALE"
    } else {
        "ANALYSIS_ERROR"
    }
}

fn default_socket() -> PathBuf {
    PathBuf::from(DEFAULT_SOCKET)
}

fn default_long_poll() -> u64 {
    25
}

fn default_registration_state() -> String {
    "PENDING".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeintel_core::create_project_manifest;
    use tempfile::tempdir;

    #[test]
    fn same_manifest_opens_as_same_project_with_new_instance_registry() {
        let dir = tempdir().unwrap();
        let manifest = create_project_manifest(dir.path(), "Test", "test").unwrap();
        std::env::set_var("CODEINTEL_DATA_DIR", dir.path().join("data"));
        fs::write(dir.path().join("app.py"), "def main():\n    return 1\n").unwrap();
        let (_, _, entry, _) = open_project(dir.path()).unwrap();
        assert_eq!(entry.project_id, manifest.project_id);
    }

    #[test]
    fn coverage_output_is_sampled_instead_of_losing_the_whole_response() {
        let coverage = Coverage {
            semantic: "COMPLETE".into(),
            framework: "COMPLETE".into(),
            dynamic_boundaries: (0..45)
                .map(|index| format!("boundary-{index}-{}", "x".repeat(500)))
                .collect(),
            ..Coverage::default()
        };
        let bounded = bounded_coverage(&coverage);
        assert_eq!(bounded["dynamic_boundaries"].as_array().unwrap().len(), 5);
        assert_eq!(bounded["items_remaining"]["dynamic_boundaries"], 40);
        assert!(
            bounded["dynamic_boundaries"][0]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= 241
        );
    }

    #[test]
    fn remote_graph_result_matches_ephemeral_hub_contract() {
        let graph = json!({
            "nodes": [],
            "edges": [],
            "bounded": true,
            "ephemeral": true,
            "source_bodies_included": false,
        });
        let value = remote_structured_result(
            "graph_visualization",
            json!({"source_revision_id": "revision", "result": graph}),
        )
        .unwrap();
        assert!(value.get("result").is_none());
        assert_eq!(value["source_bodies_included"], false);
    }

    #[test]
    fn project_hub_secret_is_local_only_and_owner_readable() {
        let dir = tempdir().unwrap();
        create_project_manifest(dir.path(), "Test", "test").unwrap();
        configure_project_hub_binding(
            dir.path(),
            Some("https://hub.example"),
            "CONNECTED",
            false,
            Some("ci_proj_v1_secret"),
        )
        .unwrap();
        let local = dir.path().join(".code-intel/local");
        assert_eq!(
            fs::read_to_string(local.join("project-access.token")).unwrap(),
            "ci_proj_v1_secret"
        );
        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(&local).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(local.join("project-access.token"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(local.join("hub-binding.yaml"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(
            fs::read_to_string(dir.path().join(".code-intel/.gitignore"))
                .unwrap()
                .lines()
                .any(|line| line == "local/")
        );
    }
}
