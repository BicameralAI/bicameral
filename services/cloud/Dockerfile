# Base image pinned by digest (SOC2 supply-chain control #43). Tag: python:3.11-slim
FROM python:3.11-slim@sha256:3f71c419ae1e0459df63ff527a391ee622d237c0fe551137edff2ebb1b2dcb5a

ENV PYTHONUNBUFFERED=1

WORKDIR /app

# README is required by package metadata at build time, but it is contributor-facing
# repository material and must not remain in the customer runtime filesystem.
COPY pyproject.toml README.md requirements.lock ./
COPY src ./src

# Install fully pinned, hash-verified dependencies from the lockfile (SOC2
# supply-chain control #43 — reproducible builds), then the app itself without
# re-resolving deps. Remove all build-only source/metadata before the final image
# is used so repository governance text cannot become customer runtime content.
RUN python -m pip install --no-cache-dir --upgrade pip \
    && python -m pip install --no-cache-dir --require-hashes -r requirements.lock \
    && python -m pip install --no-cache-dir --no-deps . \
    && rm -rf /app/README.md /app/pyproject.toml /app/requirements.lock /app/src \
    && useradd --create-home --uid 10001 appuser \
    && chown -R appuser:appuser /app

# Run as non-root (SOC2 hardening control #43)
USER appuser

EXPOSE 8080

CMD ["uvicorn", "bicameral_cloud.app:app", "--host", "0.0.0.0", "--port", "8080"]
