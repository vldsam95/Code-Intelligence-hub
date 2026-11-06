from __future__ import annotations

from functools import lru_cache
from pathlib import Path

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="CODEINTEL_", env_file=".env", extra="ignore", case_sensitive=False
    )

    env: str = "development"
    base_url: str = "http://localhost:8000"
    database_url: str = "sqlite:////tmp/codeintel-hub.db"
    artifacts_dir: Path = Path("/artifacts")
    docs_dir: Path = Path("/app/docs")
    master_key_file: Path = Path("/run/codeintel-secrets/hub-master.key")
    owner_username: str = "admin"
    owner_password_hash: str = ""
    session_secret: str = ""
    token_hmac_key: str = ""
    trusted_hosts: str = "localhost,127.0.0.1"
    secure_cookies: bool = True
    log_level: str = "INFO"
    hub_version: str = "1.3.0"
    agent_policy_version: int = 4
    worker_mcp_schema_version: int = 1
    hub_mcp_schema_version: int = 1
    project_manifest_schema_version: int = 1
    login_attempts: int = 5
    login_window_sec: int = 900
    session_idle_sec: int = 43_200
    session_absolute_sec: int = 604_800
    max_api_body_bytes: int = 2 * 1024 * 1024
    long_poll_hold_sec: int = Field(default=25, ge=1, le=30)

    @property
    def trusted_host_list(self) -> list[str]:
        return [part.strip() for part in self.trusted_hosts.split(",") if part.strip()]

    @property
    def production(self) -> bool:
        return self.env.lower() == "production"


@lru_cache
def get_settings() -> Settings:
    return Settings()
