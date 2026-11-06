#!/usr/bin/env python3
"""Tiny acceptance-only HTTP endpoint for exercising Worker offline retry."""

from __future__ import annotations

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class Handler(BaseHTTPRequestHandler):
    output = Path(sys.argv[2])

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def do_GET(self) -> None:  # noqa: N802
        if self.path.endswith("/jobs/next"):
            self.send_response(204)
            self.end_headers()
            return
        self.send_error(404)

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0"))
        payload = json.loads(self.rfile.read(length) or b"{}")
        if self.path.endswith("/host-access/bootstrap"):
            self.respond(
                200,
                {
                    "status": "ACTIVE",
                    "host_access_token": "ci_host_v1_acceptance",
                    "credential_id": "10000000-0000-4000-8000-000000000003",
                },
            )
        elif self.path.endswith("/projects/register"):
            temporary = self.output.with_suffix(".tmp")
            temporary.write_text(json.dumps(payload), encoding="utf-8")
            temporary.replace(self.output)
            self.respond(
                201,
                {
                    "registration_state": "REGISTERED",
                    "project_access": {
                        "token": "ci_proj_v1_acceptance",
                        "credential_id": "10000000-0000-4000-8000-000000000004",
                    },
                },
            )
        elif "/reports/" in self.path:
            self.respond(201, {"accepted": True})
        elif self.path.endswith("/heartbeat"):
            self.respond(200, {"accepted": True})
        elif "/claim" in self.path or self.path.endswith("/result"):
            self.respond(200, {"accepted": True})
        else:
            self.send_error(404)

    def respond(self, status: int, payload: dict[str, object]) -> None:
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
