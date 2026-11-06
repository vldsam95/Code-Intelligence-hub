from __future__ import annotations

from datetime import UTC, datetime, timedelta

from sqlalchemy import desc, func, select
from sqlalchemy.orm import Session

from ..models import Finding, Project, ProjectInstance, QualityRun, WikiBinding, Worker


def project_health(db: Session, project: Project) -> dict[str, object]:
    instance = db.scalar(
        select(ProjectInstance)
        .where(ProjectInstance.project_id == project.id, ProjectInstance.status != "RETIRED")
        .order_by(desc(ProjectInstance.heartbeat_at))
    )
    if not instance:
        return {
            "project_id": project.id,
            "status": "NO_INSTANCE",
            "currentness": "STALE",
            "worker_online": False,
            "current_revision": None,
            "last_quality_gate": None,
            "open_fail": 0,
            "blocking_unknowns": 1,
            "registration_state": project.registration_state,
            "hub_sync": "OFFLINE",
            "wiki": {"mode": "none", "binding_status": "NONE"},
        }
    worker = db.get(Worker, instance.worker_id) if instance.worker_id else None
    online = False
    if worker and worker.last_heartbeat_at:
        heartbeat = worker.last_heartbeat_at
        if heartbeat.tzinfo is None:
            heartbeat = heartbeat.replace(tzinfo=UTC)
        online = heartbeat >= datetime.now(UTC) - timedelta(seconds=90)
    quality = db.scalar(
        select(QualityRun)
        .where(QualityRun.project_id == project.id, QualityRun.instance_id == instance.id)
        .order_by(desc(QualityRun.completed_at))
    )
    currentness = "UNVERIFIED_CHANGE"
    if quality and quality.source_revision_id == instance.current_source_revision_id:
        currentness = quality.currentness
    blocking = (
        db.scalar(
            select(func.count(Finding.id)).where(
                Finding.project_id == project.id,
                Finding.status == "OPEN",
                Finding.blocking.is_(True),
            )
        )
        or 0
    )
    binding = db.scalar(select(WikiBinding).where(WikiBinding.project_id == project.id))
    return {
        "project_id": project.id,
        "status": "ONLINE" if online else "OFFLINE",
        "currentness": currentness,
        "worker_online": online,
        "current_revision": instance.current_source_revision_id,
        "last_quality_gate": quality.verdict if quality else None,
        "last_quality_at": quality.completed_at if quality else None,
        "open_fail": blocking if quality and quality.verdict == "FAIL" else 0,
        "blocking_unknowns": len(quality.blocking_unknowns) if quality else 1,
        "instance_id": instance.id,
        "host_id": instance.host_id,
        "last_sync": instance.heartbeat_at,
        "registration_state": instance.registration_state,
        "hub_sync": "CURRENT" if online else "OFFLINE",
        "index_status": instance.index_status,
        "wiki": {
            "mode": binding.mode if binding else "none",
            "binding_status": binding.status if binding else "NONE",
        },
    }
