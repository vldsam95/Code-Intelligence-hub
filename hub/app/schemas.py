from __future__ import annotations

from datetime import datetime
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator


class APIModel(BaseModel):
    model_config = ConfigDict(from_attributes=True)


class ErrorDetail(APIModel):
    code: str
    message: str
    request_id: str
    details: dict[str, Any] = Field(default_factory=dict)
    retryable: bool = False


class ErrorEnvelope(APIModel):
    error: ErrorDetail


class Page(APIModel):
    items: list[Any]
    next_cursor: str | None = None


class ProjectCreate(APIModel):
    project_id: str
    slug: str = Field(min_length=2, max_length=128, pattern=r"^[a-z0-9][a-z0-9-]*$")
    name: str = Field(min_length=1, max_length=255)
    purpose: str | None = Field(default=None, max_length=10_000)
    parent_project_id: str | None = None


class ProjectPatch(APIModel):
    slug: str | None = Field(
        default=None, min_length=2, max_length=128, pattern=r"^[a-z0-9][a-z0-9-]*$"
    )
    name: str | None = Field(default=None, min_length=1, max_length=255)
    purpose: str | None = Field(default=None, max_length=10_000)
    version: int


class InstanceRegister(APIModel):
    instance_id: str
    host_id: str
    worker_id: str | None = None
    primary_path: str = Field(min_length=1, max_length=2048)
    related_roots: list[dict[str, Any]] = Field(default_factory=list, max_length=64)
    current_source_revision_id: str | None = Field(default=None, max_length=64)


class RevisionCreate(APIModel):
    revision_id: str = Field(min_length=64, max_length=64)
    project_id: str
    policy_hash: str = Field(min_length=64, max_length=64)
    indexer_version: str
    root_mapping_version: int = 1
    file_count: int = Field(ge=0)
    inventory_hash: str = Field(min_length=64, max_length=64)
    git_commit: str | None = None
    git_dirty: bool | None = None


class WorkerEnroll(APIModel):
    enrollment_token: str
    host_id: str
    host_name: str
    fingerprint: str
    os: str
    architecture: str
    worker_version: str
    protocol_version: int = 1
    adapters: list[str] = Field(default_factory=list)


class Heartbeat(APIModel):
    worker_version: str
    resource_summary: dict[str, Any] = Field(default_factory=dict)
    project_instances: list[dict[str, Any]] = Field(default_factory=list, max_length=500)


class WorkerProjectRegister(APIModel):
    project_id: str
    parent_project_id: str | None = None
    slug: str = Field(min_length=2, max_length=128, pattern=r"^[a-z0-9][a-z0-9-]*$")
    name: str = Field(min_length=1, max_length=255)
    instance_id: str
    host_id: str
    primary_path: str = Field(min_length=1, max_length=2048)
    related_roots: list[dict[str, Any]] = Field(default_factory=list, max_length=64)
    capabilities: dict[str, Any] = Field(default_factory=dict)
    wiki: dict[str, Any] = Field(default_factory=lambda: {"mode": "none", "binding_status": "NONE"})
    index_status: str = Field(default="UNKNOWN", max_length=32)
    worker_version: str | None = Field(default=None, max_length=32)
    policy_schema_version: int | None = None
    project_manifest_schema_version: int | None = None
    initialized_at: datetime | None = None
    revision: RevisionCreate


class ChangeRunCreate(APIModel):
    change_run_id: str
    project_id: str
    agent_client: str | None = None
    agent_session_id: str | None = None
    task_summary: str = Field(min_length=1, max_length=20_000)
    before_source_revision_id: str
    risk_before: int = Field(default=0, ge=0, le=100)
    required_checks: list[str] = Field(default_factory=list, max_length=100)


class ChangeRunPatch(APIModel):
    status: Literal[
        "OPEN", "PREFLIGHTED", "EDITING", "REVIEWING", "VERIFYING", "GATED", "CLOSED", "ABANDONED"
    ]
    after_source_revision_id: str | None = None
    risk_after: int | None = Field(default=None, ge=0, le=100)
    quality_run_id: str | None = None


class EvidenceIn(APIModel):
    evidence_id: str
    project_id: str
    source_revision_id: str
    change_run_id: str | None = None
    producer: str
    producer_version: str
    origin: str
    confidence: float = Field(ge=0, le=1)
    coverage: dict[str, Any] = Field(default_factory=dict)
    unsupported_areas: list[str] = Field(default_factory=list)
    unknowns: list[str] = Field(default_factory=list)
    policy_hash: str
    summary: str = Field(max_length=100_000)
    started_at: datetime
    completed_at: datetime | None = None


class QualityReport(APIModel):
    quality_run_id: str
    project_id: str
    source_revision_id: str
    change_run_id: str | None = None
    verdict: Literal["PASS", "PASS_WITH_WARNINGS", "UNKNOWN", "FAIL"]
    currentness: Literal["CURRENT", "STALE", "UNVERIFIED_CHANGE"]
    risk_score: int = Field(ge=0, le=100)
    required_checks: list[str] = Field(default_factory=list)
    completed_checks: list[str] = Field(default_factory=list)
    blocking_unknowns: list[str] = Field(default_factory=list)
    warnings: list[str] = Field(default_factory=list)
    policy_hash: str


class ContextPackIn(APIModel):
    context_pack_id: str
    project_id: str
    source_revision_id: str | None = None
    freshness: Literal["CURRENT", "STALE", "LAST_KNOWN"]
    purpose: str | None = None
    languages: list[str] = Field(default_factory=list)
    frameworks: list[str] = Field(default_factory=list)
    architecture_summary: str | None = Field(default=None, max_length=50_000)
    entry_points: list[dict[str, Any]] = Field(default_factory=list, max_length=500)
    important_flows: list[dict[str, Any]] = Field(default_factory=list, max_length=500)
    external_services: list[str] = Field(default_factory=list, max_length=500)
    risk_areas: list[str] = Field(default_factory=list, max_length=500)
    recent_changes: list[dict[str, Any]] = Field(default_factory=list, max_length=200)
    open_unknowns: list[str] = Field(default_factory=list, max_length=500)
    coverage_summary: dict[str, Any] = Field(default_factory=dict)
    producer: str = "worker"


class RemoteAnalysisCreate(APIModel):
    query_type: Literal[
        "project_status",
        "architecture_refresh",
        "find_symbol_metadata",
        "callers",
        "callees",
        "impact",
        "coverage",
        "quality_status",
        "preflight_preview",
    ]
    query: dict[str, Any] = Field(default_factory=dict)
    instance_id: str | None = None
    source_revision_id: str | None = None

    @field_validator("query")
    @classmethod
    def disallow_source_body_request(cls, value: dict[str, Any]) -> dict[str, Any]:
        forbidden = {"source", "source_body", "full_source", "raw_graph"}
        if forbidden.intersection(value):
            raise ValueError("source bodies and raw graph are not accepted by the V1 relay")
        return value


class RemoteAnalysisResultIn(APIModel):
    result_id: str
    project_id: str
    instance_id: str
    source_revision_id: str
    freshness: Literal["CURRENT", "STALE", "LAST_KNOWN"]
    summary: str = Field(max_length=50_000)
    structured_result: dict[str, Any] = Field(default_factory=dict)
    coverage: dict[str, Any] = Field(default_factory=dict)
    unknowns: list[str] = Field(default_factory=list)


class TokenCreate(APIModel):
    label: str = Field(min_length=1, max_length=255)
    token_type: Literal[
        "WORKER_ENROLLMENT",
        "WORKER_CREDENTIAL",
        "HOST_ACCESS",
        "AGENT_HUB_READ",
        "AGENT_LIVE_ANALYSIS",
        "PROJECT_AGENT",
        "ADMIN_API",
        "WIKI_CONNECTION",
    ]
    scope_type: Literal["GLOBAL", "PROJECT", "WORKER", "HOST", "WIKI"]
    scope_id: str | None = None
    scopes: list[str] = Field(default_factory=list, max_length=50)
    expires_in_sec: int | None = Field(default=None, ge=60, le=31_536_000)
    one_time: bool = False


class AgentSessionIn(APIModel):
    agent_client: str = Field(min_length=1, max_length=64)
    agent_version: str | None = Field(default=None, max_length=64)
    host_id: str | None = None
    instance_id: str | None = None
    session_id: str | None = Field(default=None, max_length=255)
    started_at: datetime | None = None


class ProjectAgentNoteIn(APIModel):
    kind: Literal["TECHNICAL_NOTE", "DURABLE_CONTEXT", "OPEN_UNKNOWN"] = "TECHNICAL_NOTE"
    content: str = Field(min_length=1, max_length=20_000)
    source_revision_id: str | None = Field(default=None, max_length=64)
    agent_client: str | None = Field(default=None, max_length=64)


class WikiBindingIn(APIModel):
    mode: Literal["none", "local", "remote"]
    wiki_id: str | None = None
    local_path: str | None = None
    connector_type: str | None = None


class WorkerReportEnvelope(APIModel):
    schema_version: int = 1
    message_id: str
    sent_at: datetime
    project_id: str
    instance_id: str
    source_revision_id: str
    revision: RevisionCreate | None = None
    report_type: Literal[
        "index", "review", "tests", "smoke", "runtime", "dataflow", "quality", "context_pack"
    ]
    payload: dict[str, Any]
