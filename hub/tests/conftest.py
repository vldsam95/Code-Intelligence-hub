from __future__ import annotations

import hashlib
import json
import os
import shutil
import tempfile
import zipfile
from pathlib import Path

from argon2 import PasswordHasher

TEST_ROOT = Path(tempfile.mkdtemp(prefix="codeintel-hub-tests-"))
DB_PATH = TEST_ROOT / "hub.db"
KEY_PATH = TEST_ROOT / "hub-master.key"
KEY_PATH.write_bytes(os.urandom(32))
KEY_PATH.chmod(0o600)

os.environ.update(
    {
        "CODEINTEL_ENV": "testing",
        "CODEINTEL_BASE_URL": "http://testserver",
        "CODEINTEL_DATABASE_URL": f"sqlite:///{DB_PATH}",
        "CODEINTEL_ARTIFACTS_DIR": str(TEST_ROOT / "artifacts"),
        "CODEINTEL_DOCS_DIR": str(Path(__file__).resolve().parents[2] / "docs"),
        "CODEINTEL_MASTER_KEY_FILE": str(KEY_PATH),
        "CODEINTEL_OWNER_USERNAME": "test-owner",
        "CODEINTEL_OWNER_PASSWORD_HASH": PasswordHasher().hash("test-password"),
        "CODEINTEL_SESSION_SECRET": "test-session-secret-that-is-at-least-32-bytes",
        "CODEINTEL_TOKEN_HMAC_KEY": "test-token-hmac-key-that-is-at-least-32-bytes",
        "CODEINTEL_TRUSTED_HOSTS": "testserver,localhost",
        "CODEINTEL_SECURE_COOKIES": "false",
        "CODEINTEL_LONG_POLL_HOLD_SEC": "1",
        "CODEINTEL_HUB_VERSION": "1.3.0",
    }
)
(TEST_ROOT / "artifacts").mkdir()
with zipfile.ZipFile(TEST_ROOT / "artifacts" / "codeintel-docs-v1.3.0.zip", "w") as archive:
    docs = Path(__file__).resolve().parents[2] / "docs"
    for path in docs.glob("*.md"):
        archive.write(path, path.name)
manual_bundle = TEST_ROOT / "artifacts" / "codeintel-complete-agent-kit-v1.3.0.zip"
with zipfile.ZipFile(manual_bundle, "w") as archive:
    archive.writestr("CODEINTEL_AGENT_PROMPT.md", "opt-in\noffline registration\n")
(TEST_ROOT / "artifacts" / "install.sh").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
(TEST_ROOT / "artifacts" / "release-manifest.json").write_text(
    json.dumps(
        {
            "product_version": "1.3.0",
            "artifacts": [
                {
                    "name": manual_bundle.name,
                    "relative_path": manual_bundle.name,
                    "platform": "linux-windows-wsl",
                    "sha256": hashlib.sha256(manual_bundle.read_bytes()).hexdigest(),
                    "size_bytes": manual_bundle.stat().st_size,
                }
            ],
        }
    ),
    encoding="utf-8",
)


def pytest_sessionstart(session) -> None:  # type: ignore[no-untyped-def]
    from alembic import command
    from alembic.config import Config

    config = Config(str(Path(__file__).resolve().parents[2] / "alembic.ini"))
    command.upgrade(config, "head")


def pytest_sessionfinish(session, exitstatus) -> None:  # type: ignore[no-untyped-def]
    shutil.rmtree(TEST_ROOT, ignore_errors=True)
