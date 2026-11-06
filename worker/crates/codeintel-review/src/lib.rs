use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use codeintel_core::{
    assess_risk, atomic_write, cache_content, compute_source_revision, safe_read, ChangeRun,
    ChangeRunStatus, Policy, ProjectManifest, ProjectPaths, RiskAssessment, SourceRevision,
};
use codeintel_graph::{Coverage, GraphStore};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightResult {
    pub change_run: ChangeRun,
    pub current_revision: SourceRevision,
    pub planned_files: Vec<String>,
    pub baseline_recommended: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDelta {
    pub path: String,
    pub status: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub before_range: Option<String>,
    pub after_range: Option<String>,
    pub excerpt: Option<String>,
    pub excerpt_truncated: bool,
    pub excerpt_omitted_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub finding_id: Uuid,
    pub severity: String,
    pub category: String,
    pub path: Option<String>,
    pub source_range: Option<String>,
    pub why: String,
    pub evidence: serde_json::Value,
    pub suggested_repair: Option<String>,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewBundle {
    pub change_run_id: Uuid,
    pub before_source_revision_id: String,
    pub after_source_revision_id: String,
    pub change_summary: String,
    pub deltas: Vec<FileDelta>,
    pub impacted_symbols: Vec<serde_json::Value>,
    pub critical_callers_and_routes: Vec<serde_json::Value>,
    pub mapped_tests: Vec<String>,
    pub test_gaps: Vec<String>,
    pub risk: RiskAssessment,
    pub runtime_relevance: Vec<String>,
    pub known_unknowns: Vec<String>,
    pub findings: Vec<Finding>,
    pub context_contract: String,
}

pub struct PreflightRequest<'a> {
    pub root: &'a Path,
    pub manifest: &'a ProjectManifest,
    pub policy: &'a Policy,
    pub paths: &'a ProjectPaths,
    pub instance_id: Uuid,
    pub task_summary: &'a str,
    pub planned_files: &'a [String],
    pub agent_client: Option<String>,
}

pub fn preflight_change(request: PreflightRequest<'_>) -> Result<PreflightResult> {
    let PreflightRequest {
        root,
        manifest,
        policy,
        paths,
        instance_id,
        task_summary,
        planned_files,
        agent_client,
    } = request;
    if active_change_run(paths)?.is_some() {
        return Err(anyhow!(
            "IN_USE: another ChangeRun is active for this ProjectInstance"
        ));
    }
    let revision = compute_source_revision(root, policy)?;
    if !revision.volatile_files.is_empty() {
        return Err(anyhow!("Cannot preflight while source files are volatile"));
    }
    fs::create_dir_all(&paths.reports_dir)?;
    for entry in &revision.files {
        let absolute = root.join(&entry.logical_path);
        if let Ok((bytes, _)) = safe_read(&absolute, 3) {
            cache_content(&paths.content_cache, &entry.content_sha256, &bytes)?;
        }
    }
    let has_tests = revision
        .files
        .iter()
        .any(|entry| is_test_path(&entry.logical_path));
    let coverage_partial = current_coverage(paths)
        .map(|coverage| !coverage.absence_scope_complete())
        .unwrap_or(true);
    let risk = apply_policy_checks(
        assess_risk(planned_files, task_summary, coverage_partial, has_tests),
        policy,
        planned_files,
        task_summary,
    );
    let run = ChangeRun {
        change_run_id: Uuid::new_v4(),
        project_id: manifest.project_id,
        instance_id,
        agent_client,
        agent_session_id: None,
        task_summary: task_summary.into(),
        before_source_revision_id: revision.source_revision_id.clone(),
        after_source_revision_id: None,
        risk_before: risk.clone(),
        risk_after: None,
        required_checks: risk.required_checks.clone(),
        evidence_ids: vec![],
        finding_ids: vec![],
        quality_run_id: None,
        manual_snapshot_reference: None,
        started_at: Utc::now(),
        completed_at: None,
        closure_reason: None,
        status: ChangeRunStatus::Preflighted,
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("change-run-{}.json", run.change_run_id)),
        &serde_json::to_vec_pretty(&run)?,
    )?;
    atomic_write(
        &paths.locks_dir.join("active-change-run"),
        run.change_run_id.to_string().as_bytes(),
    )?;
    save_before_revision(paths, &run, &revision)?;
    Ok(PreflightResult {
        change_run: run,
        current_revision: revision,
        planned_files: planned_files.to_vec(),
        baseline_recommended: risk.risk_score >= 25,
        warnings: if coverage_partial {
            vec!["Graph/semantic coverage is partial; absence claims remain unproven.".into()]
        } else {
            vec![]
        },
    })
}

pub fn review_change(
    root: &Path,
    policy: &Policy,
    paths: &ProjectPaths,
    change_run_id: Uuid,
) -> Result<ReviewBundle> {
    let run_path = paths
        .reports_dir
        .join(format!("change-run-{change_run_id}.json"));
    let mut run: ChangeRun =
        serde_json::from_slice(&fs::read(&run_path).context("read ChangeRun")?)?;
    if !matches!(
        run.status,
        ChangeRunStatus::Preflighted
            | ChangeRunStatus::Editing
            | ChangeRunStatus::Reviewing
            | ChangeRunStatus::Verifying
    ) {
        return Err(anyhow!(
            "ChangeRun is not reviewable in state {:?}",
            run.status
        ));
    }
    let after = compute_source_revision(root, policy)?;
    if !after.volatile_files.is_empty() {
        return Err(anyhow!("SOURCE_DRIFT: source was volatile during review"));
    }
    let before = load_before_revision(paths, &run.before_source_revision_id, &run_path)?;
    let before_map: BTreeMap<_, _> = before
        .files
        .iter()
        .map(|entry| (entry.logical_path.as_str(), entry))
        .collect();
    let after_map: BTreeMap<_, _> = after
        .files
        .iter()
        .map(|entry| (entry.logical_path.as_str(), entry))
        .collect();
    let paths_changed: BTreeSet<_> = before_map
        .keys()
        .chain(after_map.keys())
        .filter(|path| {
            before_map.get(**path).map(|entry| &entry.content_sha256)
                != after_map.get(**path).map(|entry| &entry.content_sha256)
        })
        .copied()
        .collect();
    let mut deltas = vec![];
    let mut findings = vec![];
    for path in paths_changed {
        let before_entry = before_map.get(path).copied();
        let after_entry = after_map.get(path).copied();
        let status = match (before_entry, after_entry) {
            (None, Some(_)) => "ADDED",
            (Some(_), None) => "REMOVED",
            _ => "MODIFIED",
        };
        let before_content = before_entry
            .and_then(|entry| {
                codeintel_core::read_cached_content(&paths.content_cache, &entry.content_sha256)
                    .ok()
            })
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let after_content = after_entry
            .and_then(|entry| safe_read(&root.join(&entry.logical_path), 3).ok())
            .and_then(|(bytes, _)| String::from_utf8(bytes).ok());
        let (before_range, after_range, mut excerpt, excerpt_truncated) =
            changed_range(before_content.as_deref(), after_content.as_deref());
        let excerpt_omitted_reason = if deltas.len() >= 12 && excerpt.is_some() {
            excerpt = None;
            Some("Review-wide excerpt budget reached; inspect the current source directly.".into())
        } else {
            None
        };
        deltas.push(FileDelta {
            path: path.into(),
            status: status.into(),
            before_hash: before_entry.map(|entry| entry.content_sha256.clone()),
            after_hash: after_entry.map(|entry| entry.content_sha256.clone()),
            before_range,
            after_range,
            excerpt,
            excerpt_truncated,
            excerpt_omitted_reason,
        });
        if status == "REMOVED" && is_source_path(path) {
            findings.push(Finding {
                finding_id: Uuid::new_v4(),
                severity: "HIGH".into(),
                category: "SOURCE_REMOVAL_REQUIRES_PROOF".into(),
                path: Some(path.into()),
                source_range: None,
                why: "A source file was removed. Empty caller results cannot prove safe deletion.".into(),
                evidence: serde_json::json!({"status": "REMOVED", "absence_proven": false}),
                suggested_repair: Some(
                    "Inspect framework/config registrations, callers, tests and runtime evidence before keeping the removal."
                        .into(),
                ),
                blocking: true,
            });
        }
    }
    let changed_paths: Vec<String> = deltas.iter().map(|delta| delta.path.clone()).collect();
    let has_tests = after
        .files
        .iter()
        .any(|entry| is_test_path(&entry.logical_path));
    let coverage = current_coverage(paths);
    let coverage_partial = coverage
        .as_ref()
        .map(|item| !item.absence_scope_complete())
        .unwrap_or(true);
    let risk = apply_policy_checks(
        assess_risk(
            &changed_paths,
            &run.task_summary,
            coverage_partial,
            has_tests,
        ),
        policy,
        &changed_paths,
        &run.task_summary,
    );
    let mapped_tests: Vec<String> = after
        .files
        .iter()
        .filter(|entry| is_test_path(&entry.logical_path))
        .filter(|entry| {
            changed_paths
                .iter()
                .any(|path| likely_related_test(path, &entry.logical_path))
        })
        .map(|entry| entry.logical_path.clone())
        .take(50)
        .collect();
    let test_gaps =
        if changed_paths.iter().any(|path| is_source_path(path)) && mapped_tests.is_empty() {
            vec![
                "Changed source has no mapped test; this is not equivalent to 'no tests needed'."
                    .into(),
            ]
        } else {
            vec![]
        };
    let mut impacted_symbols = vec![];
    let mut callers = vec![];
    let mut impacted_ids = BTreeSet::new();
    let mut caller_keys = BTreeSet::new();
    if let Ok(store) = GraphStore::open(&paths.graph_path) {
        for delta in deltas.iter().take(20) {
            if let Ok(matches) = store.find_symbol(&delta.path, 10) {
                for symbol in matches.items {
                    if impacted_ids.insert(symbol.id.clone()) && impacted_symbols.len() < 50 {
                        impacted_symbols.push(serde_json::json!({
                            "id": symbol.id,
                            "kind": symbol.kind,
                            "qualified_name": symbol.qualified_name,
                            "file_path": symbol.file_path,
                            "start_line": symbol.start_line,
                            "language": symbol.language,
                        }));
                    }
                    if let Ok(result) = store.neighbors(&symbol.id, "callers", 20) {
                        for caller in result.items {
                            let key = format!(
                                "{}|{}|{}|{}",
                                caller["symbol_id"].as_str().unwrap_or_default(),
                                caller["edge_kind"].as_str().unwrap_or_default(),
                                caller["path"].as_str().unwrap_or_default(),
                                caller["line"].as_u64().unwrap_or_default()
                            );
                            if caller_keys.insert(key) && callers.len() < 50 {
                                callers.push(caller);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut unknowns = vec![];
    if coverage_partial {
        unknowns
            .push("Graph/semantic/framework coverage is partial; absence is not proven.".into());
    }
    unknowns.extend(test_gaps.clone());
    let runtime_relevance = if risk.required_checks.contains(&"runtime_trace".to_string()) {
        vec!["Risk engine requires runtime evidence for the changed dynamic/critical flow.".into()]
    } else {
        vec![]
    };
    run.after_source_revision_id = Some(after.source_revision_id.clone());
    run.risk_after = Some(risk.clone());
    run.required_checks = risk.required_checks.clone();
    run.status = ChangeRunStatus::Reviewing;
    run.finding_ids = findings.iter().map(|finding| finding.finding_id).collect();
    atomic_write(&run_path, &serde_json::to_vec_pretty(&run)?)?;
    let bundle = ReviewBundle {
        change_run_id,
        before_source_revision_id: run.before_source_revision_id.clone(),
        after_source_revision_id: after.source_revision_id.clone(),
        change_summary: format!(
            "{} files changed: {} added, {} modified, {} removed.",
            deltas.len(),
            deltas.iter().filter(|delta| delta.status == "ADDED").count(),
            deltas.iter().filter(|delta| delta.status == "MODIFIED").count(),
            deltas.iter().filter(|delta| delta.status == "REMOVED").count(),
        ),
        deltas,
        impacted_symbols,
        critical_callers_and_routes: callers,
        mapped_tests,
        test_gaps,
        risk,
        runtime_relevance,
        known_unknowns: unknowns,
        findings,
        context_contract: "This bundle is bounded: at most 12 excerpts of 800 characters and compact graph metadata, never whole source bodies. Inspect actual current source before repair."
            .into(),
    };
    atomic_write(
        &paths
            .reports_dir
            .join(format!("review-{change_run_id}.json")),
        &serde_json::to_vec_pretty(&bundle)?,
    )?;
    Ok(bundle)
}

fn apply_policy_checks(
    mut risk: RiskAssessment,
    policy: &Policy,
    paths: &[String],
    intent: &str,
) -> RiskAssessment {
    let surface = format!("{} {intent}", paths.join(" ")).to_ascii_lowercase();
    let security_relevant = [
        "api/",
        "endpoint",
        "route",
        "controller",
        "auth",
        "permission",
        "token",
        "billing",
        "budget",
        "webhook",
    ]
    .iter()
    .any(|marker| surface.contains(marker));
    if policy.security_scenarios.enabled && security_relevant {
        risk.reasons
            .push("authorized security scenario policy applies".into());
        risk.required_checks.push("security_scenarios".into());
        risk.reasons.sort();
        risk.reasons.dedup();
        risk.required_checks.sort();
        risk.required_checks.dedup();
    }
    risk
}

pub fn save_before_revision(
    paths: &ProjectPaths,
    run: &ChangeRun,
    revision: &SourceRevision,
) -> Result<()> {
    atomic_write(
        &paths
            .reports_dir
            .join(format!("before-revision-{}.json", run.change_run_id)),
        &serde_json::to_vec_pretty(revision)?,
    )
}

pub fn active_change_run(paths: &ProjectPaths) -> Result<Option<Uuid>> {
    let lock = paths.locks_dir.join("active-change-run");
    if !lock.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(lock)?;
    Ok(Some(Uuid::parse_str(raw.trim())?))
}

pub fn close_change_run(
    paths: &ProjectPaths,
    change_run_id: Uuid,
    status: ChangeRunStatus,
) -> Result<()> {
    let run_path = paths
        .reports_dir
        .join(format!("change-run-{change_run_id}.json"));
    let mut run: ChangeRun = serde_json::from_slice(&fs::read(&run_path)?)?;
    run.status = status;
    run.completed_at = Some(Utc::now());
    atomic_write(&run_path, &serde_json::to_vec_pretty(&run)?)?;
    let lock = paths.locks_dir.join("active-change-run");
    if lock.exists() {
        let active = fs::read_to_string(&lock)?;
        if active.trim() == change_run_id.to_string() {
            fs::remove_file(lock)?;
        }
    }
    Ok(())
}

pub fn abandon_change_run(
    paths: &ProjectPaths,
    change_run_id: Uuid,
    reason: &str,
) -> Result<ChangeRun> {
    if reason.trim().len() < 8 || reason.len() > 500 {
        return Err(anyhow!("abandon reason must contain 8 to 500 characters"));
    }
    let run_path = paths
        .reports_dir
        .join(format!("change-run-{change_run_id}.json"));
    let mut run: ChangeRun = serde_json::from_slice(&fs::read(&run_path)?)?;
    let active = active_change_run(paths)?;
    if active != Some(change_run_id) {
        return Err(anyhow!("ChangeRun is not the active run for this project"));
    }
    if matches!(run.status, ChangeRunStatus::Gated | ChangeRunStatus::Closed) {
        return Err(anyhow!("completed ChangeRun cannot be abandoned"));
    }
    run.status = ChangeRunStatus::Abandoned;
    run.completed_at = Some(Utc::now());
    run.closure_reason = Some(reason.trim().into());
    atomic_write(&run_path, &serde_json::to_vec_pretty(&run)?)?;
    let lock = paths.locks_dir.join("active-change-run");
    if lock.exists() {
        fs::remove_file(lock)?;
    }
    Ok(run)
}

fn load_before_revision(
    paths: &ProjectPaths,
    revision_id: &str,
    run_path: &Path,
) -> Result<SourceRevision> {
    let run: ChangeRun = serde_json::from_slice(&fs::read(run_path)?)?;
    let path = paths
        .reports_dir
        .join(format!("before-revision-{}.json", run.change_run_id));
    let revision: SourceRevision = serde_json::from_slice(
        &fs::read(&path)
            .context("before revision inventory is missing; preflight must pin it before edits")?,
    )?;
    if revision.source_revision_id != revision_id {
        return Err(anyhow!("Pinned before revision does not match ChangeRun"));
    }
    Ok(revision)
}

fn current_coverage(paths: &ProjectPaths) -> Option<Coverage> {
    GraphStore::open(&paths.graph_path)
        .and_then(|store| store.coverage())
        .ok()
}

fn changed_range(
    before: Option<&str>,
    after: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>, bool) {
    let before_lines: Vec<_> = before.unwrap_or("").lines().collect();
    let after_lines: Vec<_> = after.unwrap_or("").lines().collect();
    let mut prefix = 0;
    while prefix < before_lines.len()
        && prefix < after_lines.len()
        && before_lines[prefix] == after_lines[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < before_lines.len().saturating_sub(prefix)
        && suffix < after_lines.len().saturating_sub(prefix)
        && before_lines[before_lines.len() - suffix - 1]
            == after_lines[after_lines.len() - suffix - 1]
    {
        suffix += 1;
    }
    let before_end = before_lines.len().saturating_sub(suffix);
    let after_end = after_lines.len().saturating_sub(suffix);
    let excerpt_start = prefix.saturating_sub(2);
    let excerpt_end = (after_end + 2).min(after_lines.len());
    let raw_excerpt = after_lines[excerpt_start..excerpt_end].join("\n");
    let excerpt_truncated = raw_excerpt.chars().count() > 800;
    let excerpt = (!after_lines.is_empty()).then(|| raw_excerpt.chars().take(800).collect());
    (
        before.map(|_| format!("{}-{}", prefix + 1, before_end.max(prefix + 1))),
        after.map(|_| format!("{}-{}", prefix + 1, after_end.max(prefix + 1))),
        excerpt,
        excerpt_truncated,
    )
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("/test")
        || lower.starts_with("test")
        || lower.contains("__tests__")
        || lower.contains(".spec.")
        || lower.contains(".test.")
}

fn is_source_path(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|value| value.to_str()),
        Some("php" | "js" | "jsx" | "ts" | "tsx" | "py" | "pyi" | "cs" | "rs")
    )
}

fn likely_related_test(source: &str, test: &str) -> bool {
    let source_stem = Path::new(source)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    test.to_lowercase()
        .contains(source_stem.trim_end_matches("controller"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn diff_is_bounded() {
        let before = "a\nb\nc\n";
        let after = "a\nchanged\nc\n";
        let (before_range, after_range, excerpt, truncated) =
            changed_range(Some(before), Some(after));
        assert_eq!(before_range.as_deref(), Some("2-2"));
        assert_eq!(after_range.as_deref(), Some("2-2"));
        assert!(excerpt.unwrap().contains("changed"));
        assert!(!truncated);
    }

    #[test]
    fn one_line_diff_excerpt_is_character_bounded() {
        let after = "x".repeat(10_000);
        let (_, _, excerpt, truncated) = changed_range(Some("old"), Some(&after));
        assert!(truncated);
        assert_eq!(excerpt.unwrap().chars().count(), 800);
    }

    #[test]
    fn abandoning_active_run_preserves_report_and_releases_lock() {
        let dir = tempdir().unwrap();
        let paths = ProjectPaths {
            project_root: dir.path().into(),
            manifest_dir: dir.path().join(".code-intel"),
            data_dir: dir.path().join("data"),
            graph_path: dir.path().join("data/graph.sqlite3"),
            content_cache: dir.path().join("data/content-cache"),
            reports_dir: dir.path().join("data/reports"),
            spool_dir: dir.path().join("data/spool"),
            locks_dir: dir.path().join("data/locks"),
        };
        fs::create_dir_all(&paths.reports_dir).unwrap();
        fs::create_dir_all(&paths.locks_dir).unwrap();
        let id = Uuid::new_v4();
        let risk = RiskAssessment {
            risk_score: 8,
            risk_level: "LOW".into(),
            tier: "TIER_0".into(),
            reasons: vec![],
            required_checks: vec!["actual_source_verify".into()],
        };
        let run = ChangeRun {
            change_run_id: id,
            project_id: Uuid::new_v4(),
            instance_id: Uuid::new_v4(),
            agent_client: None,
            agent_session_id: None,
            task_summary: "test stale run".into(),
            before_source_revision_id: "before".into(),
            after_source_revision_id: None,
            risk_before: risk,
            risk_after: None,
            required_checks: vec![],
            evidence_ids: vec![],
            finding_ids: vec![],
            quality_run_id: None,
            manual_snapshot_reference: None,
            started_at: Utc::now(),
            completed_at: None,
            closure_reason: None,
            status: ChangeRunStatus::Reviewing,
        };
        atomic_write(
            &paths.reports_dir.join(format!("change-run-{id}.json")),
            &serde_json::to_vec_pretty(&run).unwrap(),
        )
        .unwrap();
        atomic_write(
            &paths.locks_dir.join("active-change-run"),
            id.to_string().as_bytes(),
        )
        .unwrap();

        let abandoned = abandon_change_run(&paths, id, "superseded after explicit review").unwrap();
        assert_eq!(abandoned.status, ChangeRunStatus::Abandoned);
        assert!(abandoned.completed_at.is_some());
        assert!(!paths.locks_dir.join("active-change-run").exists());
        assert!(paths
            .reports_dir
            .join(format!("change-run-{id}.json"))
            .is_file());
    }
}
