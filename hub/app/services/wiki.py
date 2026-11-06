from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class WikiContext:
    status: str
    summary: str | None = None
    links: list[str] = field(default_factory=list)
    unknowns: list[str] = field(default_factory=list)


class WikiConnector(ABC):
    """Optional adapter contract. Code quality never depends on this interface."""

    @abstractmethod
    async def detect_capabilities(self) -> dict[str, Any]: ...

    @abstractmethod
    async def get_context(self, project_binding: dict[str, Any]) -> WikiContext: ...

    @abstractmethod
    async def get_update_instructions(self) -> dict[str, Any]: ...

    @abstractmethod
    async def submit_durable_update(self, payload: dict[str, Any]) -> str: ...

    @abstractmethod
    async def get_update_status(self, update_id: str) -> str: ...

    @abstractmethod
    async def health(self) -> dict[str, Any]: ...


class NoWikiConnector(WikiConnector):
    async def detect_capabilities(self) -> dict[str, Any]:
        return {"enabled": False, "mode": "none"}

    async def get_context(self, project_binding: dict[str, Any]) -> WikiContext:
        return WikiContext(status="NOT_CONFIGURED")

    async def get_update_instructions(self) -> dict[str, Any]:
        return {"required": False, "reason": "wiki.mode=none"}

    async def submit_durable_update(self, payload: dict[str, Any]) -> str:
        return "NOT_ATTEMPTED"

    async def get_update_status(self, update_id: str) -> str:
        return "NOT_ATTEMPTED"

    async def health(self) -> dict[str, Any]:
        return {"status": "DISABLED", "core_available": True}
