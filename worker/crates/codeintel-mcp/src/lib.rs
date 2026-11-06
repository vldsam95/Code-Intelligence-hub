use anyhow::{Context, Result};
use codeintel_worker::{send_local_request, RpcRequest, DEFAULT_SOCKET};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use uuid::Uuid;

pub fn serve_stdio(socket: Option<PathBuf>) -> Result<()> {
    let socket = socket.unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET));
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line).context("parse MCP JSON-RPC request")?;
        if request.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
            continue;
        }
        let response = handle_mcp(&socket, &request);
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_mcp(socket: &std::path::Path, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": request["params"]["protocolVersion"]
                .as_str()
                .unwrap_or("2025-06-18"),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "codeintel-local", "version": env!("CARGO_PKG_VERSION")},
            "instructions": operating_instructions(),
        })),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let args = request["params"]["arguments"].clone();
            let worker_method = match name {
                "open_project"
                | "get_project_context"
                | "understand"
                | "find_code"
                | "preflight_change"
                | "review_change"
                | "verify_change"
                | "quality_gate"
                | "security_scenarios"
                | "abandon_change_run"
                | "project_status"
                | "advanced.graph_query"
                | "advanced.graph_visualization"
                | "advanced.find_path"
                | "advanced.edge_evidence"
                | "advanced.coverage"
                | "advanced.dead_code_audit"
                | "advanced.runtime_ingest"
                | "advanced.dataflow" => name,
                _ => {
                    return json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32602, "message": "Unknown Code Intelligence tool"}
                    })
                }
            };
            send_local_request(
                socket,
                &RpcRequest {
                    request_id: Uuid::new_v4(),
                    method: worker_method.into(),
                    params: args,
                },
            )
            .map(bounded_tool_result)
        }
        _ => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "Method not found"}
            })
        }
    };
    match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": error.to_string()}
        }),
    }
}

fn bounded_tool_result(value: Value) -> Value {
    let mut encoded = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    if encoded.len() > 6_000 {
        encoded = serde_json::to_string(&json!({
            "summary": "Result exceeded the bounded MCP response budget.",
            "truncated": true,
            "next": "Use a narrower symbol/path/intent query or request paginated details.",
            "unknowns": ["The omitted items were not evaluated by the receiving Agent."]
        }))
        .unwrap();
    }
    json!({
        "content": [{"type": "text", "text": encoded}],
        "isError": false
    })
}

fn operating_instructions() -> &'static str {
    "Code Intelligence is opt-in per project. Perform only a cheap marker check first. Never run init, indexing, registration, Quality Gate, runtime, data-flow or security scenarios for an unmarked project unless the user explicitly requested onboarding. When .code-intel/project.yaml exists and enabled=true, read .code-intel/SKILL.md and open it automatically. Local Graph, Review, Runtime, Data-flow and Gate never require Hub access or a token; report Hub sync separately. Use graph results only for discovery; actual current source is truth. Empty graph results never prove absence. Run preflight before significant changes, then review_change, every required check, and quality_gate for the exact current revision. Security scenarios are local-evidence-only and may run only from a predeclared policy against an explicitly authorized lab/staging origin; tool arguments can never supply targets, credentials or payloads. When configured, correlate executed evidence to exact current-revision route symbols before verification; missing or ambiguous matches stay UNKNOWN. Load conditional security guidance only when relevant. Wiki is optional and its outage never blocks code quality. Runtime proves only its workload. Dead-code results are candidates and must never trigger automatic deletion. End each code task by reporting the gate, revision, checks and remaining warnings/unknowns."
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "open_project",
            "Detect/open the portable manifest, compute the non-Git revision and refresh the local index.",
            json!({"type":"object","properties":{"path":{"type":"string"}}}),
        ),
        tool(
            "get_project_context",
            "Return bounded project identity, architecture, policy, health, coverage and unknowns.",
            json!({"type":"object","properties":{"path":{"type":"string"}}}),
        ),
        tool(
            "understand",
            "Intent-based bounded retrieval for a feature, flow, route, integration or architecture area.",
            json!({"type":"object","properties":{"path":{"type":"string"},"intent":{"type":"string"}},"required":["intent"]}),
        ),
        tool(
            "find_code",
            "Find symbols, FTS matches, callers or callees with ranges, provenance and absence semantics.",
            json!({"type":"object","properties":{"path":{"type":"string"},"query":{"type":"string"},"operation":{"type":"string","enum":["symbol","search","callers","callees"]}},"required":["query"]}),
        ),
        tool(
            "preflight_change",
            "Start a ChangeRun, pin its before revision/cache, compute deterministic risk and required checks.",
            json!({"type":"object","properties":{"path":{"type":"string"},"intent":{"type":"string"},"planned_files":{"type":"array","items":{"type":"string"}},"agent_client":{"type":"string"}},"required":["intent"]}),
        ),
        tool(
            "review_change",
            "Compare before/after revisions without Git and return a bounded review/blast-radius bundle.",
            json!({"type":"object","properties":{"path":{"type":"string"},"change_run_id":{"type":"string"}},"required":["change_run_id"]}),
        ),
        tool(
            "verify_change",
            "Run tests, lint, build, smoke, semantic/runtime/data-flow evidence selected by risk.",
            json!({"type":"object","properties":{"path":{"type":"string"},"change_run_id":{"type":"string"}},"required":["change_run_id"]}),
        ),
        tool(
            "quality_gate",
            "Return deterministic PASS/PASS_WITH_WARNINGS/UNKNOWN/FAIL for the exact current revision.",
            json!({"type":"object","properties":{"path":{"type":"string"},"change_run_id":{"type":"string"}},"required":["change_run_id"]}),
        ),
        tool(
            "security_scenarios",
            "Inventory graph-discovered endpoints, validate/execute only the revision-bound predeclared authorized lab/staging matrix, or correlate an existing run to bounded code paths. Targets, credentials and payloads cannot be supplied in this call.",
            json!({"type":"object","properties":{"path":{"type":"string"},"change_run_id":{"type":"string"},"action":{"type":"string","enum":["inventory","validate","run","correlate"],"default":"validate"},"scenario_ids":{"type":"array","items":{"type":"string"}},"page":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1,"maximum":100,"default":25}},"required":["change_run_id"]}),
        ),
        tool(
            "abandon_change_run",
            "Recover from an intentionally stopped or stale local ChangeRun while preserving its reports and recording a reason.",
            json!({"type":"object","properties":{"path":{"type":"string"},"change_run_id":{"type":"string"},"reason":{"type":"string","minLength":8,"maxLength":500}},"required":["change_run_id","reason"]}),
        ),
        tool(
            "project_status",
            "Return currentness, graph state, gate state, active run and Hub spool status.",
            json!({"type":"object","properties":{"path":{"type":"string"}}}),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":input_schema})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_surface_has_eleven_primary_tools() {
        assert_eq!(tools().len(), 11);
    }

    #[test]
    fn large_output_is_bounded() {
        let value = json!({"body": "x".repeat(120_000)});
        let result = bounded_tool_result(value);
        assert!(result["content"][0]["text"].as_str().unwrap().len() < 5_000);
    }

    #[test]
    fn initialize_policy_is_opt_in_and_wiki_independent() {
        let instructions = operating_instructions();
        assert!(instructions.contains("opt-in per project"));
        assert!(instructions.contains("Never run init"));
        assert!(instructions.contains("Wiki is optional"));
        assert!(instructions.contains("Empty graph results never prove absence"));
        assert!(instructions.contains("predeclared policy"));
        assert!(instructions.contains("correlate executed evidence"));
    }
}
