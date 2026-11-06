use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use fs2::FileExt;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};
use uuid::Uuid;
use walkdir::{DirEntry, WalkDir};

pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const INDEXER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ROOT_MAPPING_VERSION: u32 = 1;
pub const MANIFEST_DIR: &str = ".code-intel";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub project_id: Uuid,
    #[serde(default)]
    pub parent_project_id: Option<Uuid>,
    pub slug: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub hub: HubConfig,
    #[serde(default)]
    pub source: SourceConfig,
    #[serde(default)]
    pub wiki: WikiConfig,
    #[serde(default)]
    pub languages: LanguagesConfig,
    #[serde(default)]
    pub frameworks: FrameworksConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub resources: ResourceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    #[serde(default = "default_hub_mode")]
    pub mode: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_project_credential_ref")]
    pub credential_ref: String,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            mode: default_hub_mode(),
            url: None,
            credential_ref: default_project_credential_ref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    #[serde(default = "dot")]
    pub primary_root: String,
    #[serde(default)]
    pub related_roots: Vec<RelatedRoot>,
    #[serde(default)]
    pub follow_symlinks: bool,
    #[serde(default)]
    pub git: GitConfig,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            primary_root: dot(),
            related_roots: vec![],
            follow_symlinks: false,
            git: GitConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedRoot {
    pub path: String,
    pub role: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default = "default_index_mode")]
    pub index_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    #[serde(default = "default_git_mode")]
    pub mode: String,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            mode: default_git_mode(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiConfig {
    #[serde(default = "default_wiki_mode")]
    pub mode: String,
    #[serde(default)]
    pub wiki_id: Option<String>,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default = "default_wiki_binding_status")]
    pub binding_status: String,
}

impl Default for WikiConfig {
    fn default() -> Self {
        Self {
            mode: default_wiki_mode(),
            wiki_id: None,
            local_path: None,
            binding_status: default_wiki_binding_status(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguagesConfig {
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    #[serde(default)]
    pub priority: Vec<String>,
}

impl Default for LanguagesConfig {
    fn default() -> Self {
        Self {
            auto_detect: true,
            priority: vec![
                "php".into(),
                "typescript".into(),
                "javascript".into(),
                "python".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworksConfig {
    #[serde(default = "default_true")]
    pub auto_detect: bool,
}

impl Default for FrameworksConfig {
    fn default() -> Self {
        Self { auto_detect: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default = "default_workspace_mode")]
    pub mode: String,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            mode: default_workspace_mode(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    #[serde(default = "default_resource_profile")]
    pub profile: String,
    #[serde(default = "one")]
    pub max_heavy_jobs: u32,
    #[serde(default = "one")]
    pub max_lsp_processes: u32,
    #[serde(default = "project_soft")]
    pub project_soft_mb: u64,
    #[serde(default = "project_hard")]
    pub project_hard_mb: u64,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            profile: default_resource_profile(),
            max_heavy_jobs: one(),
            max_lsp_processes: one(),
            project_soft_mb: project_soft(),
            project_hard_mb: project_hard(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub ignore: IgnoreConfig,
    #[serde(default)]
    pub quality: QualityPolicy,
    #[serde(default)]
    pub checks: ChecksPolicy,
    #[serde(default)]
    pub smoke: SmokePolicy,
    #[serde(default)]
    pub runtime: RuntimePolicy,
    #[serde(default)]
    pub security_scenarios: SecurityScenariosPolicy,
    #[serde(default)]
    pub remote_analysis: RemoteAnalysisPolicy,
    #[serde(default)]
    pub storage: StoragePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScenariosPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_security_mode")]
    pub mode: String,
    #[serde(default = "default_security_environment")]
    pub environment: String,
    #[serde(default)]
    pub authorization: SecurityAuthorization,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default = "default_true")]
    pub deny_production: bool,
    #[serde(default)]
    pub allow_state_changing: bool,
    #[serde(default = "default_security_max_requests")]
    pub max_requests: u32,
    #[serde(default = "one")]
    pub max_concurrency: u32,
    #[serde(default = "default_security_timeout")]
    pub timeout_sec: u64,
    #[serde(default = "default_security_max_response")]
    pub max_response_bytes: usize,
    #[serde(default = "default_true")]
    pub local_evidence_only: bool,
    #[serde(default = "default_true")]
    pub require_correlation: bool,
    #[serde(default)]
    pub actors: Vec<SecurityActor>,
    #[serde(default)]
    pub endpoint_inventory: Vec<String>,
    #[serde(default)]
    pub scenarios: Vec<SecurityScenario>,
    #[serde(default)]
    pub exclusions: Vec<SecurityScenarioExclusion>,
}

impl Default for SecurityScenariosPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_security_mode(),
            environment: default_security_environment(),
            authorization: SecurityAuthorization::default(),
            base_url: None,
            allowed_hosts: vec![],
            deny_production: true,
            allow_state_changing: false,
            max_requests: default_security_max_requests(),
            max_concurrency: 1,
            timeout_sec: default_security_timeout(),
            max_response_bytes: default_security_max_response(),
            local_evidence_only: true,
            require_correlation: true,
            actors: vec![],
            endpoint_inventory: vec![],
            scenarios: vec![],
            exclusions: vec![],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecurityAuthorization {
    #[serde(default = "default_authorization_status")]
    pub status: String,
    #[serde(default)]
    pub scope_ref: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityActor {
    pub id: String,
    #[serde(default)]
    pub bearer_token_env: Option<String>,
    #[serde(default)]
    pub headers_from_env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScenario {
    pub id: String,
    pub endpoint_id: String,
    pub case: String,
    #[serde(default = "default_http_method")]
    pub method: String,
    pub path: String,
    /// Exact graph route label (for example `GET /api/orders/{id}`). This is deliberately
    /// separate from `path`, which can contain a concrete lab object identifier.
    #[serde(default)]
    pub route_symbol: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub json_body: Option<serde_json::Value>,
    #[serde(default = "default_statuses")]
    pub expect_status: Vec<u16>,
    #[serde(default)]
    pub expect_terminal_status: Vec<u16>,
    #[serde(default = "one")]
    pub repeat: u32,
    #[serde(default)]
    pub parallel_burst: bool,
    #[serde(default)]
    pub allow_state_change: bool,
    #[serde(default)]
    pub reuse_idempotency_key: bool,
    #[serde(default)]
    pub cost_probe: Option<SecurityCostProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityCostProbe {
    pub path: String,
    pub actor: String,
    pub json_pointer: String,
    pub max_delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScenarioExclusion {
    pub endpoint_id: String,
    pub case: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreConfig {
    #[serde(default = "default_ignores")]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self {
            exclude: default_ignores(),
            include: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityPolicy {
    #[serde(default = "default_true")]
    pub auto_risk: bool,
    #[serde(default = "default_true")]
    pub graph_is_advisory: bool,
    #[serde(default = "default_true")]
    pub absence_requires_coverage: bool,
    #[serde(default)]
    pub dead_code_auto_delete: bool,
    #[serde(default = "default_true")]
    pub final_chat_report: bool,
    #[serde(default = "three")]
    pub max_auto_repair_cycles: u32,
}

impl Default for QualityPolicy {
    fn default() -> Self {
        Self {
            auto_risk: true,
            graph_is_advisory: true,
            absence_requires_coverage: true,
            dead_code_auto_delete: false,
            final_chat_report: true,
            max_auto_repair_cycles: 3,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChecksPolicy {
    #[serde(default)]
    pub commands: Vec<CheckCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckCommand {
    pub id: String,
    pub kind: String,
    pub argv: Vec<String>,
    #[serde(default = "dot")]
    pub cwd: String,
    #[serde(default = "default_check_timeout")]
    pub timeout_sec: u64,
    #[serde(default)]
    pub required_for: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmokePolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<SmokeEndpoint>,
    #[serde(default = "default_true")]
    pub compare_latency: bool,
}

impl Default for SmokePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: None,
            endpoints: vec![],
            compare_latency: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmokeEndpoint {
    pub id: String,
    #[serde(default = "default_http_method")]
    pub method: String,
    pub path: String,
    #[serde(default = "default_statuses")]
    pub expect_status: Vec<u16>,
    #[serde(default)]
    pub max_latency_ms: Option<u64>,
    #[serde(default)]
    pub body_contains: Option<String>,
    #[serde(default)]
    pub header_exists: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_runtime_mode")]
    pub mode: String,
    #[serde(default = "default_runtime_adapters")]
    pub adapters: Vec<String>,
    #[serde(default = "three")]
    pub raw_trace_retention_days: u32,
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_runtime_mode(),
            adapters: default_runtime_adapters(),
            raw_trace_retention_days: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAnalysisPolicy {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub allow_source_bodies: bool,
    #[serde(default = "default_true")]
    pub allow_structured_live_queries: bool,
}

impl Default for RemoteAnalysisPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_source_bodies: false,
            allow_structured_live_queries: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePolicy {
    #[serde(default = "global_soft")]
    pub global_soft_mb: u64,
    #[serde(default = "global_hard")]
    pub global_hard_mb: u64,
    #[serde(default = "minimum_free")]
    pub minimum_free_disk_mb: u64,
    #[serde(default = "project_soft")]
    pub per_project_soft_mb: u64,
    #[serde(default = "project_hard")]
    pub per_project_hard_mb: u64,
    #[serde(default = "cache_cap")]
    pub raw_runtime_global_mb: u64,
    #[serde(default = "cache_cap")]
    pub content_cache_global_mb: u64,
}

impl Default for StoragePolicy {
    fn default() -> Self {
        Self {
            global_soft_mb: global_soft(),
            global_hard_mb: global_hard(),
            minimum_free_disk_mb: minimum_free(),
            per_project_soft_mb: project_soft(),
            per_project_hard_mb: project_hard(),
            raw_runtime_global_mb: cache_cap(),
            content_cache_global_mb: cache_cap(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileInventoryEntry {
    pub logical_path: String,
    pub size: u64,
    pub mode: u32,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRevision {
    pub source_revision_id: String,
    pub inventory_hash: String,
    pub policy_hash: String,
    pub indexer_version: String,
    pub root_mapping_version: u32,
    pub files: Vec<FileInventoryEntry>,
    pub volatile_files: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub evidence_id: Uuid,
    pub project_id: Uuid,
    pub instance_id: Uuid,
    pub source_revision_id: String,
    pub change_run_id: Option<Uuid>,
    pub producer: String,
    pub producer_version: String,
    pub origin: String,
    pub confidence: f64,
    pub coverage: serde_json::Value,
    pub unsupported_areas: Vec<String>,
    pub unknowns: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub policy_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeRunStatus {
    Open,
    Preflighted,
    Editing,
    Reviewing,
    Verifying,
    Gated,
    Closed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRun {
    pub change_run_id: Uuid,
    pub project_id: Uuid,
    pub instance_id: Uuid,
    pub agent_client: Option<String>,
    pub agent_session_id: Option<String>,
    pub task_summary: String,
    pub before_source_revision_id: String,
    pub after_source_revision_id: Option<String>,
    pub risk_before: RiskAssessment,
    pub risk_after: Option<RiskAssessment>,
    pub required_checks: Vec<String>,
    pub evidence_ids: Vec<Uuid>,
    pub finding_ids: Vec<Uuid>,
    pub quality_run_id: Option<Uuid>,
    pub manual_snapshot_reference: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub closure_reason: Option<String>,
    pub status: ChangeRunStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskAssessment {
    pub risk_score: u8,
    pub risk_level: String,
    pub tier: String,
    pub reasons: Vec<String>,
    pub required_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateInput {
    pub evidence_revision_id: String,
    pub current_source_revision_id: String,
    pub required_checks: Vec<String>,
    pub completed_checks: Vec<String>,
    #[serde(default)]
    pub failed_checks: Vec<String>,
    #[serde(default)]
    pub blocking_findings: Vec<String>,
    #[serde(default)]
    pub blocking_unknowns: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub graph_state: String,
    #[serde(default)]
    pub unsupported_required_areas: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateResult {
    pub verdict: String,
    pub currentness: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPaths {
    pub project_root: PathBuf,
    pub manifest_dir: PathBuf,
    pub data_dir: PathBuf,
    pub graph_path: PathBuf,
    pub content_cache: PathBuf,
    pub reports_dir: PathBuf,
    pub spool_dir: PathBuf,
    pub locks_dir: PathBuf,
}

pub fn data_root() -> PathBuf {
    std::env::var_os("CODEINTEL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/code-intelligence"))
}

pub fn project_paths(root: &Path, manifest: &ProjectManifest, instance_id: Uuid) -> ProjectPaths {
    let data_dir = data_root()
        .join("projects")
        .join(manifest.project_id.to_string())
        .join(instance_id.to_string());
    ProjectPaths {
        project_root: root.to_path_buf(),
        manifest_dir: root.join(MANIFEST_DIR),
        graph_path: data_dir.join("graph.sqlite3"),
        content_cache: data_dir.join("content-cache"),
        reports_dir: data_dir.join("reports"),
        spool_dir: data_dir.join("spool"),
        locks_dir: data_dir.join("locks"),
        data_dir,
    }
}

pub fn ensure_project_dirs(paths: &ProjectPaths) -> Result<()> {
    for path in [
        &paths.data_dir,
        &paths.content_cache,
        &paths.reports_dir,
        &paths.spool_dir,
        &paths.locks_dir,
    ] {
        fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    }
    Ok(())
}

pub fn discover_project(start: &Path) -> Result<PathBuf> {
    let start = fs::canonicalize(start).with_context(|| format!("resolve {}", start.display()))?;
    for ancestor in start.ancestors() {
        if ancestor.join(MANIFEST_DIR).join("project.yaml").is_file() {
            return Ok(ancestor.to_path_buf());
        }
    }
    Err(anyhow!(
        "No .code-intel/project.yaml found from {}",
        start.display()
    ))
}

pub fn load_project(root: &Path) -> Result<(ProjectManifest, Policy)> {
    let manifest_path = root.join(MANIFEST_DIR).join("project.yaml");
    let policy_path = root.join(MANIFEST_DIR).join("policy.yaml");
    let manifest: ProjectManifest = serde_yaml::from_slice(
        &fs::read(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .context("parse project manifest")?;
    if manifest.schema_version != 1 {
        return Err(anyhow!(
            "Unsupported project manifest schema {}",
            manifest.schema_version
        ));
    }
    if !manifest.enabled {
        return Err(anyhow!("Code Intelligence is disabled for this project"));
    }
    let policy = if policy_path.is_file() {
        serde_yaml::from_slice(&fs::read(&policy_path)?).context("parse project policy")?
    } else {
        Policy::default()
    };
    if policy.quality.dead_code_auto_delete {
        return Err(anyhow!(
            "dead_code_auto_delete=true violates the V1 safety contract"
        ));
    }
    Ok((manifest, policy))
}

#[derive(Debug, Clone)]
pub struct ProjectInitialization {
    pub manifest: ProjectManifest,
    pub created: bool,
}

/// Create the portable identity exactly once. The short file lock makes concurrent `init`
/// calls converge on the same manifest instead of racing two UUIDs into the project.
pub fn initialize_project(
    root: &Path,
    name: &str,
    slug: &str,
    wiki: WikiConfig,
) -> Result<ProjectInitialization> {
    if !root.is_dir() {
        return Err(anyhow!(
            "Project root is not a directory: {}",
            root.display()
        ));
    }
    let canonical_root = fs::canonicalize(root)?;
    let root_key = hex::encode(Sha256::digest(canonical_root.to_string_lossy().as_bytes()));
    let init_locks = std::env::temp_dir().join("code-intelligence-init-locks");
    fs::create_dir_all(&init_locks)?;
    let lock_path = init_locks.join(format!("{root_key}.lock"));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    lock.lock_exclusive()?;
    let manifest_dir = root.join(MANIFEST_DIR);
    fs::create_dir_all(&manifest_dir)?;
    let manifest_path = manifest_dir.join("project.yaml");
    if manifest_path.is_file() {
        let manifest: ProjectManifest = serde_yaml::from_slice(&fs::read(&manifest_path)?)
            .context("parse existing project manifest")?;
        write_portable_project_files(root, &manifest)?;
        return Ok(ProjectInitialization {
            manifest,
            created: false,
        });
    }
    let manifest = ProjectManifest {
        schema_version: 1,
        project_id: Uuid::new_v4(),
        parent_project_id: None,
        slug: slug.to_string(),
        name: name.to_string(),
        enabled: true,
        hub: HubConfig::default(),
        source: SourceConfig::default(),
        wiki,
        languages: LanguagesConfig::default(),
        frameworks: FrameworksConfig::default(),
        workspace: WorkspaceConfig::default(),
        resources: ResourceConfig::default(),
    };
    atomic_write(
        &manifest_dir.join("project.yaml"),
        serde_yaml::to_string(&manifest)?.as_bytes(),
    )?;
    atomic_write(
        &manifest_dir.join("policy.yaml"),
        serde_yaml::to_string(&detect_policy(root))?.as_bytes(),
    )?;
    atomic_write(
        &manifest_dir.join("AGENT_POLICY.md"),
        b"# Project-specific Code Intelligence policy\n\nNo project-specific additions. The canonical policy is supplied by the Hub/installer.\n",
    )?;
    write_portable_project_files(root, &manifest)?;
    Ok(ProjectInitialization {
        manifest,
        created: true,
    })
}

pub fn write_portable_project_files(root: &Path, manifest: &ProjectManifest) -> Result<()> {
    let dir = root.join(MANIFEST_DIR);
    let context = dir.join("context");
    let portability = dir.join("portability");
    let local = dir.join("local");
    fs::create_dir_all(&context)?;
    fs::create_dir_all(&portability)?;
    fs::create_dir_all(&local)?;
    #[cfg(unix)]
    fs::set_permissions(&local, std::fs::Permissions::from_mode(0o700))?;
    let ignore_path = dir.join(".gitignore");
    let mut ignore = fs::read_to_string(&ignore_path).unwrap_or_default();
    if !ignore.lines().any(|line| line.trim() == "local/") {
        if !ignore.is_empty() && !ignore.ends_with('\n') {
            ignore.push('\n');
        }
        ignore.push_str("local/\n");
        atomic_write(&ignore_path, ignore.as_bytes())?;
    }
    atomic_write_if_missing(
        &dir.join("SKILL.md"),
        format!(
            "---\nname: code-intelligence-project\ndescription: Project-local Code Intelligence workflow for {}.\n---\n\n# Project Code Intelligence\n\n1. Open this project through the local Code Intelligence MCP.\n2. Source is canonical; graph output is discovery evidence only.\n3. Hub is optional. Missing/revoked Hub access never disables local analysis.\n4. Before significant edits run preflight and inspect the actual source.\n5. After edits run review, required verification and Quality Gate for the exact revision.\n6. Report the local Gate and Hub sync as separate results.\n7. Never commit `.code-intel/local/`, graph databases, caches, traces or credentials.\n8. Security scenarios require the installed conditional reference and may run only from a predeclared policy against an explicitly authorized lab/staging origin.\n9. Security evidence remains local and excludes response bodies and credential values.\n",
            manifest.name
        )
        .as_bytes(),
    )?;
    atomic_write_if_missing(
        &dir.join("README_AGENT.md"),
        b"# Code Intelligence in this project\n\nThe portable files in this directory are sufficient to rebuild local intelligence after clone. Run `codeintel open .`; do not create a new identity unless this is a fork. Hub access is optional and secrets belong only in `.code-intel/local/`.\n",
    )?;
    atomic_write_if_missing(
        &context.join("PROJECT_CONTEXT.md"),
        format!(
            "# {} — portable technical context\n\nProject ID: `{}`\n\nThis bounded context travels with source. Current code and a locally rebuilt graph remain the live technical truth.\n",
            manifest.name, manifest.project_id
        )
        .as_bytes(),
    )?;
    atomic_write_if_missing(
        &context.join("ARCHITECTURE.md"),
        b"# Architecture\n\nGenerated architecture has not yet been validated. Treat this file as a stale hint until its revision matches the current source.\n",
    )?;
    atomic_write_if_missing(
        &context.join("ENTRYPOINTS.yaml"),
        b"schema_version: 1\nentrypoints: []\n",
    )?;
    atomic_write_if_missing(
        &context.join("KNOWN_UNKNOWNS.md"),
        b"# Known unknowns\n\n- Initial architecture and dynamic coverage are not yet validated.\n",
    )?;
    atomic_write_if_missing(
        &context.join("LAST_VERIFIED.json"),
        b"{\n  \"source_revision_id\": null,\n  \"quality_gate\": \"UNKNOWN\",\n  \"freshness\": \"STALE\",\n  \"produced_at\": null\n}\n",
    )?;
    atomic_write_if_missing(
        &portability.join("INDEX_CONFIG.yaml"),
        serde_yaml::to_string(&serde_json::json!({
            "schema_version": 1,
            "primary_root": manifest.source.primary_root,
            "related_roots": manifest.source.related_roots,
            "graph_database_tracked": false,
            "rebuild_command": "codeintel open ."
        }))?
        .as_bytes(),
    )?;
    atomic_write_if_missing(
        &portability.join("FRAMEWORK_HINTS.yaml"),
        b"schema_version: 1\nauto_detect: true\nvalidated: false\nframeworks: []\n",
    )?;
    atomic_write_if_missing(
        &portability.join("GRAPH_BUILD_INFO.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "indexer_version": INDEXER_VERSION,
            "root_mapping_version": ROOT_MAPPING_VERSION,
            "derived_graph_tracked": false,
            "source_revision_required_for_current": true
        }))?
        .as_bytes(),
    )?;
    atomic_write_if_missing(
        &dir.join("hub.example.yaml"),
        b"mode: optional\nurl: https://code-hub.example\ncredential_ref: local/project-access.token\n# Never place a token in this tracked file.\n",
    )?;
    Ok(())
}

fn atomic_write_if_missing(path: &Path, contents: &[u8]) -> Result<()> {
    if !path.exists() {
        atomic_write(path, contents)?;
    }
    Ok(())
}

pub fn create_project_manifest(root: &Path, name: &str, slug: &str) -> Result<ProjectManifest> {
    let result = initialize_project(root, name, slug, WikiConfig::default())?;
    if !result.created {
        return Err(anyhow!("Project is already initialized"));
    }
    Ok(result.manifest)
}

/// Conservative Wiki discovery: only explicit Code Intelligence metadata or uniquely named
/// LLM-Wiki directories are bound. Ambiguous candidates never become an arbitrary binding.
pub fn discover_wiki(root: &Path) -> WikiConfig {
    let candidates: Vec<PathBuf> = ["LLM-WIKI", "llm-wiki", ".llm-wiki"]
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.is_dir())
        .collect();
    match candidates.as_slice() {
        [] => WikiConfig::default(),
        [path] => WikiConfig {
            mode: "local".into(),
            wiki_id: None,
            local_path: path
                .strip_prefix(root)
                .ok()
                .map(|value| value.to_string_lossy().to_string()),
            binding_status: "BOUND".into(),
        },
        _ => WikiConfig {
            binding_status: "UNRESOLVED".into(),
            ..WikiConfig::default()
        },
    }
}

/// Detect repeatable checks only when the repository already advertises them. The initializer
/// never invents smoke assertions or commands that are not represented by project files.
pub fn detect_policy(root: &Path) -> Policy {
    let mut policy = Policy::default();
    let required = vec![
        "LOW".into(),
        "MEDIUM".into(),
        "HIGH".into(),
        "CRITICAL".into(),
    ];
    if root.join("Cargo.toml").is_file() {
        policy.checks.commands.push(CheckCommand {
            id: "rust-tests".into(),
            kind: "test".into(),
            argv: vec!["cargo".into(), "test".into(), "--workspace".into()],
            cwd: ".".into(),
            timeout_sec: 900,
            required_for: required.clone(),
        });
    }
    if root.join("pytest.ini").is_file()
        || root.join("pyproject.toml").is_file() && root.join("tests").is_dir()
    {
        policy.checks.commands.push(CheckCommand {
            id: "python-tests".into(),
            kind: "test".into(),
            argv: vec!["python3".into(), "-m".into(), "pytest".into(), "-q".into()],
            cwd: ".".into(),
            timeout_sec: 900,
            required_for: required.clone(),
        });
    }
    if let Ok(package) = fs::read(root.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .ok_or_else(|| anyhow!("package metadata unavailable"))
    {
        if let Some(scripts) = package
            .get("scripts")
            .and_then(serde_json::Value::as_object)
        {
            for (id, kind) in [
                ("test", "test"),
                ("lint", "lint"),
                ("typecheck", "typecheck"),
                ("build", "build"),
            ] {
                if scripts.contains_key(id) {
                    policy.checks.commands.push(CheckCommand {
                        id: format!("node-{id}"),
                        kind: kind.into(),
                        argv: vec!["npm".into(), "run".into(), id.into()],
                        cwd: ".".into(),
                        timeout_sec: 900,
                        required_for: required.clone(),
                    });
                }
            }
        }
    }
    if root.join("vendor/bin/phpunit").is_file() || root.join("phpunit.xml").is_file() {
        policy.checks.commands.push(CheckCommand {
            id: "php-tests".into(),
            kind: "test".into(),
            argv: vec!["./vendor/bin/phpunit".into()],
            cwd: ".".into(),
            timeout_sec: 900,
            required_for: required,
        });
    }
    policy
}

pub fn fork_project(root: &Path) -> Result<(Uuid, Uuid)> {
    let (mut manifest, _) = load_project(root)?;
    let old = manifest.project_id;
    manifest.project_id = Uuid::new_v4();
    manifest.parent_project_id = Some(old);
    atomic_write(
        &root.join(MANIFEST_DIR).join("project.yaml"),
        serde_yaml::to_string(&manifest)?.as_bytes(),
    )?;
    atomic_write(
        &root.join(MANIFEST_DIR).join("context/PROJECT_CONTEXT.md"),
        format!(
            "# {} — portable technical context\n\nProject ID: `{}`\nParent Project ID: `{}`\n\nThis fork has a new logical identity. Rebuild the graph locally and create a new Hub Project.\n",
            manifest.name, manifest.project_id, old
        )
        .as_bytes(),
    )?;
    Ok((old, manifest.project_id))
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("Path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        Uuid::new_v4()
    ));
    {
        let mut file = File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn policy_hash(root: &Path, policy: &Policy) -> Result<String> {
    let bytes = serde_yaml::to_string(policy)?.into_bytes();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.update(ROOT_MAPPING_VERSION.to_be_bytes());
    if let Ok(agent_policy) = fs::read(root.join(MANIFEST_DIR).join("AGENT_POLICY.md")) {
        hasher.update(agent_policy);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for raw in patterns {
        let normalized = raw.trim_start_matches('/').trim_end_matches('/');
        if normalized.is_empty() {
            continue;
        }
        let pattern = if raw.ends_with('/') && !normalized.contains('/') {
            format!("**/{normalized}/**")
        } else if raw.ends_with('/') {
            format!("{normalized}/**")
        } else if normalized.contains('/') {
            normalized.to_string()
        } else {
            format!("**/{normalized}")
        };
        builder.add(Glob::new(&pattern).with_context(|| format!("invalid ignore glob {raw}"))?);
    }
    Ok(builder.build()?)
}

fn excluded(
    entry: &DirEntry,
    root: &Path,
    excludes: &GlobSet,
    includes: &GlobSet,
    include_patterns: &[String],
) -> bool {
    let Ok(relative) = entry.path().strip_prefix(root) else {
        return true;
    };
    if relative.as_os_str().is_empty() {
        return false;
    }
    if includes.is_match(relative) {
        return false;
    }
    if entry.file_type().is_dir() {
        let prefix = format!("{}/", relative.to_string_lossy().replace('\\', "/"));
        if include_patterns.iter().any(|pattern| {
            pattern
                .trim_start_matches('/')
                .trim_start_matches("**/")
                .starts_with(&prefix)
        }) {
            return false;
        }
    }
    excludes.is_match(relative)
        || relative.components().any(|component| {
            matches!(component, Component::Normal(value) if value == ".git" || value == ".code-intel")
        })
}

pub fn enumerate_inventory(
    root: &Path,
    policy: &Policy,
) -> Result<(Vec<FileInventoryEntry>, Vec<String>)> {
    let excludes = build_globset(&policy.ignore.exclude)?;
    let includes = build_globset(&policy.ignore.include)?;
    let mut files = Vec::new();
    let mut volatile = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !excluded(entry, root, &excludes, &includes, &policy.ignore.include));
    for item in walker {
        let entry = item?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if excludes.is_match(&relative) && !includes.is_match(&relative) {
            continue;
        }
        match safe_read(entry.path(), 3) {
            Ok((bytes, metadata)) => {
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                files.push(FileInventoryEntry {
                    logical_path: relative,
                    size: metadata.len(),
                    mode: metadata_mode(&metadata),
                    content_sha256: hex::encode(hasher.finalize()),
                });
            }
            Err(ReadSafetyError::Volatile) => volatile.push(relative),
            Err(ReadSafetyError::Io(error)) => return Err(error.into()),
        }
    }
    files.sort_by(|a, b| a.logical_path.cmp(&b.logical_path));
    volatile.sort();
    Ok((files, volatile))
}

pub fn compute_source_revision(root: &Path, policy: &Policy) -> Result<SourceRevision> {
    let (files, volatile_files) = enumerate_inventory(root, policy)?;
    let policy_hash = policy_hash(root, policy)?;
    let inventory_bytes = serde_json::to_vec(&files)?;
    let inventory_hash = hex::encode(Sha256::digest(&inventory_bytes));
    let mut revision = Sha256::new();
    revision.update(&inventory_bytes);
    revision.update(policy_hash.as_bytes());
    revision.update(INDEXER_VERSION.as_bytes());
    revision.update(ROOT_MAPPING_VERSION.to_be_bytes());
    Ok(SourceRevision {
        source_revision_id: hex::encode(revision.finalize()),
        inventory_hash,
        policy_hash,
        indexer_version: INDEXER_VERSION.to_string(),
        root_mapping_version: ROOT_MAPPING_VERSION,
        files,
        volatile_files,
        created_at: Utc::now(),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum ReadSafetyError {
    #[error("file remained volatile during bounded safe reads")]
    Volatile,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn safe_read(
    path: &Path,
    retries: usize,
) -> std::result::Result<(Vec<u8>, Metadata), ReadSafetyError> {
    for attempt in 0..=retries {
        let before = fs::metadata(path)?;
        let before_modified = before.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let mut bytes = Vec::with_capacity(before.len().min(16 * 1024 * 1024) as usize);
        File::open(path)?.read_to_end(&mut bytes)?;
        let after = fs::metadata(path)?;
        let after_modified = after.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if before.len() == after.len() && before_modified == after_modified {
            return Ok((bytes, after));
        }
        if attempt < retries {
            thread::sleep(Duration::from_millis(50 * (attempt as u64 + 1)));
        }
    }
    Err(ReadSafetyError::Volatile)
}

#[cfg(unix)]
fn metadata_mode(metadata: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn metadata_mode(metadata: &Metadata) -> u32 {
    if metadata.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

pub fn cache_content(cache_root: &Path, content_sha256: &str, bytes: &[u8]) -> Result<PathBuf> {
    let shard = &content_sha256[..2];
    let path = cache_root.join(shard).join(format!("{content_sha256}.gz"));
    if path.exists() {
        return Ok(path);
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes)?;
    atomic_write(&path, &encoder.finish()?)?;
    Ok(path)
}

pub fn read_cached_content(cache_root: &Path, content_sha256: &str) -> Result<Vec<u8>> {
    let path = cache_root
        .join(&content_sha256[..2])
        .join(format!("{content_sha256}.gz"));
    let mut decoder = GzDecoder::new(File::open(path)?);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn detect_languages(files: &[FileInventoryEntry]) -> BTreeMap<String, usize> {
    let mut result = BTreeMap::new();
    for file in files {
        let language = match Path::new(&file.logical_path)
            .extension()
            .and_then(|value| value.to_str())
        {
            Some("php") => Some("php"),
            Some("js" | "jsx" | "mjs" | "cjs") => Some("javascript"),
            Some("ts" | "tsx" | "mts" | "cts") => Some("typescript"),
            Some("py" | "pyi") => Some("python"),
            Some("cs") => Some("csharp"),
            Some("rs") => Some("rust"),
            _ => None,
        };
        if let Some(language) = language {
            *result.entry(language.to_string()).or_insert(0) += 1;
        }
    }
    result
}

pub fn assess_risk(
    paths: &[String],
    intent: &str,
    coverage_partial: bool,
    has_tests: bool,
) -> RiskAssessment {
    let mut score: i32 = 8;
    let mut reasons = BTreeSet::new();
    let joined = format!("{} {}", paths.join(" "), intent).to_lowercase();
    let mut add = |points: i32, reason: &str| {
        score += points;
        reasons.insert(reason.to_string());
    };
    if paths.len() > 8 {
        add(12, "broad file change");
    }
    if contains_any(&joined, &["route", "controller", "endpoint", "api/"]) {
        add(18, "public route or API surface");
    }
    if contains_any(
        &joined,
        &["auth", "login", "permission", "middleware", "token"],
    ) {
        add(24, "authentication or authorization path");
    }
    if contains_any(
        &joined,
        &[
            "migration",
            "schema",
            "database",
            "model",
            "repository",
            "sql",
        ],
    ) {
        add(22, "database read/write or schema path");
    }
    if contains_any(&joined, &["payment", "billing", "invoice", "checkout"]) {
        add(28, "payment or billing path");
    }
    if contains_any(&joined, &["shared", "common", "util", "core/"]) {
        add(16, "shared or central utility");
    }
    if contains_any(
        &joined,
        &["queue", "job", "event", "listener", "cron", "schedule"],
    ) {
        add(18, "asynchronous or scheduled flow");
    }
    if contains_any(&joined, &["config", ".env", "docker", "systemd", "nginx"]) {
        add(14, "runtime configuration or service boundary");
    }
    if contains_any(
        &joined,
        &["reflection", "dynamic", "callback", "__call", "eval("],
    ) {
        add(20, "dynamic dispatch boundary");
    }
    if coverage_partial {
        add(15, "semantic or graph coverage is partial");
    }
    if !has_tests {
        add(10, "mapped tests are unavailable");
    }
    if paths.len() == 1 && contains_any(&joined, &[".css", ".scss", ".md"]) {
        score = score.min(24);
        reasons.insert("localized presentation or documentation change".into());
    }
    score = score.clamp(0, 100);
    let (level, tier) = match score {
        0..=24 => ("LOW", "TIER_0"),
        25..=49 => ("MEDIUM", "TIER_1"),
        50..=74 => ("HIGH", "TIER_2"),
        _ => ("CRITICAL", "TIER_3"),
    };
    let mut checks = vec![
        "actual_source_verify".to_string(),
        "structural_review".to_string(),
    ];
    if score >= 25 {
        checks.push("targeted_tests".into());
    }
    if contains_any(
        &joined,
        &["route", "controller", "endpoint", "api/", ".css"],
    ) {
        checks.push("http_smoke".into());
    }
    if score >= 50 {
        checks.push("semantic_resolution".into());
    }
    if score >= 75 || contains_any(&joined, &["dynamic", "payment", "queue", "callback"]) {
        checks.push("runtime_trace".into());
    }
    if contains_any(
        &joined,
        &["sql", "database", "payment", "file write", "secret"],
    ) {
        checks.push("dataflow".into());
    }
    checks.sort();
    checks.dedup();
    RiskAssessment {
        risk_score: score as u8,
        risk_level: level.into(),
        tier: tier.into(),
        reasons: reasons.into_iter().collect(),
        required_checks: checks,
    }
}

pub fn evaluate_gate(input: &GateInput) -> GateResult {
    if input.evidence_revision_id != input.current_source_revision_id {
        return GateResult {
            verdict: "UNKNOWN".into(),
            currentness: "UNVERIFIED_CHANGE".into(),
            reasons: vec!["evidence belongs to another source revision".into()],
            warnings: input.warnings.clone(),
        };
    }
    if input.graph_state != "CURRENT" {
        return GateResult {
            verdict: "UNKNOWN".into(),
            currentness: "CURRENT".into(),
            reasons: vec![format!("graph state is {}", input.graph_state)],
            warnings: input.warnings.clone(),
        };
    }
    if !input.failed_checks.is_empty() || !input.blocking_findings.is_empty() {
        let mut reasons: Vec<String> = input
            .failed_checks
            .iter()
            .map(|check| format!("required check failed: {check}"))
            .collect();
        reasons.extend(input.blocking_findings.clone());
        return GateResult {
            verdict: "FAIL".into(),
            currentness: "CURRENT".into(),
            reasons,
            warnings: input.warnings.clone(),
        };
    }
    let completed: BTreeSet<_> = input.completed_checks.iter().collect();
    let mut unknowns: Vec<String> = input
        .required_checks
        .iter()
        .filter(|check| !completed.contains(check))
        .map(|check| format!("required check missing: {check}"))
        .collect();
    unknowns.extend(input.blocking_unknowns.clone());
    unknowns.extend(
        input
            .unsupported_required_areas
            .iter()
            .map(|area| format!("unsupported required area: {area}")),
    );
    if !unknowns.is_empty() {
        return GateResult {
            verdict: "UNKNOWN".into(),
            currentness: "CURRENT".into(),
            reasons: unknowns,
            warnings: input.warnings.clone(),
        };
    }
    GateResult {
        verdict: if input.warnings.is_empty() {
            "PASS".into()
        } else {
            "PASS_WITH_WARNINGS".into()
        },
        currentness: "CURRENT".into(),
        reasons: vec![],
        warnings: input.warnings.clone(),
    }
}

pub fn absence_result(result_count: usize, supported_scope_complete: bool) -> serde_json::Value {
    serde_json::json!({
        "result_count": result_count,
        "absence_proven": result_count == 0 && supported_scope_complete,
        "coverage": if supported_scope_complete { "COMPLETE" } else { "PARTIAL" },
        "warning": if result_count == 0 && !supported_scope_complete {
            Some("No graph result is not proof that the relationship does not exist.")
        } else { None }
    })
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn default_true() -> bool {
    true
}
fn one() -> u32 {
    1
}
fn three() -> u32 {
    3
}
fn dot() -> String {
    ".".into()
}
fn default_index_mode() -> String {
    "source".into()
}
fn default_git_mode() -> String {
    "metadata_optional".into()
}
fn default_wiki_mode() -> String {
    "none".into()
}
fn default_hub_mode() -> String {
    "optional".into()
}
fn default_project_credential_ref() -> String {
    "local/project-access.token".into()
}

fn default_wiki_binding_status() -> String {
    "NONE".into()
}
fn default_workspace_mode() -> String {
    "live".into()
}
fn default_resource_profile() -> String {
    "ram_constrained".into()
}
fn default_check_timeout() -> u64 {
    900
}
fn default_http_method() -> String {
    "GET".into()
}
fn default_statuses() -> Vec<u16> {
    vec![200]
}
fn default_security_mode() -> String {
    "plan_only".into()
}
fn default_security_environment() -> String {
    "unclassified".into()
}
fn default_authorization_status() -> String {
    "not_granted".into()
}
fn default_security_max_requests() -> u32 {
    24
}
fn default_security_timeout() -> u64 {
    10
}
fn default_security_max_response() -> usize {
    16 * 1024
}
fn default_runtime_mode() -> String {
    "on_demand".into()
}
fn default_runtime_adapters() -> Vec<String> {
    vec!["generic_trace_ingest".into(), "http_trace".into()]
}
fn global_soft() -> u64 {
    2048
}
fn global_hard() -> u64 {
    3072
}
fn minimum_free() -> u64 {
    8192
}
fn project_soft() -> u64 {
    768
}
fn project_hard() -> u64 {
    1536
}
fn cache_cap() -> u64 {
    512
}
fn default_ignores() -> Vec<String> {
    [
        ".git/",
        ".code-intel/local/",
        "*.log",
        "tmp/",
        "cache/",
        "logs/",
        "node_modules/",
        ".next/",
        "dist/",
        "build/",
        "vendor/",
        "bootstrap/cache/",
        "storage/logs/",
        "storage/framework/",
        "__pycache__/",
        "venv/",
        ".venv/",
        ".pytest_cache/",
        ".ruff_cache/",
        "*.egg-info/",
        "*.pyc",
        "uploads/",
        "public/uploads/",
        "storage/app/public/",
        "target/",
        "release/artifacts/",
    ]
    .iter()
    .map(|value| value.to_string())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn revision_changes_without_git() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("app.py"), "print('a')\n").unwrap();
        let first = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        fs::write(dir.path().join("app.py"), "print('b')\n").unwrap();
        let second = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        assert_ne!(first.source_revision_id, second.source_revision_id);
    }

    #[test]
    fn ignored_files_do_not_change_revision() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("app.py"), "print('a')\n").unwrap();
        let first = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        fs::create_dir(dir.path().join("logs")).unwrap();
        fs::write(dir.path().join("logs/server.log"), "noise").unwrap();
        let second = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        assert_eq!(first.source_revision_id, second.source_revision_id);
    }

    #[test]
    fn portable_intelligence_metadata_does_not_invalidate_source_revision() {
        let dir = tempdir().unwrap();
        create_project_manifest(dir.path(), "Portable", "portable").unwrap();
        fs::write(dir.path().join("app.py"), "print('a')\n").unwrap();
        let first = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        fs::write(
            dir.path().join(".code-intel/context/LAST_VERIFIED.json"),
            r#"{"source_revision_id":"example","quality_gate":"PASS"}"#,
        )
        .unwrap();
        let second = compute_source_revision(dir.path(), &Policy::default()).unwrap();
        assert_eq!(first.source_revision_id, second.source_revision_id);
    }

    #[test]
    fn explicit_include_overrides_ignored_user_content_directory() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("uploads/source")).unwrap();
        fs::write(
            dir.path().join("uploads/source/handler.py"),
            "def handler(): pass",
        )
        .unwrap();
        let mut policy = Policy::default();
        policy.ignore.include = vec!["uploads/source/handler.py".into()];
        let (files, _) = enumerate_inventory(dir.path(), &policy).unwrap();
        assert!(files
            .iter()
            .any(|file| file.logical_path == "uploads/source/handler.py"));
    }

    #[test]
    fn stale_evidence_never_passes() {
        let result = evaluate_gate(&GateInput {
            evidence_revision_id: "old".into(),
            current_source_revision_id: "new".into(),
            required_checks: vec![],
            completed_checks: vec![],
            failed_checks: vec![],
            blocking_findings: vec![],
            blocking_unknowns: vec![],
            warnings: vec![],
            graph_state: "CURRENT".into(),
            unsupported_required_areas: vec![],
        });
        assert_eq!(result.verdict, "UNKNOWN");
        assert_eq!(result.currentness, "UNVERIFIED_CHANGE");
    }

    #[test]
    fn empty_graph_is_not_absence_proof() {
        let value = absence_result(0, false);
        assert_eq!(value["absence_proven"], false);
    }

    #[test]
    fn high_risk_without_tests_requires_behavioral_evidence() {
        let risk = assess_risk(
            &["src/AuthController.php".into()],
            "change login database write",
            true,
            false,
        );
        assert!(risk.risk_score >= 50);
        assert!(risk.required_checks.contains(&"http_smoke".to_string()));
    }

    #[test]
    fn legacy_policy_loads_with_security_scenarios_safely_disabled() {
        let policy: Policy = serde_yaml::from_str("quality:\n  auto_risk: true\n").unwrap();
        assert!(!policy.security_scenarios.enabled);
        assert_eq!(policy.security_scenarios.mode, "plan_only");
        assert_eq!(policy.security_scenarios.environment, "unclassified");
        assert!(policy.security_scenarios.local_evidence_only);
        assert!(policy.security_scenarios.deny_production);
    }

    #[test]
    fn project_activation_is_opt_in_and_never_creates_a_marker_during_detection() {
        let dir = tempdir().unwrap();
        assert!(discover_project(dir.path()).is_err());
        assert!(!dir.path().join(MANIFEST_DIR).exists());
    }

    #[test]
    fn concurrent_initialization_converges_on_one_portable_identity() {
        let dir = tempdir().unwrap();
        let root = std::sync::Arc::new(dir.path().to_path_buf());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    initialize_project(&root, "Concurrent", "concurrent", WikiConfig::default())
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            results[0].manifest.project_id,
            results[1].manifest.project_id
        );
        assert_eq!(results.iter().filter(|result| result.created).count(), 1);
        let (manifest, _) = load_project(dir.path()).unwrap();
        assert_eq!(manifest.project_id, results[0].manifest.project_id);
        let portable = dir.path().join(MANIFEST_DIR);
        for required in [
            "project.yaml",
            "policy.yaml",
            "AGENT_POLICY.md",
            "SKILL.md",
            "README_AGENT.md",
            "context/LAST_VERIFIED.json",
            "portability/GRAPH_BUILD_INFO.json",
            "hub.example.yaml",
            ".gitignore",
        ] {
            assert!(portable.join(required).is_file(), "missing {required}");
        }
        assert!(portable.join("local").is_dir());
        assert!(fs::read_to_string(portable.join(".gitignore"))
            .unwrap()
            .contains("local/"));
        assert!(!portable.join("graph.sqlite3").exists());
    }

    #[test]
    fn ambiguous_local_wiki_is_not_bound_automatically() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("LLM-WIKI")).unwrap();
        fs::create_dir(dir.path().join(".llm-wiki")).unwrap();
        let wiki = discover_wiki(dir.path());
        assert_eq!(wiki.mode, "none");
        assert_eq!(wiki.binding_status, "UNRESOLVED");
    }
}
