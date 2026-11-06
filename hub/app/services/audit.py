from __future__ import annotations

from typing import Any

from sqlalchemy.orm import Session

from ..models import AuditEvent


def audit(
    db: Session,
    *,
    event_code: str,
    request_id: str,
    actor_type: str,
    actor_id: str | None = None,
    target_type: str | None = None,
    target_id: str | None = None,
    source_ip_hash: str | None = None,
    details: dict[str, Any] | None = None,
) -> None:
    safe_details = {
        key: value
        for key, value in (details or {}).items()
        if key.lower() not in {"token", "secret", "password", "session", "authorization"}
    }
    db.add(
        AuditEvent(
            event_code=event_code,
            request_id=request_id,
            actor_type=actor_type,
            actor_id=actor_id,
            target_type=target_type,
            target_id=target_id,
            source_ip_hash=source_ip_hash,
            details=safe_details,
        )
    )
