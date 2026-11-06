"""onboarding registration and sync state

Revision ID: 3f7d0b6e9c11
Revises: 27cbbc294dc8
Create Date: 2026-08-17
"""

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "3f7d0b6e9c11"
down_revision: str | Sequence[str] | None = "27cbbc294dc8"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # SQLite DDL is not transactional.  A process interruption can therefore
    # leave every operation in one batch applied while the Alembic revision is
    # still at the previous value.  Inspecting before each batch makes startup
    # safe to retry instead of trapping the Hub in a duplicate-column loop.
    bind = op.get_bind()
    # Alembic's SQLite batch implementation can also leave an empty shadow
    # table when the container is interrupted before the copy/swap phase.  The
    # canonical tables still hold all rows; remove only those known batch
    # scratch tables before retrying the migration.
    inspector = sa.inspect(bind)
    table_names = set(inspector.get_table_names())
    if "projects" in table_names and "_alembic_tmp_projects" in table_names:
        op.execute(sa.text("DROP TABLE _alembic_tmp_projects"))
    if "project_instances" in table_names and "_alembic_tmp_project_instances" in table_names:
        op.execute(sa.text("DROP TABLE _alembic_tmp_project_instances"))

    project_columns = {
        "registration_state": sa.Column(
            "registration_state",
            sa.String(length=32),
            nullable=False,
            server_default="REGISTERED",
        ),
        "first_registered_at": sa.Column("first_registered_at", sa.DateTime(timezone=True)),
        "last_synced_at": sa.Column("last_synced_at", sa.DateTime(timezone=True)),
    }
    existing = {column["name"] for column in sa.inspect(bind).get_columns("projects")}
    missing = [column for name, column in project_columns.items() if name not in existing]
    for column in missing:
        op.add_column("projects", column)
    project_indexes = {index["name"] for index in sa.inspect(bind).get_indexes("projects")}
    if "ix_projects_registration_state" not in project_indexes:
        op.create_index("ix_projects_registration_state", "projects", ["registration_state"])

    instance_columns = {
        "registration_state": sa.Column(
            "registration_state",
            sa.String(length=32),
            nullable=False,
            server_default="REGISTERED",
        ),
        "capabilities": sa.Column("capabilities", sa.JSON(), nullable=False, server_default="{}"),
        "index_status": sa.Column(
            "index_status", sa.String(length=32), nullable=False, server_default="UNKNOWN"
        ),
        "worker_version": sa.Column("worker_version", sa.String(length=32)),
        "policy_schema_version": sa.Column("policy_schema_version", sa.Integer()),
        "project_manifest_schema_version": sa.Column(
            "project_manifest_schema_version", sa.Integer()
        ),
        "initialized_at": sa.Column("initialized_at", sa.DateTime(timezone=True)),
        "first_seen_at": sa.Column(
            "first_seen_at",
            sa.DateTime(timezone=True),
            nullable=False,
            # SQLite cannot ADD COLUMN with a non-constant CURRENT_TIMESTAMP
            # default on a populated table.  The application always supplies
            # this value for new instances; the constant is only a safe bridge
            # for rows created by older releases.
            server_default="1970-01-01 00:00:00",
        ),
    }
    existing = {column["name"] for column in sa.inspect(bind).get_columns("project_instances")}
    missing = [column for name, column in instance_columns.items() if name not in existing]
    for column in missing:
        op.add_column("project_instances", column)
    if "first_seen_at" not in existing:
        op.execute(
            sa.text(
                "UPDATE project_instances "
                "SET first_seen_at = COALESCE(created_at, CURRENT_TIMESTAMP)"
            )
        )
    instance_indexes = {
        index["name"] for index in sa.inspect(bind).get_indexes("project_instances")
    }
    if "ix_project_instances_registration_state" not in instance_indexes:
        op.create_index(
            "ix_project_instances_registration_state",
            "project_instances",
            ["registration_state"],
        )


def downgrade() -> None:
    with op.batch_alter_table("project_instances") as batch:
        batch.drop_index("ix_project_instances_registration_state")
        for name in (
            "first_seen_at",
            "initialized_at",
            "project_manifest_schema_version",
            "policy_schema_version",
            "worker_version",
            "index_status",
            "capabilities",
            "registration_state",
        ):
            batch.drop_column(name)
    with op.batch_alter_table("projects") as batch:
        batch.drop_index("ix_projects_registration_state")
        batch.drop_column("last_synced_at")
        batch.drop_column("first_registered_at")
        batch.drop_column("registration_state")
