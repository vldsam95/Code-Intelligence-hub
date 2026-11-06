use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeDetection {
    pub available: bool,
    pub adapter: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeWorkload {
    pub name: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeCoverage {
    pub status: String,
    pub workload: String,
    pub observed_records: usize,
    pub unsupported_records: usize,
    pub timed_out: bool,
    pub not_observed_semantics: String,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct NormalizedRuntimeEdge {
    pub source_symbol: String,
    pub target_symbol: String,
    pub kind: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedTrace {
    pub edges: Vec<NormalizedRuntimeEdge>,
    pub coverage: RuntimeCoverage,
}

/// Runtime collectors are short-lived adapters. Implementations must clean up even after timeout;
/// an empty observation can only mean NOT_OBSERVED for the declared workload.
pub trait RuntimeAdapter {
    fn detect(&self, project_root: &Path) -> RuntimeDetection;
    fn prepare(&mut self, workload: RuntimeWorkload) -> Result<()>;
    fn start_or_attach(&mut self) -> Result<()>;
    fn collect(&mut self) -> Result<Vec<Value>>;
    fn normalize(&self, records: &[Value]) -> Result<NormalizedTrace>;
    fn coverage(&self) -> RuntimeCoverage;
    fn cleanup(&mut self) -> Result<()>;
}

pub struct GenericTraceIngestAdapter {
    input: PathBuf,
    workload: Option<RuntimeWorkload>,
    records: Vec<Value>,
    coverage: RuntimeCoverage,
}

impl GenericTraceIngestAdapter {
    pub fn new(input: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            workload: None,
            records: vec![],
            coverage: RuntimeCoverage::default(),
        }
    }
}

impl RuntimeAdapter for GenericTraceIngestAdapter {
    fn detect(&self, _: &Path) -> RuntimeDetection {
        RuntimeDetection {
            available: self.input.is_file(),
            adapter: "generic_trace_ingest".into(),
            reason: if self.input.is_file() {
                "structured trace input exists".into()
            } else {
                "structured trace input is missing".into()
            },
        }
    }

    fn prepare(&mut self, workload: RuntimeWorkload) -> Result<()> {
        self.workload = Some(workload);
        Ok(())
    }

    fn start_or_attach(&mut self) -> Result<()> {
        if !self.input.is_file() {
            return Err(anyhow!("runtime trace input does not exist"));
        }
        Ok(())
    }

    fn collect(&mut self) -> Result<Vec<Value>> {
        self.records = parse_trace_records(&fs::read_to_string(&self.input)?)?;
        Ok(self.records.clone())
    }

    fn normalize(&self, records: &[Value]) -> Result<NormalizedTrace> {
        let workload = self
            .workload
            .as_ref()
            .map(|value| value.name.as_str())
            .unwrap_or("unspecified");
        Ok(normalize_records(records, workload, false))
    }

    fn coverage(&self) -> RuntimeCoverage {
        self.coverage.clone()
    }

    fn cleanup(&mut self) -> Result<()> {
        self.records.clear();
        Ok(())
    }
}

pub struct HttpTraceAdapter {
    inner: GenericTraceIngestAdapter,
}

impl HttpTraceAdapter {
    pub fn new(input: impl Into<PathBuf>) -> Self {
        Self {
            inner: GenericTraceIngestAdapter::new(input),
        }
    }
}

impl RuntimeAdapter for HttpTraceAdapter {
    fn detect(&self, root: &Path) -> RuntimeDetection {
        let mut detection = self.inner.detect(root);
        detection.adapter = "http_trace".into();
        detection
    }

    fn prepare(&mut self, workload: RuntimeWorkload) -> Result<()> {
        self.inner.prepare(workload)
    }

    fn start_or_attach(&mut self) -> Result<()> {
        self.inner.start_or_attach()
    }

    fn collect(&mut self) -> Result<Vec<Value>> {
        self.inner.collect()
    }

    fn normalize(&self, records: &[Value]) -> Result<NormalizedTrace> {
        let workload = self
            .inner
            .workload
            .as_ref()
            .map(|value| value.name.as_str())
            .unwrap_or("unspecified");
        Ok(normalize_records(records, workload, true))
    }

    fn coverage(&self) -> RuntimeCoverage {
        self.inner.coverage()
    }

    fn cleanup(&mut self) -> Result<()> {
        self.inner.cleanup()
    }
}

pub struct CommandWrappedRuntimeAdapter {
    argv: Vec<String>,
    cwd: PathBuf,
    workload: Option<RuntimeWorkload>,
    trace_path: PathBuf,
    child: Option<Child>,
    records: Vec<Value>,
    coverage: RuntimeCoverage,
}

impl CommandWrappedRuntimeAdapter {
    pub fn new(argv: Vec<String>, cwd: impl Into<PathBuf>) -> Result<Self> {
        if argv.is_empty() {
            return Err(anyhow!("command-wrapped runtime argv cannot be empty"));
        }
        Ok(Self {
            argv,
            cwd: cwd.into(),
            workload: None,
            trace_path: std::env::temp_dir()
                .join(format!("codeintel-runtime-{}.ndjson", Uuid::new_v4())),
            child: None,
            records: vec![],
            coverage: RuntimeCoverage::default(),
        })
    }

    pub fn trace_path(&self) -> &Path {
        &self.trace_path
    }

    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl RuntimeAdapter for CommandWrappedRuntimeAdapter {
    fn detect(&self, _: &Path) -> RuntimeDetection {
        RuntimeDetection {
            available: self.cwd.is_dir(),
            adapter: "command_wrapped_runtime".into(),
            reason: "workload command is configured and starts only on demand".into(),
        }
    }

    fn prepare(&mut self, workload: RuntimeWorkload) -> Result<()> {
        self.coverage = RuntimeCoverage {
            status: "PREPARED".into(),
            workload: workload.name.clone(),
            not_observed_semantics: "NOT_OBSERVED".into(),
            ..RuntimeCoverage::default()
        };
        self.workload = Some(workload);
        Ok(())
    }

    fn start_or_attach(&mut self) -> Result<()> {
        let workload = self
            .workload
            .as_ref()
            .ok_or_else(|| anyhow!("prepare must run before start_or_attach"))?;
        let mut command = Command::new(&self.argv[0]);
        command
            .args(&self.argv[1..])
            .current_dir(&self.cwd)
            .env("CODEINTEL_TRACE_OUTPUT", &self.trace_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.child = Some(
            command
                .spawn()
                .context("start command-wrapped runtime workload")?,
        );
        let started = Instant::now();
        loop {
            let completed = self
                .child
                .as_mut()
                .expect("runtime child exists")
                .try_wait()?;
            if completed.is_some() {
                self.child.take();
                break;
            }
            if started.elapsed() >= Duration::from_millis(workload.timeout_ms.max(1)) {
                self.coverage.timed_out = true;
                self.coverage.status = "PARTIAL".into();
                self.coverage
                    .unknowns
                    .push("Runtime workload timed out; collector was terminated.".into());
                self.stop_child();
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }

    fn collect(&mut self) -> Result<Vec<Value>> {
        self.records = if self.trace_path.is_file() {
            parse_trace_records(&fs::read_to_string(&self.trace_path)?)?
        } else {
            vec![]
        };
        Ok(self.records.clone())
    }

    fn normalize(&self, records: &[Value]) -> Result<NormalizedTrace> {
        let workload = self
            .workload
            .as_ref()
            .map(|value| value.name.as_str())
            .unwrap_or("unspecified");
        let mut normalized = normalize_records(records, workload, false);
        if self.coverage.timed_out {
            normalized.coverage.timed_out = true;
            normalized.coverage.status = "PARTIAL".into();
            normalized
                .coverage
                .unknowns
                .extend(self.coverage.unknowns.clone());
        }
        Ok(normalized)
    }

    fn coverage(&self) -> RuntimeCoverage {
        self.coverage.clone()
    }

    fn cleanup(&mut self) -> Result<()> {
        self.stop_child();
        if self.trace_path.exists() {
            fs::remove_file(&self.trace_path)?;
        }
        self.records.clear();
        Ok(())
    }
}

impl Drop for CommandWrappedRuntimeAdapter {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

pub trait LanguageRuntimePlugin {
    fn language(&self) -> &'static str;
    fn detect(&self, project_root: &Path) -> RuntimeDetection;
    fn collector_argv(&self, workload_argv: &[String], trace_output: &Path) -> Vec<String>;
}

pub struct PythonRuntimePlugin;
pub struct NodeRuntimePlugin;
pub struct PhpRuntimePlugin;

impl LanguageRuntimePlugin for PythonRuntimePlugin {
    fn language(&self) -> &'static str {
        "python"
    }

    fn detect(&self, root: &Path) -> RuntimeDetection {
        plugin_detection("python", root, &["pyproject.toml", "requirements.txt"])
    }

    fn collector_argv(&self, workload: &[String], trace_output: &Path) -> Vec<String> {
        collector_contract("python", workload, trace_output)
    }
}

impl LanguageRuntimePlugin for NodeRuntimePlugin {
    fn language(&self) -> &'static str {
        "node"
    }

    fn detect(&self, root: &Path) -> RuntimeDetection {
        plugin_detection("node", root, &["package.json"])
    }

    fn collector_argv(&self, workload: &[String], trace_output: &Path) -> Vec<String> {
        collector_contract("node", workload, trace_output)
    }
}

impl LanguageRuntimePlugin for PhpRuntimePlugin {
    fn language(&self) -> &'static str {
        "php"
    }

    fn detect(&self, root: &Path) -> RuntimeDetection {
        plugin_detection("php", root, &["composer.json", "artisan"])
    }

    fn collector_argv(&self, workload: &[String], trace_output: &Path) -> Vec<String> {
        collector_contract("php", workload, trace_output)
    }
}

pub fn normalize_trace_file(
    input: &Path,
    workload: &str,
    http_only: bool,
) -> Result<NormalizedTrace> {
    let records = parse_trace_records(&fs::read_to_string(input)?)?;
    Ok(normalize_records(&records, workload, http_only))
}

fn parse_trace_records(raw: &str) -> Result<Vec<Value>> {
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        return Ok(match value {
            Value::Array(items) => items,
            value => vec![value],
        });
    }
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("runtime input must be JSON or NDJSON"))
        .collect()
}

fn normalize_records(records: &[Value], workload: &str, http_only: bool) -> NormalizedTrace {
    let mut edges: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    let mut unsupported = 0;
    for record in records {
        let recognized = collect_direct_edge(record, http_only, &mut edges)
            | collect_otlp_edges(record, http_only, &mut edges);
        if !recognized {
            unsupported += 1;
        }
    }
    let normalized: Vec<_> = edges
        .into_iter()
        .map(
            |((source_symbol, target_symbol, kind), count)| NormalizedRuntimeEdge {
                source_symbol,
                target_symbol,
                kind,
                count,
            },
        )
        .collect();
    let mut unknowns = vec![];
    if unsupported > 0 {
        unknowns.push(format!(
            "{unsupported} runtime record(s) were outside the supported normalization schema."
        ));
    }
    if normalized.is_empty() {
        unknowns.push(
            "No path was observed for this workload; this is not proof that the path cannot execute."
                .into(),
        );
    }
    NormalizedTrace {
        coverage: RuntimeCoverage {
            status: "PARTIAL".into(),
            workload: workload.into(),
            observed_records: normalized.len(),
            unsupported_records: unsupported,
            timed_out: false,
            not_observed_semantics: "NOT_OBSERVED".into(),
            unknowns,
        },
        edges: normalized,
    }
}

fn collect_direct_edge(
    value: &Value,
    http_only: bool,
    edges: &mut BTreeMap<(String, String, String), u64>,
) -> bool {
    let Some(source) = value
        .get("source_symbol")
        .or_else(|| value.get("source"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    let Some(target) = value
        .get("target_symbol")
        .or_else(|| value.get("target"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("RUNTIME_CALLS")
        .to_uppercase();
    if http_only && kind != "HTTP_CALLS" {
        return false;
    }
    let count = value.get("count").and_then(Value::as_u64).unwrap_or(1);
    *edges
        .entry((source.into(), target.into(), normalize_kind(&kind)))
        .or_default() += count;
    true
}

fn collect_otlp_edges(
    value: &Value,
    http_only: bool,
    edges: &mut BTreeMap<(String, String, String), u64>,
) -> bool {
    let mut recognized = false;
    if let Some(resource_spans) = value.get("resourceSpans").and_then(Value::as_array) {
        for resource in resource_spans {
            let service = otlp_attribute(resource.pointer("/resource/attributes"), "service.name")
                .unwrap_or_else(|| "unknown-service".into());
            if let Some(scope_spans) = resource.get("scopeSpans").and_then(Value::as_array) {
                for scope in scope_spans {
                    if let Some(spans) = scope.get("spans").and_then(Value::as_array) {
                        for span in spans {
                            recognized |= collect_otlp_span(span, &service, http_only, edges);
                        }
                    }
                }
            }
        }
    }
    if let Some(spans) = value.get("spans").and_then(Value::as_array) {
        for span in spans {
            recognized |= collect_otlp_span(span, "unknown-service", http_only, edges);
        }
    }
    recognized
}

fn collect_otlp_span(
    span: &Value,
    service: &str,
    http_only: bool,
    edges: &mut BTreeMap<(String, String, String), u64>,
) -> bool {
    let route = otlp_attribute(span.get("attributes"), "http.route")
        .or_else(|| otlp_attribute(span.get("attributes"), "url.path"));
    let peer = otlp_attribute(span.get("attributes"), "server.address")
        .or_else(|| otlp_attribute(span.get("attributes"), "peer.service"));
    let name = span
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unnamed-span");
    let is_http = route.is_some()
        || otlp_attribute(span.get("attributes"), "http.request.method").is_some()
        || otlp_attribute(span.get("attributes"), "http.method").is_some();
    if http_only && !is_http {
        return false;
    }
    let target = route.or(peer).unwrap_or_else(|| name.into());
    let kind = if is_http {
        "HTTP_CALLS"
    } else {
        "RUNTIME_CALLS"
    };
    *edges
        .entry((service.into(), target, kind.into()))
        .or_default() += 1;
    true
}

fn otlp_attribute(attributes: Option<&Value>, key: &str) -> Option<String> {
    attributes?
        .as_array()?
        .iter()
        .find(|attribute| attribute.get("key").and_then(Value::as_str) == Some(key))
        .and_then(|attribute| attribute.get("value"))
        .and_then(|value| {
            value
                .get("stringValue")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
        })
        .map(str::to_string)
}

fn normalize_kind(kind: &str) -> String {
    match kind {
        "HTTP_CALLS" | "ASYNC_CALLS" | "RUNTIME_READS_FROM" | "RUNTIME_WRITES_TO"
        | "RUNTIME_DISPATCHES" => kind.into(),
        _ => "RUNTIME_CALLS".into(),
    }
}

fn plugin_detection(language: &str, root: &Path, markers: &[&str]) -> RuntimeDetection {
    let detected = markers.iter().any(|marker| root.join(marker).exists());
    RuntimeDetection {
        available: detected,
        adapter: format!("{language}_runtime_plugin"),
        reason: if detected {
            "language marker detected; collector remains on-demand".into()
        } else {
            "language marker not detected".into()
        },
    }
}

fn collector_contract(language: &str, workload: &[String], output: &Path) -> Vec<String> {
    let mut argv = vec![
        format!("codeintel-{language}-collector"),
        "--output".into(),
        output.to_string_lossy().into_owned(),
        "--".into(),
    ];
    argv.extend_from_slice(workload);
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn direct_trace_is_aggregated_and_never_proves_absence() {
        let records = vec![
            serde_json::json!({"source":"route","target":"service","kind":"RUNTIME_CALLS"}),
            serde_json::json!({"source":"route","target":"service","kind":"RUNTIME_CALLS","count":2}),
        ];
        let trace = normalize_records(&records, "health smoke", false);
        assert_eq!(trace.edges.len(), 1);
        assert_eq!(trace.edges[0].count, 3);
        assert_eq!(trace.coverage.unsupported_records, 0);
        assert_eq!(trace.coverage.status, "PARTIAL");
        assert_eq!(trace.coverage.not_observed_semantics, "NOT_OBSERVED");
    }

    #[test]
    fn otlp_http_span_preserves_workload_scope() {
        let records = vec![serde_json::json!({
            "resourceSpans": [{
                "resource": {"attributes": [{"key":"service.name","value":{"stringValue":"api"}}]},
                "scopeSpans": [{"spans": [{
                    "name": "GET /health",
                    "attributes": [
                        {"key":"http.request.method","value":{"stringValue":"GET"}},
                        {"key":"http.route","value":{"stringValue":"/health"}}
                    ]
                }]}]
            }]
        })];
        let trace = normalize_records(&records, "http smoke", true);
        assert_eq!(trace.edges[0].source_symbol, "api");
        assert_eq!(trace.edges[0].target_symbol, "/health");
        assert_eq!(trace.edges[0].kind, "HTTP_CALLS");
        assert_eq!(trace.coverage.workload, "http smoke");
    }

    #[test]
    fn command_timeout_terminates_and_cleans_collector() {
        let dir = tempdir().unwrap();
        let mut adapter = CommandWrappedRuntimeAdapter::new(
            vec!["sh".into(), "-c".into(), "sleep 5".into()],
            dir.path(),
        )
        .unwrap();
        adapter
            .prepare(RuntimeWorkload {
                name: "timeout fixture".into(),
                timeout_ms: 40,
            })
            .unwrap();
        let started = Instant::now();
        adapter.start_or_attach().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(adapter.coverage().timed_out);
        let trace_path = adapter.trace_path().to_path_buf();
        adapter.cleanup().unwrap();
        assert!(!trace_path.exists());
    }

    #[test]
    fn language_plugins_are_contract_only_and_on_demand() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        let plugin = NodeRuntimePlugin;
        assert!(plugin.detect(dir.path()).available);
        let argv = plugin.collector_argv(&["npm".into(), "test".into()], Path::new("trace.ndjson"));
        assert_eq!(argv[0], "codeintel-node-collector");
        assert!(argv.contains(&"--".into()));
    }
}
