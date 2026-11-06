from __future__ import annotations

import uuid
from datetime import UTC, datetime, timedelta

from fastapi.testclient import TestClient
from sqlalchemy import select

from hub.app.db import SessionLocal
from hub.app.main import app
from hub.app.models import (
    AgentCredential,
    AgentSessionActivity,
    AuditEvent,
    BrowserSession,
    Project,
    ProjectInstance,
    RecoveryEvent,
    ReleaseArtifact,
    RemoteAnalysisRequest,
    RemoteAnalysisResult,
    SourceRevision,
    WikiBinding,
    utcnow,
)


def login(client: TestClient) -> str:
    response = client.post(
        "/api/v1/auth/login",
        json={"username": "test-owner", "password": "test-password"},
    )
    assert response.status_code == 200, response.text
    cookies = response.headers.get_list("set-cookie")
    assert any("ci_session=" in value and "HttpOnly" in value for value in cookies)
    assert all("SameSite=lax" in value for value in cookies)
    return response.json()["csrf_token"]


def create_token(client: TestClient, csrf: str, payload: dict) -> dict:
    response = client.post("/api/v1/tokens", json=payload, headers={"x-csrf-token": csrf})
    assert response.status_code == 201, response.text
    return response.json()


def test_login_rate_limit_is_scoped_to_account_and_source() -> None:
    with TestClient(app) as client:
        for _ in range(5):
            response = client.post(
                "/api/v1/auth/login",
                json={"username": "blocked-probe", "password": "wrong"},
            )
            assert response.status_code == 401
        blocked = client.post(
            "/api/v1/auth/login",
            json={"username": "blocked-probe", "password": "wrong"},
        )
        assert blocked.status_code == 429
        separate_account = client.post(
            "/api/v1/auth/login",
            json={"username": "separate-probe", "password": "wrong"},
        )
        assert separate_account.status_code == 401


def test_browser_login_rate_limit_uses_same_account_and_source_contract() -> None:
    with TestClient(app) as client:
        for _ in range(5):
            response = client.post(
                "/login",
                data={"username": "blocked-ui-probe", "password": "wrong"},
            )
            assert response.status_code == 401
        blocked = client.post(
            "/login",
            data={"username": "blocked-ui-probe", "password": "wrong"},
        )
        assert blocked.status_code == 429


def worker_and_project(client: TestClient, csrf: str) -> tuple[str, str, str, str, str]:
    enrollment = create_token(
        client,
        csrf,
        {
            "label": "test enrollment",
            "token_type": "WORKER_ENROLLMENT",
            "scope_type": "GLOBAL",
            "scopes": ["worker:enroll"],
            "expires_in_sec": 600,
            "one_time": True,
        },
    )["token"]
    host_id, project_id, instance_id = (str(uuid.uuid4()) for _ in range(3))
    fingerprint = f"test-fingerprint-{host_id}"
    enrolled = client.post(
        "/api/v1/workers/enroll",
        json={
            "enrollment_token": enrollment,
            "host_id": host_id,
            "host_name": "test-worker",
            "fingerprint": fingerprint,
            "os": "linux",
            "architecture": "amd64",
            "worker_version": "1.0.0",
            "protocol_version": 1,
            "adapters": ["codex"],
        },
    )
    assert enrolled.status_code == 201, enrolled.text
    assert (
        client.post(
            "/api/v1/workers/enroll",
            json={
                "enrollment_token": enrollment,
                "host_id": host_id,
                "host_name": "duplicate",
                "fingerprint": fingerprint,
                "os": "linux",
                "architecture": "amd64",
                "worker_version": "1.0.0",
            },
        ).status_code
        == 401
    )
    body = enrolled.json()
    worker_id, credential = body["worker_id"], body["worker_credential"]
    host_access = body["host_access_token"]
    revision = "a" * 64
    registration_payload = {
        "project_id": project_id,
        "slug": f"project-{project_id[:8]}",
        "name": "Project without Git or Wiki",
        "instance_id": instance_id,
        "host_id": host_id,
        "primary_path": "/srv/project",
        "related_roots": [],
        "capabilities": {"languages": ["python"], "frameworks": [], "semantic": "PARTIAL"},
        "wiki": {"mode": "none", "binding_status": "NONE"},
        "index_status": "CURRENT",
        "worker_version": "1.1.0",
        "policy_schema_version": 1,
        "project_manifest_schema_version": 1,
        "initialized_at": datetime.now(UTC).isoformat(),
        "revision": {
            "revision_id": revision,
            "project_id": project_id,
            "policy_hash": "b" * 64,
            "indexer_version": "1.1.0",
            "root_mapping_version": 1,
            "file_count": 3,
            "inventory_hash": "c" * 64,
        },
    }
    registered = client.post(
        f"/api/v1/workers/{worker_id}/projects/register",
        headers={"authorization": f"Bearer {host_access}"},
        json=registration_payload,
    )
    assert registered.status_code == 201, registered.text
    first_registered_at = registered.json()["first_registered_at"]
    replay = client.post(
        f"/api/v1/workers/{worker_id}/projects/register",
        headers={"authorization": f"Bearer {credential}"},
        json=registration_payload,
    )
    assert replay.status_code == 201, replay.text
    assert replay.json()["project_id"] == project_id
    assert replay.json()["instance_id"] == instance_id
    assert replay.json()["first_registered_at"] == first_registered_at
    with SessionLocal() as db:
        assert len(list(db.scalars(select(Project).where(Project.id == project_id)))) == 1
        assert (
            len(list(db.scalars(select(ProjectInstance).where(ProjectInstance.id == instance_id))))
            == 1
        )
        project = db.get(Project, project_id)
        instance = db.get(ProjectInstance, instance_id)
        binding = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project_id))
        assert project is not None and project.registration_state == "REGISTERED"
        assert project.first_registered_at is not None and project.last_synced_at is not None
        assert instance is not None and instance.registration_state == "REGISTERED"
        assert instance.index_status == "CURRENT" and instance.capabilities["languages"] == [
            "python"
        ]
        assert binding is not None and binding.mode == "none" and binding.status == "NONE"
    return worker_id, credential, project_id, instance_id, revision


def report_envelope(project_id: str, instance_id: str, revision: str, payload: dict) -> dict:
    return {
        "schema_version": 1,
        "message_id": str(uuid.uuid4()),
        "sent_at": datetime.now(UTC).isoformat(),
        "project_id": project_id,
        "instance_id": instance_id,
        "source_revision_id": revision,
        "report_type": "quality",
        "payload": payload,
    }


def test_csrf_token_lifecycle_and_audit_redaction() -> None:
    with TestClient(app) as client:
        csrf = login(client)
        rejected = client.post(
            "/api/v1/tokens",
            json={
                "label": "must fail",
                "token_type": "ADMIN_API",
                "scope_type": "GLOBAL",
                "scopes": ["admin"],
            },
        )
        assert rejected.status_code == 403
        created = create_token(
            client,
            csrf,
            {
                "label": "automation",
                "token_type": "ADMIN_API",
                "scope_type": "GLOBAL",
                "scopes": ["admin"],
            },
        )
        raw = created["token"]
        assert raw.startswith("ci_admin_v1_")
        with SessionLocal() as db:
            session = db.scalar(select(BrowserSession).order_by(BrowserSession.created_at.desc()))
            assert session is not None
            session.reauthenticated_at = utcnow() - timedelta(minutes=11)
            db.commit()
        assert (
            client.post(
                f"/api/v1/tokens/{created['token_id']}/reveal",
                headers={"x-csrf-token": csrf},
            ).status_code
            == 403
        )
        assert (
            client.post(
                "/api/v1/auth/re-auth",
                json={"password": "test-password"},
                headers={"x-csrf-token": csrf},
            ).status_code
            == 200
        )
        reveal = client.post(
            f"/api/v1/tokens/{created['token_id']}/reveal",
            headers={"x-csrf-token": csrf},
        )
        assert reveal.json()["token"] == raw
        with TestClient(app) as bearer_client:
            assert (
                bearer_client.get(
                    "/api/v1/projects", headers={"authorization": f"Bearer {raw}"}
                ).status_code
                == 200
            )
        assert (
            client.delete(
                f"/api/v1/tokens/{created['token_id']}",
                headers={"x-csrf-token": csrf},
            ).status_code
            == 204
        )
        with TestClient(app) as bearer_client:
            assert (
                bearer_client.get(
                    "/api/v1/projects", headers={"authorization": f"Bearer {raw}"}
                ).status_code
                == 401
            )
        with SessionLocal() as db:
            credential = db.get(AgentCredential, created["token_id"])
            assert credential is not None and credential.status == "REVOKED"
            for event in db.scalars(select(AuditEvent)):
                assert raw not in str(event.details)


def test_worker_registration_quality_guard_and_restore_detection() -> None:
    with TestClient(app) as client:
        csrf = login(client)
        worker_id, credential, project_id, instance_id, revision = worker_and_project(client, csrf)
        payload = {
            "quality_run_id": str(uuid.uuid4()),
            "project_id": project_id,
            "source_revision_id": revision,
            "verdict": "PASS",
            "currentness": "CURRENT",
            "risk_score": 70,
            "required_checks": ["review", "tests", "smoke"],
            "completed_checks": ["review"],
            "blocking_unknowns": [],
            "warnings": [],
            "policy_hash": "b" * 64,
            "graph_state": "CURRENT",
        }
        response = client.post(
            f"/api/v1/instances/{instance_id}/reports/quality",
            headers={"authorization": f"Bearer {credential}"},
            json=report_envelope(project_id, instance_id, revision, payload),
        )
        assert response.status_code == 201, response.text
        assert response.json()["verdict"] == "UNKNOWN"

        next_revision = "d" * 64
        for current in (next_revision, revision):
            registered = client.post(
                f"/api/v1/workers/{worker_id}/projects/register",
                headers={"authorization": f"Bearer {credential}"},
                json={
                    "project_id": project_id,
                    "slug": f"project-{project_id[:8]}",
                    "name": "Project without Git or Wiki",
                    "instance_id": instance_id,
                    "host_id": client.get(f"/api/v1/workers/{worker_id}").json()["host_id"],
                    "primary_path": "/srv/project",
                    "related_roots": [],
                    "revision": {
                        "revision_id": current,
                        "project_id": project_id,
                        "policy_hash": "b" * 64,
                        "indexer_version": "1.0.0",
                        "root_mapping_version": 1,
                        "file_count": 3,
                        "inventory_hash": current,
                    },
                },
            )
            assert registered.status_code == 201, registered.text
        assert registered.json()["restored"] is True
        with SessionLocal() as db:
            assert db.scalar(select(RecoveryEvent).where(RecoveryEvent.project_id == project_id))


def test_historical_spooled_report_registers_revision_without_changing_current() -> None:
    with TestClient(app) as client:
        csrf = login(client)
        _, credential, project_id, instance_id, current_revision = worker_and_project(client, csrf)
        historical_revision = "e" * 64
        envelope = {
            "schema_version": 1,
            "message_id": str(uuid.uuid4()),
            "sent_at": datetime.now(UTC).isoformat(),
            "project_id": project_id,
            "instance_id": instance_id,
            "source_revision_id": historical_revision,
            "revision": {
                "revision_id": historical_revision,
                "project_id": project_id,
                "policy_hash": "b" * 64,
                "indexer_version": "1.0.0",
                "root_mapping_version": 1,
                "file_count": 7,
                "inventory_hash": "f" * 64,
            },
            "report_type": "index",
            "payload": {
                "evidence_id": str(uuid.uuid4()),
                "index_snapshot_id": str(uuid.uuid4()),
                "producer": "codeintel-indexer",
                "producer_version": "1.0.0",
                "origin": "STATIC_EXACT",
                "confidence": 1.0,
                "coverage": {"semantic": "PARTIAL"},
                "policy_hash": "b" * 64,
                "state": "CURRENT",
            },
        }
        response = client.post(
            f"/api/v1/instances/{instance_id}/reports/index",
            headers={"authorization": f"Bearer {credential}"},
            json=envelope,
        )
        assert response.status_code == 201, response.text
        with SessionLocal() as db:
            assert db.scalar(
                select(SourceRevision).where(
                    SourceRevision.instance_id == instance_id,
                    SourceRevision.revision_id == historical_revision,
                )
            )
            instance = db.get(ProjectInstance, instance_id)
            assert instance is not None
            assert instance.current_source_revision_id == current_revision


def test_docs_are_rendered_raw_and_downloadable() -> None:
    with TestClient(app) as client:
        login(client)
        rendered = client.get("/documentation?doc=AGENT_OPERATING_POLICY.md")
        raw = client.get("/api/v1/docs/AGENT_OPERATING_POLICY.md?format=raw")
        archive = client.get("/docs-bundle.zip")
        assert rendered.status_code == 200
        assert "graph" in rendered.text.lower()
        assert raw.status_code == 200 and "CODE INTELLIGENCE OPERATING POLICY" in raw.text
        assert archive.status_code == 200 and archive.headers["content-type"] == "application/zip"


def test_agent_toolkit_and_favicon_explain_opt_in_and_offline_registration() -> None:
    with TestClient(app) as client:
        login(client)
        toolkit = client.get("/agent-toolkit")
        favicon = client.get("/static/favicon.svg")
        onboarding = client.get(
            "/api/v1/docs/PROJECT_ONBOARDING_AND_HUB_REGISTRATION.md?format=raw"
        )
        assert toolkit.status_code == 200
        assert "ONE UNIVERSAL PROMPT" in toolkit.text
        assert "Host Install Token" in toolkit.text
        assert "LOCAL_ONLY" in toolkit.text
        assert "non-invasive" in toolkit.text
        assert "codeintel-complete-agent-kit-v1.3.0.zip" in toolkit.text
        assert favicon.status_code == 200 and "svg" in favicon.headers["content-type"]
        assert onboarding.status_code == 200
        assert "registration=PENDING" in onboarding.text
        assert "No manual Create Project action" in onboarding.text


def test_download_catalog_exposes_only_the_active_release() -> None:
    with SessionLocal() as db:
        stale = ReleaseArtifact(
            version="1.2.1",
            platform="linux-amd64",
            name="codeintel-worker-v1.2.1-linux-amd64.tar.gz",
            relative_path="codeintel-worker-v1.2.1-linux-amd64.tar.gz",
            sha256="0" * 64,
            size_bytes=1,
            signature_status="SIGNED_MANIFEST",
        )
        db.add(stale)
        db.commit()
        stale_id = stale.id

    try:
        with TestClient(app) as client:
            login(client)
            page = client.get("/downloads")
            catalog = client.get("/api/v1/downloads")
            assert page.status_code == 200
            assert "RELEASE 1.3.0" in page.text
            assert "codeintel-complete-agent-kit-v1.3.0.zip" in page.text
            assert "codeintel-worker-v1.2.1-linux-amd64.tar.gz" not in page.text
            assert catalog.status_code == 200
            assert catalog.json()["items"]
            assert {item["version"] for item in catalog.json()["items"]} == {"1.3.0"}
        with SessionLocal() as db:
            assert db.get(ReleaseArtifact, stale_id) is None
    finally:
        with SessionLocal() as db:
            row = db.get(ReleaseArtifact, stale_id)
            if row:
                db.delete(row)
                db.commit()


def test_on_demand_graph_is_bounded_rejects_source_and_is_consumed_once() -> None:
    with TestClient(app) as client:
        csrf = login(client)
        worker_id, credential, project_id, instance_id, revision = worker_and_project(client, csrf)
        started = client.post(
            f"/projects/{project_id}/graph-session", headers={"x-csrf-token": csrf}
        )
        assert started.status_code == 202, started.text
        request_id = started.json()["request_id"]
        job = client.get(
            f"/api/v1/workers/{worker_id}/jobs/next",
            headers={"authorization": f"Bearer {credential}"},
        )
        assert job.status_code == 200, job.text
        assert job.json()["job"]["query_type"] == "graph_visualization"
        claimed = client.post(
            f"/api/v1/workers/{worker_id}/remote-analysis/{request_id}/claim",
            headers={"authorization": f"Bearer {credential}"},
        )
        assert claimed.status_code == 200, claimed.text
        base_result = {
            "result_id": str(uuid.uuid4()),
            "project_id": project_id,
            "instance_id": instance_id,
            "source_revision_id": revision,
            "freshness": "CURRENT",
            "summary": "bounded graph",
            "coverage": {"semantic": "PARTIAL"},
            "unknowns": [],
        }
        rejected = client.post(
            f"/api/v1/remote-analysis/{request_id}/result",
            headers={"authorization": f"Bearer {credential}"},
            json={
                **base_result,
                "structured_result": {
                    "nodes": [{"id": "n1", "kind": "FUNCTION", "source_body": "secret"}],
                    "edges": [],
                    "source_bodies_included": False,
                },
            },
        )
        assert rejected.status_code == 422
        accepted = client.post(
            f"/api/v1/remote-analysis/{request_id}/result",
            headers={"authorization": f"Bearer {credential}"},
            json={
                **base_result,
                "structured_result": {
                    "nodes": [
                        {
                            "id": "n1",
                            "kind": "FUNCTION",
                            "qualified_name": "app.main",
                            "label": "main",
                            "path": "app.py",
                            "language": "python",
                            "entry_point": True,
                            "exported": False,
                            "line": 1,
                        }
                    ],
                    "edges": [],
                    "node_count_total": 1,
                    "edge_count_total": 0,
                    "nodes_remaining": 0,
                    "edges_remaining": 0,
                    "bounded": True,
                    "ephemeral": True,
                    "source_bodies_included": False,
                    "coverage": {"semantic": "PARTIAL"},
                    "warning": "Discovery evidence only",
                },
            },
        )
        assert accepted.status_code == 201, accepted.text
        delivered = client.get(f"/projects/{project_id}/graph-session/{request_id}")
        assert delivered.status_code == 200, delivered.text
        assert delivered.headers["cache-control"] == "no-store"
        assert delivered.json()["graph"]["source_bodies_included"] is False
        assert client.get(f"/projects/{project_id}/graph-session/{request_id}").status_code == 410
        with SessionLocal() as db:
            result = db.scalar(
                select(RemoteAnalysisResult).where(RemoteAnalysisResult.request_id == request_id)
            )
            assert result is not None and result.structured_result.get("consumed") is True


def test_projects_search_and_ui_pagination_are_server_side() -> None:
    marker = f"pagination-{uuid.uuid4().hex[:8]}"
    with SessionLocal() as db:
        for index in range(27):
            project_id = str(uuid.uuid4())
            db.add(
                Project(
                    id=project_id,
                    slug=f"{marker}-{index:02d}",
                    name=f"{marker} Project {index:02d}",
                    status="ACTIVE",
                )
            )
        db.commit()
    with TestClient(app) as client:
        login(client)
        first = client.get("/projects", params={"q": marker, "page": 1})
        second = client.get("/projects", params={"q": marker, "page": 2})
        assert first.status_code == 200 and second.status_code == 200
        assert first.text.count('class="project-card"') == 25
        assert second.text.count('class="project-card"') == 2
        assert "Page <b>2</b> of 2" in second.text
        assert f"{marker} Project 26" in second.text
        dashboard = client.get("/dashboard", params={"page": 1})
        assert dashboard.status_code == 200
        assert dashboard.text.count('class="project-link"') == 25
        assert "25 per page" in dashboard.text


def test_project_remote_reader_is_scoped_and_separate_from_local_coding() -> None:
    with TestClient(app) as client:
        csrf = login(client)
        _, _, project_id, _, _ = worker_and_project(client, csrf)
        page = client.get(f"/projects/{project_id}/view")
        assert page.status_code == 200, page.text
        assert "PROJECT ACCESS & AGENT SETUP" in page.text
        assert f"/api/v1/projects/{project_id}/context-pack" in page.text
        assert "Local Code Intelligence" in page.text
        assert "remote shell" in page.text.lower()
        assert "Project Access" in page.text

        read_token = create_token(
            client,
            csrf,
            {
                "label": "presentation reader",
                "token_type": "PROJECT_AGENT",
                "scope_type": "PROJECT",
                "scope_id": project_id,
                "scopes": [f"project:{project_id}:read"],
            },
        )["token"]
        remote = TestClient(app)
        headers = {"authorization": f"Bearer {read_token}"}
        context = remote.get(f"/api/v1/projects/{project_id}/context-pack", headers=headers)
        assert context.status_code == 200
        denied = remote.post(
            f"/api/v1/projects/{project_id}/remote-analysis",
            headers=headers,
            json={"query_type": "architecture_refresh", "query": {}},
        )
        assert denied.status_code == 403


def test_simple_guides_are_rendered_and_available_as_raw_markdown() -> None:
    with TestClient(app) as client:
        login(client)
        for guide in (
            "new-host",
            "new-project",
            "existing-code",
            "move-project",
            "llm-wiki",
            "remote-reader",
        ):
            response = client.get("/guides", params={"guide": guide})
            assert response.status_code == 200
            assert "3 SIMPLE STEPS" in response.text
            assert "Copy all" in response.text
        raw = client.get("/api/v1/docs/SIMPLE_GUIDE_NEW_HOST.md?format=raw")
        assert raw.status_code == 200
        assert "HOST_INSTALL_TOKEN" in raw.text
        assert "--local-only" in raw.text


def test_expired_remote_job_lease_is_requeued() -> None:
    with TestClient(app) as client:
        csrf = login(client)
        worker_id, credential, project_id, _, _ = worker_and_project(client, csrf)
        started = client.post(
            f"/projects/{project_id}/graph-session", headers={"x-csrf-token": csrf}
        )
        request_id = started.json()["request_id"]
        with SessionLocal() as db:
            row = db.get(RemoteAnalysisRequest, request_id)
            assert row is not None
            row.status = "RUNNING"
            row.worker_id = worker_id
            row.lease_expires_at = utcnow() - timedelta(seconds=1)
            db.commit()
        job = client.get(
            f"/api/v1/workers/{worker_id}/jobs/next",
            headers={"authorization": f"Bearer {credential}"},
        )
        assert job.status_code == 200, job.text
        assert job.json()["job"]["request_id"] == request_id


def test_simple_host_and_project_access_model_end_to_end() -> None:
    with TestClient(app) as client:
        csrf = login(client)
        install = client.post("/api/v1/host-install-tokens", headers={"x-csrf-token": csrf})
        assert install.status_code == 201
        host_id, project_id, instance_id = (str(uuid.uuid4()) for _ in range(3))
        enrolled = client.post(
            "/api/v1/workers/enroll",
            json={
                "enrollment_token": install.json()["install_token"],
                "host_id": host_id,
                "host_name": "simple-access-host",
                "fingerprint": f"fingerprint-{host_id}",
                "os": "linux",
                "architecture": "amd64",
                "worker_version": "1.2.1",
                "adapters": ["codex", "claude", "hermes"],
            },
        )
        assert enrolled.status_code == 201, enrolled.text
        worker_id = enrolled.json()["worker_id"]
        machine = enrolled.json()["worker_credential"]
        host_access = enrolled.json()["host_access_token"]
        registration_payload = {
            "project_id": project_id,
            "slug": f"simple-{project_id[:8]}",
            "name": "Simple Access Project",
            "instance_id": instance_id,
            "host_id": host_id,
            "primary_path": "/srv/simple",
            "capabilities": {"languages": ["python"]},
            "wiki": {"mode": "none", "binding_status": "NONE"},
            "index_status": "CURRENT",
            "worker_version": "1.2.1",
            "policy_schema_version": 1,
            "project_manifest_schema_version": 1,
            "revision": {
                "revision_id": "9" * 64,
                "project_id": project_id,
                "policy_hash": "8" * 64,
                "indexer_version": "1.2.1",
                "file_count": 1,
                "inventory_hash": "7" * 64,
            },
        }
        registration = client.post(
            f"/api/v1/workers/{worker_id}/projects/register",
            headers={"authorization": f"Bearer {host_access}"},
            json=registration_payload,
        )
        assert registration.status_code == 201, registration.text
        project_access = registration.json()["project_access"]["token"]
        assert project_access.startswith("ci_proj_v1_")
        project_headers = {"authorization": f"Bearer {project_access}"}

        initialized = client.post(
            "/mcp",
            headers=project_headers | {"x-agent-client": "codex", "x-agent-version": "test-client"},
            json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        )
        assert initialized.status_code == 200, initialized.text
        with SessionLocal() as db:
            automatic_activity = db.scalar(
                select(AgentSessionActivity).where(
                    AgentSessionActivity.project_id == project_id,
                    AgentSessionActivity.activity == "HUB_MCP_INITIALIZE",
                )
            )
            assert automatic_activity is not None
            assert automatic_activity.agent_client == "codex"

        for agent in ("codex", "claude-code"):
            activity = client.post(
                f"/api/v1/projects/{project_id}/agent-sessions",
                headers=project_headers,
                json={
                    "agent_client": agent,
                    "host_id": host_id,
                    "instance_id": instance_id,
                },
            )
            assert activity.status_code == 201, activity.text
        with SessionLocal() as db:
            legacy_access = db.scalar(
                select(AgentCredential).where(
                    AgentCredential.token_type == "PROJECT_AGENT",
                    AgentCredential.scope_id == project_id,
                    AgentCredential.status == "ACTIVE",
                )
            )
            assert legacy_access is not None
            legacy_access.scopes = [f"project:{project_id}:read"]
            db.commit()
        replayed_registration = client.post(
            f"/api/v1/workers/{worker_id}/projects/register",
            headers={"authorization": f"Bearer {machine}"},
            json=registration_payload,
        )
        assert replayed_registration.status_code == 201, replayed_registration.text
        migrated_activity = client.post(
            f"/api/v1/projects/{project_id}/agent-sessions",
            headers=project_headers,
            json={"agent_client": "hermes", "host_id": host_id, "instance_id": instance_id},
        )
        assert migrated_activity.status_code == 201, migrated_activity.text
        with SessionLocal() as db:
            upgraded_access = db.scalar(
                select(AgentCredential).where(
                    AgentCredential.token_type == "PROJECT_AGENT",
                    AgentCredential.scope_id == project_id,
                    AgentCredential.status == "ACTIVE",
                )
            )
            assert upgraded_access is not None
            assert f"project:{project_id}:session_log" in upgraded_access.scopes
        note = client.post(
            f"/api/v1/projects/{project_id}/agent-notes",
            headers=project_headers,
            json={"kind": "TECHNICAL_NOTE", "content": "Bounded durable note"},
        )
        assert note.status_code == 201, note.text
        notes = client.get(f"/api/v1/projects/{project_id}/agent-notes", headers=project_headers)
        assert notes.json()["items"][0]["content"] == "Bounded durable note"
        kit = client.get(f"/api/v1/projects/{project_id}/agent-kit", headers=project_headers)
        assert kit.status_code == 200
        assert project_access.encode() not in kit.content

        other_id = str(uuid.uuid4())
        assert (
            client.post(
                "/api/v1/projects",
                headers={"x-csrf-token": csrf},
                json={"project_id": other_id, "slug": f"other-{other_id[:8]}", "name": "Other"},
            ).status_code
            == 201
        )
        remote = TestClient(app)
        assert (
            remote.get(f"/api/v1/projects/{other_id}", headers=project_headers).status_code == 403
        )

        assert (
            client.post(
                "/api/v1/auth/re-auth",
                headers={"x-csrf-token": csrf},
                json={"password": "test-password"},
            ).status_code
            == 200
        )
        rotated = client.post(
            f"/api/v1/projects/{project_id}/access/rotate",
            headers={"x-csrf-token": csrf},
        )
        assert rotated.status_code == 200
        assert (
            remote.get(f"/api/v1/projects/{project_id}", headers=project_headers).status_code == 401
        )
        new_headers = {"authorization": f"Bearer {rotated.json()['token']}"}
        assert remote.get(f"/api/v1/projects/{project_id}", headers=new_headers).status_code == 200

        revoked_project = client.post(
            f"/api/v1/projects/{project_id}/access/revoke",
            headers={"x-csrf-token": csrf},
        )
        assert revoked_project.status_code == 200
        assert remote.get(f"/api/v1/projects/{project_id}", headers=new_headers).status_code == 401
        machine_replay = client.post(
            f"/api/v1/workers/{worker_id}/projects/register",
            headers={"authorization": f"Bearer {machine}"},
            json=registration_payload,
        )
        assert machine_replay.status_code == 201, machine_replay.text
        assert machine_replay.json()["project_access"]["status"] == "MISSING"
        assert machine_replay.json()["project_access"]["token"] is None
        assert client.get(f"/api/v1/projects/{project_id}/access").json()["status"] == "MISSING"

        disabled = client.post(
            f"/api/v1/hosts/{host_id}/access/revoke",
            headers={"x-csrf-token": csrf},
        )
        assert disabled.status_code == 200
        assert (
            client.post(
                f"/api/v1/workers/{worker_id}/heartbeat",
                headers={"authorization": f"Bearer {machine}"},
                json={"worker_version": "1.2.1", "project_instances": []},
            ).status_code
            == 200
        )
        new_project_id = str(uuid.uuid4())
        rejected_payload = registration_payload | {
            "project_id": new_project_id,
            "slug": f"blocked-{new_project_id[:8]}",
            "instance_id": str(uuid.uuid4()),
            "revision": registration_payload["revision"] | {"project_id": new_project_id},
        }
        assert (
            client.post(
                f"/api/v1/workers/{worker_id}/projects/register",
                headers={"authorization": f"Bearer {host_access}"},
                json=rejected_payload,
            ).status_code
            == 401
        )
