"""simple host access and agent activity

Revision ID: 8a6f2d4c1b90
Revises: 3f7d0b6e9c11
Create Date: 2026-08-18
"""

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "8a6f2d4c1b90"
down_revision: str | Sequence[str] | None = "3f7d0b6e9c11"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    bind = op.get_bind()
    host_columns = {column["name"] for column in sa.inspect(bind).get_columns("worker_hosts")}
    if "provisioning_enabled" not in host_columns:
        op.add_column(
            "worker_hosts",
            sa.Column(
                "provisioning_enabled", sa.Boolean(), nullable=False, server_default=sa.true()
            ),
        )
    host_indexes = {index["name"] for index in sa.inspect(bind).get_indexes("worker_hosts")}
    if "ix_worker_hosts_provisioning_enabled" not in host_indexes:
        op.create_index(
            "ix_worker_hosts_provisioning_enabled",
            "worker_hosts",
            ["provisioning_enabled"],
        )
    if "agent_session_activities" not in sa.inspect(bind).get_table_names():
        op.create_table(
            "agent_session_activities",
            sa.Column("id", sa.String(length=36), primary_key=True),
            sa.Column(
                "project_id",
                sa.String(length=36),
                sa.ForeignKey("projects.id", ondelete="CASCADE"),
                nullable=False,
            ),
            sa.Column(
                "instance_id",
                sa.String(length=36),
                sa.ForeignKey("project_instances.id", ondelete="SET NULL"),
            ),
            sa.Column(
                "host_id",
                sa.String(length=36),
                sa.ForeignKey("worker_hosts.id", ondelete="SET NULL"),
            ),
            sa.Column("agent_client", sa.String(length=64), nullable=False),
            sa.Column("agent_version", sa.String(length=64)),
            sa.Column("session_id", sa.String(length=255)),
            sa.Column(
                "activity", sa.String(length=32), nullable=False, server_default="OPEN_PROJECT"
            ),
            sa.Column("started_at", sa.DateTime(timezone=True), nullable=False),
        )
        op.create_index(
            "ix_agent_activity_project_started",
            "agent_session_activities",
            ["project_id", "started_at"],
        )
        op.create_index(
            "ix_agent_session_activities_project_id", "agent_session_activities", ["project_id"]
        )
        op.create_index(
            "ix_agent_session_activities_instance_id", "agent_session_activities", ["instance_id"]
        )
        op.create_index(
            "ix_agent_session_activities_host_id", "agent_session_activities", ["host_id"]
        )
    if "project_agent_notes" not in sa.inspect(bind).get_table_names():
        op.create_table(
            "project_agent_notes",
            sa.Column("id", sa.String(length=36), primary_key=True),
            sa.Column(
                "project_id",
                sa.String(length=36),
                sa.ForeignKey("projects.id", ondelete="CASCADE"),
                nullable=False,
            ),
            sa.Column("source_revision_id", sa.String(length=64)),
            sa.Column(
                "kind", sa.String(length=32), nullable=False, server_default="TECHNICAL_NOTE"
            ),
            sa.Column("content", sa.Text(), nullable=False),
            sa.Column("agent_client", sa.String(length=64)),
            sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
            sa.Column("updated_at", sa.DateTime(timezone=True), nullable=False),
        )
        op.create_index(
            "ix_project_agent_notes_project_created",
            "project_agent_notes",
            ["project_id", "created_at"],
        )
        op.create_index("ix_project_agent_notes_project_id", "project_agent_notes", ["project_id"])
        op.create_index(
            "ix_project_agent_notes_source_revision_id",
            "project_agent_notes",
            ["source_revision_id"],
        )


def downgrade() -> None:
    op.drop_table("project_agent_notes")
    op.drop_table("agent_session_activities")
    with op.batch_alter_table("worker_hosts") as batch:
        batch.drop_index("ix_worker_hosts_provisioning_enabled")
        batch.drop_column("provisioning_enabled")
