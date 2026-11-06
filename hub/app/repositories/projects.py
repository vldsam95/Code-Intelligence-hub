from __future__ import annotations

from sqlalchemy import desc, func, or_, select
from sqlalchemy.orm import Session

from ..models import Project, ProjectContextPack, ProjectInstance, QualityRun


class ProjectRepository:
    def __init__(self, db: Session) -> None:
        self.db = db

    def list_active(self, limit: int = 50) -> list[Project]:
        return list(
            self.db.scalars(
                select(Project)
                .where(Project.status != "RETIRED")
                .order_by(Project.name)
                .limit(limit)
            )
        )

    def page_active(
        self, *, page: int = 1, page_size: int = 25, query: str | None = None
    ) -> tuple[list[Project], int]:
        """Return one deterministic UI page without loading every project into memory."""
        page = max(page, 1)
        page_size = min(max(page_size, 1), 25)
        statement = select(Project).where(Project.status != "RETIRED")
        count_statement = (
            select(func.count()).select_from(Project).where(Project.status != "RETIRED")
        )
        normalized = (query or "").strip().lower()
        if normalized:
            escaped = normalized.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
            pattern = f"%{escaped}%"
            search = or_(
                func.lower(Project.name).like(pattern, escape="\\"),
                func.lower(Project.slug).like(pattern, escape="\\"),
            )
            statement = statement.where(search)
            count_statement = count_statement.where(search)
        total = int(self.db.scalar(count_statement) or 0)
        rows = list(
            self.db.scalars(
                statement.order_by(func.lower(Project.name), Project.id)
                .offset((page - 1) * page_size)
                .limit(page_size)
            )
        )
        return rows, total

    def count_active(self) -> int:
        return int(
            self.db.scalar(
                select(func.count()).select_from(Project).where(Project.status != "RETIRED")
            )
            or 0
        )

    def get(self, project_id: str) -> Project | None:
        return self.db.get(Project, project_id)

    def latest_instance(self, project_id: str) -> ProjectInstance | None:
        return self.db.scalar(
            select(ProjectInstance)
            .where(ProjectInstance.project_id == project_id, ProjectInstance.status != "RETIRED")
            .order_by(desc(ProjectInstance.heartbeat_at))
        )

    def latest_quality(self, project_id: str) -> QualityRun | None:
        return self.db.scalar(
            select(QualityRun)
            .where(QualityRun.project_id == project_id)
            .order_by(desc(QualityRun.completed_at))
        )

    def latest_context(self, project_id: str) -> ProjectContextPack | None:
        return self.db.scalar(
            select(ProjectContextPack)
            .where(ProjectContextPack.project_id == project_id)
            .order_by(desc(ProjectContextPack.produced_at))
        )
