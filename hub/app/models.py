from __future__ import annotations

import uuid
from datetime import UTC, datetime
from typing import Any

from sqlalchemy import (
    JSON,
    Boolean,
    DateTime,
    Float,
    ForeignKey,
    Index,
    Integer,
    LargeBinary,
    String,
    Text,
    UniqueConstraint,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from .db import Base


def uuid4_str() -> str:
    return str(uuid.uuid4())


def utcnow() -> datetime:
    return datetime.now(UTC)


class TimestampMixin:
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=utcnow, onupdate=utcnow
    )


class OwnerUser(Base, TimestampMixin):
    __tablename__ = "owner_users"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    username: Mapped[str] = mapped_column(String(128), unique=True, nullable=False)
    password_hash: Mapped[str] = mapped_column(String(512), nullable=False)
    is_active: Mapped[bool] = mapped_column(Boolean, default=True)
    totp_enabled: Mapped[bool] = mapped_column(Boolean, default=False)


class BrowserSession(Base):
    __tablename__ = "browser_sessions"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    owner_id: Mapped[str] = mapped_column(
        ForeignKey("owner_users.id", ondelete="CASCADE"), index=True
    )
    session_hash: Mapped[str] = mapped_column(String(64), unique=True, index=True)
    csrf_hash: Mapped[str] = mapped_column(String(64))
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    last_seen_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    reauthenticated_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    expires_at: Mapped[datetime] = mapped_column(DateTime(timezone=True))
    revoked_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class LoginAttempt(Base):
    __tablename__ = "login_attempts"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    username: Mapped[str] = mapped_column(String(128), index=True)
    source_ip_hash: Mapped[str] = mapped_column(String(64), index=True)
    succeeded: Mapped[bool] = mapped_column(Boolean, default=False)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=utcnow, index=True
    )


class WorkerHost(Base, TimestampMixin):
    __tablename__ = "worker_hosts"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    name: Mapped[str] = mapped_column(String(255), nullable=False)
    fingerprint: Mapped[str] = mapped_column(String(255), unique=True, nullable=False)
    os: Mapped[str | None] = mapped_column(String(128))
    architecture: Mapped[str | None] = mapped_column(String(64))
    status: Mapped[str] = mapped_column(String(32), default="OFFLINE", index=True)
    provisioning_enabled: Mapped[bool] = mapped_column(Boolean, default=True, index=True)
    last_heartbeat_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    retired_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class Worker(Base, TimestampMixin):
    __tablename__ = "workers"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    host_id: Mapped[str] = mapped_column(
        ForeignKey("worker_hosts.id", ondelete="RESTRICT"), index=True
    )
    version: Mapped[str] = mapped_column(String(32))
    protocol_version: Mapped[int] = mapped_column(Integer, default=1)
    compatibility: Mapped[str] = mapped_column(String(32), default="COMPATIBLE")
    status: Mapped[str] = mapped_column(String(32), default="OFFLINE", index=True)
    last_heartbeat_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    resource_summary: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    adapters: Mapped[list[str]] = mapped_column(JSON, default=list)
    host: Mapped[WorkerHost] = relationship()


class Project(Base, TimestampMixin):
    __tablename__ = "projects"
    id: Mapped[str] = mapped_column(String(36), primary_key=True)
    slug: Mapped[str] = mapped_column(String(128), unique=True, nullable=False)
    name: Mapped[str] = mapped_column(String(255), nullable=False)
    purpose: Mapped[str | None] = mapped_column(Text)
    parent_project_id: Mapped[str | None] = mapped_column(
        ForeignKey("projects.id", ondelete="SET NULL")
    )
    status: Mapped[str] = mapped_column(String(32), default="ACTIVE", index=True)
    registration_state: Mapped[str] = mapped_column(String(32), default="REGISTERED", index=True)
    first_registered_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    last_synced_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    version: Mapped[int] = mapped_column(Integer, default=1)
    retired_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    instances: Mapped[list[ProjectInstance]] = relationship(back_populates="project")


class ProjectAlias(Base):
    __tablename__ = "project_aliases"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="CASCADE"), index=True
    )
    alias: Mapped[str] = mapped_column(String(255), index=True)
    kind: Mapped[str] = mapped_column(String(32), default="HISTORICAL_SLUG")
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class ProjectInstance(Base, TimestampMixin):
    __tablename__ = "project_instances"
    __table_args__ = (Index("ix_instances_project_status", "project_id", "status"),)
    id: Mapped[str] = mapped_column(String(36), primary_key=True)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    host_id: Mapped[str] = mapped_column(ForeignKey("worker_hosts.id", ondelete="RESTRICT"))
    worker_id: Mapped[str | None] = mapped_column(ForeignKey("workers.id", ondelete="SET NULL"))
    primary_path: Mapped[str] = mapped_column(String(2048))
    related_roots: Mapped[list[dict[str, Any]]] = mapped_column(JSON, default=list)
    current_source_revision_id: Mapped[str | None] = mapped_column(String(64), index=True)
    status: Mapped[str] = mapped_column(String(32), default="ONLINE", index=True)
    registration_state: Mapped[str] = mapped_column(String(32), default="REGISTERED", index=True)
    capabilities: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    index_status: Mapped[str] = mapped_column(String(32), default="UNKNOWN")
    worker_version: Mapped[str | None] = mapped_column(String(32))
    policy_schema_version: Mapped[int | None] = mapped_column(Integer)
    project_manifest_schema_version: Mapped[int | None] = mapped_column(Integer)
    initialized_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    first_seen_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    heartbeat_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    project: Mapped[Project] = relationship(back_populates="instances")
    host: Mapped[WorkerHost] = relationship()


class SourceRevision(Base):
    __tablename__ = "source_revisions"
    __table_args__ = (
        Index("ix_revision_project_instance_created", "project_id", "instance_id", "created_at"),
        UniqueConstraint("instance_id", "revision_id", name="uq_instance_revision"),
    )
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    revision_id: Mapped[str] = mapped_column(String(64), nullable=False)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    policy_hash: Mapped[str] = mapped_column(String(64))
    indexer_version: Mapped[str] = mapped_column(String(32))
    root_mapping_version: Mapped[int] = mapped_column(Integer, default=1)
    file_count: Mapped[int] = mapped_column(Integer, default=0)
    inventory_hash: Mapped[str] = mapped_column(String(64))
    git_commit: Mapped[str | None] = mapped_column(String(64))
    git_dirty: Mapped[bool | None] = mapped_column(Boolean)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class IndexSnapshot(Base):
    __tablename__ = "index_snapshots"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="RESTRICT"), index=True
    )
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT"), index=True
    )
    source_revision_id: Mapped[str] = mapped_column(String(64), index=True)
    state: Mapped[str] = mapped_column(String(32), default="BUILDING")
    coverage: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    db_size_bytes: Mapped[int] = mapped_column(Integer, default=0)
    started_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class ChangeRun(Base):
    __tablename__ = "change_runs"
    __table_args__ = (
        Index("ix_change_project_instance_started", "project_id", "instance_id", "started_at"),
    )
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    agent_client: Mapped[str | None] = mapped_column(String(64))
    agent_session_id: Mapped[str | None] = mapped_column(String(255))
    task_summary: Mapped[str] = mapped_column(Text)
    before_source_revision_id: Mapped[str] = mapped_column(String(64))
    after_source_revision_id: Mapped[str | None] = mapped_column(String(64))
    risk_before: Mapped[int | None] = mapped_column(Integer)
    risk_after: Mapped[int | None] = mapped_column(Integer)
    required_checks: Mapped[list[str]] = mapped_column(JSON, default=list)
    quality_run_id: Mapped[str | None] = mapped_column(String(36))
    manual_snapshot_reference: Mapped[str | None] = mapped_column(String(512))
    status: Mapped[str] = mapped_column(String(32), default="OPEN", index=True)
    started_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class AnalysisRun(Base):
    __tablename__ = "analysis_runs"
    __table_args__ = (
        Index(
            "ix_analysis_revision_completed",
            "project_id",
            "instance_id",
            "source_revision_id",
            "completed_at",
        ),
    )
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    source_revision_id: Mapped[str] = mapped_column(String(64))
    change_run_id: Mapped[str | None] = mapped_column(
        ForeignKey("change_runs.id", ondelete="SET NULL")
    )
    kind: Mapped[str] = mapped_column(String(32))
    producer: Mapped[str] = mapped_column(String(128))
    producer_version: Mapped[str] = mapped_column(String(32))
    status: Mapped[str] = mapped_column(String(32))
    started_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class Evidence(Base):
    __tablename__ = "evidence"
    __table_args__ = (
        Index(
            "ix_evidence_revision_origin",
            "project_id",
            "instance_id",
            "source_revision_id",
            "origin",
        ),
    )
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    source_revision_id: Mapped[str] = mapped_column(String(64))
    change_run_id: Mapped[str | None] = mapped_column(
        ForeignKey("change_runs.id", ondelete="SET NULL")
    )
    producer: Mapped[str] = mapped_column(String(128))
    producer_version: Mapped[str] = mapped_column(String(32))
    origin: Mapped[str] = mapped_column(String(64))
    confidence: Mapped[float] = mapped_column(Float, default=0.0)
    coverage: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    unsupported_areas: Mapped[list[str]] = mapped_column(JSON, default=list)
    unknowns: Mapped[list[str]] = mapped_column(JSON, default=list)
    policy_hash: Mapped[str] = mapped_column(String(64))
    summary: Mapped[str] = mapped_column(Text)
    started_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class Finding(Base, TimestampMixin):
    __tablename__ = "findings"
    __table_args__ = (
        Index("ix_findings_status_severity", "project_id", "status", "severity", "updated_at"),
    )
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    source_revision_id: Mapped[str] = mapped_column(String(64))
    change_run_id: Mapped[str | None] = mapped_column(
        ForeignKey("change_runs.id", ondelete="SET NULL")
    )
    severity: Mapped[str] = mapped_column(String(16))
    category: Mapped[str] = mapped_column(String(64))
    path: Mapped[str | None] = mapped_column(String(2048))
    source_range: Mapped[str | None] = mapped_column(String(128))
    why: Mapped[str] = mapped_column(Text)
    evidence_summary: Mapped[str | None] = mapped_column(Text)
    suggested_repair: Mapped[str | None] = mapped_column(Text)
    blocking: Mapped[bool] = mapped_column(Boolean, default=False)
    status: Mapped[str] = mapped_column(String(32), default="OPEN")


class CheckRunBase:
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="RESTRICT"), index=True
    )
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT"), index=True
    )
    source_revision_id: Mapped[str] = mapped_column(String(64), index=True)
    change_run_id: Mapped[str | None] = mapped_column(
        ForeignKey("change_runs.id", ondelete="SET NULL")
    )
    status: Mapped[str] = mapped_column(String(32))
    started_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    completed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    summary: Mapped[str] = mapped_column(Text)


class TestRun(Base, CheckRunBase):
    __tablename__ = "test_runs"
    command_id: Mapped[str] = mapped_column(String(128))
    argv: Mapped[list[str]] = mapped_column(JSON, default=list)
    cwd: Mapped[str] = mapped_column(String(2048))
    exit_code: Mapped[int | None] = mapped_column(Integer)
    output_excerpt: Mapped[str | None] = mapped_column(Text)
    environment_profile_hash: Mapped[str | None] = mapped_column(String(64))
    baseline_kind: Mapped[str] = mapped_column(String(32), default="POST_CHANGE")


class SmokeRun(Base, CheckRunBase):
    __tablename__ = "smoke_runs"
    endpoint_id: Mapped[str] = mapped_column(String(128))
    method: Mapped[str] = mapped_column(String(16))
    target: Mapped[str] = mapped_column(String(2048))
    http_status: Mapped[int | None] = mapped_column(Integer)
    latency_ms: Mapped[int | None] = mapped_column(Integer)
    assertions: Mapped[list[dict[str, Any]]] = mapped_column(JSON, default=list)
    baseline_kind: Mapped[str] = mapped_column(String(32), default="POST_CHANGE")


class RuntimeRun(Base, CheckRunBase):
    __tablename__ = "runtime_runs"
    adapter: Mapped[str] = mapped_column(String(128))
    workload: Mapped[str] = mapped_column(Text)
    coverage_scope: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    observed_edges: Mapped[int] = mapped_column(Integer, default=0)


class DataFlowRun(Base, CheckRunBase):
    __tablename__ = "dataflow_runs"
    source_query: Mapped[str] = mapped_column(Text)
    sink_query: Mapped[str] = mapped_column(Text)
    scope: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    verdict: Mapped[str] = mapped_column(String(32))


class QualityRun(Base):
    __tablename__ = "quality_runs"
    __table_args__ = (
        Index(
            "ix_quality_revision_completed",
            "project_id",
            "instance_id",
            "source_revision_id",
            "completed_at",
        ),
    )
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    source_revision_id: Mapped[str] = mapped_column(String(64))
    change_run_id: Mapped[str | None] = mapped_column(
        ForeignKey("change_runs.id", ondelete="SET NULL")
    )
    verdict: Mapped[str] = mapped_column(String(32))
    currentness: Mapped[str] = mapped_column(String(32), default="CURRENT")
    risk_score: Mapped[int] = mapped_column(Integer, default=0)
    required_checks: Mapped[list[str]] = mapped_column(JSON, default=list)
    completed_checks: Mapped[list[str]] = mapped_column(JSON, default=list)
    blocking_unknowns: Mapped[list[str]] = mapped_column(JSON, default=list)
    warnings: Mapped[list[str]] = mapped_column(JSON, default=list)
    policy_hash: Mapped[str] = mapped_column(String(64))
    completed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class ProjectContextPack(Base):
    __tablename__ = "project_context_packs"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="CASCADE"), index=True
    )
    instance_id: Mapped[str | None] = mapped_column(
        ForeignKey("project_instances.id", ondelete="SET NULL")
    )
    source_revision_id: Mapped[str | None] = mapped_column(String(64))
    freshness: Mapped[str] = mapped_column(String(32), default="LAST_KNOWN")
    purpose: Mapped[str | None] = mapped_column(Text)
    languages: Mapped[list[str]] = mapped_column(JSON, default=list)
    frameworks: Mapped[list[str]] = mapped_column(JSON, default=list)
    architecture_summary: Mapped[str | None] = mapped_column(Text)
    entry_points: Mapped[list[dict[str, Any]]] = mapped_column(JSON, default=list)
    important_flows: Mapped[list[dict[str, Any]]] = mapped_column(JSON, default=list)
    external_services: Mapped[list[str]] = mapped_column(JSON, default=list)
    risk_areas: Mapped[list[str]] = mapped_column(JSON, default=list)
    recent_changes: Mapped[list[dict[str, Any]]] = mapped_column(JSON, default=list)
    open_unknowns: Mapped[list[str]] = mapped_column(JSON, default=list)
    coverage_summary: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    producer: Mapped[str] = mapped_column(String(128), default="worker")
    produced_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class WikiBinding(Base, TimestampMixin):
    __tablename__ = "wiki_bindings"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="CASCADE"), unique=True
    )
    mode: Mapped[str] = mapped_column(String(16), default="none")
    wiki_id: Mapped[str | None] = mapped_column(String(255))
    local_path: Mapped[str | None] = mapped_column(String(2048))
    connector_type: Mapped[str | None] = mapped_column(String(64))
    status: Mapped[str] = mapped_column(String(32), default="DISABLED")
    capability: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)


class PendingWikiUpdate(Base):
    __tablename__ = "pending_wiki_updates"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="CASCADE"), index=True
    )
    source_revision_id: Mapped[str] = mapped_column(String(64))
    quality_run_id: Mapped[str] = mapped_column(ForeignKey("quality_runs.id", ondelete="RESTRICT"))
    durable_summary: Mapped[str] = mapped_column(Text)
    status: Mapped[str] = mapped_column(String(32), default="PENDING")
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class AgentCredential(Base):
    __tablename__ = "agent_credentials"
    __table_args__ = (Index("ix_credentials_scope_status", "scope_type", "scope_id", "status"),)
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    label: Mapped[str] = mapped_column(String(255))
    token_prefix: Mapped[str] = mapped_column(String(48), index=True)
    token_type: Mapped[str] = mapped_column(String(48))
    scope_type: Mapped[str] = mapped_column(String(32))
    scope_id: Mapped[str | None] = mapped_column(String(36))
    scopes: Mapped[list[str]] = mapped_column(JSON, default=list)
    validation_hash: Mapped[str] = mapped_column(String(64), unique=True, index=True)
    encrypted_secret: Mapped[bytes] = mapped_column(LargeBinary)
    encryption_nonce: Mapped[bytes] = mapped_column(LargeBinary)
    encryption_key_id: Mapped[str] = mapped_column(String(64), default="v1")
    status: Mapped[str] = mapped_column(String(16), default="ACTIVE", index=True)
    one_time: Mapped[bool] = mapped_column(Boolean, default=False)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    expires_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    last_used_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    revoked_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class AgentSessionActivity(Base):
    __tablename__ = "agent_session_activities"
    __table_args__ = (Index("ix_agent_activity_project_started", "project_id", "started_at"),)
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="CASCADE"), index=True
    )
    instance_id: Mapped[str | None] = mapped_column(
        ForeignKey("project_instances.id", ondelete="SET NULL"), index=True
    )
    host_id: Mapped[str | None] = mapped_column(
        ForeignKey("worker_hosts.id", ondelete="SET NULL"), index=True
    )
    agent_client: Mapped[str] = mapped_column(String(64))
    agent_version: Mapped[str | None] = mapped_column(String(64))
    session_id: Mapped[str | None] = mapped_column(String(255))
    activity: Mapped[str] = mapped_column(String(32), default="OPEN_PROJECT")
    started_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class ProjectAgentNote(Base, TimestampMixin):
    __tablename__ = "project_agent_notes"
    __table_args__ = (Index("ix_project_agent_notes_project_created", "project_id", "created_at"),)
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="CASCADE"), index=True
    )
    source_revision_id: Mapped[str | None] = mapped_column(String(64), index=True)
    kind: Mapped[str] = mapped_column(String(32), default="TECHNICAL_NOTE")
    content: Mapped[str] = mapped_column(Text)
    agent_client: Mapped[str | None] = mapped_column(String(64))


class RemoteAnalysisRequest(Base):
    __tablename__ = "remote_analysis_requests"
    __table_args__ = (
        Index("ix_remote_request_status_created", "project_id", "status", "created_at"),
    )
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str | None] = mapped_column(
        ForeignKey("project_instances.id", ondelete="SET NULL")
    )
    query_type: Mapped[str] = mapped_column(String(64))
    query: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    requested_revision_id: Mapped[str | None] = mapped_column(String(64))
    status: Mapped[str] = mapped_column(String(32), default="PENDING")
    worker_id: Mapped[str | None] = mapped_column(ForeignKey("workers.id", ondelete="SET NULL"))
    lease_expires_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=utcnow, onupdate=utcnow
    )
    expires_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))


class RemoteAnalysisResult(Base):
    __tablename__ = "remote_analysis_results"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    request_id: Mapped[str] = mapped_column(
        ForeignKey("remote_analysis_requests.id", ondelete="CASCADE"), unique=True
    )
    project_id: Mapped[str] = mapped_column(ForeignKey("projects.id", ondelete="RESTRICT"))
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    source_revision_id: Mapped[str] = mapped_column(String(64))
    freshness: Mapped[str] = mapped_column(String(32))
    summary: Mapped[str] = mapped_column(Text)
    structured_result: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    coverage: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    unknowns: Mapped[list[str]] = mapped_column(JSON, default=list)
    completed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class RecoveryEvent(Base):
    __tablename__ = "recovery_events"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    project_id: Mapped[str] = mapped_column(
        ForeignKey("projects.id", ondelete="RESTRICT"), index=True
    )
    instance_id: Mapped[str] = mapped_column(
        ForeignKey("project_instances.id", ondelete="RESTRICT")
    )
    detected_revision_id: Mapped[str] = mapped_column(String(64))
    previous_revision_id: Mapped[str | None] = mapped_column(String(64))
    reason: Mapped[str] = mapped_column(Text)
    suspected: Mapped[bool] = mapped_column(Boolean, default=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class AgentPolicyVersion(Base):
    __tablename__ = "agent_policy_versions"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    product_version: Mapped[str] = mapped_column(String(32))
    content_hash: Mapped[str] = mapped_column(String(64), unique=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class ReleaseArtifact(Base):
    __tablename__ = "release_artifacts"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    version: Mapped[str] = mapped_column(String(32), index=True)
    platform: Mapped[str] = mapped_column(String(64))
    name: Mapped[str] = mapped_column(String(255))
    relative_path: Mapped[str] = mapped_column(String(1024))
    sha256: Mapped[str] = mapped_column(String(64))
    size_bytes: Mapped[int] = mapped_column(Integer)
    signature_status: Mapped[str] = mapped_column(String(32), default="UNSIGNED")
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class AuditEvent(Base):
    __tablename__ = "audit_events"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    actor_type: Mapped[str] = mapped_column(String(32))
    actor_id: Mapped[str | None] = mapped_column(String(36))
    event_code: Mapped[str] = mapped_column(String(64), index=True)
    target_type: Mapped[str | None] = mapped_column(String(64))
    target_id: Mapped[str | None] = mapped_column(String(64))
    request_id: Mapped[str] = mapped_column(String(36), index=True)
    source_ip_hash: Mapped[str | None] = mapped_column(String(64))
    details: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=utcnow, index=True
    )


class JobQueue(Base):
    __tablename__ = "job_queue"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    job_type: Mapped[str] = mapped_column(String(64), index=True)
    project_id: Mapped[str | None] = mapped_column(ForeignKey("projects.id", ondelete="CASCADE"))
    worker_id: Mapped[str | None] = mapped_column(ForeignKey("workers.id", ondelete="SET NULL"))
    payload: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    status: Mapped[str] = mapped_column(String(32), default="PENDING", index=True)
    attempts: Mapped[int] = mapped_column(Integer, default=0)
    lease_expires_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    available_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=utcnow, onupdate=utcnow
    )


class IdempotencyRecord(Base):
    __tablename__ = "idempotency_records"
    key: Mapped[str] = mapped_column(String(255), primary_key=True)
    operation: Mapped[str] = mapped_column(String(128), primary_key=True)
    response_code: Mapped[int] = mapped_column(Integer)
    response_body: Mapped[dict[str, Any]] = mapped_column(JSON)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)


class BackupRecord(Base):
    __tablename__ = "backup_records"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=uuid4_str)
    kind: Mapped[str] = mapped_column(String(32))
    status: Mapped[str] = mapped_column(String(32))
    path: Mapped[str | None] = mapped_column(String(2048))
    encrypted: Mapped[bool] = mapped_column(Boolean, default=True)
    checksum: Mapped[str | None] = mapped_column(String(64))
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=utcnow)
    verified_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
