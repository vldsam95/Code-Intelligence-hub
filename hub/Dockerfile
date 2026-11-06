FROM node:24-alpine AS ui-assets
WORKDIR /ui
COPY hub/package.json hub/package-lock.json /ui/
RUN npm ci --omit=dev --ignore-scripts && cp node_modules/htmx.org/dist/htmx.min.js /tmp/htmx.min.js

FROM python:3.12-slim AS runtime

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1

RUN useradd --system --uid 10001 --create-home --home-dir /app codeintel
WORKDIR /app

COPY pyproject.toml /app/pyproject.toml
COPY LICENSE THIRD_PARTY_NOTICES.md /app/
COPY licenses /app/licenses
COPY hub /app/hub
RUN pip install --constraint /app/licenses/python-constraints.txt .

COPY --from=ui-assets /tmp/htmx.min.js /app/hub/static/htmx.min.js
COPY alembic.ini /app/alembic.ini
COPY docs /app/docs
COPY protocol /app/protocol
COPY adapters /app/adapters
COPY installers /app/installers
COPY release /app/release

RUN mkdir -p /data /artifacts && chown -R codeintel:codeintel /app /data /artifacts && chmod 0755 /app

# The entrypoint starts as container root only long enough to copy the root-only
# bind-mounted vault key into a private tmpfs, then permanently drops to codeintel.
ENTRYPOINT ["python", "/app/hub/entrypoint.py"]

EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD ["python", "-c", "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8000/health/ready', timeout=3)"]
CMD ["uvicorn", "hub.app.main:app", "--host", "0.0.0.0", "--port", "8000", "--proxy-headers", "--forwarded-allow-ips", "*"]
