use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use codeintel_core::{
    atomic_write, compute_source_revision, evaluate_gate, CheckCommand, GateInput, GateResult,
    Policy, ProjectPaths, RiskAssessment, SecurityActor, SecurityCostProbe, SecurityScenario,
    SecurityScenariosPolicy, SmokeEndpoint,
};
use codeintel_graph::{Coverage, GraphStore};
use codeintel_runtime::normalize_trace_file;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEvidence {
    pub evidence_id: Uuid,
    pub command_id: String,
    pub kind: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub stdout_summary: String,
    pub stderr_summary: String,
    pub source_revision_id: String,
    pub environment_profile_hash: String,
    pub baseline_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmokeEvidence {
    pub evidence_id: Uuid,
    pub endpoint_id: String,
    pub method: String,
    pub target: String,
    pub http_status: Option<u16>,
    pub latency_ms: u64,
    pub status: String,
    pub assertion_failures: Vec<String>,
    pub source_revision_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub baseline_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEdge {
    pub source_symbol: String,
    pub target_symbol: String,
    pub kind: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRun {
    pub runtime_run_id: Uuid,
    pub source_revision_id: String,
    pub workload: String,
    pub observed_at: DateTime<Utc>,
    pub coverage_scope: serde_json::Value,
    pub adapter: String,
    pub adapter_version: String,
    pub edges: Vec<RuntimeEdge>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFlowResult {
    pub dataflow_run_id: Uuid,
    pub source_revision_id: String,
    pub source: String,
    pub sink: String,
    pub declared_scope: serde_json::Value,
    pub verdict: String,
    pub path: Vec<String>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub verification_id: Uuid,
    pub change_run_id: Uuid,
    pub source_revision_id: String,
    pub risk: RiskAssessment,
    pub required_checks: Vec<String>,
    pub completed_checks: Vec<String>,
    pub failed_checks: Vec<String>,
    pub blocking_unknowns: Vec<String>,
    pub warnings: Vec<String>,
    pub commands: Vec<CommandEvidence>,
    pub smoke: Vec<SmokeEvidence>,
    #[serde(default)]
    pub security: Option<SecurityScenarioReport>,
    #[serde(default)]
    pub security_correlation: Option<SecurityCorrelationReport>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    pub quality_run_id: Uuid,
    pub change_run_id: Uuid,
    pub source_revision_id: String,
    pub verdict: String,
    pub currentness: String,
    pub risk_score: u8,
    pub required_checks: Vec<String>,
    pub completed_checks: Vec<String>,
    pub failed_checks: Vec<String>,
    pub blocking_unknowns: Vec<String>,
    pub warnings: Vec<String>,
    pub gate_reasons: Vec<String>,
    pub policy_hash: String,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScenarioAttempt {
    pub attempt: u32,
    pub http_status: Option<u16>,
    pub latency_ms: u64,
    pub response_bytes: usize,
    pub body_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScenarioResult {
    pub scenario_id: String,
    pub endpoint_id: String,
    pub case: String,
    pub method: String,
    pub path: String,
    pub status: String,
    pub assertion_failures: Vec<String>,
    pub attempts: Vec<SecurityScenarioAttempt>,
    pub observed_cost_delta: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScenarioReport {
    pub security_run_id: Uuid,
    pub change_run_id: Uuid,
    pub source_revision_id: String,
    pub mode: String,
    pub environment: String,
    pub status: String,
    pub executed_requests: u32,
    pub selected_scenarios: Vec<String>,
    pub results: Vec<SecurityScenarioResult>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub evidence_sha256: String,
    pub local_evidence_only: bool,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScenarioCorrelation {
    pub scenario_id: String,
    pub endpoint_id: String,
    pub route_symbol: Option<String>,
    pub correlation_status: String,
    pub dynamic_status: String,
    pub assertion_failures: Vec<String>,
    pub attempt_count: usize,
    pub observed_cost_delta: Option<f64>,
    pub matched_routes: Vec<serde_json::Value>,
    pub code_flow: Option<serde_json::Value>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityCorrelationReport {
    pub correlation_run_id: Uuid,
    pub change_run_id: Uuid,
    pub security_run_id: Uuid,
    pub source_revision_id: String,
    pub status: String,
    pub selected_scenarios: Vec<String>,
    pub correlations: Vec<SecurityScenarioCorrelation>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub evidence_sha256: String,
    pub local_evidence_only: bool,
    pub completed_at: DateTime<Utc>,
}

const SECURITY_CASES: [&str; 12] = [
    "anonymous",
    "other_user",
    "lower_role",
    "foreign_object",
    "missing_parameters",
    "unexpected_parameters",
    "oversized_values",
    "rapid_repeat",
    "request_replay",
    "expired_or_revoked_token",
    "method_change",
    "direct_api",
];

pub fn validate_security_scenarios(
    policy: &SecurityScenariosPolicy,
    now: DateTime<Utc>,
    for_execution: bool,
) -> Vec<String> {
    let mut blockers = BTreeSet::new();
    if !policy.enabled {
        blockers.insert("security scenarios are disabled".to_string());
    }
    if !matches!(
        policy.mode.as_str(),
        "plan_only" | "lab" | "authorized_staging"
    ) {
        blockers.insert("mode must be plan_only, lab, or authorized_staging".into());
    }
    if !matches!(
        policy.environment.as_str(),
        "lab" | "staging" | "unclassified"
    ) {
        blockers.insert("environment must be lab, staging, or unclassified".into());
    }
    if policy.environment == "unclassified" && for_execution {
        blockers.insert(
            "execution requires an explicitly classified lab or staging environment".into(),
        );
    }
    if policy.deny_production && policy.environment.eq_ignore_ascii_case("production") {
        blockers.insert("production execution is denied by policy".into());
    }
    if !policy.local_evidence_only {
        blockers.insert("security evidence must remain local in this release".into());
    }
    if policy.max_requests == 0 || policy.max_requests > 100 {
        blockers.insert("max_requests must be between 1 and 100".into());
    }
    if policy.max_concurrency == 0 || policy.max_concurrency > 4 {
        blockers.insert("max_concurrency must be between 1 and 4".into());
    }
    if policy.timeout_sec == 0 || policy.timeout_sec > 30 {
        blockers.insert("timeout_sec must be between 1 and 30".into());
    }
    if policy.max_response_bytes == 0 || policy.max_response_bytes > 1_048_576 {
        blockers.insert("max_response_bytes must be between 1 and 1048576".into());
    }
    if for_execution {
        if policy.mode == "plan_only" {
            blockers.insert("plan_only mode never sends network requests".into());
        }
        if policy.authorization.status != "granted" {
            blockers.insert("explicit authorization status is not granted".into());
        }
        if policy
            .authorization
            .scope_ref
            .as_deref()
            .map_or(true, |value| value.trim().is_empty())
        {
            blockers.insert("authorization scope_ref is required".into());
        }
        match policy.authorization.expires_at.as_ref() {
            Some(expires) if *expires > now => {}
            Some(_) => {
                blockers.insert("authorization has expired".into());
            }
            None => {
                blockers.insert("authorization expires_at is required".into());
            }
        }
    }
    let base = policy
        .base_url
        .as_deref()
        .and_then(|value| reqwest::Url::parse(value).ok());
    if policy.base_url.is_some() && base.is_none() {
        blockers.insert("base_url is invalid".into());
    }
    if for_execution && base.is_none() {
        blockers.insert("base_url is required for execution".into());
    }
    if let Some(base) = &base {
        if !matches!(base.scheme(), "http" | "https") {
            blockers.insert("base_url scheme must be http or https".into());
        }
        let host = base.host_str().unwrap_or_default();
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            blockers.insert(
                "base_url cannot contain credentials, query parameters or a fragment".into(),
            );
        }
        if policy.mode == "authorized_staging" && base.scheme() != "https" {
            blockers.insert("authorized_staging requires HTTPS".into());
        }
        if !policy.allowed_hosts.iter().any(|allowed| allowed == host) {
            blockers.insert("base_url host is not in the exact allowed_hosts list".into());
        }
        if policy.mode == "lab" && !is_loopback_host(host) {
            blockers.insert("lab mode is restricted to loopback hosts".into());
        }
    }
    let actors: BTreeSet<_> = policy
        .actors
        .iter()
        .map(|actor| actor.id.as_str())
        .collect();
    if actors.len() != policy.actors.len() {
        blockers.insert("actor ids must be unique".into());
    }
    for actor in &policy.actors {
        validate_actor(actor, &mut blockers);
    }
    let endpoint_inventory: BTreeSet<_> = policy
        .endpoint_inventory
        .iter()
        .map(String::as_str)
        .collect();
    if policy.enabled && endpoint_inventory.is_empty() {
        blockers
            .insert("endpoint_inventory is required when security scenarios are enabled".into());
    }
    if endpoint_inventory.len() != policy.endpoint_inventory.len() {
        blockers.insert("endpoint inventory ids must be unique".into());
    }
    let mut ids = BTreeSet::new();
    let mut covered_pairs = BTreeSet::new();
    for scenario in &policy.scenarios {
        if !ids.insert(scenario.id.as_str()) {
            blockers.insert("scenario ids must be unique".into());
        }
        if !SECURITY_CASES.contains(&scenario.case.as_str()) {
            blockers.insert(format!("scenario {} has an unsupported case", scenario.id));
        }
        if !endpoint_inventory.contains(scenario.endpoint_id.as_str()) {
            blockers.insert(format!(
                "scenario {} references an endpoint outside the inventory",
                scenario.id
            ));
        }
        covered_pairs.insert((scenario.endpoint_id.clone(), scenario.case.clone()));
        if !safe_relative_path(&scenario.path) {
            blockers.insert(format!("scenario {} has an unsafe path", scenario.id));
        }
        if policy.require_correlation {
            match scenario.route_symbol.as_deref() {
                Some(route) if valid_route_symbol(route) => {}
                _ => {
                    blockers.insert(format!(
                        "scenario {} requires an exact method-and-template route_symbol for correlation",
                        scenario.id
                    ));
                }
            }
        } else if scenario
            .route_symbol
            .as_deref()
            .is_some_and(|route| !valid_route_symbol(route))
        {
            blockers.insert(format!(
                "scenario {} has an invalid route_symbol",
                scenario.id
            ));
        }
        if let Some(actor) = &scenario.actor {
            if !actors.contains(actor.as_str()) {
                blockers.insert(format!(
                    "scenario {} references an unknown actor",
                    scenario.id
                ));
            }
        }
        if scenario.repeat == 0 || scenario.repeat > 5 {
            blockers.insert(format!(
                "scenario {} repeat must be between 1 and 5",
                scenario.id
            ));
        }
        if scenario.repeat > 1
            && !matches!(scenario.case.as_str(), "rapid_repeat" | "request_replay")
        {
            blockers.insert(format!(
                "scenario {} repeats an ineligible case",
                scenario.id
            ));
        }
        if scenario.parallel_burst && scenario.case != "rapid_repeat" {
            blockers.insert(format!(
                "scenario {} enables parallel_burst outside rapid_repeat",
                scenario.id
            ));
        }
        if scenario.parallel_burst && (scenario.repeat < 2 || policy.max_concurrency < 2) {
            blockers.insert(format!(
                "scenario {} parallel_burst requires repeat and max_concurrency of at least 2",
                scenario.id
            ));
        }
        let method = scenario.method.to_ascii_uppercase();
        if !matches!(
            method.as_str(),
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"
        ) {
            blockers.insert(format!(
                "scenario {} has an unsupported HTTP method",
                scenario.id
            ));
        }
        if is_state_changing(&method)
            && !(policy.allow_state_changing && scenario.allow_state_change)
        {
            blockers.insert(format!(
                "scenario {} is state-changing but not doubly authorized",
                scenario.id
            ));
        }
        if scenario.expect_status.is_empty() {
            blockers.insert(format!("scenario {} has no expected status", scenario.id));
        }
        if scenario.case == "rapid_repeat" && scenario.expect_terminal_status.is_empty() {
            blockers.insert(format!(
                "scenario {} must declare an expected terminal rate-limit status",
                scenario.id
            ));
        }
        if scenario.case == "request_replay" && !scenario.reuse_idempotency_key {
            blockers.insert(format!(
                "scenario {} must reuse one idempotency key",
                scenario.id
            ));
        }
        let query_size = scenario
            .query
            .iter()
            .map(|(key, value)| key.len().saturating_add(value.len()))
            .sum::<usize>();
        if query_size > 16 * 1024 || scenario.query.keys().any(|key| secret_like_name(key)) {
            blockers.insert(format!("scenario {} has an unsafe query", scenario.id));
        }
        if let Some(body) = &scenario.json_body {
            if serde_json::to_vec(body).map_or(true, |bytes| bytes.len() > 16 * 1024) {
                blockers.insert(format!("scenario {} body exceeds 16 KiB", scenario.id));
            }
            if contains_secret_key(body) {
                blockers.insert(format!(
                    "scenario {} body contains a secret-like key",
                    scenario.id
                ));
            }
        }
        if let Some(probe) = &scenario.cost_probe {
            validate_cost_probe(probe, &actors, &scenario.id, &mut blockers);
        }
    }
    let mut excluded_pairs = BTreeSet::new();
    for exclusion in &policy.exclusions {
        let pair = (exclusion.endpoint_id.clone(), exclusion.case.clone());
        if !endpoint_inventory.contains(exclusion.endpoint_id.as_str())
            || !SECURITY_CASES.contains(&exclusion.case.as_str())
        {
            blockers.insert("scenario exclusion references an unknown endpoint or case".into());
        }
        if exclusion.reason.trim().len() < 12 {
            blockers.insert(format!(
                "exclusion for {} / {} requires a substantive reason",
                exclusion.endpoint_id, exclusion.case
            ));
        }
        if !excluded_pairs.insert(pair.clone()) {
            blockers.insert("scenario exclusions must be unique".into());
        }
        if covered_pairs.contains(&pair) {
            blockers.insert(format!(
                "{} / {} cannot be both covered and excluded",
                exclusion.endpoint_id, exclusion.case
            ));
        }
    }
    for endpoint in &policy.endpoint_inventory {
        for case in SECURITY_CASES {
            let pair = (endpoint.clone(), case.to_string());
            if !covered_pairs.contains(&pair) && !excluded_pairs.contains(&pair) {
                blockers.insert(format!(
                    "endpoint {endpoint} is missing security case {case}"
                ));
            }
        }
    }
    blockers.into_iter().collect()
}

pub fn run_security_scenarios(
    paths: &ProjectPaths,
    change_run_id: Uuid,
    source_revision_id: &str,
    policy: &SecurityScenariosPolicy,
    selected: &[String],
    execute: bool,
) -> Result<SecurityScenarioReport> {
    let completed_at = Utc::now();
    let mut blockers = validate_security_scenarios(policy, completed_at, execute);
    let selected_set: BTreeSet<_> = selected.iter().map(String::as_str).collect();
    let scenarios: Vec<_> = policy
        .scenarios
        .iter()
        .filter(|scenario| selected_set.is_empty() || selected_set.contains(scenario.id.as_str()))
        .collect();
    for requested in &selected_set {
        if !policy
            .scenarios
            .iter()
            .any(|scenario| scenario.id == *requested)
        {
            blockers.push(format!("unknown selected scenario id: {requested}"));
        }
    }
    let requested_count = scenarios.iter().fold(0_u32, |count, scenario| {
        count
            .saturating_add(scenario.repeat)
            .saturating_add(if scenario.cost_probe.is_some() { 2 } else { 0 })
    });
    if execute && requested_count > policy.max_requests {
        blockers.push("selected scenarios exceed max_requests".into());
    }
    blockers.sort();
    blockers.dedup();
    let mut results = vec![];
    let mut executed_requests = 0;
    if execute && blockers.is_empty() {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(policy.timeout_sec))
            .build()?;
        let actors: BTreeMap<_, _> = policy
            .actors
            .iter()
            .map(|actor| (actor.id.as_str(), actor))
            .collect();
        for scenario in scenarios {
            let result = execute_security_scenario(
                &client,
                policy,
                &actors,
                scenario,
                change_run_id,
                &mut executed_requests,
            )?;
            results.push(result);
        }
    }
    let status = if !blockers.is_empty() {
        "DENIED"
    } else if !execute {
        "VALIDATED"
    } else if results.iter().all(|result| result.status == "PASS") {
        "PASS"
    } else {
        "FAIL"
    };
    let selected_scenarios = policy
        .scenarios
        .iter()
        .filter(|scenario| selected_set.is_empty() || selected_set.contains(scenario.id.as_str()))
        .map(|scenario| scenario.id.clone())
        .collect::<Vec<_>>();
    let evidence_sha256 = hash_security_evidence(change_run_id, source_revision_id, &results)?;
    let report = SecurityScenarioReport {
        security_run_id: Uuid::new_v4(),
        change_run_id,
        source_revision_id: source_revision_id.into(),
        mode: policy.mode.clone(),
        environment: policy.environment.clone(),
        status: status.into(),
        executed_requests,
        selected_scenarios,
        results,
        blockers,
        warnings: vec![
            "Runtime evidence covers only the declared scenario matrix; unlisted paths remain unknown."
                .into(),
        ],
        evidence_sha256,
        local_evidence_only: true,
        completed_at,
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("security-{change_run_id}.json")),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    if execute && report.executed_requests > 0 {
        let runtime = RuntimeRun {
            runtime_run_id: Uuid::new_v4(),
            source_revision_id: source_revision_id.into(),
            workload: "security-scenarios".into(),
            observed_at: Utc::now(),
            coverage_scope: serde_json::json!({
                "scenario_ids": report.selected_scenarios,
                "requests": report.executed_requests,
                "status": "PARTIAL"
            }),
            adapter: "security_http_matrix".into(),
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            edges: vec![],
            status: "PARTIAL".into(),
        };
        atomic_write(
            &paths
                .reports_dir
                .join(format!("runtime-{change_run_id}.json")),
            &serde_json::to_vec_pretty(&runtime)?,
        )?;
    }
    Ok(report)
}

/// Correlate already-collected dynamic scenario evidence to exact route symbols and a bounded
/// downstream graph. This action is read-only, sends no requests, and never returns source bodies.
pub fn correlate_security_scenarios(
    paths: &ProjectPaths,
    change_run_id: Uuid,
    source_revision_id: &str,
    policy: &SecurityScenariosPolicy,
    selected: &[String],
) -> Result<SecurityCorrelationReport> {
    let security_path = paths
        .reports_dir
        .join(format!("security-{change_run_id}.json"));
    let security: SecurityScenarioReport = serde_json::from_slice(
        &fs::read(&security_path).context("security scenario report missing")?,
    )?;
    let mut blockers = vec![];
    if security.change_run_id != change_run_id
        || security.source_revision_id != source_revision_id
        || !matches!(security.status.as_str(), "PASS" | "FAIL")
    {
        blockers.push(
            "Correlation requires executed security evidence for this ChangeRun and source revision."
                .into(),
        );
    }
    let graph = GraphStore::open(&paths.graph_path)?;
    if graph.revision()? != source_revision_id {
        blockers.push("The graph does not match the security evidence revision.".into());
    }
    let selected_set: BTreeSet<_> = selected.iter().map(String::as_str).collect();
    for requested in &selected_set {
        if !security
            .results
            .iter()
            .any(|result| result.scenario_id == *requested)
        {
            blockers.push(format!(
                "selected scenario has no executed evidence in the security report: {requested}"
            ));
        }
    }
    blockers.sort();
    blockers.dedup();

    let mut correlations = vec![];
    if blockers.is_empty() {
        for result in security.results.iter().filter(|result| {
            selected_set.is_empty() || selected_set.contains(result.scenario_id.as_str())
        }) {
            let configured = policy
                .scenarios
                .iter()
                .find(|scenario| scenario.id == result.scenario_id);
            let route_symbol = configured.and_then(|scenario| scenario.route_symbol.clone());
            let mut matched_routes = vec![];
            let mut code_flow = None;
            let mut unknowns = vec![];
            let correlation_status = if let Some(route) = route_symbol.as_deref() {
                let candidates = graph.find_symbol(route, 25)?;
                let exact = candidates
                    .items
                    .into_iter()
                    .filter(|node| {
                        matches!(node.kind.as_str(), "ROUTE" | "ENDPOINT")
                            && (node.qualified_name == route || node.display_name == route)
                    })
                    .collect::<Vec<_>>();
                matched_routes = exact
                    .iter()
                    .map(|node| {
                        serde_json::json!({
                            "id": node.id,
                            "kind": node.kind,
                            "qualified_name": node.qualified_name,
                            "path": node.file_path,
                            "line": node.start_line,
                            "language": node.language,
                        })
                    })
                    .collect();
                if exact.len() == 1 {
                    let flow = graph.bounded_flow_subgraph(&exact[0].id, 4, 80)?;
                    unknowns.extend(
                        flow["unknowns"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_string),
                    );
                    code_flow = Some(flow);
                    "FOUND"
                } else if exact.is_empty() {
                    unknowns.push(format!(
                        "No exact route symbol was found for {route}; graph coverage may be partial."
                    ));
                    "UNKNOWN"
                } else {
                    unknowns.push(format!(
                        "Multiple exact route symbols matched {route}; a unique code path is not proven."
                    ));
                    "AMBIGUOUS"
                }
            } else {
                unknowns.push(
                    "The scenario has no exact route_symbol, so runtime-to-code correlation is unknown."
                        .into(),
                );
                "UNKNOWN"
            };
            correlations.push(SecurityScenarioCorrelation {
                scenario_id: result.scenario_id.clone(),
                endpoint_id: result.endpoint_id.clone(),
                route_symbol,
                correlation_status: correlation_status.into(),
                dynamic_status: result.status.clone(),
                assertion_failures: result.assertion_failures.clone(),
                attempt_count: result.attempts.len(),
                observed_cost_delta: result.observed_cost_delta,
                matched_routes,
                code_flow,
                unknowns,
            });
        }
    }
    let status = if !blockers.is_empty() {
        "DENIED"
    } else if correlations.is_empty()
        || correlations
            .iter()
            .any(|item| item.correlation_status != "FOUND")
    {
        "UNKNOWN"
    } else {
        "PASS"
    };
    let selected_scenarios = correlations
        .iter()
        .map(|item| item.scenario_id.clone())
        .collect::<Vec<_>>();
    let evidence_sha256 = hex::encode(Sha256::digest(serde_json::to_vec(&serde_json::json!({
        "security_evidence_sha256": security.evidence_sha256,
        "source_revision_id": source_revision_id,
        "correlations": correlations,
    }))?));
    let report = SecurityCorrelationReport {
        correlation_run_id: Uuid::new_v4(),
        change_run_id,
        security_run_id: security.security_run_id,
        source_revision_id: source_revision_id.into(),
        status: status.into(),
        selected_scenarios,
        correlations,
        blockers,
        warnings: vec![
            "Correlation is bounded advisory evidence; UNKNOWN never means a route or flaw is absent."
                .into(),
        ],
        evidence_sha256,
        local_evidence_only: true,
        completed_at: Utc::now(),
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("security-correlation-{change_run_id}.json")),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

pub fn verify_change(
    root: &Path,
    policy: &Policy,
    paths: &ProjectPaths,
    change_run_id: Uuid,
    source_revision_id: &str,
    risk: &RiskAssessment,
    baseline_kind: &str,
) -> Result<VerificationResult> {
    let before = compute_source_revision(root, policy)?;
    if before.source_revision_id != source_revision_id {
        return Err(anyhow!(
            "SOURCE_DRIFT: review revision is no longer current"
        ));
    }
    let mut completed = BTreeSet::new();
    let mut failed = vec![];
    let mut unknowns = vec![];
    let mut warnings = vec![];
    let mut commands = vec![];
    let mut smoke = vec![];
    let mut security = None;
    let mut security_correlation = None;
    completed.insert("actual_source_verify".to_string());
    let review_path = paths
        .reports_dir
        .join(format!("review-{change_run_id}.json"));
    if review_path.is_file() {
        let review: serde_json::Value = serde_json::from_slice(&fs::read(&review_path)?)?;
        if review["after_source_revision_id"] == source_revision_id {
            completed.insert("structural_review".to_string());
        } else {
            unknowns.push("Review bundle belongs to another source revision.".into());
        }
    } else {
        unknowns.push("Required structural review bundle is missing.".into());
    }
    let risk_level = risk.risk_level.as_str();
    let mut applicable_commands: Vec<&CheckCommand> = policy
        .checks
        .commands
        .iter()
        .filter(|command| {
            command.required_for.is_empty()
                || command
                    .required_for
                    .iter()
                    .any(|level| level.eq_ignore_ascii_case(risk_level))
        })
        .collect();
    applicable_commands.sort_by(|a, b| a.id.cmp(&b.id));
    let needs_tests = risk.required_checks.contains(&"targeted_tests".to_string());
    let has_test_command = applicable_commands
        .iter()
        .any(|command| command.kind == "test");
    if needs_tests && !has_test_command {
        unknowns.push("Targeted tests are required but no test command is configured.".into());
    }
    for command in applicable_commands {
        let evidence = run_command(root, command, source_revision_id, baseline_kind)?;
        if evidence.status == "PASS" {
            completed.insert(if command.kind == "test" {
                "targeted_tests".into()
            } else {
                command.kind.clone()
            });
        } else if evidence.status == "FAIL" {
            failed.push(command.id.clone());
        } else {
            unknowns.push(format!(
                "Command {} could not provide evidence: {}",
                command.id, evidence.status
            ));
        }
        commands.push(evidence);
    }
    if risk.required_checks.contains(&"http_smoke".to_string()) {
        if policy.smoke.enabled
            && policy.smoke.base_url.is_some()
            && !policy.smoke.endpoints.is_empty()
        {
            for endpoint in &policy.smoke.endpoints {
                let evidence = run_smoke(
                    policy.smoke.base_url.as_deref().unwrap(),
                    endpoint,
                    source_revision_id,
                    baseline_kind,
                )?;
                if evidence.status == "FAIL" {
                    failed.push(format!("smoke:{}", endpoint.id));
                }
                smoke.push(evidence);
            }
            if smoke.iter().all(|result| result.status == "PASS") {
                completed.insert("http_smoke".into());
            }
        } else {
            unknowns
                .push("HTTP smoke is required but endpoints/base_url are not configured.".into());
        }
    }
    if risk
        .required_checks
        .contains(&"semantic_resolution".to_string())
    {
        match GraphStore::open(&paths.graph_path).and_then(|store| store.coverage()) {
            Ok(coverage) => {
                let changed_files = changed_files_from_review(&review_path)?;
                let (scope_complete, scope_unknowns, scope_warnings) =
                    semantic_scope_evidence(&coverage, &changed_files);
                if scope_complete {
                    completed.insert("semantic_resolution".into());
                } else {
                    unknowns.extend(scope_unknowns);
                }
                warnings.extend(scope_warnings);
            }
            Err(error) => {
                unknowns.push(format!("Semantic resolution evidence unavailable: {error}"))
            }
        }
    }
    if risk.required_checks.contains(&"runtime_trace".to_string()) {
        let runtime_path = paths
            .reports_dir
            .join(format!("runtime-{change_run_id}.json"));
        if runtime_path.is_file() {
            let runtime: RuntimeRun = serde_json::from_slice(&fs::read(runtime_path)?)?;
            if runtime.source_revision_id == source_revision_id && runtime.status == "PARTIAL" {
                completed.insert("runtime_trace".into());
                warnings.push(
                    "Runtime evidence is PARTIAL and proves only its declared workload.".into(),
                );
            } else if runtime.source_revision_id == source_revision_id
                && runtime.status == "COMPLETE"
            {
                completed.insert("runtime_trace".into());
            } else {
                unknowns.push("Runtime evidence is stale or invalid.".into());
            }
        } else {
            unknowns.push("Runtime trace is required but no run exists for this ChangeRun.".into());
        }
    }
    if risk.required_checks.contains(&"dataflow".to_string()) {
        let flow_path = paths
            .reports_dir
            .join(format!("dataflow-{change_run_id}.json"));
        if flow_path.is_file() {
            let flow: DataFlowResult = serde_json::from_slice(&fs::read(flow_path)?)?;
            if flow.source_revision_id != source_revision_id || flow.verdict == "UNKNOWN" {
                unknowns.push("Required data-flow evidence is stale or UNKNOWN.".into());
            } else if flow.verdict == "FOUND" {
                failed.push("dataflow_violation".into());
            } else {
                completed.insert("dataflow".into());
            }
        } else {
            unknowns.push(
                "Data-flow analysis is required but no run exists for this ChangeRun.".into(),
            );
        }
    }
    if risk
        .required_checks
        .contains(&"security_scenarios".to_string())
    {
        let security_path = paths
            .reports_dir
            .join(format!("security-{change_run_id}.json"));
        if security_path.is_file() {
            let report: SecurityScenarioReport =
                serde_json::from_slice(&fs::read(&security_path)?)?;
            if report.source_revision_id != source_revision_id {
                unknowns
                    .push("Security scenario evidence belongs to another source revision.".into());
            } else if report.status == "PASS" {
                if policy.security_scenarios.require_correlation {
                    let correlation_path = paths
                        .reports_dir
                        .join(format!("security-correlation-{change_run_id}.json"));
                    if correlation_path.is_file() {
                        let correlation: SecurityCorrelationReport =
                            serde_json::from_slice(&fs::read(&correlation_path)?)?;
                        if correlation.source_revision_id == source_revision_id
                            && correlation.security_run_id == report.security_run_id
                            && correlation.status == "PASS"
                        {
                            completed.insert("security_scenarios".into());
                        } else {
                            unknowns.push(
                                "Security correlation is stale, incomplete, or belongs to another run."
                                    .into(),
                            );
                        }
                        warnings.extend(correlation.warnings.clone());
                        security_correlation = Some(correlation);
                    } else {
                        unknowns.push(
                            "Security scenarios passed, but required runtime-to-code correlation is missing."
                                .into(),
                        );
                    }
                } else {
                    completed.insert("security_scenarios".into());
                }
            } else if report.status == "FAIL" {
                failed.push("security_scenarios".into());
            } else {
                unknowns.push(format!(
                    "Security scenarios did not execute successfully: {}.",
                    report.status
                ));
            }
            warnings.extend(report.warnings.clone());
            security = Some(report);
        } else {
            unknowns.push(
                "Security scenarios are required but no revision-bound local run exists.".into(),
            );
        }
    }
    let after = compute_source_revision(root, policy)?;
    if after.source_revision_id != source_revision_id {
        unknowns.push("Source changed during verification; collected evidence is invalid.".into());
    }
    let result = VerificationResult {
        verification_id: Uuid::new_v4(),
        change_run_id,
        source_revision_id: source_revision_id.into(),
        risk: risk.clone(),
        required_checks: risk.required_checks.clone(),
        completed_checks: completed.into_iter().collect(),
        failed_checks: failed,
        blocking_unknowns: unknowns,
        warnings,
        commands,
        smoke,
        security,
        security_correlation,
        completed_at: Utc::now(),
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("verification-{change_run_id}.json")),
        &serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

fn changed_files_from_review(review_path: &Path) -> Result<BTreeSet<String>> {
    if !review_path.is_file() {
        return Ok(BTreeSet::new());
    }
    let review: serde_json::Value = serde_json::from_slice(&fs::read(review_path)?)?;
    Ok(review["deltas"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|delta| delta["path"].as_str().map(str::to_string))
        .collect())
}

fn semantic_scope_evidence(
    coverage: &Coverage,
    changed_files: &BTreeSet<String>,
) -> (bool, Vec<String>, Vec<String>) {
    if changed_files.is_empty() {
        return (
            false,
            vec!["Semantic resolution cannot be scoped because the review has no changed-file inventory."
                .into()],
            vec![],
        );
    }
    let changed_source_files = changed_files
        .iter()
        .filter_map(|path| semantic_language_for_path(path).map(|language| (path, language)))
        .collect::<Vec<_>>();
    let mut unknowns = vec![];
    for (path, language) in &changed_source_files {
        if !coverage
            .supported_languages
            .iter()
            .any(|supported| supported == language)
        {
            unknowns.push(format!(
                "Changed source file {path} uses {language}, which was not semantically analyzed."
            ));
        }
        if coverage
            .failed_files
            .iter()
            .any(|failure| failure == *path || failure.starts_with(&format!("{path}:")))
        {
            unknowns.push(format!(
                "Changed source file {path} has parser or semantic analysis failures."
            ));
        }
    }
    let unresolved: Vec<_> = coverage
        .semantic_unresolved_files
        .iter()
        .filter(|path| changed_files.contains(*path))
        .cloned()
        .collect();
    let ambiguous: Vec<_> = coverage
        .semantic_ambiguous_files
        .iter()
        .filter(|path| changed_files.contains(*path))
        .cloned()
        .collect();
    if !unresolved.is_empty() {
        unknowns.push(format!(
            "Changed files contain unresolved semantic call targets: {}.",
            unresolved.join(", ")
        ));
    }
    if !ambiguous.is_empty() {
        unknowns.push(format!(
            "Changed files contain ambiguous semantic call targets: {}.",
            ambiguous.join(", ")
        ));
    }
    let external_or_unindexed: Vec<_> = coverage
        .semantic_external_or_unindexed_files
        .iter()
        .filter(|path| changed_files.contains(*path))
        .cloned()
        .collect();
    let outside_scope_unknowns = coverage
        .semantic_unresolved_files
        .iter()
        .chain(coverage.semantic_ambiguous_files.iter())
        .filter(|path| !changed_files.contains(*path))
        .collect::<BTreeSet<_>>();
    let outside_scope_failures = coverage
        .failed_files
        .iter()
        .filter(|failure| {
            !changed_files
                .iter()
                .any(|path| *failure == path || failure.starts_with(&format!("{path}:")))
        })
        .count();
    let mut warnings = vec![];
    if !external_or_unindexed.is_empty() {
        warnings.push(format!(
            "Changed scope contains external-or-unindexed calls in {} file(s); these cannot support absence claims and require source/runtime corroboration when relevant.",
            external_or_unindexed.len()
        ));
    }
    if !outside_scope_unknowns.is_empty() {
        warnings.push(format!(
            "Semantic unknowns remain outside the changed scope in {} file(s); absence is not proven there.",
            outside_scope_unknowns.len()
        ));
    }
    if outside_scope_failures > 0 || coverage.semantic != "COMPLETE" {
        warnings.push(format!(
            "Global semantic coverage is {}; {} analyzer failure(s) remain outside the changed scope.",
            coverage.semantic, outside_scope_failures
        ));
    }
    if changed_source_files.is_empty() {
        warnings.push(
            "No semantically supported source file changed; semantic resolution is not applicable to this scope."
                .into(),
        );
    }
    (unknowns.is_empty(), unknowns, warnings)
}

fn semantic_language_for_path(path: &str) -> Option<&'static str> {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "php" => Some("php"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "ts" | "tsx" | "mts" | "cts" => Some("typescript"),
        "py" | "pyi" => Some("python"),
        "cs" => Some("csharp"),
        "rs" => Some("rust"),
        "rb" => Some("ruby"),
        "go" => Some("go"),
        "java" => Some("java"),
        "kt" | "kts" => Some("kotlin"),
        "c" | "h" | "cpp" | "hpp" => Some("c-cpp"),
        "swift" => Some("swift"),
        "scala" => Some("scala"),
        "dart" => Some("dart"),
        "lua" => Some("lua"),
        _ => None,
    }
}

pub fn quality_gate(
    root: &Path,
    policy: &Policy,
    paths: &ProjectPaths,
    change_run_id: Uuid,
    blocking_findings: Vec<String>,
) -> Result<QualityReport> {
    let verification_path = paths
        .reports_dir
        .join(format!("verification-{change_run_id}.json"));
    let verification: VerificationResult = serde_json::from_slice(
        &fs::read(verification_path).context("verification result missing")?,
    )?;
    let current = compute_source_revision(root, policy)?;
    let graph_state = match GraphStore::open(&paths.graph_path) {
        Ok(store) if store.revision()? == current.source_revision_id => "CURRENT",
        Ok(_) => "STALE",
        Err(_) => "INVALID",
    };
    let unsupported = if verification
        .required_checks
        .contains(&"semantic_resolution".to_string())
        && !verification
            .completed_checks
            .contains(&"semantic_resolution".to_string())
    {
        vec!["semantic_resolution".into()]
    } else {
        vec![]
    };
    let gate: GateResult = evaluate_gate(&GateInput {
        evidence_revision_id: verification.source_revision_id.clone(),
        current_source_revision_id: current.source_revision_id.clone(),
        required_checks: verification.required_checks.clone(),
        completed_checks: verification.completed_checks.clone(),
        failed_checks: verification.failed_checks.clone(),
        blocking_findings,
        blocking_unknowns: verification.blocking_unknowns.clone(),
        warnings: verification.warnings.clone(),
        graph_state: graph_state.into(),
        unsupported_required_areas: unsupported,
    });
    let report = QualityReport {
        quality_run_id: Uuid::new_v4(),
        change_run_id,
        source_revision_id: current.source_revision_id,
        verdict: gate.verdict,
        currentness: gate.currentness,
        risk_score: verification.risk.risk_score,
        required_checks: verification.required_checks,
        completed_checks: verification.completed_checks,
        failed_checks: verification.failed_checks,
        blocking_unknowns: verification.blocking_unknowns,
        warnings: gate.warnings,
        gate_reasons: gate.reasons,
        policy_hash: current.policy_hash,
        completed_at: Utc::now(),
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("quality-{change_run_id}.json")),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn run_command(
    root: &Path,
    policy: &CheckCommand,
    revision: &str,
    baseline_kind: &str,
) -> Result<CommandEvidence> {
    if policy.argv.is_empty() {
        return Err(anyhow!("Command {} has an empty argv", policy.id));
    }
    let started_at = Utc::now();
    let started = Instant::now();
    let temp_prefix = format!("codeintel-{}-{}", std::process::id(), Uuid::new_v4());
    let stdout_path = std::env::temp_dir().join(format!("{temp_prefix}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{temp_prefix}.stderr"));
    let stdout_file = File::create(&stdout_path)?;
    let stderr_file = File::create(&stderr_path)?;
    let mut child = Command::new(&policy.argv[0])
        .args(&policy.argv[1..])
        .current_dir(root.join(&policy.cwd))
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .with_context(|| format!("start command {}", policy.id))?;
    let deadline = Instant::now() + Duration::from_secs(policy.timeout_sec);
    let exit_code;
    let status;
    loop {
        if let Some(exit) = child.try_wait()? {
            exit_code = exit.code();
            status = if exit.success() { "PASS" } else { "FAIL" };
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            exit_code = None;
            status = "TIMEOUT";
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let stdout_summary = read_bounded(&stdout_path, 32 * 1024)?;
    let stderr_summary = read_bounded(&stderr_path, 32 * 1024)?;
    let _ = fs::remove_file(stdout_path);
    let _ = fs::remove_file(stderr_path);
    Ok(CommandEvidence {
        evidence_id: Uuid::new_v4(),
        command_id: policy.id.clone(),
        kind: policy.kind.clone(),
        argv: policy.argv.clone(),
        cwd: policy.cwd.clone(),
        exit_code,
        status: status.into(),
        started_at,
        completed_at: Utc::now(),
        duration_ms: started.elapsed().as_millis() as u64,
        stdout_summary: redact_output(&stdout_summary),
        stderr_summary: redact_output(&stderr_summary),
        source_revision_id: revision.into(),
        environment_profile_hash: environment_profile_hash(root),
        baseline_kind: baseline_kind.into(),
    })
}

fn run_smoke(
    base_url: &str,
    endpoint: &SmokeEndpoint,
    revision: &str,
    baseline_kind: &str,
) -> Result<SmokeEvidence> {
    let target = format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        endpoint.path.as_str()
    );
    let started_at = Utc::now();
    let started = Instant::now();
    let client = Client::builder().timeout(Duration::from_secs(15)).build()?;
    let method = reqwest::Method::from_bytes(endpoint.method.as_bytes())?;
    let response = client.request(method, &target).send();
    let mut failures = vec![];
    let mut http_status = None;
    if let Ok(response) = response {
        http_status = Some(response.status().as_u16());
        if !endpoint.expect_status.contains(&response.status().as_u16()) {
            failures.push(format!(
                "expected status {:?}, received {}",
                endpoint.expect_status,
                response.status().as_u16()
            ));
        }
        if let Some(header) = &endpoint.header_exists {
            if !response.headers().contains_key(header) {
                failures.push(format!("required header is missing: {header}"));
            }
        }
        if let Some(marker) = &endpoint.body_contains {
            let body = response.text().unwrap_or_default();
            if !body.contains(marker) {
                failures.push("stable body marker was not found".into());
            }
        }
    } else if let Err(error) = response {
        failures.push(format!("HTTP request failed: {error}"));
    }
    let elapsed = started.elapsed().as_millis() as u64;
    if let Some(max) = endpoint.max_latency_ms {
        if elapsed > max {
            failures.push(format!("latency {elapsed}ms exceeds {max}ms"));
        }
    }
    Ok(SmokeEvidence {
        evidence_id: Uuid::new_v4(),
        endpoint_id: endpoint.id.clone(),
        method: endpoint.method.clone(),
        target,
        http_status,
        latency_ms: elapsed,
        status: if failures.is_empty() { "PASS" } else { "FAIL" }.into(),
        assertion_failures: failures,
        source_revision_id: revision.into(),
        started_at,
        completed_at: Utc::now(),
        baseline_kind: baseline_kind.into(),
    })
}

fn validate_actor(actor: &SecurityActor, blockers: &mut BTreeSet<String>) {
    if actor.id.trim().is_empty() {
        blockers.insert("actor id cannot be empty".into());
    }
    if let Some(name) = &actor.bearer_token_env {
        if !valid_environment_name(name) {
            blockers.insert(format!(
                "actor {} has an invalid token environment reference",
                actor.id
            ));
        }
    }
    for (header, environment) in &actor.headers_from_env {
        if header.eq_ignore_ascii_case("host") || header.eq_ignore_ascii_case("content-length") {
            blockers.insert(format!(
                "actor {} attempts to control a forbidden header",
                actor.id
            ));
        }
        if !valid_environment_name(environment) {
            blockers.insert(format!(
                "actor {} has an invalid header environment reference",
                actor.id
            ));
        }
    }
}

fn validate_cost_probe(
    probe: &SecurityCostProbe,
    actors: &BTreeSet<&str>,
    scenario_id: &str,
    blockers: &mut BTreeSet<String>,
) {
    if !safe_relative_path(&probe.path) {
        blockers.insert(format!(
            "scenario {scenario_id} has an unsafe cost probe path"
        ));
    }
    if !actors.contains(probe.actor.as_str()) {
        blockers.insert(format!(
            "scenario {scenario_id} cost probe references an unknown actor"
        ));
    }
    if !probe.json_pointer.starts_with('/') || probe.json_pointer.len() > 256 {
        blockers.insert(format!(
            "scenario {scenario_id} has an invalid cost probe JSON pointer"
        ));
    }
    if !probe.max_delta.is_finite() || probe.max_delta < 0.0 {
        blockers.insert(format!(
            "scenario {scenario_id} has an invalid maximum cost delta"
        ));
    }
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_digit())
        && !value.as_bytes()[0].is_ascii_digit()
}

fn safe_relative_path(value: &str) -> bool {
    value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains("://")
        && !value.contains('\\')
        && !value.split('/').any(|part| part == "..")
        && value.len() <= 2_048
}

fn valid_route_symbol(route: &str) -> bool {
    let route = route.trim();
    let (method, path) = route.split_once(' ').unwrap_or_default();
    route.len() <= 256
        && matches!(method, "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE")
        && path.starts_with('/')
        && !route.contains(['\r', '\n'])
        && !route.contains("..")
        && !route.contains("://")
}

fn is_loopback_host(value: &str) -> bool {
    matches!(value, "localhost" | "127.0.0.1" | "::1")
}

fn is_state_changing(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
}

fn contains_secret_key(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(items) => items
            .iter()
            .any(|(key, value)| secret_like_name(key) || contains_secret_key(value)),
        serde_json::Value::Array(items) => items.iter().any(contains_secret_key),
        _ => false,
    }
}

fn secret_like_name(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    [
        "password",
        "secret",
        "token",
        "authorization",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn http_error_class(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_body() {
        "body"
    } else if error.is_request() {
        "request"
    } else {
        "transport"
    }
}

fn execute_security_scenario(
    client: &Client,
    policy: &SecurityScenariosPolicy,
    actors: &BTreeMap<&str, &SecurityActor>,
    scenario: &SecurityScenario,
    change_run_id: Uuid,
    executed_requests: &mut u32,
) -> Result<SecurityScenarioResult> {
    let mut failures = vec![];
    let mut attempts = vec![];
    let before_cost = if let Some(probe) = &scenario.cost_probe {
        *executed_requests = executed_requests.saturating_add(1);
        match read_cost_probe(client, policy, actors, probe) {
            Ok(value) => Some(value),
            Err(error) => {
                failures.push(format!("cost probe before scenario failed: {error}"));
                None
            }
        }
    } else {
        None
    };
    let idempotency_key = format!("codeintel-{change_run_id}-{}", scenario.id);
    if scenario.parallel_burst {
        *executed_requests = executed_requests.saturating_add(scenario.repeat);
        let concurrency = policy.max_concurrency.min(scenario.repeat) as usize;
        for first in (1..=scenario.repeat).step_by(concurrency) {
            let last = (first + concurrency as u32 - 1).min(scenario.repeat);
            let batch = thread::scope(|scope| {
                let handles = (first..=last)
                    .map(|attempt| {
                        let idempotency_key = idempotency_key.as_str();
                        scope.spawn(move || {
                            perform_security_attempt(
                                client,
                                policy,
                                actors,
                                scenario,
                                attempt,
                                idempotency_key,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .map_err(|_| anyhow!("parallel security request worker panicked"))?
                    })
                    .collect::<Result<Vec<_>>>()
            })?;
            for (evidence, transport_failure) in batch {
                if let Some(failure) = transport_failure {
                    failures.push(failure);
                }
                attempts.push(evidence);
            }
        }
        attempts.sort_by_key(|attempt| attempt.attempt);
        for attempt in &attempts {
            if let Some(status) = attempt.http_status {
                if !scenario.expect_status.contains(&status)
                    && !scenario.expect_terminal_status.contains(&status)
                {
                    failures.push(format!(
                        "attempt {} received undeclared status {status}",
                        attempt.attempt
                    ));
                }
            }
        }
        if !attempts.iter().any(|attempt| {
            attempt
                .http_status
                .is_some_and(|status| scenario.expect_terminal_status.contains(&status))
        }) {
            failures.push(format!(
                "parallel burst observed no terminal rate-limit status {:?}",
                scenario.expect_terminal_status
            ));
        }
    } else {
        for attempt in 1..=scenario.repeat {
            *executed_requests = executed_requests.saturating_add(1);
            let (evidence, transport_failure) = perform_security_attempt(
                client,
                policy,
                actors,
                scenario,
                attempt,
                &idempotency_key,
            )?;
            if let Some(failure) = transport_failure {
                failures.push(failure);
            } else if let Some(status) = evidence.http_status {
                let expected =
                    if attempt == scenario.repeat && !scenario.expect_terminal_status.is_empty() {
                        &scenario.expect_terminal_status
                    } else {
                        &scenario.expect_status
                    };
                if !expected.contains(&status) {
                    failures.push(format!(
                        "attempt {attempt} expected status {:?}, received {status}",
                        expected
                    ));
                }
            }
            attempts.push(evidence);
        }
    }
    let after_cost = if let Some(probe) = &scenario.cost_probe {
        *executed_requests = executed_requests.saturating_add(1);
        match read_cost_probe(client, policy, actors, probe) {
            Ok(value) => Some(value),
            Err(error) => {
                failures.push(format!("cost probe after scenario failed: {error}"));
                None
            }
        }
    } else {
        None
    };
    let observed_cost_delta = before_cost
        .zip(after_cost)
        .map(|(before, after)| after - before);
    if let (Some(delta), Some(probe)) = (observed_cost_delta, &scenario.cost_probe) {
        if delta.abs() > probe.max_delta {
            failures.push(format!(
                "absolute observed cost delta {delta:.6} exceeds allowed delta {:.6}",
                probe.max_delta
            ));
        }
    }
    Ok(SecurityScenarioResult {
        scenario_id: scenario.id.clone(),
        endpoint_id: scenario.endpoint_id.clone(),
        case: scenario.case.clone(),
        method: scenario.method.to_ascii_uppercase(),
        path: scenario.path.clone(),
        status: if failures.is_empty() { "PASS" } else { "FAIL" }.into(),
        assertion_failures: failures,
        attempts,
        observed_cost_delta,
    })
}

fn perform_security_attempt(
    client: &Client,
    policy: &SecurityScenariosPolicy,
    actors: &BTreeMap<&str, &SecurityActor>,
    scenario: &SecurityScenario,
    attempt: u32,
    idempotency_key: &str,
) -> Result<(SecurityScenarioAttempt, Option<String>)> {
    let started = Instant::now();
    let mut request = build_security_request(client, policy, actors, scenario)?;
    if scenario.reuse_idempotency_key {
        request = request.header("Idempotency-Key", idempotency_key);
    }
    let response = request.send();
    let mut evidence = SecurityScenarioAttempt {
        attempt,
        http_status: None,
        latency_ms: started.elapsed().as_millis() as u64,
        response_bytes: 0,
        body_sha256: None,
    };
    let failure = match response {
        Ok(mut response) => {
            evidence.http_status = Some(response.status().as_u16());
            let body = read_response_bounded(&mut response, policy.max_response_bytes)?;
            evidence.latency_ms = started.elapsed().as_millis() as u64;
            evidence.response_bytes = body.len();
            evidence.body_sha256 = Some(hex::encode(Sha256::digest(&body)));
            None
        }
        Err(error) => Some(format!(
            "attempt {attempt} request failed: {}",
            http_error_class(&error)
        )),
    };
    Ok((evidence, failure))
}

fn build_security_request(
    client: &Client,
    policy: &SecurityScenariosPolicy,
    actors: &BTreeMap<&str, &SecurityActor>,
    scenario: &SecurityScenario,
) -> Result<reqwest::blocking::RequestBuilder> {
    let method = reqwest::Method::from_bytes(scenario.method.to_ascii_uppercase().as_bytes())?;
    let target = checked_target(policy, &scenario.path)?;
    let mut request = client.request(method, target).query(&scenario.query);
    if let Some(actor_id) = &scenario.actor {
        request = apply_actor(request, actors.get(actor_id.as_str()).copied())?;
    }
    if let Some(body) = &scenario.json_body {
        request = request.json(body);
    }
    Ok(request)
}

fn read_cost_probe(
    client: &Client,
    policy: &SecurityScenariosPolicy,
    actors: &BTreeMap<&str, &SecurityActor>,
    probe: &SecurityCostProbe,
) -> Result<f64> {
    let target = checked_target(policy, &probe.path)?;
    let request = apply_actor(
        client.get(target),
        actors.get(probe.actor.as_str()).copied(),
    )?;
    let mut response = request.send()?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "probe returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = read_response_bounded(&mut response, policy.max_response_bytes)?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).context("parse bounded probe JSON")?;
    value
        .pointer(&probe.json_pointer)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| anyhow!("probe JSON pointer is not numeric"))
}

fn apply_actor(
    mut request: reqwest::blocking::RequestBuilder,
    actor: Option<&SecurityActor>,
) -> Result<reqwest::blocking::RequestBuilder> {
    if let Some(actor) = actor {
        if let Some(environment) = &actor.bearer_token_env {
            let value = std::env::var(environment).with_context(|| {
                format!("required actor credential {environment} is unavailable")
            })?;
            request = request.bearer_auth(value);
        }
        for (header, environment) in &actor.headers_from_env {
            let value = std::env::var(environment).with_context(|| {
                format!("required actor header value {environment} is unavailable")
            })?;
            request = request.header(header, value);
        }
    }
    Ok(request)
}

fn checked_target(policy: &SecurityScenariosPolicy, path: &str) -> Result<reqwest::Url> {
    if !safe_relative_path(path) {
        return Err(anyhow!("unsafe scenario path"));
    }
    let base = reqwest::Url::parse(policy.base_url.as_deref().context("base_url missing")?)?;
    let target = base.join(path)?;
    let host = target.host_str().unwrap_or_default();
    if !policy.allowed_hosts.iter().any(|allowed| allowed == host)
        || target.host_str() != base.host_str()
        || target.port_or_known_default() != base.port_or_known_default()
    {
        return Err(anyhow!("target escaped the authorized origin"));
    }
    Ok(target)
}

fn read_response_bounded(
    response: &mut reqwest::blocking::Response,
    maximum: usize,
) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    response
        .take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut body)?;
    if body.len() > maximum {
        return Err(anyhow!("response exceeded configured byte limit"));
    }
    Ok(body)
}

fn hash_security_evidence(
    change_run_id: Uuid,
    source_revision_id: &str,
    results: &[SecurityScenarioResult],
) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(
        &serde_json::json!({
            "change_run_id": change_run_id,
            "source_revision_id": source_revision_id,
            "results": results,
        }),
    )?)))
}

pub fn ingest_runtime_trace(
    paths: &ProjectPaths,
    change_run_id: Uuid,
    source_revision_id: &str,
    workload: &str,
    adapter: &str,
    input: &Path,
) -> Result<RuntimeRun> {
    let normalized = normalize_trace_file(input, workload, adapter == "http_trace")?;
    let run = RuntimeRun {
        runtime_run_id: Uuid::new_v4(),
        source_revision_id: source_revision_id.into(),
        workload: workload.into(),
        observed_at: Utc::now(),
        coverage_scope: serde_json::to_value(&normalized.coverage)?,
        adapter: adapter.into(),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        edges: normalized
            .edges
            .into_iter()
            .map(|edge| RuntimeEdge {
                source_symbol: edge.source_symbol,
                target_symbol: edge.target_symbol,
                kind: edge.kind,
                count: edge.count,
            })
            .collect(),
        status: normalized.coverage.status,
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("runtime-{change_run_id}.json")),
        &serde_json::to_vec_pretty(&run)?,
    )?;
    Ok(run)
}

pub fn analyze_dataflow(
    paths: &ProjectPaths,
    change_run_id: Uuid,
    source_revision_id: &str,
    source_symbol: &str,
    sink_symbol: &str,
    declared_scope_complete: bool,
) -> Result<DataFlowResult> {
    let store = GraphStore::open(&paths.graph_path)?;
    let coverage = store.coverage()?;
    let path = store.find_flow_path(source_symbol, sink_symbol, 24)?;
    let scope_complete = declared_scope_complete && coverage.absence_scope_complete();
    let verdict = if !path.is_empty() {
        "FOUND"
    } else if scope_complete {
        "NO_FLOW"
    } else {
        "UNKNOWN"
    };
    let result = DataFlowResult {
        dataflow_run_id: Uuid::new_v4(),
        source_revision_id: source_revision_id.into(),
        source: source_symbol.into(),
        sink: sink_symbol.into(),
        declared_scope: serde_json::json!({
            "declared_complete": declared_scope_complete,
            "analysis_complete": scope_complete,
            "max_depth": 24,
            "relationship_scope": "flow_call_route_and_current_runtime_edges",
        }),
        verdict: verdict.into(),
        path,
        unknowns: if verdict == "UNKNOWN" {
            vec!["Dynamic or unsupported analysis area prevents a NO_FLOW claim.".into()]
        } else {
            vec![]
        },
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("dataflow-{change_run_id}.json")),
        &serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

fn read_bounded(path: &Path, max: usize) -> Result<String> {
    let mut file = File::open(path)?;
    let mut bytes = vec![];
    file.by_ref().take(max as u64).read_to_end(&mut bytes)?;
    let mut value = String::from_utf8_lossy(&bytes).to_string();
    if file.read(&mut [0_u8; 1])? > 0 {
        value.push_str("\n… output truncated by Code Intelligence …");
    }
    Ok(value)
}

fn redact_output(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let lower = line.to_lowercase();
            if [
                "token=",
                "password=",
                "secret=",
                "authorization:",
                "api_key=",
            ]
            .iter()
            .any(|marker| lower.contains(marker))
            {
                "[REDACTED SENSITIVE LINE]".into()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn environment_profile_hash(root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(std::env::consts::ARCH.as_bytes());
    hasher.update(root.to_string_lossy().as_bytes());
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeintel_core::{SecurityAuthorization, SourceRevision};
    use codeintel_graph::{edge, EdgeEvidence, GraphBuilder, GraphNode, NodeProvenance};
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn complete_empty_graph(paths: &ProjectPaths) {
        fs::create_dir_all(&paths.reports_dir).unwrap();
        let builder = GraphBuilder::create(&paths.graph_path).unwrap();
        builder
            .validate(
                &SourceRevision {
                    source_revision_id: "rev".into(),
                    inventory_hash: "inventory".into(),
                    policy_hash: "policy".into(),
                    indexer_version: "test".into(),
                    root_mapping_version: 1,
                    files: vec![],
                    volatile_files: vec![],
                    created_at: Utc::now(),
                },
                &Coverage {
                    supported_languages: vec!["python".into()],
                    semantic: "COMPLETE".into(),
                    framework: "COMPLETE".into(),
                    ..Coverage::default()
                },
            )
            .unwrap();
    }

    fn complete_security_graph(paths: &ProjectPaths) {
        let mut builder = GraphBuilder::create(&paths.graph_path).unwrap();
        let tx = builder.begin().unwrap();
        for (id, kind, name) in [
            ("route", "ROUTE", "GET /api/users/{id}"),
            ("handler", "FUNCTION", "get_user"),
        ] {
            tx.add_node(&GraphNode {
                id: id.into(),
                kind: kind.into(),
                qualified_name: name.into(),
                display_name: name.into(),
                file_path: "app.py".into(),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 1,
                signature: None,
                language: "python".into(),
                exported: true,
                entry_point: kind == "ROUTE",
                logical_root: "primary".into(),
                canonical_identity: format!("test:{id}"),
                provenance: vec![NodeProvenance {
                    origin: "STATIC_EXACT".into(),
                    producer: "test".into(),
                }],
            })
            .unwrap();
        }
        tx.add_edge(&edge(EdgeEvidence {
            source_id: "route",
            target_id: "handler",
            kind: "ROUTES_TO",
            origin: "FRAMEWORK_RESOLVED",
            confidence: 1.0,
            source_file: "app.py",
            source_range: "1",
            revision: "revision",
            ambiguity_reason: None,
        }))
        .unwrap();
        tx.commit().unwrap();
        builder
            .validate(
                &SourceRevision {
                    source_revision_id: "revision".into(),
                    inventory_hash: "inventory".into(),
                    policy_hash: "policy".into(),
                    indexer_version: "test".into(),
                    root_mapping_version: 1,
                    files: vec![],
                    volatile_files: vec![],
                    created_at: Utc::now(),
                },
                &Coverage {
                    supported_languages: vec!["python".into()],
                    semantic: "COMPLETE".into(),
                    framework: "COMPLETE".into(),
                    ..Coverage::default()
                },
            )
            .unwrap();
    }

    fn test_paths(root: &Path) -> ProjectPaths {
        ProjectPaths {
            project_root: root.into(),
            manifest_dir: root.join(".code-intel"),
            data_dir: root.join("data"),
            graph_path: root.join("data/graph.sqlite3"),
            content_cache: root.join("data/content-cache"),
            reports_dir: root.join("data/reports"),
            spool_dir: root.join("data/spool"),
            locks_dir: root.join("data/locks"),
        }
    }

    fn safe_security_policy(base_url: String) -> SecurityScenariosPolicy {
        SecurityScenariosPolicy {
            enabled: true,
            mode: "lab".into(),
            environment: "lab".into(),
            authorization: SecurityAuthorization {
                status: "granted".into(),
                scope_ref: Some("owner-approved-local-test".into()),
                expires_at: Some(Utc::now() + chrono::Duration::minutes(5)),
            },
            base_url: Some(base_url),
            allowed_hosts: vec!["127.0.0.1".into()],
            deny_production: true,
            allow_state_changing: false,
            max_requests: 10,
            max_concurrency: 1,
            timeout_sec: 3,
            max_response_bytes: 1024,
            local_evidence_only: true,
            require_correlation: true,
            actors: vec![],
            endpoint_inventory: vec!["users-read".into()],
            scenarios: vec![SecurityScenario {
                id: "anonymous-read".into(),
                endpoint_id: "users-read".into(),
                case: "anonymous".into(),
                method: "GET".into(),
                path: "/api/users/742".into(),
                route_symbol: Some("GET /api/users/{id}".into()),
                actor: None,
                query: BTreeMap::new(),
                json_body: None,
                expect_status: vec![401],
                expect_terminal_status: vec![],
                repeat: 1,
                parallel_burst: false,
                allow_state_change: false,
                reuse_idempotency_key: false,
                cost_probe: None,
            }],
            exclusions: SECURITY_CASES
                .iter()
                .filter(|case| **case != "anonymous")
                .map(|case| codeintel_core::SecurityScenarioExclusion {
                    endpoint_id: "users-read".into(),
                    case: (*case).into(),
                    reason: "not applicable to this read-only unit-test fixture".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn secrets_are_redacted_from_command_output() {
        let output = redact_output("ok\nTOKEN=abc\npassword=hunter2\ndone");
        assert!(!output.contains("abc"));
        assert!(!output.contains("hunter2"));
    }

    #[test]
    fn security_policy_defaults_to_non_executing_local_only() {
        let policy = SecurityScenariosPolicy::default();
        assert!(!policy.enabled);
        assert_eq!(policy.mode, "plan_only");
        assert!(policy.local_evidence_only);
        assert_eq!(policy.max_concurrency, 1);
    }

    #[test]
    fn security_policy_rejects_secret_bodies_and_origin_escape() {
        let mut policy = safe_security_policy("http://127.0.0.1:3000".into());
        policy.scenarios[0].path = "//outside.example/api".into();
        policy.scenarios[0].json_body = Some(serde_json::json!({"api_key": "inline"}));
        let blockers = validate_security_scenarios(&policy, Utc::now(), true);
        assert!(blockers.iter().any(|item| item.contains("unsafe path")));
        assert!(blockers.iter().any(|item| item.contains("secret-like key")));
    }

    #[test]
    fn security_policy_requires_explicit_authorization_for_execution() {
        let mut policy = safe_security_policy("http://127.0.0.1:3000".into());
        policy.authorization.status = "pending".into();
        let blockers = validate_security_scenarios(&policy, Utc::now(), true);
        assert!(blockers.iter().any(|item| item.contains("not granted")));
        assert!(!validate_security_scenarios(&policy, Utc::now(), false)
            .iter()
            .any(|item| item.contains("not granted")));
    }

    #[test]
    fn incomplete_endpoint_matrix_is_denied_not_assumed_safe() {
        let mut policy = safe_security_policy("http://127.0.0.1:3000".into());
        policy.exclusions.clear();
        let blockers = validate_security_scenarios(&policy, Utc::now(), false);
        assert!(blockers
            .iter()
            .any(|item| item.contains("missing security case foreign_object")));
    }

    #[test]
    fn security_runner_keeps_response_body_out_of_evidence() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 18\r\nConnection: close\r\n\r\nsensitive-response",
                )
                .unwrap();
        });
        let policy = safe_security_policy(format!("http://{address}"));
        let dir = tempdir().unwrap();
        let paths = test_paths(dir.path());
        fs::create_dir_all(&paths.reports_dir).unwrap();
        let report =
            run_security_scenarios(&paths, Uuid::new_v4(), "revision", &policy, &[], true).unwrap();
        server.join().unwrap();
        assert_eq!(report.status, "PASS");
        assert_eq!(report.executed_requests, 1);
        let encoded = serde_json::to_string(&report).unwrap();
        assert!(!encoded.contains("sensitive-response"));
        assert!(report.results[0].attempts[0].body_sha256.is_some());
        complete_security_graph(&paths);
        let correlation =
            correlate_security_scenarios(&paths, report.change_run_id, "revision", &policy, &[])
                .unwrap();
        assert_eq!(correlation.status, "PASS");
        assert_eq!(correlation.correlations[0].correlation_status, "FOUND");
        assert!(!serde_json::to_string(&correlation)
            .unwrap()
            .contains("sensitive-response"));
    }

    #[test]
    fn security_runner_detects_paid_api_budget_delta() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut used = 0;
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let read = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..read]);
                if request.starts_with("POST /api/chat ") {
                    used = 5;
                }
                let body = format!("{{\"used\":{used}}}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        let mut policy = safe_security_policy(format!("http://{address}"));
        policy.allow_state_changing = true;
        policy.actors.push(SecurityActor {
            id: "meter".into(),
            bearer_token_env: None,
            headers_from_env: BTreeMap::new(),
        });
        policy.scenarios[0].id = "paid-chat".into();
        policy.scenarios[0].endpoint_id = "users-read".into();
        policy.scenarios[0].method = "POST".into();
        policy.scenarios[0].path = "/api/chat".into();
        policy.scenarios[0].route_symbol = Some("POST /api/chat".into());
        policy.scenarios[0].allow_state_change = true;
        policy.scenarios[0].expect_status = vec![200];
        policy.scenarios[0].cost_probe = Some(SecurityCostProbe {
            path: "/api/meter".into(),
            actor: "meter".into(),
            json_pointer: "/used".into(),
            max_delta: 0.0,
        });
        let dir = tempdir().unwrap();
        let paths = test_paths(dir.path());
        fs::create_dir_all(&paths.reports_dir).unwrap();
        let report =
            run_security_scenarios(&paths, Uuid::new_v4(), "revision", &policy, &[], true).unwrap();
        server.join().unwrap();
        assert_eq!(report.status, "FAIL");
        assert_eq!(report.results[0].observed_cost_delta, Some(5.0));
        assert!(report.results[0]
            .assertion_failures
            .iter()
            .any(|failure| failure.contains("cost delta")));
    }

    #[test]
    fn bounded_parallel_burst_exposes_missing_atomic_rate_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let server_active = Arc::clone(&active);
        let server_maximum = Arc::clone(&maximum);
        let server = std::thread::spawn(move || {
            let mut handlers = vec![];
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let active = Arc::clone(&server_active);
                let maximum = Arc::clone(&server_maximum);
                handlers.push(std::thread::spawn(move || {
                    let mut request = [0_u8; 1024];
                    let _ = stream.read(&mut request).unwrap();
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(50));
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                        )
                        .unwrap();
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        let mut policy = safe_security_policy(format!("http://{address}"));
        policy.max_concurrency = 3;
        policy.scenarios[0].id = "quota-race".into();
        policy.scenarios[0].case = "rapid_repeat".into();
        policy.scenarios[0].repeat = 3;
        policy.scenarios[0].parallel_burst = true;
        policy.scenarios[0].expect_status = vec![200];
        policy.scenarios[0].expect_terminal_status = vec![429];
        policy.exclusions = SECURITY_CASES
            .iter()
            .filter(|case| **case != "rapid_repeat")
            .map(|case| codeintel_core::SecurityScenarioExclusion {
                endpoint_id: "users-read".into(),
                case: (*case).into(),
                reason: "not applicable to this bounded parallel unit-test fixture".into(),
            })
            .collect();
        let dir = tempdir().unwrap();
        let paths = test_paths(dir.path());
        fs::create_dir_all(&paths.reports_dir).unwrap();
        let report =
            run_security_scenarios(&paths, Uuid::new_v4(), "revision", &policy, &[], true).unwrap();
        server.join().unwrap();
        assert_eq!(report.status, "FAIL");
        assert_eq!(report.executed_requests, 3);
        assert!(maximum.load(Ordering::SeqCst) >= 2);
        assert!(report.results[0]
            .assertion_failures
            .iter()
            .any(|failure| failure.contains("no terminal rate-limit status")));
    }

    #[test]
    fn security_case_catalog_matches_required_endpoint_matrix() {
        assert_eq!(SECURITY_CASES.len(), 12);
        assert!(SECURITY_CASES.contains(&"foreign_object"));
        assert!(SECURITY_CASES.contains(&"rapid_repeat"));
        assert!(SECURITY_CASES.contains(&"direct_api"));
    }

    #[test]
    fn runtime_not_observed_is_never_absence() {
        let scope = serde_json::json!({"not_observed_semantics": "NOT_OBSERVED"});
        assert_eq!(scope["not_observed_semantics"], "NOT_OBSERVED");
    }

    #[test]
    fn semantic_unknown_in_changed_file_blocks_completion() {
        let coverage = Coverage {
            semantic: "COMPLETE".into(),
            supported_languages: vec!["python".into()],
            semantic_unresolved_files: vec!["src/payment.py".into()],
            ..Coverage::default()
        };
        let changed = BTreeSet::from(["src/payment.py".into()]);
        let (complete, unknowns, _) = semantic_scope_evidence(&coverage, &changed);
        assert!(!complete);
        assert_eq!(unknowns.len(), 1);
    }

    #[test]
    fn semantic_unknown_outside_changed_scope_is_warning_only() {
        let coverage = Coverage {
            semantic: "COMPLETE".into(),
            supported_languages: vec!["python".into()],
            semantic_unresolved_files: vec!["legacy/dynamic.py".into()],
            ..Coverage::default()
        };
        let changed = BTreeSet::from(["src/payment.py".into()]);
        let (complete, unknowns, warnings) = semantic_scope_evidence(&coverage, &changed);
        assert!(complete);
        assert!(unknowns.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn global_partial_semantics_outside_changed_scope_is_warning_only() {
        let coverage = Coverage {
            semantic: "PARTIAL".into(),
            supported_languages: vec!["python".into()],
            failed_files: vec!["legacy/broken.py: parser error".into()],
            ..Coverage::default()
        };
        let changed = BTreeSet::from(["src/payment.py".into()]);
        let (complete, unknowns, warnings) = semantic_scope_evidence(&coverage, &changed);
        assert!(complete);
        assert!(unknowns.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn no_flow_requires_declared_and_analyzed_complete_scope() {
        let dir = tempdir().unwrap();
        let paths = test_paths(dir.path());
        complete_empty_graph(&paths);
        let complete = analyze_dataflow(
            &paths,
            Uuid::new_v4(),
            "rev",
            "external_input",
            "database_write",
            true,
        )
        .unwrap();
        assert_eq!(complete.verdict, "NO_FLOW");
        assert!(complete.unknowns.is_empty());

        let undeclared = analyze_dataflow(
            &paths,
            Uuid::new_v4(),
            "rev",
            "external_input",
            "database_write",
            false,
        )
        .unwrap();
        assert_eq!(undeclared.verdict, "UNKNOWN");
        assert!(!undeclared.unknowns.is_empty());
    }
}
