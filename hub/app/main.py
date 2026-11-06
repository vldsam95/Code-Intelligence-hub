from __future__ import annotations

import hashlib
import json
import logging
import mimetypes
import uuid
from contextlib import asynccontextmanager
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any
from urllib.parse import urlencode

import markdown
from fastapi import Depends, FastAPI, Form, HTTPException, Request, Response
from fastapi.exceptions import RequestValidationError
from fastapi.responses import FileResponse, HTMLResponse, JSONResponse, RedirectResponse
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates
from sqlalchemy import desc, func, select
from sqlalchemy.orm import Session
from starlette.middleware.trustedhost import TrustedHostMiddleware

from .api.routes import router as api_router
from .config import get_settings
from .db import SessionLocal, check_database, get_db
from .mcp.routes import router as mcp_router
from .models import (
    AgentCredential,
    AgentPolicyVersion,
    AgentSessionActivity,
    BackupRecord,
    Finding,
    IndexSnapshot,
    LoginAttempt,
    OwnerUser,
    Project,
    ProjectInstance,
    QualityRun,
    RecoveryEvent,
    ReleaseArtifact,
    RemoteAnalysisRequest,
    RemoteAnalysisResult,
    WikiBinding,
    Worker,
    WorkerHost,
    utcnow,
)
from .repositories.projects import ProjectRepository
from .security import (
    CSRF_COOKIE,
    SESSION_COOKIE,
    authenticate_browser,
    create_browser_session,
    revoke_browser_session,
    source_ip_hash,
    verify_csrf,
    verify_password,
)
from .services.audit import audit
from .services.health import project_health

settings = get_settings()
BASE_DIR = Path(__file__).resolve().parents[1]
templates = Jinja2Templates(directory=str(BASE_DIR / "templates"))
UI_PAGE_SIZE = 25


def paged_scalars(
    db: Session,
    statement: Any,
    page: int,
    *,
    path: str,
    page_param: str = "page",
    params: dict[str, Any] | None = None,
    anchor: str | None = None,
) -> tuple[list[Any], dict[str, Any]]:
    """Execute a bounded server-side UI query and build stable previous/next links."""
    total = int(
        db.scalar(select(func.count()).select_from(statement.order_by(None).subquery())) or 0
    )
    pages = max(1, (total + UI_PAGE_SIZE - 1) // UI_PAGE_SIZE)
    current = min(max(page, 1), pages)
    rows = list(db.scalars(statement.offset((current - 1) * UI_PAGE_SIZE).limit(UI_PAGE_SIZE)))
    base_params = {key: value for key, value in (params or {}).items() if value not in {None, ""}}

    def url_for(target: int) -> str:
        query = urlencode(base_params | {page_param: target})
        suffix = f"#{anchor}" if anchor else ""
        return f"{path}?{query}{suffix}"

    return rows, {
        "page": current,
        "pages": pages,
        "total": total,
        "page_size": UI_PAGE_SIZE,
        "previous_url": url_for(current - 1) if current > 1 else None,
        "next_url": url_for(current + 1) if current < pages else None,
    }


class JsonFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        return json.dumps(
            {
                "timestamp": datetime.now(UTC).isoformat(),
                "level": record.levelname,
                "service": "codeintel-hub",
                "event_code": getattr(record, "event_code", "APPLICATION_LOG"),
                "request_id": getattr(record, "request_id", None),
                "message": record.getMessage(),
            },
            ensure_ascii=False,
        )


handler = logging.StreamHandler()
handler.setFormatter(JsonFormatter())
logging.basicConfig(
    level=getattr(logging, settings.log_level.upper(), logging.INFO), handlers=[handler], force=True
)
logger = logging.getLogger("codeintel-hub")


def ensure_production_configuration() -> None:
    if not settings.production:
        return
    missing: list[str] = []
    if not settings.owner_password_hash.startswith("$argon2"):
        missing.append("CODEINTEL_OWNER_PASSWORD_HASH (Argon2id)")
    if len(settings.session_secret) < 32:
        missing.append("CODEINTEL_SESSION_SECRET")
    if len(settings.token_hmac_key) < 32:
        missing.append("CODEINTEL_TOKEN_HMAC_KEY")
    if not settings.master_key_file.exists():
        missing.append("CODEINTEL_MASTER_KEY_FILE")
    elif settings.master_key_file.stat().st_mode & 0o077:
        missing.append("master key permissions must be 0600")
    if not settings.base_url.startswith("https://"):
        missing.append("CODEINTEL_BASE_URL must use HTTPS")
    if missing:
        raise RuntimeError("Unsafe production configuration: " + ", ".join(missing))


def bootstrap_database() -> None:
    with SessionLocal() as db:
        try:
            owner = db.scalar(select(OwnerUser).limit(1))
        except Exception as exc:
            raise RuntimeError("Database migrations are required before Hub startup") from exc
        if not owner:
            if not settings.owner_password_hash:
                raise RuntimeError("Owner password hash is required for initial bootstrap")
            db.add(
                OwnerUser(
                    username=settings.owner_username, password_hash=settings.owner_password_hash
                )
            )
        policy_path = settings.docs_dir / "AGENT_OPERATING_POLICY.md"
        if policy_path.exists():
            digest = hashlib.sha256(policy_path.read_bytes()).hexdigest()
            if not db.get(AgentPolicyVersion, settings.agent_policy_version):
                db.add(
                    AgentPolicyVersion(
                        id=settings.agent_policy_version,
                        product_version=settings.hub_version,
                        content_hash=digest,
                    )
                )
        manifest_path = settings.artifacts_dir / "release-manifest.json"
        if manifest_path.is_file():
            release = json.loads(manifest_path.read_text(encoding="utf-8"))
            release_version = str(release.get("product_version", ""))
            if release_version != settings.hub_version:
                raise RuntimeError(
                    "Release manifest version does not match CODEINTEL_HUB_VERSION"
                )
            advertised: set[tuple[str, str]] = set()
            for artifact in release.get("artifacts", []):
                name = Path(str(artifact.get("name", ""))).name
                relative_path = str(artifact.get("relative_path") or name)
                if (
                    not name
                    or Path(relative_path).is_absolute()
                    or ".." in Path(relative_path).parts
                ):
                    continue
                platform = str(artifact.get("platform", "any"))
                path = (settings.artifacts_dir / relative_path).resolve()
                if settings.artifacts_dir.resolve() not in path.parents or not path.is_file():
                    raise RuntimeError(f"Release artifact is unavailable: {name}")
                expected_size = int(artifact.get("size_bytes", 0))
                expected_sha256 = str(artifact.get("sha256", ""))
                if path.stat().st_size != expected_size:
                    raise RuntimeError(f"Release artifact size mismatch: {name}")
                if hashlib.sha256(path.read_bytes()).hexdigest() != expected_sha256:
                    raise RuntimeError(f"Release artifact checksum mismatch: {name}")
                advertised.add((platform, name))
                row = db.scalar(
                    select(ReleaseArtifact).where(
                        ReleaseArtifact.version == release_version,
                        ReleaseArtifact.platform == platform,
                        ReleaseArtifact.name == name,
                    )
                )
                if not row:
                    row = ReleaseArtifact(
                        version=release_version,
                        platform=platform,
                        name=name,
                    )
                    db.add(row)
                row.relative_path = relative_path
                row.sha256 = str(artifact.get("sha256", ""))
                row.size_bytes = int(artifact.get("size_bytes", 0))
                row.signature_status = "SIGNED_MANIFEST"
            for stale in db.scalars(select(ReleaseArtifact)).all():
                if stale.version != release_version or (stale.platform, stale.name) not in advertised:
                    db.delete(stale)
        db.commit()


@asynccontextmanager
async def lifespan(_app: FastAPI):
    ensure_production_configuration()
    bootstrap_database()
    logger.info("Hub ready", extra={"event_code": "HUB_READY"})
    yield


app = FastAPI(
    title="Code Intelligence Hub API",
    version=settings.hub_version,
    docs_url="/api-docs",
    openapi_url="/api/v1/openapi.json",
    lifespan=lifespan,
)
app.add_middleware(TrustedHostMiddleware, allowed_hosts=settings.trusted_host_list)
app.mount("/static", StaticFiles(directory=str(BASE_DIR / "static")), name="static")
app.include_router(api_router)
app.include_router(mcp_router)


@app.middleware("http")
async def request_controls(request: Request, call_next):  # type: ignore[no-untyped-def]
    request.state.request_id = request.headers.get("x-request-id") or str(uuid.uuid4())
    content_length = request.headers.get("content-length")
    if content_length and int(content_length) > settings.max_api_body_bytes:
        return JSONResponse(
            status_code=413,
            content={
                "error": {
                    "code": "PAYLOAD_TOO_LARGE",
                    "message": "Request body exceeds the configured limit.",
                    "request_id": request.state.request_id,
                    "details": {"max_bytes": settings.max_api_body_bytes},
                    "retryable": False,
                }
            },
        )
    response = await call_next(request)
    response.headers["X-Request-ID"] = request.state.request_id
    response.headers["X-Content-Type-Options"] = "nosniff"
    response.headers["Referrer-Policy"] = "no-referrer"
    response.headers["X-Frame-Options"] = "DENY"
    response.headers["Content-Security-Policy"] = (
        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; "
        "connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
    )
    if settings.production:
        response.headers["Strict-Transport-Security"] = "max-age=31536000; includeSubDomains"
    if request.url.path.startswith("/api/") or request.url.path == "/mcp":
        response.headers["Cache-Control"] = "no-store"
    return response


def error_code(status_code: int) -> str:
    return {
        400: "BAD_REQUEST",
        401: "UNAUTHORIZED",
        403: "FORBIDDEN",
        404: "NOT_FOUND",
        409: "CONFLICT",
        412: "PRECONDITION_FAILED",
        413: "PAYLOAD_TOO_LARGE",
        422: "VALIDATION_ERROR",
        429: "RATE_LIMITED",
        503: "UNAVAILABLE",
    }.get(status_code, "INTERNAL_ERROR")


@app.exception_handler(HTTPException)
async def http_exception_handler(request: Request, exc: HTTPException) -> JSONResponse:
    if not request.url.path.startswith("/api/") and request.url.path != "/mcp":
        return JSONResponse({"detail": exc.detail}, status_code=exc.status_code)
    return JSONResponse(
        status_code=exc.status_code,
        content={
            "error": {
                "code": error_code(exc.status_code),
                "message": str(exc.detail),
                "request_id": getattr(request.state, "request_id", str(uuid.uuid4())),
                "details": {},
                "retryable": exc.status_code in {429, 503},
            }
        },
    )


@app.exception_handler(RequestValidationError)
async def validation_exception_handler(
    request: Request, exc: RequestValidationError
) -> JSONResponse:
    return JSONResponse(
        status_code=422,
        content={
            "error": {
                "code": "VALIDATION_ERROR",
                "message": "The request did not satisfy the API contract.",
                "request_id": getattr(request.state, "request_id", str(uuid.uuid4())),
                "details": {"issues": exc.errors()},
                "retryable": False,
            }
        },
    )


@app.get("/health/live")
def health_live() -> dict[str, str]:
    return {"status": "live", "version": settings.hub_version}


@app.get("/health/ready")
def health_ready() -> JSONResponse:
    ready = check_database()
    return JSONResponse(
        status_code=200 if ready else 503,
        content={
            "status": "ready" if ready else "unavailable",
            "database": ready,
            "version": settings.hub_version,
        },
    )


def ui_identity(request: Request, db: Session):
    identity = authenticate_browser(request, db)
    if not identity:
        raise HTTPException(401, "Login required")
    return identity


def template_context(request: Request, identity, **kwargs: Any) -> dict[str, Any]:
    return {
        "request": request,
        "owner": identity.owner,
        "csrf_token": request.cookies.get(CSRF_COOKIE, ""),
        "product_version": settings.hub_version,
        "base_url": settings.base_url,
        **kwargs,
    }


@app.get("/", include_in_schema=False)
def root(request: Request, db: Session = Depends(get_db)) -> RedirectResponse:
    return RedirectResponse(
        "/dashboard" if authenticate_browser(request, db) else "/login", status_code=303
    )


@app.get("/login", response_class=HTMLResponse, include_in_schema=False)
def login_page(request: Request, db: Session = Depends(get_db)) -> Response:
    if authenticate_browser(request, db):
        return RedirectResponse("/dashboard", status_code=303)
    return templates.TemplateResponse(request, "login.html", {"request": request, "error": None})


@app.post("/login", response_class=HTMLResponse, include_in_schema=False)
def login_form(
    request: Request,
    username: str = Form(...),
    password: str = Form(...),
    db: Session = Depends(get_db),
) -> Response:
    ip = request.headers.get(
        "x-forwarded-for", request.client.host if request.client else "unknown"
    ).split(",")[0]
    ip_digest = source_ip_hash(ip)
    cutoff = utcnow().timestamp() - settings.login_window_sec
    failures = (
        db.scalar(
            select(func.count())
            .select_from(LoginAttempt)
            .where(
                LoginAttempt.username == username,
                LoginAttempt.source_ip_hash == ip_digest,
                LoginAttempt.succeeded.is_(False),
                LoginAttempt.created_at >= datetime.fromtimestamp(cutoff, UTC),
            )
        )
        or 0
    )
    owner = db.scalar(select(OwnerUser).where(OwnerUser.username == username))
    limited = failures >= settings.login_attempts
    if limited or not owner or not verify_password(owner.password_hash, password):
        if not limited:
            db.add(
                LoginAttempt(
                    username=username,
                    source_ip_hash=ip_digest,
                    succeeded=False,
                )
            )
        audit(
            db,
            event_code="LOGIN_FAILED",
            request_id=request.state.request_id,
            actor_type="ANONYMOUS",
            source_ip_hash=ip_digest,
        )
        db.commit()
        return templates.TemplateResponse(
            request,
            "login.html",
            {"request": request, "error": "Invalid credentials or too many login attempts."},
            status_code=429 if limited else 401,
        )
    db.add(
        LoginAttempt(
            username=username,
            source_ip_hash=ip_digest,
            succeeded=True,
        )
    )
    session, raw_session, raw_csrf = create_browser_session(db, owner)
    audit(
        db,
        event_code="LOGIN_SUCCEEDED",
        request_id=request.state.request_id,
        actor_type="OWNER",
        actor_id=owner.id,
        source_ip_hash=ip_digest,
    )
    db.commit()
    response = RedirectResponse("/dashboard", status_code=303)
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
    return response


@app.post("/logout", include_in_schema=False)
def logout(
    request: Request, csrf_token: str = Form(...), db: Session = Depends(get_db)
) -> Response:
    identity = ui_identity(request, db)
    verify_csrf(request, identity, csrf_token)
    revoke_browser_session(db, request.cookies.get(SESSION_COOKIE))
    response = RedirectResponse("/login", status_code=303)
    response.delete_cookie(SESSION_COOKIE, path="/")
    response.delete_cookie(CSRF_COOKIE, path="/")
    return response


@app.get("/dashboard", response_class=HTMLResponse, include_in_schema=False)
def dashboard(request: Request, page: int = 1, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    projects, total = ProjectRepository(db).page_active(page=page, page_size=UI_PAGE_SIZE)
    pages = max(1, (total + UI_PAGE_SIZE - 1) // UI_PAGE_SIZE)
    current = min(max(page, 1), pages)
    pager = {
        "page": current,
        "pages": pages,
        "total": total,
        "page_size": UI_PAGE_SIZE,
        "previous_url": f"/dashboard?page={current - 1}" if current > 1 else None,
        "next_url": f"/dashboard?page={current + 1}" if current < pages else None,
    }
    rows = [{"project": row, "health": project_health(db, row)} for row in projects]
    workers = list(db.scalars(select(Worker).order_by(desc(Worker.last_heartbeat_at)).limit(20)))
    counters = {
        "projects": ProjectRepository(db).count_active(),
        "workers": int(db.scalar(select(func.count()).select_from(Worker)) or 0),
        "unverified": sum(1 for row in rows if row["health"]["currentness"] == "UNVERIFIED_CHANGE"),
        "blocking": sum(int(row["health"].get("blocking_unknowns", 0)) for row in rows),
    }
    return templates.TemplateResponse(
        request,
        "dashboard.html",
        template_context(
            request,
            identity,
            active="dashboard",
            projects=rows,
            workers=workers,
            counters=counters,
            pager=pager,
        ),
    )


@app.get("/projects", response_class=HTMLResponse, include_in_schema=False)
def projects_page(
    request: Request, q: str = "", page: int = 1, db: Session = Depends(get_db)
) -> HTMLResponse:
    identity = ui_identity(request, db)
    project_rows, total = ProjectRepository(db).page_active(page=page, page_size=25, query=q)
    pages = max(1, (total + UI_PAGE_SIZE - 1) // UI_PAGE_SIZE)
    current = min(max(page, 1), pages)
    query_params = {"q": q.strip()} if q.strip() else {}
    pager = {
        "page": current,
        "pages": pages,
        "total": total,
        "page_size": UI_PAGE_SIZE,
        "previous_url": f"/projects?{urlencode(query_params | {'page': current - 1})}"
        if current > 1
        else None,
        "next_url": f"/projects?{urlencode(query_params | {'page': current + 1})}"
        if current < pages
        else None,
    }
    rows = [{"project": row, "health": project_health(db, row)} for row in project_rows]
    return templates.TemplateResponse(
        request,
        "projects.html",
        template_context(
            request,
            identity,
            active="projects",
            projects=rows,
            project_query=q.strip(),
            pager=pager,
        ),
    )


@app.get("/projects/{project_id}/view", response_class=HTMLResponse, include_in_schema=False)
def project_page(
    project_id: str,
    request: Request,
    quality_page: int = 1,
    finding_page: int = 1,
    instance_page: int = 1,
    index_page: int = 1,
    recovery_page: int = 1,
    token_page: int = 1,
    activity_page: int = 1,
    db: Session = Depends(get_db),
) -> HTMLResponse:
    identity = ui_identity(request, db)
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    repo = ProjectRepository(db)
    route_path = f"/projects/{project_id}/view"
    page_params = {
        "quality_page": quality_page,
        "finding_page": finding_page,
        "instance_page": instance_page,
        "index_page": index_page,
        "recovery_page": recovery_page,
        "token_page": token_page,
        "activity_page": activity_page,
    }
    instances, instances_pager = paged_scalars(
        db,
        select(ProjectInstance)
        .where(ProjectInstance.project_id == project_id)
        .order_by(desc(ProjectInstance.heartbeat_at), ProjectInstance.id),
        instance_page,
        path=route_path,
        page_param="instance_page",
        params=page_params,
        anchor="instances",
    )
    qualities, qualities_pager = paged_scalars(
        db,
        select(QualityRun)
        .where(QualityRun.project_id == project_id)
        .order_by(desc(QualityRun.completed_at), QualityRun.id),
        quality_page,
        path=route_path,
        page_param="quality_page",
        params=page_params,
        anchor="history",
    )
    findings, findings_pager = paged_scalars(
        db,
        select(Finding)
        .where(Finding.project_id == project_id)
        .order_by(desc(Finding.updated_at), Finding.id),
        finding_page,
        path=route_path,
        page_param="finding_page",
        params=page_params,
        anchor="findings",
    )
    snapshots, snapshots_pager = paged_scalars(
        db,
        select(IndexSnapshot)
        .where(IndexSnapshot.project_id == project_id)
        .order_by(desc(IndexSnapshot.started_at), IndexSnapshot.id),
        index_page,
        path=route_path,
        page_param="index_page",
        params=page_params,
        anchor="indexes",
    )
    recoveries, recoveries_pager = paged_scalars(
        db,
        select(RecoveryEvent)
        .where(RecoveryEvent.project_id == project_id)
        .order_by(desc(RecoveryEvent.created_at), RecoveryEvent.id),
        recovery_page,
        path=route_path,
        page_param="recovery_page",
        params=page_params,
        anchor="recoveries",
    )
    project_tokens, project_tokens_pager = paged_scalars(
        db,
        select(AgentCredential)
        .where(
            AgentCredential.scope_type == "PROJECT",
            AgentCredential.scope_id == project_id,
        )
        .order_by(desc(AgentCredential.created_at), AgentCredential.id),
        token_page,
        path=route_path,
        page_param="token_page",
        params=page_params,
        anchor="project-access",
    )
    project_access = db.scalar(
        select(AgentCredential)
        .where(
            AgentCredential.token_type == "PROJECT_AGENT",
            AgentCredential.scope_type == "PROJECT",
            AgentCredential.scope_id == project_id,
            AgentCredential.status == "ACTIVE",
        )
        .order_by(
            AgentCredential.label.like("Project Access ·%").desc(),
            desc(AgentCredential.created_at),
        )
    )
    recent_agent_activity, activity_pager = paged_scalars(
        db,
        select(AgentSessionActivity)
        .where(AgentSessionActivity.project_id == project_id)
        .order_by(desc(AgentSessionActivity.started_at), AgentSessionActivity.id),
        activity_page,
        path=route_path,
        page_param="activity_page",
        params=page_params,
        anchor="agent-activity",
    )
    wiki_binding = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project_id))
    reader_path = settings.docs_dir / "REMOTE_PROJECT_READER_PROMPT.md"
    reader_prompt = (
        reader_path.read_text(encoding="utf-8")
        if reader_path.is_file()
        else "Remote Project Reader prompt is unavailable."
    )
    reader_prompt = (
        reader_prompt.replace("<HUB_URL>", settings.base_url)
        .replace("<PROJECT_ID>", project.id)
        .replace("<PROJECT_NAME>", project.name)
    )
    return templates.TemplateResponse(
        request,
        "project.html",
        template_context(
            request,
            identity,
            active="projects",
            project=project,
            health=project_health(db, project),
            context_pack=repo.latest_context(project_id),
            instances=instances,
            qualities=qualities,
            findings=findings,
            snapshots=snapshots,
            recoveries=recoveries,
            recoveries_pager=recoveries_pager,
            wiki_binding=wiki_binding,
            qualities_pager=qualities_pager,
            findings_pager=findings_pager,
            instances_pager=instances_pager,
            snapshots_pager=snapshots_pager,
            project_tokens=project_tokens,
            project_tokens_pager=project_tokens_pager,
            project_access=project_access,
            recent_agent_activity=recent_agent_activity,
            activity_pager=activity_pager,
            remote_reader_prompt=reader_prompt,
        ),
    )


@app.post(
    "/projects/{project_id}/graph-session", response_class=JSONResponse, include_in_schema=False
)
def create_graph_session(
    project_id: str, request: Request, db: Session = Depends(get_db)
) -> JSONResponse:
    """Queue an owner-requested, bounded graph snapshot; no source and no durable raw graph."""
    identity = ui_identity(request, db)
    verify_csrf(request, identity, request.headers.get("x-csrf-token"))
    project = db.get(Project, project_id)
    if not project:
        raise HTTPException(404, "Project not found")
    if not project_health(db, project)["worker_online"]:
        raise HTTPException(409, "LIVE WORKER OFFLINE: on-demand visualization is unavailable")
    instance = ProjectRepository(db).latest_instance(project_id)
    if not instance:
        raise HTTPException(409, "No ProjectInstance is available")
    now = utcnow()
    expired = list(
        db.scalars(
            select(RemoteAnalysisRequest).where(
                RemoteAnalysisRequest.query_type == "graph_visualization",
                RemoteAnalysisRequest.expires_at.is_not(None),
                RemoteAnalysisRequest.expires_at < now,
            )
        )
    )
    for old in expired:
        result = db.scalar(
            select(RemoteAnalysisResult).where(RemoteAnalysisResult.request_id == old.id)
        )
        if result:
            result.structured_result = {"expired": True}
        old.status = "EXPIRED"
    row = RemoteAnalysisRequest(
        project_id=project_id,
        instance_id=instance.id,
        query_type="graph_visualization",
        query={"max_nodes": 220, "max_edges": 420},
        requested_revision_id=instance.current_source_revision_id,
        status="PENDING",
        expires_at=now + timedelta(minutes=5),
    )
    db.add(row)
    audit(
        db,
        event_code="GRAPH_VISUALIZATION_REQUESTED",
        request_id=request.state.request_id,
        actor_type="OWNER",
        actor_id=identity.owner.id,
        target_type="REMOTE_ANALYSIS",
        target_id=row.id,
        details={"project_id": project_id, "ephemeral": True},
    )
    db.commit()
    return JSONResponse(
        status_code=202,
        content={"request_id": row.id, "status": "PENDING", "expires_in_sec": 300},
        headers={"Cache-Control": "no-store"},
    )


@app.get(
    "/projects/{project_id}/graph-session/{session_id}",
    response_class=JSONResponse,
    include_in_schema=False,
)
def get_graph_session(
    project_id: str, session_id: str, request: Request, db: Session = Depends(get_db)
) -> JSONResponse:
    ui_identity(request, db)
    row = db.get(RemoteAnalysisRequest, session_id)
    if not row or row.project_id != project_id or row.query_type != "graph_visualization":
        raise HTTPException(404, "Graph session not found")
    if row.expires_at:
        expiry = row.expires_at
        if expiry.tzinfo is None:
            expiry = expiry.replace(tzinfo=UTC)
        if expiry <= utcnow():
            row.status = "EXPIRED"
            result = db.scalar(
                select(RemoteAnalysisResult).where(RemoteAnalysisResult.request_id == session_id)
            )
            if result:
                result.structured_result = {"expired": True}
            db.commit()
            return JSONResponse(
                status_code=410,
                content={"status": "EXPIRED"},
                headers={"Cache-Control": "no-store"},
            )
    result = db.scalar(
        select(RemoteAnalysisResult).where(RemoteAnalysisResult.request_id == session_id)
    )
    if not result:
        return JSONResponse(content={"status": row.status}, headers={"Cache-Control": "no-store"})
    if result.structured_result.get("consumed"):
        return JSONResponse(
            status_code=410,
            content={"status": "CONSUMED"},
            headers={"Cache-Control": "no-store"},
        )
    payload = {
        "status": "SUCCEEDED",
        "source_revision_id": result.source_revision_id,
        "freshness": result.freshness,
        "graph": result.structured_result,
        "unknowns": result.unknowns,
    }
    result.structured_result = {"consumed": True, "consumed_at": utcnow().isoformat()}
    row.status = "CONSUMED"
    db.commit()
    return JSONResponse(content=payload, headers={"Cache-Control": "no-store"})


@app.get("/hosts", response_class=HTMLResponse, include_in_schema=False)
def hosts_page(request: Request, page: int = 1, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    workers, pager = paged_scalars(
        db,
        select(Worker).order_by(desc(Worker.last_heartbeat_at), Worker.id),
        page,
        path="/hosts",
    )
    host_access: dict[str, AgentCredential | None] = {}
    hosts: dict[str, WorkerHost | None] = {}
    for worker in workers:
        hosts[worker.host_id] = db.get(WorkerHost, worker.host_id)
        host_access[worker.host_id] = db.scalar(
            select(AgentCredential)
            .where(
                AgentCredential.token_type == "HOST_ACCESS",
                AgentCredential.scope_type == "HOST",
                AgentCredential.scope_id == worker.host_id,
                AgentCredential.status == "ACTIVE",
            )
            .order_by(desc(AgentCredential.created_at))
        )
    return templates.TemplateResponse(
        request,
        "hosts.html",
        template_context(
            request,
            identity,
            active="hosts",
            workers=workers,
            hosts=hosts,
            host_access=host_access,
            pager=pager,
        ),
    )


@app.get("/tokens", response_class=HTMLResponse, include_in_schema=False)
def tokens_page(request: Request, page: int = 1, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    tokens, pager = paged_scalars(
        db,
        select(AgentCredential).order_by(desc(AgentCredential.created_at), AgentCredential.id),
        page,
        path="/tokens",
    )
    projects = ProjectRepository(db).list_active(200)
    return templates.TemplateResponse(
        request,
        "tokens.html",
        template_context(
            request, identity, active="tokens", tokens=tokens, projects=projects, pager=pager
        ),
    )


@app.get("/downloads", response_class=HTMLResponse, include_in_schema=False)
def downloads_page(request: Request, page: int = 1, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    artifacts, pager = paged_scalars(
        db,
        select(ReleaseArtifact)
        .where(ReleaseArtifact.version == settings.hub_version)
        .order_by(desc(ReleaseArtifact.created_at), ReleaseArtifact.id),
        page,
        path="/downloads",
    )
    manual = db.scalar(
        select(ReleaseArtifact)
        .where(
            ReleaseArtifact.version == settings.hub_version,
            ReleaseArtifact.name.like("codeintel-complete-agent-kit-%"),
        )
        .order_by(desc(ReleaseArtifact.created_at))
    )
    return templates.TemplateResponse(
        request,
        "downloads.html",
        template_context(
            request,
            identity,
            active="downloads",
            artifacts=artifacts,
            manual_bundle=manual,
            pager=pager,
        ),
    )


@app.get("/downloads/install.sh", include_in_schema=False)
def public_installer() -> FileResponse:
    path = settings.artifacts_dir / "install.sh"
    if not path.is_file():
        raise HTTPException(404, "Installer is not available")
    return FileResponse(path, filename="install.sh", media_type="text/x-shellscript")


@app.get("/downloads/{artifact}", include_in_schema=False)
def public_release_artifact(artifact: str) -> FileResponse:
    """Release files are public; credentials and project data are never stored here."""
    if Path(artifact).name != artifact:
        raise HTTPException(404, "Artifact not found")
    path = settings.artifacts_dir / artifact
    if not path.is_file():
        raise HTTPException(404, "Artifact not found")
    return FileResponse(path, filename=artifact, media_type=mimetypes.guess_type(artifact)[0])


@app.get("/agent-center", response_class=HTMLResponse, include_in_schema=False)
def agent_center(request: Request, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    return templates.TemplateResponse(
        request, "agent_center.html", template_context(request, identity, active="agent-center")
    )


@app.get("/guides", response_class=HTMLResponse, include_in_schema=False)
def simple_guides(
    request: Request, guide: str = "new-host", db: Session = Depends(get_db)
) -> HTMLResponse:
    identity = ui_identity(request, db)
    catalog = {
        "new-host": {
            "title": "Install on a new host",
            "icon": "01",
            "file": "SIMPLE_GUIDE_NEW_HOST.md",
            "steps": ["Create a token", "Send the prompt/ZIP to your agent", "Verify the Worker is ONLINE"],
        },
        "new-project": {
            "title": "Enable a new project",
            "icon": "02",
            "file": "SIMPLE_GUIDE_NEW_PROJECT.md",
            "steps": ["Open an agent in the project", "Request opt-in onboarding", "Find the project in the Hub"],
        },
        "existing-code": {
            "title": "Work on existing code",
            "icon": "03",
            "file": "SIMPLE_GUIDE_EXISTING_CODE.md",
            "steps": ["Request a change", "The agent verifies and fixes", "Receive a quality-gate report"],
        },
        "move-project": {
            "title": "Move a project to another host",
            "icon": "04",
            "file": "SIMPLE_GUIDE_MOVE_PROJECT.md",
            "steps": ["Transfer source and marker", "Run open", "New instance, same history"],
        },
        "llm-wiki": {
            "title": "Connect an LLM Wiki",
            "icon": "05",
            "file": "SIMPLE_GUIDE_LLM_WIKI.md",
            "steps": ["Select the exact Wiki", "Create the binding", "Store durable knowledge only"],
        },
        "remote-reader": {
            "title": "Remote agent / presentation",
            "icon": "06",
            "file": "SIMPLE_GUIDE_REMOTE_READER.md",
            "steps": ["Create project access", "Copy the prompt", "Rotate or revoke when finished"],
        },
    }
    if guide not in catalog:
        raise HTTPException(404, "Guide not found")
    selected = catalog[guide]
    path = settings.docs_dir / selected["file"]
    if not path.is_file():
        raise HTTPException(404, "Guide source is unavailable")
    source = path.read_text(encoding="utf-8").replace("<HUB_URL>", settings.base_url)
    rendered = markdown.markdown(source, extensions=["fenced_code", "tables", "toc"])
    return templates.TemplateResponse(
        request,
        "guides.html",
        template_context(
            request,
            identity,
            active="guides",
            guides=catalog,
            selected_slug=guide,
            selected=selected,
            rendered=rendered,
            raw_guide=source,
            raw_url=f"/api/v1/docs/{selected['file']}?format=raw",
        ),
    )


@app.get("/agent-toolkit", response_class=HTMLResponse, include_in_schema=False)
def agent_toolkit(request: Request, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    prompt_path = settings.docs_dir / "UNIVERSAL_AGENT_BOOTSTRAP_PROMPT.md"
    prompt = (
        prompt_path.read_text(encoding="utf-8")
        if prompt_path.is_file()
        else "Universal bootstrap prompt is unavailable."
    ).replace("<HUB_URL>", settings.base_url)
    skill_path = BASE_DIR.parent / "adapters/code-intelligence-skill/SKILL.md"
    if not skill_path.is_file():
        skill_path = settings.docs_dir / "AGENT_OPERATING_POLICY.md"
    skill = skill_path.read_text(encoding="utf-8") if skill_path.is_file() else ""
    manual = db.scalar(
        select(ReleaseArtifact)
        .where(
            ReleaseArtifact.version == settings.hub_version,
            ReleaseArtifact.name.like("codeintel-complete-agent-kit-%"),
        )
        .order_by(desc(ReleaseArtifact.created_at))
    )
    return templates.TemplateResponse(
        request,
        "agent_toolkit.html",
        template_context(
            request,
            identity,
            active="agent-toolkit",
            universal_prompt=prompt,
            skill_markdown=skill,
            manual_bundle=manual,
        ),
    )


@app.get("/api-mcp", response_class=HTMLResponse, include_in_schema=False)
def api_mcp_page(request: Request, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    return templates.TemplateResponse(
        request, "api_mcp.html", template_context(request, identity, active="api-mcp")
    )


@app.get("/documentation", response_class=HTMLResponse, include_in_schema=False)
def documentation(
    request: Request, doc: str = "START_HERE.md", db: Session = Depends(get_db)
) -> HTMLResponse:
    identity = ui_identity(request, db)
    allowed = sorted(path.name for path in settings.docs_dir.glob("*.md"))
    if doc not in allowed:
        raise HTTPException(404, "Document not found")
    source = (settings.docs_dir / doc).read_text(encoding="utf-8")
    rendered = markdown.markdown(source, extensions=["fenced_code", "tables", "toc"])
    return templates.TemplateResponse(
        request,
        "documentation.html",
        template_context(
            request,
            identity,
            active="documentation",
            docs=allowed,
            selected=doc,
            rendered=rendered,
            raw_url=f"/api/v1/docs/{doc}?format=raw",
        ),
    )


@app.get("/api/v1/docs/{doc_path}", include_in_schema=False)
def api_doc(
    doc_path: str, request: Request, format: str = "rendered", db: Session = Depends(get_db)
) -> Response:
    identity = authenticate_browser(request, db)
    if not identity:
        from .security import authenticate_bearer

        authenticate_bearer(request, db)
    name = Path(doc_path).name
    if name != doc_path or not name.endswith(".md"):
        raise HTTPException(404, "Document not found")
    path = settings.docs_dir / name
    if not path.is_file():
        raise HTTPException(404, "Document not found")
    source = path.read_text(encoding="utf-8")
    if format == "raw":
        return Response(source, media_type="text/markdown; charset=utf-8")
    return HTMLResponse(markdown.markdown(source, extensions=["fenced_code", "tables", "toc"]))


@app.get("/docs-bundle.zip", include_in_schema=False)
def docs_bundle(request: Request, db: Session = Depends(get_db)) -> FileResponse:
    ui_identity(request, db)
    path = settings.artifacts_dir / f"codeintel-docs-v{settings.hub_version}.zip"
    if not path.is_file():
        raise HTTPException(404, "Documentation bundle is not available")
    return FileResponse(path, filename=path.name, media_type="application/zip")


@app.get("/system-health", response_class=HTMLResponse, include_in_schema=False)
def system_health_page(
    request: Request, page: int = 1, db: Session = Depends(get_db)
) -> HTMLResponse:
    identity = ui_identity(request, db)
    workers, pager = paged_scalars(
        db,
        select(Worker).order_by(desc(Worker.last_heartbeat_at), Worker.id),
        page,
        path="/system-health",
    )
    pending = (
        db.scalar(
            select(func.count())
            .select_from(RemoteAnalysisRequest)
            .where(RemoteAnalysisRequest.status.in_(["PENDING", "CLAIMED", "RUNNING"]))
        )
        or 0
    )
    backup = db.scalar(
        select(BackupRecord)
        .where(BackupRecord.status == "SUCCEEDED")
        .order_by(desc(BackupRecord.created_at))
    )
    return templates.TemplateResponse(
        request,
        "system_health.html",
        template_context(
            request,
            identity,
            active="system-health",
            db_ok=check_database(),
            workers=workers,
            pending=pending,
            backup=backup,
            pager=pager,
        ),
    )


@app.get("/settings", response_class=HTMLResponse, include_in_schema=False)
def settings_page(request: Request, db: Session = Depends(get_db)) -> HTMLResponse:
    identity = ui_identity(request, db)
    return templates.TemplateResponse(
        request,
        "settings.html",
        template_context(request, identity, active="settings", settings=settings),
    )
