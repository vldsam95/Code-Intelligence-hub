from __future__ import annotations

import json
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import JSONResponse
from sqlalchemy import desc, select
from sqlalchemy.orm import Session

from ..config import get_settings
from ..db import get_db
from ..models import (
    AgentCredential,
    AgentSessionActivity,
    ChangeRun,
    Finding,
    QualityRun,
    RemoteAnalysisRequest,
    utcnow,
)
from ..repositories.projects import ProjectRepository
from ..security import authenticate_bearer
from ..services.health import project_health

router = APIRouter()
settings = get_settings()


TOOLS: list[dict[str, Any]] = [
    {
        "name": "list_projects",
        "description": "List only projects visible to this credential, with currentness and last quality state.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "get_project_context",
        "description": "Return the bounded central Context Pack. It contains no full source or raw graph.",
        "inputSchema": {
            "type": "object",
            "properties": {"project_id": {"type": "string"}},
            "required": ["project_id"],
        },
    },
    {
        "name": "get_code_health",
        "description": "Get current/STALE/UNVERIFIED status and last quality evidence.",
        "inputSchema": {
            "type": "object",
            "properties": {"project_id": {"type": "string"}},
            "required": ["project_id"],
        },
    },
    {
        "name": "get_quality_history",
        "description": "Get recent revision-pinned quality results.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "project_id": {"type": "string"},
                "limit": {"type": "integer", "maximum": 50},
            },
            "required": ["project_id"],
        },
    },
    {
        "name": "get_findings",
        "description": "Get bounded open findings and unknowns for a project.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "project_id": {"type": "string"},
                "limit": {"type": "integer", "maximum": 50},
            },
            "required": ["project_id"],
        },
    },
    {
        "name": "get_recent_changes",
        "description": "Get recent ChangeRuns without source bodies.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "project_id": {"type": "string"},
                "limit": {"type": "integer", "maximum": 50},
            },
            "required": ["project_id"],
        },
    },
    {
        "name": "ask_project_context",
        "description": "Return the most relevant structured central context; no internal LLM is used.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "project_id": {"type": "string"},
                "question": {"type": "string", "maxLength": 2000},
            },
            "required": ["project_id", "question"],
        },
    },
    {
        "name": "request_live_analysis",
        "description": "Queue a read-only structured query for an outbound-connected Worker.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "project_id": {"type": "string"},
                "query_type": {
                    "type": "string",
                    "enum": [
                        "project_status",
                        "architecture_refresh",
                        "find_symbol_metadata",
                        "callers",
                        "callees",
                        "impact",
                        "coverage",
                        "quality_status",
                        "preflight_preview",
                    ],
                },
                "query": {"type": "object"},
            },
            "required": ["project_id", "query_type"],
        },
    },
    {
        "name": "get_agent_setup",
        "description": "Get client-specific local and remote setup metadata.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "project_id": {"type": "string"},
                "client": {"type": "string", "enum": ["codex", "claude", "hermes", "generic-mcp"]},
            },
            "required": ["project_id", "client"],
        },
    },
]


def visible_project(credential: AgentCredential, project_id: str) -> bool:
    return (
        "admin" in credential.scopes
        or "hub:read" in credential.scopes
        or (credential.scope_type == "PROJECT" and credential.scope_id == project_id)
    )


def require_visible(credential: AgentCredential, project_id: str) -> None:
    if not visible_project(credential, project_id):
        raise HTTPException(403, "Project is outside credential scope")


def text_result(payload: Any) -> dict[str, Any]:
    encoded = json.dumps(payload, ensure_ascii=False, default=str, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > 96_000:
        encoded = json.dumps(
            {
                "summary": "Result exceeded the MCP budget and was truncated.",
                "unknowns": ["Request a narrower result."],
                "truncated": True,
            },
            ensure_ascii=False,
        )
    return {"content": [{"type": "text", "text": encoded}], "isError": False}


def record_mcp_session(request: Request, credential: AgentCredential, db: Session) -> None:
    """Record client/time metadata once per MCP initialize; it is audit context, not identity."""
    if credential.scope_type != "PROJECT" or not credential.scope_id:
        return
    client = request.headers.get("x-agent-client", "remote-mcp")[:64]
    version = request.headers.get("x-agent-version")
    session_id = request.headers.get("x-agent-session-id")
    db.add(
        AgentSessionActivity(
            project_id=credential.scope_id,
            agent_client=client,
            agent_version=version[:64] if version else None,
            session_id=session_id[:128] if session_id else None,
            activity="HUB_MCP_INITIALIZE",
            started_at=utcnow(),
        )
    )
    db.commit()


def call_tool(
    name: str, arguments: dict[str, Any], credential: AgentCredential, db: Session
) -> dict[str, Any]:
    repo = ProjectRepository(db)
    if name == "list_projects":
        projects = [row for row in repo.list_active(200) if visible_project(credential, row.id)]
        return text_result(
            [
                {
                    "project_id": row.id,
                    "slug": row.slug,
                    "name": row.name,
                    "health": project_health(db, row),
                }
                for row in projects
            ]
        )
    project_id = str(arguments.get("project_id", ""))
    require_visible(credential, project_id)
    project = repo.get(project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    if name in {"get_project_context", "ask_project_context"}:
        pack = repo.latest_context(project_id)
        health = project_health(db, project)
        return text_result(
            {
                "project_id": project.id,
                "purpose": pack.purpose if pack else project.purpose,
                "source_revision_id": pack.source_revision_id
                if pack
                else health.get("current_revision"),
                "freshness": pack.freshness if pack else "LAST_KNOWN",
                "languages": pack.languages if pack else [],
                "frameworks": pack.frameworks if pack else [],
                "architecture_summary": pack.architecture_summary if pack else None,
                "entry_points": (pack.entry_points if pack else [])[:50],
                "important_flows": (pack.important_flows if pack else [])[:50],
                "risk_areas": (pack.risk_areas if pack else [])[:50],
                "open_unknowns": (
                    pack.open_unknowns
                    if pack
                    else ["No Worker Context Pack has been synchronized."]
                )[:50],
                "health": health,
                "question": arguments.get("question") if name == "ask_project_context" else None,
                "answering_contract": "Host LLM should answer from this structured evidence and state freshness; Hub runs no internal LLM.",
            }
        )
    if name == "get_code_health":
        return text_result(project_health(db, project))
    if name == "get_quality_history":
        limit = min(max(int(arguments.get("limit", 20)), 1), 50)
        rows = list(
            db.scalars(
                select(QualityRun)
                .where(QualityRun.project_id == project_id)
                .order_by(desc(QualityRun.completed_at))
                .limit(limit)
            )
        )
        return text_result(
            [
                {
                    "source_revision_id": row.source_revision_id,
                    "verdict": row.verdict,
                    "currentness": row.currentness,
                    "risk_score": row.risk_score,
                    "blocking_unknowns": row.blocking_unknowns,
                    "warnings": row.warnings,
                    "completed_at": row.completed_at,
                }
                for row in rows
            ]
        )
    if name == "get_findings":
        limit = min(max(int(arguments.get("limit", 20)), 1), 50)
        rows = list(
            db.scalars(
                select(Finding)
                .where(Finding.project_id == project_id)
                .order_by(desc(Finding.updated_at))
                .limit(limit)
            )
        )
        return text_result(
            [
                {
                    "severity": row.severity,
                    "category": row.category,
                    "path": row.path,
                    "source_range": row.source_range,
                    "why": row.why,
                    "blocking": row.blocking,
                    "status": row.status,
                }
                for row in rows
            ]
        )
    if name == "get_recent_changes":
        limit = min(max(int(arguments.get("limit", 20)), 1), 50)
        rows = list(
            db.scalars(
                select(ChangeRun)
                .where(ChangeRun.project_id == project_id)
                .order_by(desc(ChangeRun.started_at))
                .limit(limit)
            )
        )
        return text_result(
            [
                {
                    "change_run_id": row.id,
                    "task_summary": row.task_summary,
                    "before_revision": row.before_source_revision_id,
                    "after_revision": row.after_source_revision_id,
                    "risk_before": row.risk_before,
                    "risk_after": row.risk_after,
                    "status": row.status,
                    "started_at": row.started_at,
                    "completed_at": row.completed_at,
                }
                for row in rows
            ]
        )
    if name == "request_live_analysis":
        if (
            f"project:{project_id}:live_analysis" not in credential.scopes
            and "admin" not in credential.scopes
        ):
            raise HTTPException(403, "Credential cannot request live analysis")
        forbidden = {"source", "source_body", "full_source", "raw_graph"}
        query = arguments.get("query") or {}
        if forbidden.intersection(query):
            raise HTTPException(422, "Source bodies and raw graph are forbidden in V1 relay")
        instance = repo.latest_instance(project_id)
        row = RemoteAnalysisRequest(
            project_id=project_id,
            instance_id=instance.id if instance else None,
            query_type=str(arguments.get("query_type")),
            query=query,
            requested_revision_id=instance.current_source_revision_id if instance else None,
            status="PENDING",
        )
        db.add(row)
        db.commit()
        return text_result(
            {
                "request_id": row.id,
                "status": "PENDING" if instance else "WORKER_OFFLINE",
                "source_revision_id": row.requested_revision_id,
                "next": "Poll get_remote_analysis through the Hub API.",
            }
        )
    if name == "get_agent_setup":
        client = arguments.get("client")
        return text_result(
            {
                "client": client,
                "project_id": project_id,
                "local_mcp": {"command": "codeintel", "args": ["mcp", "serve", "--stdio"]},
                "remote_mcp": {
                    "url": f"{settings.base_url}/mcp",
                    "token": "<PROJECT_ACCESS_TOKEN>",
                },
                "access_model": "One Project Access per Project; no token per Agent. Local MCP requires no Hub token.",
                "policy": f"{settings.base_url}/api/v1/docs/AGENT_OPERATING_POLICY.md?format=raw",
            }
        )
    raise HTTPException(404, "Unknown MCP tool")


@router.post("/mcp")
def remote_mcp(
    request: Request, payload: dict[str, Any], db: Session = Depends(get_db)
) -> JSONResponse:
    credential = authenticate_bearer(request, db)
    rpc_id = payload.get("id")
    method = payload.get("method")
    try:
        if method == "initialize":
            record_mcp_session(request, credential, db)
            result = {
                "protocolVersion": payload.get("params", {}).get("protocolVersion", "2025-06-18"),
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "codeintel-hub", "version": settings.hub_version},
                "instructions": (
                    "Project Access is shared by authorized clients; client/time metadata is recorded, not used as a security identity. "
                    "Use Hub context for durable/last-known knowledge. Use a live Worker and actual source for current line-level truth. "
                    "Graph absence is never proof of absence. Evidence is current only for its exact source revision."
                ),
            }
        elif method == "notifications/initialized":
            return JSONResponse(status_code=202, content={})
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "tools/call":
            params = payload.get("params", {})
            result = call_tool(
                str(params.get("name")), params.get("arguments") or {}, credential, db
            )
        else:
            return JSONResponse(
                {
                    "jsonrpc": "2.0",
                    "id": rpc_id,
                    "error": {"code": -32601, "message": "Method not found"},
                },
                status_code=404,
            )
        return JSONResponse({"jsonrpc": "2.0", "id": rpc_id, "result": result})
    except HTTPException as exc:
        return JSONResponse(
            {"jsonrpc": "2.0", "id": rpc_id, "error": {"code": -32000, "message": str(exc.detail)}},
            status_code=exc.status_code,
        )
