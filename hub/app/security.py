from __future__ import annotations

import base64
import hashlib
import hmac
import os
import secrets
from dataclasses import dataclass
from datetime import UTC, timedelta
from pathlib import Path

from argon2 import PasswordHasher
from argon2.exceptions import InvalidHashError, VerifyMismatchError
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from fastapi import HTTPException, Request, status
from sqlalchemy import select
from sqlalchemy.orm import Session

from .config import Settings, get_settings
from .models import AgentCredential, BrowserSession, OwnerUser, utcnow

SESSION_COOKIE = "ci_session"
CSRF_COOKIE = "ci_csrf"
password_hasher = PasswordHasher(time_cost=3, memory_cost=65_536, parallelism=2)


@dataclass(frozen=True)
class SessionIdentity:
    owner: OwnerUser
    session: BrowserSession


def hash_password(password: str) -> str:
    return password_hasher.hash(password)


def verify_password(password_hash: str, password: str) -> bool:
    try:
        return password_hasher.verify(password_hash, password)
    except (VerifyMismatchError, InvalidHashError):
        return False


def password_needs_rehash(password_hash: str) -> bool:
    try:
        return password_hasher.check_needs_rehash(password_hash)
    except InvalidHashError:
        return True


def _keyed_hash(value: str, key: str) -> str:
    return hmac.new(key.encode("utf-8"), value.encode("utf-8"), hashlib.sha256).hexdigest()


def _require_secret(value: str, name: str, settings: Settings) -> str:
    if value:
        return value
    if settings.production:
        raise RuntimeError(f"{name} is required in production")
    return f"development-only-{name}-do-not-use"


def session_hash(value: str, settings: Settings | None = None) -> str:
    settings = settings or get_settings()
    key = _require_secret(settings.session_secret, "session-secret", settings)
    return _keyed_hash(value, key)


def token_validation_hash(value: str, settings: Settings | None = None) -> str:
    settings = settings or get_settings()
    key = _require_secret(settings.token_hmac_key, "token-hmac-key", settings)
    return _keyed_hash(value, key)


def source_ip_hash(value: str, settings: Settings | None = None) -> str:
    settings = settings or get_settings()
    key = _require_secret(settings.session_secret, "session-secret", settings)
    return _keyed_hash(value, key)


def create_browser_session(db: Session, owner: OwnerUser) -> tuple[BrowserSession, str, str]:
    settings = get_settings()
    raw_session = secrets.token_urlsafe(48)
    raw_csrf = secrets.token_urlsafe(32)
    now = utcnow()
    row = BrowserSession(
        owner_id=owner.id,
        session_hash=session_hash(raw_session, settings),
        csrf_hash=session_hash(raw_csrf, settings),
        created_at=now,
        last_seen_at=now,
        expires_at=now + timedelta(seconds=settings.session_absolute_sec),
    )
    db.add(row)
    db.commit()
    db.refresh(row)
    return row, raw_session, raw_csrf


def revoke_browser_session(db: Session, raw_session: str | None) -> None:
    if not raw_session:
        return
    row = db.scalar(
        select(BrowserSession).where(BrowserSession.session_hash == session_hash(raw_session))
    )
    if row and not row.revoked_at:
        row.revoked_at = utcnow()
        db.commit()


def authenticate_browser(request: Request, db: Session) -> SessionIdentity | None:
    raw = request.cookies.get(SESSION_COOKIE)
    if not raw:
        return None
    row = db.scalar(select(BrowserSession).where(BrowserSession.session_hash == session_hash(raw)))
    now = utcnow()
    if not row or row.revoked_at:
        return None
    expires_at = row.expires_at
    if expires_at.tzinfo is None:
        expires_at = expires_at.replace(tzinfo=UTC)
    if expires_at <= now:
        return None
    last_seen = row.last_seen_at
    if last_seen.tzinfo is None:
        last_seen = last_seen.replace(tzinfo=UTC)
    if last_seen + timedelta(seconds=get_settings().session_idle_sec) <= now:
        row.revoked_at = now
        db.commit()
        return None
    owner = db.get(OwnerUser, row.owner_id)
    if not owner or not owner.is_active:
        return None
    if (now - last_seen).total_seconds() > 300:
        row.last_seen_at = now
        db.commit()
    return SessionIdentity(owner=owner, session=row)


def verify_csrf(request: Request, identity: SessionIdentity, submitted: str | None) -> None:
    cookie = request.cookies.get(CSRF_COOKIE)
    if not submitted or not cookie or not hmac.compare_digest(submitted, cookie):
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="CSRF validation failed")
    if not hmac.compare_digest(session_hash(submitted), identity.session.csrf_hash):
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="CSRF validation failed")


class TokenVault:
    def __init__(self, key: bytes, key_id: str = "v1") -> None:
        if len(key) != 32:
            raise ValueError("Hub master key must contain exactly 32 bytes")
        self._cipher = AESGCM(key)
        self.key_id = key_id

    @classmethod
    def from_file(cls, path: Path) -> TokenVault:
        file_bytes = path.read_bytes()
        # A raw 32-byte key is binary data: stripping it can silently remove a leading/trailing
        # whitespace-valued byte and make a valid production key intermittently unreadable.
        raw = file_bytes if len(file_bytes) == 32 else file_bytes.strip()
        if len(raw) != 32:
            try:
                raw = base64.urlsafe_b64decode(raw)
            except Exception as exc:
                raise RuntimeError("Invalid master-key encoding") from exc
        return cls(raw)

    def encrypt(self, secret: str, associated_data: bytes) -> tuple[bytes, bytes]:
        nonce = os.urandom(12)
        return self._cipher.encrypt(nonce, secret.encode("utf-8"), associated_data), nonce

    def decrypt(self, encrypted: bytes, nonce: bytes, associated_data: bytes) -> str:
        return self._cipher.decrypt(nonce, encrypted, associated_data).decode("utf-8")


def get_vault(settings: Settings | None = None) -> TokenVault:
    settings = settings or get_settings()
    if not settings.master_key_file.exists():
        if settings.production:
            raise RuntimeError("Hub master encryption key file is missing")
        development_key = hashlib.sha256(b"codeintel-development-vault-key").digest()
        return TokenVault(development_key, "development")
    return TokenVault.from_file(settings.master_key_file)


TOKEN_PREFIXES = {
    "WORKER_ENROLLMENT": "ci_enroll_v1_",
    "WORKER_CREDENTIAL": "ci_worker_v1_",
    "HOST_ACCESS": "ci_host_v1_",
    "AGENT_HUB_READ": "ci_read_v1_",
    "AGENT_LIVE_ANALYSIS": "ci_live_v1_",
    "PROJECT_AGENT": "ci_proj_v1_",
    "ADMIN_API": "ci_admin_v1_",
    "WIKI_CONNECTION": "ci_wiki_v1_",
}


def create_token_secret(token_type: str) -> str:
    return TOKEN_PREFIXES[token_type] + secrets.token_urlsafe(32)


def encrypt_credential_secret(row_id: str, secret: str) -> tuple[bytes, bytes, str]:
    vault = get_vault()
    encrypted, nonce = vault.encrypt(secret, row_id.encode("ascii"))
    return encrypted, nonce, vault.key_id


def decrypt_credential_secret(row: AgentCredential) -> str:
    vault = get_vault()
    if row.encryption_key_id != vault.key_id and row.encryption_key_id != "v1":
        raise RuntimeError("The credential requires an unavailable encryption key")
    return vault.decrypt(row.encrypted_secret, row.encryption_nonce, row.id.encode("ascii"))


def authenticate_bearer(
    request: Request, db: Session, required_scope: str | None = None
) -> AgentCredential:
    authorization = request.headers.get("authorization", "")
    if not authorization.startswith("Bearer "):
        raise HTTPException(status_code=401, detail="Bearer credential required")
    raw = authorization[7:].strip()
    if not raw:
        raise HTTPException(status_code=401, detail="Bearer credential required")
    digest = token_validation_hash(raw)
    credential = db.scalar(select(AgentCredential).where(AgentCredential.validation_hash == digest))
    now = utcnow()
    if not credential or credential.status != "ACTIVE":
        raise HTTPException(status_code=401, detail="Invalid or revoked credential")
    if credential.expires_at:
        expiry = credential.expires_at
        if expiry.tzinfo is None:
            expiry = expiry.replace(tzinfo=UTC)
        if expiry <= now:
            raise HTTPException(status_code=401, detail="Credential expired")
    if (
        required_scope
        and required_scope not in credential.scopes
        and "admin" not in credential.scopes
    ):
        raise HTTPException(status_code=403, detail="Credential scope denied")
    credential.last_used_at = now
    if credential.one_time:
        credential.status = "USED"
    db.commit()
    return credential


def authenticate_raw_token(
    db: Session,
    raw: str,
    *,
    required_type: str | None = None,
    required_scope: str | None = None,
    consume_one_time: bool = True,
) -> AgentCredential:
    digest = token_validation_hash(raw)
    credential = db.scalar(select(AgentCredential).where(AgentCredential.validation_hash == digest))
    now = utcnow()
    if not credential or credential.status != "ACTIVE":
        raise HTTPException(status_code=401, detail="Invalid or revoked credential")
    if required_type and credential.token_type != required_type:
        raise HTTPException(status_code=403, detail="Credential type denied")
    if credential.expires_at:
        expiry = credential.expires_at
        if expiry.tzinfo is None:
            expiry = expiry.replace(tzinfo=UTC)
        if expiry <= now:
            raise HTTPException(status_code=401, detail="Credential expired")
    if (
        required_scope
        and required_scope not in credential.scopes
        and "admin" not in credential.scopes
    ):
        raise HTTPException(status_code=403, detail="Credential scope denied")
    credential.last_used_at = now
    if credential.one_time and consume_one_time:
        credential.status = "USED"
    db.commit()
    return credential
