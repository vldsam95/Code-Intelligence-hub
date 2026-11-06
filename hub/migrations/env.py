from __future__ import annotations

from logging.config import fileConfig

from alembic import context
from sqlalchemy import engine_from_config, pool

from hub.app import models  # noqa: F401
from hub.app.config import get_settings
from hub.app.db import Base

config = context.config
if config.config_file_name is not None:
    fileConfig(config.config_file_name)
config.set_main_option("sqlalchemy.url", get_settings().database_url)
target_metadata = Base.metadata


def run_migrations_offline() -> None:
    context.configure(
        url=config.get_main_option("sqlalchemy.url"),
        target_metadata=target_metadata,
        literal_binds=True,
        dialect_opts={"paramstyle": "named"},
        compare_type=True,
    )
    with context.begin_transaction():
        context.run_migrations()


def run_migrations_online() -> None:
    connectable = engine_from_config(
        config.get_section(config.config_ini_section, {}),
        prefix="sqlalchemy.",
        poolclass=pool.NullPool,
    )
    with connectable.connect() as connection:
        if connection.dialect.name == "sqlite":
            connection.exec_driver_sql("PRAGMA journal_mode=WAL")
            connection.exec_driver_sql("PRAGMA foreign_keys=ON")
            connection.exec_driver_sql("PRAGMA busy_timeout=5000")
            # PRAGMA journal_mode opens an implicit SQLAlchemy transaction. Commit it before
            # Alembic starts its own versioning lifecycle, otherwise SQLite keeps the DDL but
            # rolls back the alembic_version row when the connection closes.
            connection.commit()
        context.configure(connection=connection, target_metadata=target_metadata, compare_type=True)
        with context.begin_transaction():
            context.run_migrations()


if context.is_offline_mode():
    run_migrations_offline()
else:
    run_migrations_online()
