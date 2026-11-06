from __future__ import annotations

import hashlib
import io
import json
import mimetypes
import time
import uuid
import zipfile
from datetime import UTC, timedelta
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request, Response
from fastapi.responses import FileResponse
from pydantic import BaseModel, Field
from sqlalchemy import desc, func, or_, select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from ..config import get_settings
from ..db import get_db
from ..models import (
    AgentCredential,
    AgentSessionActivity,
    BackupRecord,
    ChangeRun,
    Evidence,
    Finding,
    IndexSnapshot,
    LoginAttempt,
    OwnerUser,
    Project,
    ProjectAgentNote,
    ProjectAlias,
    ProjectContextPack,
    ProjectInstance,
    QualityRun,
    RecoveryEvent,
    ReleaseArtifact,
    RemoteAnalysisRequest,
    RemoteAnalysisResult,
    SourceRevision,
    WikiBinding,
    Worker,
    WorkerHost,
    utcnow,
    uuid4_str,
)
from ..repositories.projects import ProjectRepository
from ..schemas import (
    AgentSessionIn,
    ChangeRunCreate,
    ChangeRunPatch,
    ContextPackIn,
    Heartbeat,
    InstanceRegister,
    ProjectAgentNoteIn,
    ProjectCreate,
    ProjectPatch,
    QualityReport,
    RemoteAnalysisCreate,
    RemoteAnalysisResultIn,
    RevisionCreate,
    TokenCreate,
    WikiBindingIn,
    WorkerEnroll,
    WorkerProjectRegister,
    WorkerReportEnvelope,
)
from ..security import (
    CSRF_COOKIE,
    SESSION_COOKIE,
    SessionIdentity,
    authenticate_bearer,
    authenticate_browser,
    authenticate_raw_token,
    create_browser_session,
    create_token_secret,
    decrypt_credential_secret,
    encrypt_credential_secret,
    hash_password,
    password_needs_rehash,
    source_ip_hash,
    token_validation_hash,
    verify_csrf,
    verify_password,
)
from ..services.audit import audit
from ..services.health import project_health
from ..services.quality import GateInputs, evaluate_gate

router = APIRouter(prefix="/api/v1")
settings = get_settings()


def project_access_scopes(project_id: str) -> list[str]:
    """The single default Project Access contract exposed to internal Agents."""
    return [
        f"project:{project_id}:read",
        f"project:{project_id}:context_read",
        f"project:{project_id}:context_write",
        f"project:{project_id}:health_read",
        f"project:{project_id}:quality_read",
        f"project:{project_id}:findings_read",
        f"project:{project_id}:history_read",
        f"project:{project_id}:agent_setup_read",
        f"project:{project_id}:session_log",
        f"project:{project_id}:live_analysis",
        f"project:{project_id}:instance_bind",
    ]


def normalize_project_access_scopes(
    credential: AgentCredential, project_id: str
) -> AgentCredential:
    """Upgrade legacy PROJECT_AGENT credentials to the single V1.2 Project Access contract."""
    if (
        credential.token_type == "PROJECT_AGENT"
        and credential.scope_type == "PROJECT"
        and credential.scope_id == project_id
    ):
        expected = project_access_scopes(project_id)
        if set(credential.scopes) != set(expected):
            credential.scopes = expected
    return credential


def active_credential(
    db: Session, token_type: str, scope_type: str, scope_id: str
) -> AgentCredential | None:
    return db.scalar(
        select(AgentCredential)
        .where(
            AgentCredential.token_type == token_type,
            AgentCredential.scope_type == scope_type,
            AgentCredential.scope_id == scope_id,
            AgentCredential.status == "ACTIVE",
        )
        .order_by(desc(AgentCredential.created_at))
    )


def active_project_access(db: Session, project_id: str) -> AgentCredential | None:
    """Prefer the one user-facing Project Access credential over hidden legacy credentials."""
    canonical = db.scalar(
        select(AgentCredential)
        .where(
            AgentCredential.token_type == "PROJECT_AGENT",
            AgentCredential.scope_type == "PROJECT",
            AgentCredential.scope_id == project_id,
            AgentCredential.status == "ACTIVE",
            AgentCredential.label.like("Project Access ·%"),
        )
        .order_by(desc(AgentCredential.created_at))
    )
    return canonical or active_credential(db, "PROJECT_AGENT", "PROJECT", project_id)


class LoginIn(BaseModel):
    username: str = Field(min_length=1, max_length=128)
    password: str = Field(min_length=1, max_length=1024)


class ReAuthIn(BaseModel):
    password: str = Field(min_length=1, max_length=1024)


def request_id(request: Request) -> str:
    return getattr(request.state, "request_id", str(uuid.uuid4()))


def remote_ip(request: Request) -> str:
    forwarded = request.headers.get("x-forwarded-for", "").split(",", 1)[0].strip()
    return forwarded or (request.client.host if request.client else "unknown")


def rfc3339(value: Any) -> str | None:
    if value is None:
        return None
    if value.tzinfo is None:
        value = value.replace(tzinfo=UTC)
    return value.isoformat().replace("+00:00", "Z")


def validate_ephemeral_graph_payload(value: dict[str, Any]) -> None:
    """Reject source-like or unbounded data before the transient UI payload reaches Hub storage."""
    nodes = value.get("nodes")
    edges = value.get("edges")
    top_fields = {
        "nodes",
        "edges",
        "node_count_total",
        "edge_count_total",
        "nodes_remaining",
        "edges_remaining",
        "bounded",
        "ephemeral",
        "source_bodies_included",
        "coverage",
        "warning",
    }
    if not set(value).issubset(top_fields):
        raise HTTPException(422, "Graph visualization contains a forbidden top-level field")
    if not isinstance(nodes, list) or not isinstance(edges, list):
        raise HTTPException(422, "Graph visualization requires bounded nodes and edges")
    if len(nodes) > 300 or len(edges) > 600:
        raise HTTPException(413, "Graph visualization exceeds the bounded V1 contract")
    node_fields = {
        "id",
        "kind",
        "qualified_name",
        "label",
        "path",
        "language",
        "entry_point",
        "exported",
        "line",
    }
    edge_fields = {"source", "target", "kind", "origin", "confidence", "ambiguity_reason"}
    for node in nodes:
        if not isinstance(node, dict) or not set(node).issubset(node_fields):
            raise HTTPException(422, "Graph node contains a forbidden field")
        if len(str(node)) > 16_384:
            raise HTTPException(413, "Graph node metadata is too large")
    for edge in edges:
        if not isinstance(edge, dict) or not set(edge).issubset(edge_fields):
            raise HTTPException(422, "Graph edge contains a forbidden field")
        if len(str(edge)) > 8_192:
            raise HTTPException(413, "Graph edge metadata is too large")
    if value.get("source_bodies_included") is not False:
        raise HTTPException(422, "Graph visualization must explicitly exclude source bodies")


def normalized_worker_wiki(value: dict[str, Any]) -> tuple[str, str]:
    mode = str(value.get("mode", "none"))
    if mode not in {"none", "local", "remote"}:
        mode = "none"
    status = str(value.get("binding_status", "NONE"))
    if status not in {"BOUND", "NONE", "UNRESOLVED", "UNAVAILABLE", "PENDING"}:
        status = "UNRESOLVED"
    return mode, status


def require_owner(request: Request, db: Session) -> SessionIdentity | AgentCredential:
    identity = authenticate_browser(request, db)
    if identity:
        return identity
    return authenticate_bearer(request, db, "admin")


def require_owner_csrf(
    request: Request, db: Session, submitted: str | None
) -> SessionIdentity | AgentCredential:
    identity = require_owner(request, db)
    if isinstance(identity, SessionIdentity):
        verify_csrf(request, identity, submitted or request.headers.get("x-csrf-token"))
    return identity


def require_project_scope(
    request: Request, db: Session, project_id: str, live: bool = False
) -> AgentCredential:
    required = f"project:{project_id}:{'live_analysis' if live else 'read'}"
    credential = authenticate_bearer(request, db)
    if "admin" in credential.scopes:
        return credential
    if credential.scope_type == "PROJECT" and credential.scope_id != project_id:
        raise HTTPException(403, "Credential is scoped to another project")
    if required not in credential.scopes and ("hub:read" not in credential.scopes or live):
        raise HTTPException(403, "Credential scope denied")
    return credential


def require_worker(request: Request, db: Session, worker_id: str) -> AgentCredential:
    credential = authenticate_bearer(request, db)
    if credential.token_type != "WORKER_CREDENTIAL" or credential.scope_id != worker_id:
        raise HTTPException(403, "Worker credential scope denied")
    return credential


def serialize_project(project: Project) -> dict[str, Any]:
    return {
        "project_id": project.id,
        "slug": project.slug,
        "name": project.name,
        "purpose": project.purpose,
        "parent_project_id": project.parent_project_id,
        "status": project.status,
        "registration_state": project.registration_state,
        "first_registered_at": rfc3339(project.first_registered_at),
        "last_synced_at": rfc3339(project.last_synced_at),
        "version": project.version,
        "created_at": project.created_at,
        "updated_at": project.updated_at,
    }


@router.post("/auth/login")
def login(
    payload: LoginIn, request: Request, response: Response, db: Session = Depends(get_db)
) -> dict[str, Any]:
    ip_digest = source_ip_hash(remote_ip(request))
    cutoff = utcnow() - timedelta(seconds=settings.login_window_sec)
    failures = (
        db.scalar(
            select(func.count())
            .select_from(LoginAttempt)
            .where(
                LoginAttempt.username == payload.username,
                LoginAttempt.source_ip_hash == ip_digest,
                LoginAttempt.succeeded.is_(False),
                LoginAttempt.created_at >= cutoff,
            )
        )
        or 0
    )
    if failures >= settings.login_attempts:
        raise HTTPException(429, "Too many login attempts; retry later")
    owner = db.scalar(select(OwnerUser).where(OwnerUser.username == payload.username))
    if (
        not owner
        or not owner.is_active
        or not verify_password(owner.password_hash, payload.password)
    ):
        db.add(
            LoginAttempt(
                username=payload.username,
                source_ip_hash=ip_digest,
                succeeded=False,
            )
        )
        audit(
            db,
            event_code="LOGIN_FAILED",
            request_id=request_id(request),
            actor_type="ANONYMOUS",
            source_ip_hash=ip_digest,
            details={"username_hash": hashlib.sha256(payload.username.encode()).hexdigest()},
        )
        db.commit()
        raise HTTPException(401, "Invalid credentials")
    if password_needs_rehash(owner.password_hash):
        owner.password_hash = hash_password(payload.password)
    db.add(
        LoginAttempt(
            username=payload.username,
            source_ip_hash=ip_digest,
            succeeded=True,
        )
    )
    session, raw_session, raw_csrf = create_browser_session(db, owner)
    response.set_cookie(
        SESSION_COOKIE,
        raw_session,
        httponly=True,
        secure=settings.secure_cookies,
        samesite="lax",
        path="/",
        max_age=settings.session_absolute_sec,
    )
    response.set_cookie(
        CSRF_COOKIE,
        raw_csrf,
        httponly=False,
        secure=settings.secure_cookies,
        samesite="lax",
        path="/",
        max_age=settings.session_absolute_sec,
    )
    audit(
        db,
        event_code="LOGIN_SUCCEEDED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=owner.id,
        source_ip_hash=ip_digest,
    )
    db.commit()
    return {"authenticated": True, "csrf_token": raw_csrf, "expires_at": session.expires_at}


@router.post("/auth/re-auth")
def re_auth(payload: ReAuthIn, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    identity = authenticate_browser(request, db)
    if not identity or not verify_password(identity.owner.password_hash, payload.password):
        raise HTTPException(401, "Re-authentication failed")
    verify_csrf(request, identity, request.headers.get("x-csrf-token"))
    identity.session.reauthenticated_at = utcnow()
    audit(
        db,
        event_code="OWNER_REAUTHENTICATED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id,
    )
    db.commit()
    return {"reauthenticated": True, "at": identity.session.reauthenticated_at}


@router.get("/tokens")
def list_tokens(request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    require_owner(request, db)
    rows = list(
        db.scalars(select(AgentCredential).order_by(desc(AgentCredential.created_at)).limit(200))
    )
    return {
        "items": [
            {
                "token_id": row.id,
                "label": row.label,
                "prefix": row.token_prefix,
                "type": row.token_type,
                "scope_type": row.scope_type,
                "scope_id": row.scope_id,
                "scopes": row.scopes,
                "status": row.status,
                "one_time": row.one_time,
                "created_at": row.created_at,
                "expires_at": row.expires_at,
                "last_used_at": row.last_used_at,
            }
            for row in rows
        ],
        "next_cursor": None,
    }


def create_credential(db: Session, payload: TokenCreate) -> tuple[AgentCredential, str]:
    if payload.scope_type != "GLOBAL" and not payload.scope_id:
        raise HTTPException(422, "scope_id is required for scoped credentials")
    if payload.token_type == "PROJECT_AGENT" and payload.scope_type != "PROJECT":
        raise HTTPException(422, "PROJECT_AGENT must be project scoped")
    if payload.token_type == "HOST_ACCESS" and payload.scope_type != "HOST":
        raise HTTPException(422, "HOST_ACCESS must be host scoped")
    raw = create_token_secret(payload.token_type)
    row = AgentCredential(
        id=uuid4_str(),
        label=payload.label,
        token_prefix=raw[: min(len(raw), 24)],
        token_type=payload.token_type,
        scope_type=payload.scope_type,
        scope_id=payload.scope_id,
        scopes=payload.scopes,
        validation_hash=token_validation_hash(raw),
        encrypted_secret=b"pending",
        encryption_nonce=b"pending",
        encryption_key_id="v1",
        one_time=payload.one_time,
        expires_at=utcnow() + timedelta(seconds=payload.expires_in_sec)
        if payload.expires_in_sec
        else None,
    )
    encrypted, nonce, key_id = encrypt_credential_secret(row.id, raw)
    row.encrypted_secret = encrypted
    row.encryption_nonce = nonce
    row.encryption_key_id = key_id
    db.add(row)
    return row, raw


def ensure_project_access(
    db: Session, project_id: str, project_name: str
) -> tuple[AgentCredential, str | None]:
    row = active_project_access(db, project_id)
    if row:
        row.label = f"Project Access · {project_name}"
        return normalize_project_access_scopes(row, project_id), None
    row, raw = create_credential(
        db,
        TokenCreate(
            label=f"Project Access · {project_name}",
            token_type="PROJECT_AGENT",
            scope_type="PROJECT",
            scope_id=project_id,
            scopes=project_access_scopes(project_id),
        ),
    )
    return row, raw


@router.post("/tokens", status_code=201)
def create_token(
    payload: TokenCreate, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row, raw = create_credential(db, payload)
    audit(
        db,
        event_code="TOKEN_CREATED",
        request_id=request_id(request),
        actor_type="OWNER" if isinstance(identity, SessionIdentity) else "API",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="TOKEN",
        target_id=row.id,
        details={
            "token_type": row.token_type,
            "scope_type": row.scope_type,
            "scope_id": row.scope_id,
        },
    )
    db.commit()
    return {
        "token_id": row.id,
        "token": raw,
        "token_type": row.token_type,
        "scope_type": row.scope_type,
        "scope_id": row.scope_id,
        "scopes": row.scopes,
        "shown_once": row.one_time,
        "expires_at": row.expires_at,
    }


def require_recent_reauth(identity: SessionIdentity | AgentCredential) -> None:
    if isinstance(identity, AgentCredential):
        return
    value = identity.session.reauthenticated_at
    if value.tzinfo is None:
        value = value.replace(tzinfo=UTC)
    if value < utcnow() - timedelta(minutes=10):
        raise HTTPException(403, "Recent re-authentication is required")


@router.post("/tokens/{token_id}/reveal")
def reveal_token(token_id: str, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    require_recent_reauth(identity)
    row = db.get(AgentCredential, token_id)
    if not row:
        raise HTTPException(404, "Token not found")
    if row.one_time:
        raise HTTPException(409, "One-time token cannot be revealed again")
    raw = decrypt_credential_secret(row)
    audit(
        db,
        event_code="TOKEN_REVEALED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="TOKEN",
        target_id=row.id,
    )
    db.commit()
    return {
        "token_id": row.id,
        "token": raw,
        "token_type": row.token_type,
        "scope_type": row.scope_type,
        "scope_id": row.scope_id,
        "scopes": row.scopes,
    }


@router.post("/tokens/{token_id}/rotate")
def rotate_token(token_id: str, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    require_recent_reauth(identity)
    old = db.get(AgentCredential, token_id)
    if not old or old.status != "ACTIVE":
        raise HTTPException(404, "Active token not found")
    old.status = "ROTATED"
    old.revoked_at = utcnow()
    payload = TokenCreate(
        label=old.label,
        token_type=old.token_type,
        scope_type=old.scope_type,
        scope_id=old.scope_id,
        scopes=old.scopes,
        expires_in_sec=900 if old.one_time else None,
        one_time=old.one_time,
    )
    new, raw = create_credential(db, payload)
    audit(
        db,
        event_code="TOKEN_ROTATED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="TOKEN",
        target_id=old.id,
        details={"replacement_token_id": new.id},
    )
    db.commit()
    return {
        "old_token_id": old.id,
        "token_id": new.id,
        "token": raw,
        "token_type": new.token_type,
        "scope_type": new.scope_type,
        "scope_id": new.scope_id,
        "scopes": new.scopes,
        "shown_once": new.one_time,
        "expires_at": new.expires_at,
    }


@router.delete("/tokens/{token_id}", status_code=204)
def revoke_token(token_id: str, request: Request, db: Session = Depends(get_db)) -> Response:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row = db.get(AgentCredential, token_id)
    if not row:
        raise HTTPException(404, "Token not found")
    row.status = "REVOKED"
    row.revoked_at = utcnow()
    audit(
        db,
        event_code="TOKEN_REVOKED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="TOKEN",
        target_id=row.id,
    )
    db.commit()
    return Response(status_code=204)


@router.post("/tokens/{token_id}/test")
def test_token(token_id: str, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row = db.get(AgentCredential, token_id)
    if not row:
        raise HTTPException(404, "Token not found")
    return {
        "token_id": row.id,
        "status": row.status,
        "usable": row.status == "ACTIVE",
        "last_used_at": row.last_used_at,
    }


@router.post("/projects", status_code=201)
def create_project(
    payload: ProjectCreate, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    if db.get(Project, payload.project_id):
        raise HTTPException(409, "Project identity already exists")
    row = Project(
        id=payload.project_id,
        slug=payload.slug,
        name=payload.name,
        purpose=payload.purpose,
        parent_project_id=payload.parent_project_id,
    )
    db.add(row)
    db.add(WikiBinding(project_id=row.id, mode="none", status="DISABLED"))
    ensure_project_access(db, row.id, row.name)
    audit(
        db,
        event_code="PROJECT_CREATED",
        request_id=request_id(request),
        actor_type="OWNER" if isinstance(identity, SessionIdentity) else "API",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="PROJECT",
        target_id=row.id,
    )
    try:
        db.commit()
    except IntegrityError as exc:
        db.rollback()
        raise HTTPException(409, "Project slug already exists") from exc
    return serialize_project(row)


@router.get("/project-recovery-manifest")
def recover_manifest(request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    credential = authenticate_bearer(request, db)
    if (
        credential.token_type != "PROJECT_AGENT"
        or credential.scope_type != "PROJECT"
        or not credential.scope_id
        or f"project:{credential.scope_id}:recover" not in credential.scopes
    ):
        raise HTTPException(403, "A project recovery token is required")
    project = db.get(Project, credential.scope_id)
    if not project:
        raise HTTPException(404, "Project not found")
    project_access, created_token = ensure_project_access(db, project.id, project.name)
    project_access_token = created_token or decrypt_credential_secret(project_access)
    audit(
        db,
        event_code="PROJECT_MANIFEST_RECOVERED",
        request_id=request_id(request),
        actor_type="RECOVERY_TOKEN",
        actor_id=credential.id,
        target_type="PROJECT",
        target_id=project.id,
    )
    db.commit()
    return {
        "project_id": project.id,
        "slug": project.slug,
        "name": project.name,
        "parent_project_id": project.parent_project_id,
        "schema_version": settings.project_manifest_schema_version,
        "project_access_token": project_access_token,
        "project_access_credential_id": project_access.id,
    }


@router.get("/projects")
def list_projects(
    request: Request, limit: int = 50, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner(request, db)
    limit = min(max(limit, 1), 200)
    rows = ProjectRepository(db).list_active(limit)
    return {
        "items": [serialize_project(row) | {"health": project_health(db, row)} for row in rows],
        "next_cursor": None,
    }


@router.get("/projects/{project_id}")
def get_project(project_id: str, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    row = db.get(Project, project_id)
    if not row:
        raise HTTPException(404, "Project not found")
    return serialize_project(row) | {"health": project_health(db, row)}


@router.patch("/projects/{project_id}")
def patch_project(
    project_id: str, payload: ProjectPatch, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row = db.get(Project, project_id)
    if not row:
        raise HTTPException(404, "Project not found")
    if row.version != payload.version:
        raise HTTPException(412, "Project version changed")
    if payload.slug and payload.slug != row.slug:
        db.add(ProjectAlias(project_id=row.id, alias=row.slug, kind="HISTORICAL_SLUG"))
        row.slug = payload.slug
    if payload.name is not None:
        row.name = payload.name
    if payload.purpose is not None:
        row.purpose = payload.purpose
    row.version += 1
    db.commit()
    return serialize_project(row)


@router.post("/projects/{project_id}/recover-token", status_code=201)
def create_recovery_token(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    if not db.get(Project, project_id):
        raise HTTPException(404, "Project not found")
    payload = TokenCreate(
        label="Project manifest recovery",
        token_type="PROJECT_AGENT",
        scope_type="PROJECT",
        scope_id=project_id,
        scopes=[f"project:{project_id}:recover"],
        expires_in_sec=900,
        one_time=True,
    )
    row, raw = create_credential(db, payload)
    db.commit()
    return {
        "token_id": row.id,
        "recover_token": raw,
        "expires_at": row.expires_at,
        "shown_once": True,
    }


@router.post("/projects/{project_id}/retire")
def retire_project(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row = db.get(Project, project_id)
    if not row:
        raise HTTPException(404, "Project not found")
    row.status = "RETIRED"
    row.retired_at = utcnow()
    db.commit()
    return {"project_id": row.id, "status": row.status}


@router.post("/worker-enrollment-tokens", status_code=201)
def worker_enrollment_token(request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row, raw = create_credential(
        db,
        TokenCreate(
            label="Worker enrollment",
            token_type="WORKER_ENROLLMENT",
            scope_type="GLOBAL",
            scopes=["worker:enroll"],
            expires_in_sec=900,
            one_time=True,
        ),
    )
    db.commit()
    return {
        "token_id": row.id,
        "enrollment_token": raw,
        "expires_at": row.expires_at,
        "shown_once": True,
    }


@router.post("/host-install-tokens", status_code=201)
def host_install_token(request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    """Friendly alias: an install token is consumed once and is never an Agent token."""
    result = worker_enrollment_token(request, db)
    return {
        "token_id": result["token_id"],
        "install_token": result["enrollment_token"],
        "expires_at": result["expires_at"],
        "shown_once": True,
    }


@router.post("/workers/enroll", status_code=201)
def enroll_worker(
    payload: WorkerEnroll, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    authenticate_raw_token(
        db,
        payload.enrollment_token,
        required_type="WORKER_ENROLLMENT",
        required_scope="worker:enroll",
    )
    host = db.get(WorkerHost, payload.host_id)
    if host and host.fingerprint != payload.fingerprint:
        raise HTTPException(409, "Host identity conflicts with existing fingerprint")
    if not host:
        host = WorkerHost(
            id=payload.host_id,
            name=payload.host_name,
            fingerprint=payload.fingerprint,
            os=payload.os,
            architecture=payload.architecture,
            status="ONLINE",
            last_heartbeat_at=utcnow(),
        )
        db.add(host)
    worker = Worker(
        host_id=host.id,
        version=payload.worker_version,
        protocol_version=payload.protocol_version,
        compatibility="COMPATIBLE"
        if payload.worker_version.split(".")[0] == settings.hub_version.split(".")[0]
        else "INCOMPATIBLE",
        status="ONLINE",
        last_heartbeat_at=utcnow(),
        adapters=payload.adapters,
    )
    db.add(worker)
    db.flush()
    credential, raw = create_credential(
        db,
        TokenCreate(
            label=f"Worker {payload.host_name}",
            token_type="WORKER_CREDENTIAL",
            scope_type="WORKER",
            scope_id=worker.id,
            scopes=[f"worker:{worker.id}:sync"],
            one_time=False,
        ),
    )
    host_access, raw_host_access = create_credential(
        db,
        TokenCreate(
            label=f"Host Access · {payload.host_name}",
            token_type="HOST_ACCESS",
            scope_type="HOST",
            scope_id=host.id,
            scopes=[f"host:{host.id}:provision"],
            one_time=False,
        ),
    )
    host.provisioning_enabled = True
    audit(
        db,
        event_code="WORKER_ENROLLED",
        request_id=request_id(request),
        actor_type="WORKER_ENROLLMENT",
        target_type="WORKER",
        target_id=worker.id,
        details={"host_id": host.id, "version": worker.version},
    )
    db.commit()
    return {
        "worker_id": worker.id,
        "host_id": host.id,
        "worker_credential": raw,
        "credential_id": credential.id,
        "host_access_token": raw_host_access,
        "host_access_credential_id": host_access.id,
        "hub_version": settings.hub_version,
        "compatibility": worker.compatibility,
        "long_poll_sec": settings.long_poll_hold_sec,
    }


@router.post("/workers/{worker_id}/host-access/bootstrap")
def bootstrap_host_access(
    worker_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    """Idempotently upgrades an existing Worker without re-enrollment."""
    require_worker(request, db, worker_id)
    worker = db.get(Worker, worker_id)
    if not worker:
        raise HTTPException(404, "Worker not found")
    host = db.get(WorkerHost, worker.host_id)
    if not host:
        raise HTTPException(404, "Host not found")
    row = active_credential(db, "HOST_ACCESS", "HOST", host.id)
    created = False
    if not row:
        if not host.provisioning_enabled:
            return {"status": "DISABLED", "host_id": host.id}
        row, raw = create_credential(
            db,
            TokenCreate(
                label=f"Host Access · {host.name}",
                token_type="HOST_ACCESS",
                scope_type="HOST",
                scope_id=host.id,
                scopes=[f"host:{host.id}:provision"],
            ),
        )
        created = True
    else:
        raw = decrypt_credential_secret(row)
    db.commit()
    return {
        "status": "ACTIVE",
        "host_id": host.id,
        "host_access_token": raw,
        "credential_id": row.id,
        "created": created,
    }


@router.post("/workers/{worker_id}/heartbeat")
def worker_heartbeat(
    worker_id: str, payload: Heartbeat, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_worker(request, db, worker_id)
    worker = db.get(Worker, worker_id)
    if not worker:
        raise HTTPException(404, "Worker not found")
    worker.last_heartbeat_at = utcnow()
    worker.status = "ONLINE"
    worker.version = payload.worker_version
    worker.resource_summary = payload.resource_summary
    host = db.get(WorkerHost, worker.host_id)
    if host:
        host.status = "ONLINE"
        host.last_heartbeat_at = utcnow()
    for item in payload.project_instances:
        instance_id = item.get("instance_id")
        if instance_id:
            instance = db.get(ProjectInstance, instance_id)
            if instance and instance.worker_id == worker.id:
                instance.heartbeat_at = utcnow()
                instance.current_source_revision_id = item.get(
                    "source_revision_id", instance.current_source_revision_id
                )
                instance.status = item.get("status", "ONLINE")
                instance.registration_state = "REGISTERED"
                project = db.get(Project, instance.project_id)
                if project:
                    project.registration_state = "REGISTERED"
                    project.last_synced_at = utcnow()
    db.commit()
    return {"accepted": True, "server_time": utcnow(), "compatibility": worker.compatibility}


@router.post("/workers/{worker_id}/projects/register", status_code=201)
def register_worker_project(
    worker_id: str,
    payload: WorkerProjectRegister,
    request: Request,
    db: Session = Depends(get_db),
) -> dict[str, Any]:
    """Idempotently bind a portable project manifest to this Worker and revision."""
    credential = authenticate_bearer(request, db)
    worker = db.get(Worker, worker_id)
    if not worker or worker.host_id != payload.host_id:
        raise HTTPException(409, "Worker host identity does not match registration")
    if payload.revision.project_id != payload.project_id:
        raise HTTPException(409, "Revision project identity does not match manifest")

    project = db.get(Project, payload.project_id)
    existing_instance = db.get(ProjectInstance, payload.instance_id)
    creates_or_links = project is None or existing_instance is None
    host = db.get(WorkerHost, payload.host_id)
    if credential.token_type == "WORKER_CREDENTIAL":
        if credential.scope_id != worker_id:
            raise HTTPException(403, "Worker credential scope denied")
        if creates_or_links:
            raise HTTPException(
                403,
                "HOST_ACCESS_REQUIRED: machine sync cannot create or link a new Hub project",
            )
    elif credential.token_type == "HOST_ACCESS":
        if (
            credential.scope_type != "HOST"
            or credential.scope_id != payload.host_id
            or not host
            or not host.provisioning_enabled
            or f"host:{payload.host_id}:provision" not in credential.scopes
        ):
            raise HTTPException(403, "Host Access is disabled or scoped to another Host")
        if project is not None and existing_instance is None:
            raise HTTPException(
                403,
                "PROJECT_ACCESS_REQUIRED: bind an existing logical Project with its Project Access token",
            )
    elif credential.token_type == "PROJECT_AGENT":
        if (
            project is None
            or credential.scope_type != "PROJECT"
            or credential.scope_id != payload.project_id
            or f"project:{payload.project_id}:instance_bind" not in credential.scopes
        ):
            raise HTTPException(403, "Project Access cannot bind this ProjectInstance")
    else:
        raise HTTPException(403, "Credential cannot register a ProjectInstance")

    project_created = project is None
    if not project:
        requested_slug = payload.slug
        collision = db.scalar(select(Project).where(Project.slug == requested_slug))
        selected_slug = (
            f"{requested_slug[:118]}-{payload.project_id[:8]}" if collision else requested_slug
        )
        project = Project(
            id=payload.project_id,
            slug=selected_slug,
            name=payload.name,
            parent_project_id=payload.parent_project_id,
            registration_state="REGISTERED",
            first_registered_at=utcnow(),
            last_synced_at=utcnow(),
        )
        db.add(project)
        db.flush()
        if collision:
            db.add(
                ProjectAlias(
                    project_id=project.id,
                    alias=requested_slug,
                    kind="MANIFEST_SLUG_COLLISION",
                )
            )
        wiki_mode, wiki_status = normalized_worker_wiki(payload.wiki)
        db.add(
            WikiBinding(
                project_id=project.id,
                mode=wiki_mode,
                wiki_id=str(payload.wiki.get("wiki_id"))[:255]
                if payload.wiki.get("wiki_id")
                else None,
                local_path=str(payload.wiki.get("local_path"))[:2048]
                if payload.wiki.get("local_path")
                else None,
                status=wiki_status,
            )
        )
    elif project.status == "RETIRED":
        raise HTTPException(409, "Project is retired")
    else:
        project.registration_state = "REGISTERED"
        project.last_synced_at = utcnow()
        if project.first_registered_at is None:
            project.first_registered_at = utcnow()
        binding = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project.id))
        if binding:
            wiki_mode, wiki_status = normalized_worker_wiki(payload.wiki)
            binding.mode = wiki_mode
            binding.wiki_id = (
                str(payload.wiki.get("wiki_id"))[:255] if payload.wiki.get("wiki_id") else None
            )
            binding.local_path = (
                str(payload.wiki.get("local_path"))[:2048]
                if payload.wiki.get("local_path")
                else None
            )
            binding.status = wiki_status

    instance = existing_instance
    if instance and instance.project_id != payload.project_id:
        raise HTTPException(409, "Instance identity belongs to another project")
    if not instance:
        instance = ProjectInstance(
            id=payload.instance_id,
            project_id=payload.project_id,
            host_id=payload.host_id,
            worker_id=worker_id,
            primary_path=payload.primary_path,
            related_roots=payload.related_roots,
            heartbeat_at=utcnow(),
            status="ONLINE",
            registration_state="REGISTERED",
            capabilities=payload.capabilities,
            index_status=payload.index_status,
            worker_version=payload.worker_version,
            policy_schema_version=payload.policy_schema_version,
            project_manifest_schema_version=payload.project_manifest_schema_version,
            initialized_at=payload.initialized_at,
            first_seen_at=utcnow(),
        )
        db.add(instance)
        db.flush()
    else:
        instance.worker_id = worker_id
        instance.host_id = payload.host_id
        instance.primary_path = payload.primary_path
        instance.related_roots = payload.related_roots
        instance.heartbeat_at = utcnow()
        instance.status = "ONLINE"
        instance.registration_state = "REGISTERED"
        instance.capabilities = payload.capabilities
        instance.index_status = payload.index_status
        instance.worker_version = payload.worker_version
        instance.policy_schema_version = payload.policy_schema_version
        instance.project_manifest_schema_version = payload.project_manifest_schema_version
        instance.initialized_at = payload.initialized_at or instance.initialized_at

    revision = db.scalar(
        select(SourceRevision).where(
            SourceRevision.instance_id == instance.id,
            SourceRevision.revision_id == payload.revision.revision_id,
        )
    )
    previous = instance.current_source_revision_id
    restored = revision is not None and previous not in (None, payload.revision.revision_id)
    if not revision:
        revision = SourceRevision(
            revision_id=payload.revision.revision_id,
            project_id=payload.project_id,
            instance_id=instance.id,
            policy_hash=payload.revision.policy_hash,
            indexer_version=payload.revision.indexer_version,
            root_mapping_version=payload.revision.root_mapping_version,
            file_count=payload.revision.file_count,
            inventory_hash=payload.revision.inventory_hash,
            git_commit=payload.revision.git_commit,
            git_dirty=payload.revision.git_dirty,
        )
        db.add(revision)
    if restored:
        db.add(
            RecoveryEvent(
                project_id=payload.project_id,
                instance_id=instance.id,
                detected_revision_id=payload.revision.revision_id,
                previous_revision_id=previous,
                reason="Source returned to a known revision; external restore or revert suspected.",
            )
        )
    instance.current_source_revision_id = payload.revision.revision_id
    project_access = active_project_access(db, project.id)
    if project_access is not None:
        project_access.label = f"Project Access · {project.name}"
        project_access = normalize_project_access_scopes(project_access, project.id)
    historical_project_access = (
        db.scalar(
            select(func.count())
            .select_from(AgentCredential)
            .where(
                AgentCredential.token_type == "PROJECT_AGENT",
                AgentCredential.scope_type == "PROJECT",
                AgentCredential.scope_id == project.id,
            )
        )
        or 0
    )
    project_access_token = None
    if project_access is None and (project_created or historical_project_access == 0):
        project_access, project_access_token = ensure_project_access(db, project.id, project.name)
    if (
        project_access is not None
        and project_access_token is None
        and credential.token_type
        in {
            "HOST_ACCESS",
            "WORKER_CREDENTIAL",
        }
    ):
        # A source-owning Worker may keep the local project binding current. This does not grant
        # shell access, and a deliberate Project Access revoke is never recreated automatically.
        project_access_token = decrypt_credential_secret(project_access)
    db.commit()
    return {
        "project_id": project.id,
        "instance_id": instance.id,
        "revision_id": payload.revision.revision_id,
        "restored": restored,
        "registration_state": "REGISTERED",
        "first_registered_at": rfc3339(project.first_registered_at),
        "last_synced_at": rfc3339(project.last_synced_at),
        "project_access": {
            "credential_id": project_access.id if project_access else None,
            "status": project_access.status if project_access else "MISSING",
            "token": project_access_token,
            "shown_now": project_access_token is not None,
            "created_for_new_project": project_created and project_access_token is not None,
        },
    }


@router.get("/workers")
def list_workers(request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    require_owner(request, db)
    rows = list(db.scalars(select(Worker).order_by(desc(Worker.last_heartbeat_at)).limit(200)))
    return {
        "items": [
            {
                "worker_id": row.id,
                "host_id": row.host_id,
                "version": row.version,
                "status": row.status,
                "compatibility": row.compatibility,
                "last_heartbeat_at": row.last_heartbeat_at,
                "resources": row.resource_summary,
                "adapters": row.adapters,
            }
            for row in rows
        ],
        "next_cursor": None,
    }


def serialize_host_access(db: Session, host: WorkerHost) -> dict[str, Any]:
    row = active_credential(db, "HOST_ACCESS", "HOST", host.id)
    return {
        "host_id": host.id,
        "host_name": host.name,
        "status": "ACTIVE" if row and host.provisioning_enabled else "DISABLED",
        "can_create_or_link_projects": bool(row and host.provisioning_enabled),
        "credential_id": row.id if row else None,
        "token_prefix": row.token_prefix if row else None,
        "last_used_at": row.last_used_at if row else None,
        "worker_sync_separate": True,
    }


@router.get("/hosts/{host_id}/access")
def get_host_access(
    host_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner(request, db)
    host = db.get(WorkerHost, host_id)
    if not host:
        raise HTTPException(404, "Host not found")
    return serialize_host_access(db, host)


@router.post("/hosts/{host_id}/access/rotate")
def rotate_host_access(
    host_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    require_recent_reauth(identity)
    host = db.get(WorkerHost, host_id)
    if not host:
        raise HTTPException(404, "Host not found")
    old = active_credential(db, "HOST_ACCESS", "HOST", host_id)
    if old:
        old.status = "ROTATED"
        old.revoked_at = utcnow()
    row, raw = create_credential(
        db,
        TokenCreate(
            label=f"Host Access · {host.name}",
            token_type="HOST_ACCESS",
            scope_type="HOST",
            scope_id=host_id,
            scopes=[f"host:{host_id}:provision"],
        ),
    )
    host.provisioning_enabled = True
    audit(
        db,
        event_code="HOST_ACCESS_ROTATED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="HOST",
        target_id=host_id,
        details={"credential_id": row.id},
    )
    db.commit()
    return {
        "host_id": host_id,
        "token": raw,
        "token_type": "HOST_ACCESS",
        "status": "ACTIVE",
    }


@router.post("/hosts/{host_id}/access/revoke")
def revoke_host_access(
    host_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    host = db.get(WorkerHost, host_id)
    if not host:
        raise HTTPException(404, "Host not found")
    host.provisioning_enabled = False
    rows = list(
        db.scalars(
            select(AgentCredential).where(
                AgentCredential.token_type == "HOST_ACCESS",
                AgentCredential.scope_type == "HOST",
                AgentCredential.scope_id == host_id,
                AgentCredential.status == "ACTIVE",
            )
        )
    )
    for row in rows:
        row.status = "REVOKED"
        row.revoked_at = utcnow()
    audit(
        db,
        event_code="HOST_ACCESS_REVOKED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="HOST",
        target_id=host_id,
        details={"worker_sync_unchanged": True},
    )
    db.commit()
    return {
        "host_id": host_id,
        "status": "DISABLED",
        "local_intelligence_unchanged": True,
        "worker_sync_unchanged": True,
    }


@router.get("/workers/{worker_id}")
def get_worker(worker_id: str, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    require_owner(request, db)
    row = db.get(Worker, worker_id)
    if not row:
        raise HTTPException(404, "Worker not found")
    return {
        "worker_id": row.id,
        "host_id": row.host_id,
        "version": row.version,
        "status": row.status,
        "compatibility": row.compatibility,
        "last_heartbeat_at": row.last_heartbeat_at,
        "resources": row.resource_summary,
        "adapters": row.adapters,
    }


@router.post("/projects/{project_id}/instances/register", status_code=201)
def register_instance(
    project_id: str, payload: InstanceRegister, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    if payload.worker_id:
        require_worker(request, db, payload.worker_id)
    else:
        require_owner(request, db)
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found; register it before the instance")
    existing = db.get(ProjectInstance, payload.instance_id)
    if existing:
        if existing.project_id != project_id:
            raise HTTPException(409, "Instance identity belongs to another project")
        return {"instance_id": existing.id, "status": existing.status, "existing": True}
    instance = ProjectInstance(
        id=payload.instance_id,
        project_id=project_id,
        host_id=payload.host_id,
        worker_id=payload.worker_id,
        primary_path=payload.primary_path,
        related_roots=payload.related_roots,
        current_source_revision_id=payload.current_source_revision_id,
        heartbeat_at=utcnow(),
    )
    db.add(instance)
    db.commit()
    return {
        "instance_id": instance.id,
        "project_id": project_id,
        "status": instance.status,
        "existing": False,
    }


@router.get("/projects/{project_id}/instances")
def list_instances(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    rows = list(db.scalars(select(ProjectInstance).where(ProjectInstance.project_id == project_id)))
    return {
        "items": [
            {
                "instance_id": row.id,
                "host_id": row.host_id,
                "worker_id": row.worker_id,
                "primary_path": row.primary_path,
                "current_source_revision_id": row.current_source_revision_id,
                "registration_state": row.registration_state,
                "status": row.status,
                "heartbeat_at": row.heartbeat_at,
                "first_seen_at": row.first_seen_at,
                "index_status": row.index_status,
                "worker_version": row.worker_version,
            }
            for row in rows
        ],
        "next_cursor": None,
    }


@router.post("/instances/{instance_id}/revisions", status_code=201)
def create_revision(
    instance_id: str, payload: RevisionCreate, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    instance = db.get(ProjectInstance, instance_id)
    if not instance:
        raise HTTPException(404, "Instance not found")
    if instance.worker_id:
        require_worker(request, db, instance.worker_id)
    if payload.project_id != instance.project_id:
        raise HTTPException(409, "Revision project does not match instance")
    existing = db.scalar(
        select(SourceRevision).where(
            SourceRevision.instance_id == instance_id,
            SourceRevision.revision_id == payload.revision_id,
        )
    )
    if existing:
        previous = instance.current_source_revision_id
        restored = previous not in (None, payload.revision_id)
        if restored:
            db.add(
                RecoveryEvent(
                    project_id=payload.project_id,
                    instance_id=instance_id,
                    detected_revision_id=payload.revision_id,
                    previous_revision_id=previous,
                    reason="Source returned to a previously known revision; external restore or revert suspected.",
                )
            )
            instance.current_source_revision_id = payload.revision_id
            db.commit()
        return {"revision_id": existing.revision_id, "existing": True, "restored": restored}
    previous = instance.current_source_revision_id
    row = SourceRevision(
        revision_id=payload.revision_id,
        project_id=payload.project_id,
        instance_id=instance_id,
        policy_hash=payload.policy_hash,
        indexer_version=payload.indexer_version,
        root_mapping_version=payload.root_mapping_version,
        file_count=payload.file_count,
        inventory_hash=payload.inventory_hash,
        git_commit=payload.git_commit,
        git_dirty=payload.git_dirty,
    )
    db.add(row)
    instance.current_source_revision_id = payload.revision_id
    db.commit()
    return {"revision_id": row.revision_id, "existing": False, "previous_revision_id": previous}


@router.get("/projects/{project_id}/revisions")
def list_revisions(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    rows = list(
        db.scalars(
            select(SourceRevision)
            .where(SourceRevision.project_id == project_id)
            .order_by(desc(SourceRevision.created_at))
            .limit(200)
        )
    )
    return {
        "items": [
            {
                "revision_id": row.revision_id,
                "instance_id": row.instance_id,
                "policy_hash": row.policy_hash,
                "file_count": row.file_count,
                "git_commit": row.git_commit,
                "git_dirty": row.git_dirty,
                "created_at": row.created_at,
            }
            for row in rows
        ],
        "next_cursor": None,
    }


@router.post("/instances/{instance_id}/change-runs", status_code=201)
def create_change_run(
    instance_id: str, payload: ChangeRunCreate, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    instance = db.get(ProjectInstance, instance_id)
    if not instance:
        raise HTTPException(404, "Instance not found")
    if instance.worker_id:
        require_worker(request, db, instance.worker_id)
    if payload.project_id != instance.project_id:
        raise HTTPException(409, "ChangeRun project mismatch")
    active = db.scalar(
        select(ChangeRun).where(
            ChangeRun.instance_id == instance_id,
            ChangeRun.status.in_(["OPEN", "PREFLIGHTED", "EDITING", "REVIEWING", "VERIFYING"]),
        )
    )
    if active and active.id != payload.change_run_id:
        raise HTTPException(409, f"Instance is in use by ChangeRun {active.id}")
    existing = db.get(ChangeRun, payload.change_run_id)
    if existing:
        return {"change_run_id": existing.id, "status": existing.status, "existing": True}
    row = ChangeRun(
        id=payload.change_run_id,
        project_id=payload.project_id,
        instance_id=instance_id,
        agent_client=payload.agent_client,
        agent_session_id=payload.agent_session_id,
        task_summary=payload.task_summary,
        before_source_revision_id=payload.before_source_revision_id,
        risk_before=payload.risk_before,
        required_checks=payload.required_checks,
        status="PREFLIGHTED",
    )
    db.add(row)
    if payload.agent_client:
        db.add(
            AgentSessionActivity(
                project_id=payload.project_id,
                instance_id=instance_id,
                host_id=instance.host_id,
                agent_client=payload.agent_client,
                session_id=payload.agent_session_id,
                activity="CHANGE_RUN",
                started_at=utcnow(),
            )
        )
    db.commit()
    return {"change_run_id": row.id, "status": row.status, "existing": False}


@router.patch("/change-runs/{change_run_id}")
def patch_change_run(
    change_run_id: str, payload: ChangeRunPatch, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    row = db.get(ChangeRun, change_run_id)
    if not row:
        raise HTTPException(404, "ChangeRun not found")
    instance = db.get(ProjectInstance, row.instance_id)
    if instance and instance.worker_id:
        require_worker(request, db, instance.worker_id)
    row.status = payload.status
    row.after_source_revision_id = payload.after_source_revision_id
    row.risk_after = payload.risk_after
    row.quality_run_id = payload.quality_run_id
    if payload.status in {"CLOSED", "ABANDONED"}:
        row.completed_at = utcnow()
    db.commit()
    return {"change_run_id": row.id, "status": row.status}


def ingest_report(instance_id: str, envelope: WorkerReportEnvelope, db: Session) -> dict[str, Any]:
    instance = db.get(ProjectInstance, instance_id)
    if not instance or instance.project_id != envelope.project_id:
        raise HTTPException(404, "Project instance not found")
    if envelope.instance_id != instance_id:
        raise HTTPException(409, "Report instance mismatch")
    revision = db.scalar(
        select(SourceRevision).where(
            SourceRevision.instance_id == instance_id,
            SourceRevision.revision_id == envelope.source_revision_id,
        )
    )
    if not revision and envelope.revision:
        metadata = envelope.revision
        if (
            metadata.revision_id != envelope.source_revision_id
            or metadata.project_id != envelope.project_id
        ):
            raise HTTPException(409, "Report revision metadata does not match its envelope")
        revision = SourceRevision(
            revision_id=metadata.revision_id,
            project_id=envelope.project_id,
            instance_id=instance_id,
            policy_hash=metadata.policy_hash,
            indexer_version=metadata.indexer_version,
            root_mapping_version=metadata.root_mapping_version,
            file_count=metadata.file_count,
            inventory_hash=metadata.inventory_hash,
            git_commit=metadata.git_commit,
            git_dirty=metadata.git_dirty,
        )
        db.add(revision)
        db.flush()
    if not revision:
        raise HTTPException(409, "Report references an unknown source revision")
    if envelope.report_type == "quality":
        payload = QualityReport.model_validate(envelope.payload)
        if (
            payload.project_id != envelope.project_id
            or payload.source_revision_id != envelope.source_revision_id
        ):
            raise HTTPException(409, "Quality report identity does not match its envelope")
        gate = evaluate_gate(
            GateInputs(
                source_revision_id=payload.source_revision_id,
                current_source_revision_id=instance.current_source_revision_id,
                required_checks=tuple(payload.required_checks),
                completed_checks=tuple(payload.completed_checks),
                failed_checks=tuple(envelope.payload.get("failed_checks", [])),
                blocking_findings=int(envelope.payload.get("blocking_findings", 0)),
                blocking_unknowns=tuple(payload.blocking_unknowns),
                warnings=tuple(payload.warnings),
                graph_state=envelope.payload.get("graph_state", "CURRENT"),
                unsupported_required_areas=tuple(
                    envelope.payload.get("unsupported_required_areas", [])
                ),
            )
        )
        verdict = gate.verdict
        if payload.verdict == "FAIL":
            verdict = "FAIL"
        row = db.get(QualityRun, payload.quality_run_id)
        if not row:
            row = QualityRun(
                id=payload.quality_run_id,
                project_id=envelope.project_id,
                instance_id=instance_id,
                source_revision_id=payload.source_revision_id,
                change_run_id=payload.change_run_id,
                verdict=verdict,
                currentness=gate.currentness,
                risk_score=payload.risk_score,
                required_checks=payload.required_checks,
                completed_checks=payload.completed_checks,
                blocking_unknowns=list(gate.reasons),
                warnings=list(gate.warnings),
                policy_hash=payload.policy_hash,
            )
            db.add(row)
        change_run = db.get(ChangeRun, payload.change_run_id) if payload.change_run_id else None
        if change_run:
            change_run.after_source_revision_id = payload.source_revision_id
            change_run.risk_after = payload.risk_score
            change_run.quality_run_id = payload.quality_run_id
            if verdict in {"PASS", "PASS_WITH_WARNINGS"}:
                change_run.status = "GATED"
                change_run.completed_at = utcnow()
            else:
                change_run.status = "VERIFYING"
        return {"quality_run_id": row.id, "verdict": row.verdict, "currentness": row.currentness}
    if envelope.report_type == "context_pack":
        payload = ContextPackIn.model_validate(envelope.payload)
        row = db.get(ProjectContextPack, payload.context_pack_id)
        if not row:
            row = ProjectContextPack(
                id=payload.context_pack_id,
                project_id=payload.project_id,
                instance_id=instance_id,
                source_revision_id=payload.source_revision_id,
                freshness=payload.freshness
                if payload.source_revision_id == instance.current_source_revision_id
                else "STALE",
                purpose=payload.purpose,
                languages=payload.languages,
                frameworks=payload.frameworks,
                architecture_summary=payload.architecture_summary,
                entry_points=payload.entry_points,
                important_flows=payload.important_flows,
                external_services=payload.external_services,
                risk_areas=payload.risk_areas,
                recent_changes=payload.recent_changes,
                open_unknowns=payload.open_unknowns,
                coverage_summary=payload.coverage_summary,
                producer=payload.producer,
            )
            db.add(row)
        return {"context_pack_id": row.id, "freshness": row.freshness}
    payload = envelope.payload
    evidence_id = str(payload.get("evidence_id") or envelope.message_id)
    row = db.get(Evidence, evidence_id)
    if not row:
        row = Evidence(
            id=evidence_id,
            project_id=envelope.project_id,
            instance_id=instance_id,
            source_revision_id=envelope.source_revision_id,
            change_run_id=payload.get("change_run_id"),
            producer=str(payload.get("producer", "worker")),
            producer_version=str(payload.get("producer_version", "unknown")),
            origin=str(payload.get("origin", envelope.report_type.upper())),
            confidence=float(payload.get("confidence", 0.0)),
            coverage=payload.get("coverage", {}),
            unsupported_areas=payload.get("unsupported_areas", []),
            unknowns=payload.get("unknowns", []),
            policy_hash=str(payload.get("policy_hash", "unknown")),
            summary=str(payload.get("summary", ""))[:100_000],
            completed_at=utcnow(),
        )
        db.add(row)
    if envelope.report_type == "index":
        snapshot_id = str(payload.get("index_snapshot_id") or uuid4_str())
        if not db.get(IndexSnapshot, snapshot_id):
            db.add(
                IndexSnapshot(
                    id=snapshot_id,
                    project_id=envelope.project_id,
                    instance_id=instance_id,
                    source_revision_id=envelope.source_revision_id,
                    state=str(payload.get("state", "INVALID")),
                    coverage=payload.get("coverage", {}),
                    db_size_bytes=int(payload.get("db_size_bytes", 0)),
                    completed_at=utcnow(),
                )
            )
    return {"evidence_id": row.id, "accepted": True}


@router.post("/instances/{instance_id}/reports/{report_type}", status_code=201)
def post_report(
    instance_id: str,
    report_type: str,
    envelope: WorkerReportEnvelope,
    request: Request,
    db: Session = Depends(get_db),
) -> dict[str, Any]:
    instance = db.get(ProjectInstance, instance_id)
    if not instance or not instance.worker_id:
        raise HTTPException(404, "Worker-backed instance not found")
    require_worker(request, db, instance.worker_id)
    if report_type not in {
        "index",
        "review",
        "tests",
        "smoke",
        "runtime",
        "dataflow",
        "quality",
        "context_pack",
    }:
        raise HTTPException(404, "Unknown report type")
    if envelope.report_type != report_type:
        raise HTTPException(409, "Report type mismatch")
    result = ingest_report(instance_id, envelope, db)
    db.commit()
    return result


@router.get("/projects/{project_id}/health")
def get_health(project_id: str, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    return project_health(db, project)


@router.get("/projects/{project_id}/quality-history")
def quality_history(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    rows = list(
        db.scalars(
            select(QualityRun)
            .where(QualityRun.project_id == project_id)
            .order_by(desc(QualityRun.completed_at))
            .limit(200)
        )
    )
    return {
        "items": [
            {
                "quality_run_id": row.id,
                "instance_id": row.instance_id,
                "source_revision_id": row.source_revision_id,
                "verdict": row.verdict,
                "currentness": row.currentness,
                "risk_score": row.risk_score,
                "blocking_unknowns": row.blocking_unknowns,
                "warnings": row.warnings,
                "completed_at": row.completed_at,
            }
            for row in rows
        ],
        "next_cursor": None,
    }


@router.get("/projects/{project_id}/findings")
def findings(project_id: str, request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    rows = list(
        db.scalars(
            select(Finding)
            .where(Finding.project_id == project_id)
            .order_by(desc(Finding.updated_at))
            .limit(200)
        )
    )
    return {
        "items": [
            {
                "finding_id": row.id,
                "source_revision_id": row.source_revision_id,
                "severity": row.severity,
                "category": row.category,
                "path": row.path,
                "source_range": row.source_range,
                "why": row.why,
                "suggested_repair": row.suggested_repair,
                "blocking": row.blocking,
                "status": row.status,
                "updated_at": row.updated_at,
            }
            for row in rows
        ],
        "next_cursor": None,
    }


@router.get("/projects/{project_id}/context-pack")
def get_context_pack(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    repo = ProjectRepository(db)
    project = repo.get(project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    pack = repo.latest_context(project_id)
    health = project_health(db, project)
    if not pack:
        return {
            "project_id": project.id,
            "purpose": project.purpose,
            "freshness": "LAST_KNOWN",
            "source_revision_id": health.get("current_revision"),
            "architecture_summary": None,
            "languages": [],
            "frameworks": [],
            "open_unknowns": ["Worker has not synchronized a Context Pack yet."],
            "health": health,
        }
    freshness = pack.freshness
    if pack.source_revision_id != health.get("current_revision"):
        freshness = "STALE"
    return {
        "context_pack_id": pack.id,
        "project_id": project.id,
        "purpose": pack.purpose or project.purpose,
        "freshness": freshness,
        "source_revision_id": pack.source_revision_id,
        "produced_at": pack.produced_at,
        "producer": pack.producer,
        "languages": pack.languages,
        "frameworks": pack.frameworks,
        "architecture_summary": pack.architecture_summary,
        "entry_points": pack.entry_points,
        "important_flows": pack.important_flows,
        "external_services": pack.external_services,
        "risk_areas": pack.risk_areas,
        "recent_changes": pack.recent_changes,
        "open_unknowns": pack.open_unknowns,
        "coverage_summary": pack.coverage_summary,
        "health": health,
    }


@router.get("/projects/{project_id}/agent-context")
def get_agent_context(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    context = get_context_pack(project_id, request, db)
    binding = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project_id))
    context["wiki"] = {
        "mode": binding.mode if binding else "none",
        "status": binding.status if binding else "DISABLED",
        "wiki_id": binding.wiki_id if binding else None,
    }
    context["operating_policy_url"] = (
        f"{settings.base_url}/api/v1/docs/AGENT_OPERATING_POLICY.md?format=raw"
    )
    return context


@router.get("/projects/{project_id}/architecture-summary")
def architecture_summary(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    context = get_context_pack(project_id, request, db)
    return {
        "project_id": project_id,
        "source_revision_id": context.get("source_revision_id"),
        "freshness": context.get("freshness"),
        "architecture_summary": context.get("architecture_summary"),
        "entry_points": context.get("entry_points", []),
        "important_flows": context.get("important_flows", []),
    }


@router.get("/projects/{project_id}/access")
def get_project_access(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner(request, db)
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    row = active_project_access(db, project_id)
    if row is not None:
        row.label = f"Project Access · {project.name}"
        row = normalize_project_access_scopes(row, project_id)
        db.commit()
    return {
        "project_id": project_id,
        "status": "ACTIVE" if row else "MISSING",
        "credential_id": row.id if row else None,
        "token_prefix": row.token_prefix if row else None,
        "scopes": row.scopes if row else project_access_scopes(project_id),
        "local_intelligence_requires_token": False,
    }


@router.post("/projects/{project_id}/access/rotate")
def rotate_project_access(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    require_recent_reauth(identity)
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    old_rows = list(
        db.scalars(
            select(AgentCredential).where(
                AgentCredential.token_type == "PROJECT_AGENT",
                AgentCredential.scope_type == "PROJECT",
                AgentCredential.scope_id == project_id,
                AgentCredential.status == "ACTIVE",
            )
        )
    )
    for old in old_rows:
        old.status = "ROTATED"
        old.revoked_at = utcnow()
    row, raw = create_credential(
        db,
        TokenCreate(
            label=f"Project Access · {project.name}",
            token_type="PROJECT_AGENT",
            scope_type="PROJECT",
            scope_id=project_id,
            scopes=project_access_scopes(project_id),
        ),
    )
    audit(
        db,
        event_code="PROJECT_ACCESS_ROTATED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="PROJECT",
        target_id=project_id,
        details={"credential_id": row.id},
    )
    db.commit()
    return {
        "project_id": project_id,
        "token_id": row.id,
        "token": raw,
        "token_type": "PROJECT_AGENT",
        "status": "ACTIVE",
        "scopes": row.scopes,
    }


@router.post("/projects/{project_id}/access/revoke")
def revoke_project_access(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    identity = require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    rows = list(
        db.scalars(
            select(AgentCredential).where(
                AgentCredential.token_type == "PROJECT_AGENT",
                AgentCredential.scope_type == "PROJECT",
                AgentCredential.scope_id == project_id,
                AgentCredential.status == "ACTIVE",
            )
        )
    )
    for row in rows:
        row.status = "REVOKED"
        row.revoked_at = utcnow()
    audit(
        db,
        event_code="PROJECT_ACCESS_REVOKED",
        request_id=request_id(request),
        actor_type="OWNER",
        actor_id=identity.owner.id if isinstance(identity, SessionIdentity) else identity.id,
        target_type="PROJECT",
        target_id=project_id,
        details={"local_intelligence_unchanged": True},
    )
    db.commit()
    return {
        "project_id": project_id,
        "status": "REVOKED",
        "local_intelligence_unchanged": True,
    }


@router.post("/projects/{project_id}/access/test")
def test_project_access(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row = active_project_access(db, project_id)
    return {
        "project_id": project_id,
        "status": row.status if row else "MISSING",
        "usable": bool(row),
        "local_intelligence_available": True,
        "last_used_at": row.last_used_at if row else None,
    }


@router.post("/projects/{project_id}/agent-sessions", status_code=201)
def record_agent_session(
    project_id: str,
    payload: AgentSessionIn,
    request: Request,
    db: Session = Depends(get_db),
) -> dict[str, Any]:
    credential = require_project_scope(request, db, project_id)
    if (
        f"project:{project_id}:session_log" not in credential.scopes
        and "admin" not in credential.scopes
    ):
        raise HTTPException(403, "Project Access cannot record session metadata")
    if payload.instance_id:
        instance = db.get(ProjectInstance, payload.instance_id)
        if not instance or instance.project_id != project_id:
            raise HTTPException(409, "ProjectInstance does not belong to this Project")
    row = AgentSessionActivity(
        project_id=project_id,
        instance_id=payload.instance_id,
        host_id=payload.host_id,
        agent_client=payload.agent_client,
        agent_version=payload.agent_version,
        session_id=payload.session_id,
        started_at=payload.started_at or utcnow(),
    )
    db.add(row)
    audit(
        db,
        event_code="AGENT_SESSION_RECORDED",
        request_id=request_id(request),
        actor_type="PROJECT_ACCESS",
        actor_id=credential.id,
        target_type="PROJECT",
        target_id=project_id,
        details={"agent_client": payload.agent_client, "host_id": payload.host_id},
    )
    db.commit()
    return {"activity_id": row.id, "project_id": project_id, "started_at": row.started_at}


@router.get("/projects/{project_id}/agent-notes")
def list_project_agent_notes(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_project_scope(request, db, project_id)
    rows = list(
        db.scalars(
            select(ProjectAgentNote)
            .where(ProjectAgentNote.project_id == project_id)
            .order_by(desc(ProjectAgentNote.created_at))
            .limit(200)
        )
    )
    return {
        "items": [
            {
                "note_id": row.id,
                "kind": row.kind,
                "content": row.content,
                "source_revision_id": row.source_revision_id,
                "agent_client": row.agent_client,
                "created_at": row.created_at,
            }
            for row in rows
        ],
        "next_cursor": None,
    }


@router.post("/projects/{project_id}/agent-notes", status_code=201)
def create_project_agent_note(
    project_id: str,
    payload: ProjectAgentNoteIn,
    request: Request,
    db: Session = Depends(get_db),
) -> dict[str, Any]:
    credential = require_project_scope(request, db, project_id)
    if (
        f"project:{project_id}:context_write" not in credential.scopes
        and "admin" not in credential.scopes
    ):
        raise HTTPException(403, "Project Access cannot write project context")
    row = ProjectAgentNote(
        project_id=project_id,
        source_revision_id=payload.source_revision_id,
        kind=payload.kind,
        content=payload.content,
        agent_client=payload.agent_client,
    )
    db.add(row)
    audit(
        db,
        event_code="PROJECT_AGENT_NOTE_CREATED",
        request_id=request_id(request),
        actor_type="PROJECT_ACCESS",
        actor_id=credential.id,
        target_type="PROJECT_NOTE",
        target_id=row.id,
        details={"project_id": project_id, "kind": payload.kind},
    )
    db.commit()
    return {"note_id": row.id, "project_id": project_id, "created_at": row.created_at}


@router.get("/projects/{project_id}/agent-kit")
def project_agent_kit(project_id: str, request: Request, db: Session = Depends(get_db)) -> Response:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    skill_path = settings.docs_dir / "AGENT_OPERATING_POLICY.md"
    start_path = settings.docs_dir / "START_WORKING_PROMPT.md"
    reference = {
        "project_id": project.id,
        "project_name": project.name,
        "hub_url": settings.base_url,
        "remote_mcp": f"{settings.base_url}/mcp",
        "project_access_token": "NOT_INCLUDED_BY_DESIGN",
        "local_mcp": {"command": "codeintel", "args": ["mcp", "serve", "--stdio"]},
    }
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(
            "README_AGENT.md",
            "This kit contains no secret. Obtain Project Access separately from the owner.\n"
            "Local Code Intelligence works without Hub access.\n",
        )
        archive.writestr(
            "SKILL.md", skill_path.read_text(encoding="utf-8") if skill_path.is_file() else ""
        )
        archive.writestr(
            "START_PROMPT.md",
            start_path.read_text(encoding="utf-8") if start_path.is_file() else "",
        )
        archive.writestr("PROJECT_REFERENCE.json", json.dumps(reference, indent=2))
        archive.writestr(
            "MCP_CONFIG_TEMPLATE.json",
            json.dumps(
                {
                    "local": reference["local_mcp"],
                    "remote": {
                        "url": reference["remote_mcp"],
                        "headers": {"Authorization": "Bearer <PROJECT_ACCESS_TOKEN>"},
                    },
                },
                indent=2,
            ),
        )
    return Response(
        content=stream.getvalue(),
        media_type="application/zip",
        headers={
            "Content-Disposition": f'attachment; filename="codeintel-{project.slug}-agent-kit.zip"',
            "Cache-Control": "no-store",
        },
    )


@router.get("/projects/{project_id}/agent-setup/{client}")
def project_agent_setup(
    project_id: str, client: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    if client not in {"codex", "claude", "hermes", "generic-mcp"}:
        raise HTTPException(404, "Unsupported client")
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    return {
        "client": client,
        "project_id": project_id,
        "local_coding": {
            "purpose": "Work beside the actual source with preflight, review, verification and gate.",
            "mcp": {"command": "codeintel", "args": ["mcp", "serve", "--stdio"]},
        },
        "local_mcp": {"command": "codeintel", "args": ["mcp", "serve", "--stdio"]},
        "remote_mcp": {
            "url": f"{settings.base_url}/mcp",
            "authorization": "Bearer <PROJECT_ACCESS_TOKEN>",
            "optional_metadata_headers": {
                "X-Agent-Client": client,
                "X-Agent-Version": "<CLIENT_VERSION>",
                "X-Agent-Session-ID": "<OPTIONAL_SESSION_ID>",
            },
            "scopes": project_access_scopes(project_id),
            "warning": "Project Access cannot edit code or replace the source-local Quality workflow.",
        },
        "project_read_endpoints": {
            "context_pack": f"{settings.base_url}/api/v1/projects/{project_id}/context-pack",
            "health": f"{settings.base_url}/api/v1/projects/{project_id}/health",
            "quality_history": f"{settings.base_url}/api/v1/projects/{project_id}/quality-history",
            "findings": f"{settings.base_url}/api/v1/projects/{project_id}/findings",
            "recent_changes": f"{settings.base_url}/api/v1/projects/{project_id}/change-runs",
            "live_analysis": f"{settings.base_url}/api/v1/projects/{project_id}/remote-analysis",
            "agent_sessions": f"{settings.base_url}/api/v1/projects/{project_id}/agent-sessions",
            "agent_notes": f"{settings.base_url}/api/v1/projects/{project_id}/agent-notes",
        },
        "install_command": f"curl -fsSL {settings.base_url}/downloads/install.sh | sh -s -- --hub {settings.base_url} --install-token <OPTIONAL_INSTALL_TOKEN> --clients codex,claude,hermes",
        "local_only_install": f"curl -fsSL {settings.base_url}/downloads/install.sh | sh -s -- --hub {settings.base_url} --local-only --clients codex,claude,hermes",
        "agent_kit": f"{settings.base_url}/api/v1/projects/{project_id}/agent-kit",
        "start_prompt_url": f"{settings.base_url}/api/v1/docs/START_WORKING_PROMPT.md?format=raw",
        "remote_reader_prompt_url": f"{settings.base_url}/api/v1/docs/REMOTE_PROJECT_READER_PROMPT.md?format=raw",
        "policy_url": f"{settings.base_url}/api/v1/docs/AGENT_OPERATING_POLICY.md?format=raw",
    }


@router.get("/agent-bootstrap")
def agent_bootstrap(
    client: str, project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    setup = project_agent_setup(project_id, client, request, db)
    binding = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project_id))
    policy_path = settings.docs_dir / "AGENT_OPERATING_POLICY.md"
    return setup | {
        "skill_markdown": policy_path.read_text(encoding="utf-8") if policy_path.exists() else "",
        "wiki": {"mode": binding.mode, "wiki_id": binding.wiki_id} if binding else {"mode": "none"},
        "policy_version": settings.agent_policy_version,
        "compatibility": {
            "product_version": settings.hub_version,
            "worker_mcp_schema_version": settings.worker_mcp_schema_version,
            "hub_mcp_schema_version": settings.hub_mcp_schema_version,
            "project_manifest_schema_version": settings.project_manifest_schema_version,
        },
    }


@router.post("/projects/{project_id}/remote-analysis", status_code=202)
def request_remote_analysis(
    project_id: str,
    payload: RemoteAnalysisCreate,
    request: Request,
    db: Session = Depends(get_db),
) -> dict[str, Any]:
    credential = require_project_scope(request, db, project_id, live=True)
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    instance = (
        db.get(ProjectInstance, payload.instance_id)
        if payload.instance_id
        else ProjectRepository(db).latest_instance(project_id)
    )
    row = RemoteAnalysisRequest(
        project_id=project_id,
        instance_id=instance.id if instance else None,
        query_type=payload.query_type,
        query=payload.query,
        requested_revision_id=payload.source_revision_id,
        status="PENDING",
        expires_at=utcnow() + timedelta(minutes=10),
    )
    db.add(row)
    audit(
        db,
        event_code="REMOTE_ANALYSIS_REQUESTED",
        request_id=request_id(request),
        actor_type="AGENT",
        actor_id=credential.id,
        target_type="REMOTE_ANALYSIS",
        target_id=row.id,
        details={"project_id": project_id, "query_type": payload.query_type},
    )
    db.commit()
    return {
        "request_id": row.id,
        "status": row.status,
        "poll_url": f"/api/v1/remote-analysis/{row.id}",
    }


@router.get("/remote-analysis/{analysis_id}")
def get_remote_analysis(
    analysis_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    row = db.get(RemoteAnalysisRequest, analysis_id)
    if not row:
        raise HTTPException(404, "Remote analysis request not found")
    require_project_scope(request, db, row.project_id)
    result = db.scalar(
        select(RemoteAnalysisResult).where(RemoteAnalysisResult.request_id == row.id)
    )
    response = {
        "request_id": row.id,
        "project_id": row.project_id,
        "status": row.status,
        "query_type": row.query_type,
    }
    if result:
        response["result"] = {
            "source_revision_id": result.source_revision_id,
            "freshness": result.freshness,
            "summary": result.summary,
            "structured_result": result.structured_result,
            "coverage": result.coverage,
            "unknowns": result.unknowns,
            "completed_at": result.completed_at,
        }
    return response


@router.get("/workers/{worker_id}/jobs/next", response_model=None)
def next_worker_job(
    worker_id: str, request: Request, db: Session = Depends(get_db)
) -> Response | dict[str, Any]:
    require_worker(request, db, worker_id)
    deadline = time.monotonic() + settings.long_poll_hold_sec
    while time.monotonic() < deadline:
        now = utcnow()
        leased = list(
            db.scalars(
                select(RemoteAnalysisRequest).where(
                    RemoteAnalysisRequest.status.in_(["CLAIMED", "RUNNING"]),
                    RemoteAnalysisRequest.lease_expires_at.is_not(None),
                    RemoteAnalysisRequest.lease_expires_at < now,
                )
            )
        )
        for stale in leased:
            if stale.expires_at and stale.expires_at.replace(tzinfo=UTC) <= now:
                stale.status = "EXPIRED"
            else:
                stale.status = "PENDING"
                stale.worker_id = None
                stale.lease_expires_at = None
        if leased:
            db.commit()
        worker = db.get(Worker, worker_id)
        if not worker:
            raise HTTPException(404, "Worker not found")
        instance_ids = list(
            db.scalars(
                select(ProjectInstance.id).where(
                    ProjectInstance.worker_id == worker_id, ProjectInstance.status != "RETIRED"
                )
            )
        )
        row = db.scalar(
            select(RemoteAnalysisRequest)
            .where(
                RemoteAnalysisRequest.status == "PENDING",
                RemoteAnalysisRequest.instance_id.in_(instance_ids) if instance_ids else False,
                or_(
                    RemoteAnalysisRequest.expires_at.is_(None),
                    RemoteAnalysisRequest.expires_at > now,
                ),
            )
            .order_by(RemoteAnalysisRequest.created_at)
        )
        if row:
            row.status = "CLAIMED"
            row.worker_id = worker_id
            row.lease_expires_at = utcnow() + timedelta(seconds=60)
            db.commit()
            return {
                "schema_version": 1,
                "message_id": str(uuid.uuid4()),
                "sent_at": utcnow(),
                "job": {
                    "request_id": row.id,
                    "project_id": row.project_id,
                    "instance_id": row.instance_id,
                    "query_type": row.query_type,
                    "query": row.query,
                    "requested_revision_id": row.requested_revision_id,
                },
            }
        time.sleep(0.5)
        db.expire_all()
    return Response(status_code=204)


@router.post("/workers/{worker_id}/remote-analysis/{analysis_id}/claim")
def claim_remote_analysis(
    worker_id: str, analysis_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_worker(request, db, worker_id)
    row = db.get(RemoteAnalysisRequest, analysis_id)
    if not row or row.status not in {"PENDING", "CLAIMED"}:
        raise HTTPException(409, "Request is not claimable")
    if row.expires_at:
        expiry = row.expires_at
        if expiry.tzinfo is None:
            expiry = expiry.replace(tzinfo=UTC)
        if expiry <= utcnow():
            row.status = "EXPIRED"
            db.commit()
            raise HTTPException(409, "Request has expired")
    row.status = "RUNNING"
    row.worker_id = worker_id
    row.lease_expires_at = utcnow() + timedelta(seconds=120)
    db.commit()
    return {"request_id": row.id, "status": row.status, "lease_expires_at": row.lease_expires_at}


@router.post("/remote-analysis/{analysis_id}/result", status_code=201)
def post_remote_result(
    analysis_id: str,
    payload: RemoteAnalysisResultIn,
    request: Request,
    db: Session = Depends(get_db),
) -> dict[str, Any]:
    row = db.get(RemoteAnalysisRequest, analysis_id)
    if not row or not row.worker_id:
        raise HTTPException(404, "Claimed request not found")
    require_worker(request, db, row.worker_id)
    if row.query_type == "graph_visualization":
        if row.expires_at:
            expiry = row.expires_at
            if expiry.tzinfo is None:
                expiry = expiry.replace(tzinfo=UTC)
            if expiry <= utcnow():
                row.status = "EXPIRED"
                db.commit()
                raise HTTPException(409, "Graph visualization session expired")
        validate_ephemeral_graph_payload(payload.structured_result)
    instance = db.get(ProjectInstance, payload.instance_id)
    freshness = payload.freshness
    if not instance or payload.source_revision_id != instance.current_source_revision_id:
        freshness = "STALE"
    existing = db.scalar(
        select(RemoteAnalysisResult).where(RemoteAnalysisResult.request_id == analysis_id)
    )
    if existing:
        return {"result_id": existing.id, "status": row.status, "existing": True}
    result = RemoteAnalysisResult(
        id=payload.result_id,
        request_id=analysis_id,
        project_id=payload.project_id,
        instance_id=payload.instance_id,
        source_revision_id=payload.source_revision_id,
        freshness=freshness,
        summary=payload.summary,
        structured_result=payload.structured_result,
        coverage=payload.coverage,
        unknowns=payload.unknowns,
    )
    db.add(result)
    row.status = "SUCCEEDED"
    db.commit()
    return {
        "result_id": result.id,
        "status": row.status,
        "freshness": result.freshness,
        "existing": False,
    }


@router.get("/projects/{project_id}/wiki-binding")
def get_wiki_binding(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    try:
        require_owner(request, db)
    except HTTPException:
        require_project_scope(request, db, project_id)
    row = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project_id))
    if not row:
        return {"project_id": project_id, "mode": "none", "status": "DISABLED"}
    return {
        "project_id": project_id,
        "mode": row.mode,
        "wiki_id": row.wiki_id,
        "local_path": row.local_path,
        "connector_type": row.connector_type,
        "status": row.status,
        "capability": row.capability,
    }


@router.put("/projects/{project_id}/wiki-binding")
def put_wiki_binding(
    project_id: str, payload: WikiBindingIn, request: Request, db: Session = Depends(get_db)
) -> dict[str, Any]:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    if not db.get(Project, project_id):
        raise HTTPException(404, "Project not found")
    if payload.mode == "local" and not payload.local_path:
        raise HTTPException(422, "local_path is required for local Wiki")
    if payload.mode == "remote" and not payload.wiki_id:
        raise HTTPException(422, "wiki_id is required for remote Wiki")
    row = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project_id))
    if not row:
        row = WikiBinding(project_id=project_id)
        db.add(row)
    row.mode = payload.mode
    row.wiki_id = payload.wiki_id
    row.local_path = payload.local_path
    row.connector_type = payload.connector_type
    row.status = "DISABLED" if payload.mode == "none" else "CONFIGURED"
    row.capability = {
        "code_intelligence": {
            "enabled": True,
            "project_id": project_id,
            "hub_url": settings.base_url,
        }
    }
    db.commit()
    return get_wiki_binding(project_id, request, db)


@router.delete("/projects/{project_id}/wiki-binding", status_code=204)
def delete_wiki_binding(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> Response:
    require_owner_csrf(request, db, request.headers.get("x-csrf-token"))
    row = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project_id))
    if row:
        row.mode = "none"
        row.status = "DISABLED"
        row.wiki_id = None
        row.local_path = None
        row.connector_type = None
        db.commit()
    return Response(status_code=204)


@router.get("/downloads")
def downloads(request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    require_owner(request, db)
    rows = list(
        db.scalars(
            select(ReleaseArtifact)
            .where(ReleaseArtifact.version == settings.hub_version)
            .order_by(desc(ReleaseArtifact.created_at))
            .limit(200)
        )
    )
    artifacts = [
        {
            "artifact_id": row.id,
            "version": row.version,
            "platform": row.platform,
            "name": row.name,
            "sha256": row.sha256,
            "size_bytes": row.size_bytes,
            "signature_status": row.signature_status,
            "url": f"/api/v1/downloads/{row.platform}/{row.name}",
        }
        for row in rows
    ]
    install_path = settings.artifacts_dir / "install.sh"
    if install_path.exists() and not any(item["name"] == "install.sh" for item in artifacts):
        artifacts.insert(
            0,
            {
                "artifact_id": "install-script",
                "version": settings.hub_version,
                "platform": "any-linux",
                "name": "install.sh",
                "sha256": hashlib.sha256(install_path.read_bytes()).hexdigest(),
                "size_bytes": install_path.stat().st_size,
                "signature_status": "MANIFEST",
                "url": "/downloads/install.sh",
            },
        )
    return {"items": artifacts, "next_cursor": None}


@router.get("/downloads/{platform}/{artifact}")
def download_artifact(
    platform: str, artifact: str, request: Request, db: Session = Depends(get_db)
) -> FileResponse:
    require_owner(request, db)
    row = db.scalar(
        select(ReleaseArtifact).where(
            ReleaseArtifact.version == settings.hub_version,
            ReleaseArtifact.platform == platform,
            ReleaseArtifact.name == artifact,
        )
    )
    if not row:
        raise HTTPException(404, "Artifact not found")
    path = (settings.artifacts_dir / row.relative_path).resolve()
    root = settings.artifacts_dir.resolve()
    if root not in path.parents or not path.is_file():
        raise HTTPException(404, "Artifact unavailable")
    return FileResponse(path, filename=row.name, media_type=mimetypes.guess_type(row.name)[0])


@router.get("/system/health")
def system_health(request: Request, db: Session = Depends(get_db)) -> dict[str, Any]:
    require_owner(request, db)
    worker_count = db.scalar(select(func.count()).select_from(Worker)) or 0
    pending_jobs = (
        db.scalar(
            select(func.count())
            .select_from(RemoteAnalysisRequest)
            .where(RemoteAnalysisRequest.status == "PENDING")
        )
        or 0
    )
    latest_worker = db.scalar(select(Worker).order_by(desc(Worker.last_heartbeat_at)))
    latest_backup = db.scalar(
        select(BackupRecord)
        .where(BackupRecord.status == "SUCCEEDED")
        .order_by(desc(BackupRecord.created_at))
    )
    latest_restore = db.scalar(
        select(BackupRecord)
        .where(BackupRecord.kind == "RESTORE_TEST", BackupRecord.status == "SUCCEEDED")
        .order_by(desc(BackupRecord.created_at))
    )
    return {
        "status": "READY",
        "version": settings.hub_version,
        "database": "AVAILABLE",
        "worker_count": worker_count,
        "last_worker_heartbeat": latest_worker.last_heartbeat_at if latest_worker else None,
        "sync_backlog": pending_jobs,
        "migration_version": "0001",
        "backup": {
            "freshness": "CURRENT" if latest_backup else "UNKNOWN",
            "last_successful": latest_backup.created_at if latest_backup else None,
            "last_restore_test": latest_restore.created_at if latest_restore else None,
        },
    }
