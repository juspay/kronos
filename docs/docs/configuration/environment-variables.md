---
id: environment-variables
title: Environment Variables
---

# Environment Variables

All Kronos configuration is via environment variables prefixed with `TE_` (Task Executor). This page is a complete reference of every supported variable, grouped by category.

:::info
The `just` task runner has `set dotenv-load` enabled, so variables defined in a `.env` file at the project root are automatically loaded. See `.env.example` for a template.
:::

## Database

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_DATABASE_URL` | *(required)* | PostgreSQL connection string (e.g. `postgresql://user:pass@host:5432/db`). Required for both API and worker. |
| `TE_DB_POOL_SIZE` | `50` | Maximum number of connections in the database connection pool. |
| `TE_TABLE_PREFIX` | *(empty)* | Prefix for all per-workspace Kronos tables. Set to e.g. `sched` to get `sched_jobs`, `sched_executions`, etc. Only alphanumeric and underscore characters allowed. |

:::warning
`TE_DATABASE_URL` is a **sensitive** variable. When KMS is enabled (`TE_KMS_ENABLED=true`), this must contain a base64-encoded KMS-encrypted ciphertext, not a plaintext connection string. See [AWS KMS Integration](../deployment/kms).
:::

## API Server

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_LISTEN_ADDR` | `0.0.0.0:8080` | Bind address and port for the API server. |
| `TE_API_KEY` | `dev-api-key` | Bearer token for API authentication. All requests require `Authorization: Bearer <api_key>`. |
| `TE_PATH_PREFIX` | *(empty)* | URL path prefix for all API routes (e.g. `/kronos`). When set, routes become `/kronos/health`, `/kronos/v1/jobs`, etc. `GET /` returns a `302` redirect to `{prefix}/health`. |
| `TE_MODE` | `api` | Server mode: `api` (REST API only), `dashboard` (dashboard only), or `both` (API + dashboard SSR). |

:::warning
`TE_API_KEY` is a **sensitive** variable. When KMS is enabled, this must contain base64-encoded KMS-encrypted ciphertext. In production, always set this to a strong, unique key — never use the default `dev-api-key`.
:::

### Path prefix

When `TE_PATH_PREFIX` is set, the prefix is normalized (leading/trailing slashes stripped, then prepended with `/`). All API routes, health check, and metrics endpoint are served under the prefix:

```bash
TE_PATH_PREFIX=/kronos ./kronos-api

# Routes:
# GET  /kronos/health
# GET  /kronos/metrics
# POST /kronos/v1/jobs
# GET  /kronos/v1/jobs/{id}
# ...
```

When a prefix is configured and `TE_MODE=both` with a dashboard prefix, `GET /` redirects to the dashboard. Otherwise, `GET /` redirects to `{prefix}/health`.

## Dashboard

These variables are **compile-time** env vars baked into the WASM binary. They must be set when building the dashboard (`just dashboard-build`), not just at runtime.

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_DASHBOARD_PATH_PREFIX` | *(empty)* | URL prefix for dashboard routes (e.g. `/dashboard`). |
| `TE_DASHBOARD_DIST_DIR` | `./dashboard-dist` | Filesystem path to the directory containing the built dashboard WASM bundle and assets. Set to `crates/dashboard/pkg` for local dev. |
| `TE_API_BASE_URL` | *(empty)* | Full API base URL including path prefix. Must include `TE_PATH_PREFIX` if set (e.g. `http://localhost:8080/kronos`). |

:::important
`TE_API_BASE_URL` must include the `TE_PATH_PREFIX` if one is set. For example, if `TE_PATH_PREFIX=/kronos`, then `TE_API_BASE_URL` should be `http://localhost:8080/kronos`.
:::

## Worker

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_WORKER_MAX_CONCURRENT` | `50` | Maximum number of concurrent job executions per worker instance. Enforced via a semaphore. |
| `TE_WORKER_POLL_INTERVAL_MS` | `200` | Interval (in milliseconds) between worker database polls for new executions. |
| `TE_WORKER_SHUTDOWN_TIMEOUT_SEC` | `30` | Graceful shutdown timeout. The worker waits this long for in-flight executions to complete before forcing shutdown. |
| `TE_CONFIG_CACHE_TTL_SEC` | `60` | TTL (in seconds) for the config cache in the worker. Configs are cached in a `DashMap` and refreshed after this interval. |
| `TE_SECRET_CACHE_TTL_SEC` | `300` | TTL (in seconds) for the secret cache in the worker. Secrets are decrypted in memory and cached for this duration. |

### Worker concurrency tuning

The `TE_WORKER_MAX_CONCURRENT` semaphore limits how many executions can be in-flight simultaneously. Each poll iteration:

1. Acquires a semaphore permit (if available)
2. Iterates all active workspace schemas
3. Attempts to claim one execution via `SELECT FOR UPDATE SKIP LOCKED`
4. If no work is found, releases the permit and backs off to `TE_WORKER_POLL_INTERVAL_MS`

If all permits are in use, the worker stops polling until one frees up.

:::tip
When scaling workers horizontally, keep `TE_WORKER_MAX_CONCURRENT` per instance low enough that `(instances × max_concurrent)` does not overwhelm your downstream endpoints or exceed PostgreSQL's `max_connections`.
:::

## Encryption

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_ENCRYPTION_KEY` | `0000...0000` (64 zeros) | AES-256-GCM encryption key for secrets, as a hex string (32 bytes = 64 hex chars). Used to encrypt/decrypt secret values at rest. |

:::danger
`TE_ENCRYPTION_KEY` is a **sensitive** variable. When KMS is enabled, this must contain base64-encoded KMS-encrypted ciphertext.

**In production, always set this to a strong, random 32-byte key.** The default all-zeros key provides no security. If the key is rotated, existing secrets encrypted with the old key cannot be decrypted.
:::

:::warning
`TE_ENCRYPTION_KEY` is also a **sensitive** variable subject to KMS decryption. See [AWS KMS Integration](../deployment/kms).
:::

## Metrics

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_METRICS_PORT` | `9090` | Port for the worker's Prometheus metrics HTTP listener. The API server serves metrics at `GET /metrics` (or `{prefix}/metrics`) on its main port. |

## Reaper

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_REAPER_CRON_EXPRESSION` | `*/15 * * * *` | 5-field pg_cron expression controlling how often Kronos's own dogfooded reaper fires per workspace. Baked into each workspace's pg_cron entry at creation time, so changing this only affects workspaces created afterward. |

:::info
The reaper is Kronos's own CRON sweep that retires expired CRON jobs and unschedules their pg_cron entries. The expression is validated at startup as a 5-field `PgCronExpr` — a typo will cause the server to fail fast rather than breaking the first `POST /workspaces` call.
:::

## Scheduler

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_CRON_TICK_INTERVAL_SEC` | `1` | Interval (in seconds) for the scheduler's CRON materializer tick. |
| `TE_CRON_BATCH_SIZE` | `100` | Maximum number of CRON jobs to process per scheduler tick. |
| `TE_PROMOTE_INTERVAL_MS` | `500` | Interval (in milliseconds) for promoting PENDING (delayed) executions to QUEUED. |
| `TE_RECLAIM_INTERVAL_SEC` | `30` | Interval (in seconds) for reclaiming stuck executions (executions in RUNNING status beyond the timeout). |
| `TE_STUCK_EXECUTION_TIMEOUT_SEC` | `300` | Timeout (in seconds) after which a RUNNING execution is considered stuck and eligible for reclaiming. |

## KMS

These variables control AWS KMS integration for encrypting sensitive environment variables at rest.

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_KMS_ENABLED` | `false` | When `true`, `TE_DATABASE_URL`, `TE_API_KEY`, and `TE_ENCRYPTION_KEY` are expected to be base64-encoded KMS-encrypted ciphertext. Requires the `kms` Cargo feature. |
| `AWS_ENDPOINT_URL` | *(unset)* | KMS endpoint URL. Set to `http://localhost:4566` for LocalStack dev. Omit for production AWS. |
| `AWS_REGION` | *(unset)* | AWS region for KMS (e.g. `us-east-1`). |
| `AWS_ACCESS_KEY_ID` | *(unset)* | AWS access key ID. Use `test` for LocalStack. |
| `AWS_SECRET_ACCESS_KEY` | *(unset)* | AWS secret access key. Use `test` for LocalStack. |

:::warning
If `TE_KMS_ENABLED=true` but the binary was compiled without the `kms` feature, the server will fail to start. Build with `cargo build --features kms` or pass `--build-arg FEATURES=kms` for Docker.
:::

See [AWS KMS Integration](../deployment/kms) for setup instructions.

## CLI / Test Scripts

These variables are used by the TypeScript CLI and test scripts (in `cli/`). They are **not** read by the Rust binaries.

| Variable | Default | Description |
|----------|---------|-------------|
| `KRONOS_URL` | *(unset)* | Base URL for the Kronos API. Must include `TE_PATH_PREFIX` if set (e.g. `http://localhost:8080/kronos`). |
| `KRONOS_API_KEY` | *(unset)* | API key for authentication (same as `TE_API_KEY`). |
| `KRONOS_ORG_ID` | *(unset)* | Organization UUID, set after creating an org via the API. |
| `KRONOS_WORKSPACE_ID` | *(unset)* | Workspace UUID, set after creating a workspace via the API. |

:::note
`KRONOS_URL` and `KRONOS_API_KEY` correspond to `TE_PATH_PREFIX` and `TE_API_KEY` respectively. The CLI uses the `KRONOS_` prefix to avoid confusion with the server's `TE_` variables.
:::

## Sensitive variables and KMS

The following variables are treated as **sensitive** by Kronos. When KMS is enabled (`TE_KMS_ENABLED=true`), they are transparently decrypted at startup:

| Variable | Sensitive? | KMS-decrypted? |
|----------|-----------|----------------|
| `TE_DATABASE_URL` | Yes | Yes |
| `TE_API_KEY` | Yes | Yes |
| `TE_ENCRYPTION_KEY` | Yes | Yes |
| All other `TE_*` variables | No | No |

## Path prefix summary

The path prefix affects the API server (runtime), the dashboard (compile-time), and monitoring configuration:

| Component | Variable | When to set | Type |
|-----------|----------|-------------|------|
| API server | `TE_PATH_PREFIX` | Runtime (env var or `.env`) | Runtime |
| Dashboard | `TE_DASHBOARD_PATH_PREFIX` | Build time (`just dashboard-build`) | Compile-time (baked into WASM) |
| Dashboard | `TE_API_BASE_URL` | Build time (`just dashboard-build`) | Compile-time (baked into WASM) |
| Prometheus | `metrics_path` in `prometheus.yml` | Config file | Config file |
| Docker healthcheck | Health check URL in compose file | Config file | Config file |

### Monitoring config updates

When using a path prefix, update these monitoring configs to match:

**Prometheus** (`monitoring/prometheus.yml`):
```yaml
scrape_configs:
  - job_name: "kronos-api"
    metrics_path: /kronos/metrics   # was /metrics
    static_configs:
      - targets: ["host.docker.internal:8080"]
```

**Docker healthchecks** (`docker-compose.prod.yml`):
```yaml
healthcheck:
  test: ["CMD-SHELL", "curl -sf http://localhost:8080/kronos/health"]
  # was: http://localhost:8080/health
```

## Complete .env.example

```bash
# KMS (requires 'kms' feature: cargo build --features kms)
TE_KMS_ENABLED=false
# AWS_ENDPOINT_URL=http://localhost:4566
# AWS_REGION=us-east-1
# AWS_ACCESS_KEY_ID=test
# AWS_SECRET_ACCESS_KEY=test

# Database
TE_DATABASE_URL=postgresql://kronos:kronos@localhost:5432/taskexecutor

# API Server
TE_LISTEN_ADDR=0.0.0.0:8080
# TE_PATH_PREFIX=/kronos
TE_DB_POOL_SIZE=20

# Worker
TE_WORKER_MAX_CONCURRENT=50
TE_WORKER_POLL_INTERVAL_MS=200
TE_CONFIG_CACHE_TTL_SEC=60
TE_SECRET_CACHE_TTL_SEC=300
TE_WORKER_SHUTDOWN_TIMEOUT_SEC=30

# Reaper
# TE_REAPER_CRON_EXPRESSION=*/15 * * * *

# Scheduler
TE_CRON_TICK_INTERVAL_SEC=1
TE_CRON_BATCH_SIZE=100
TE_PROMOTE_INTERVAL_MS=500
TE_RECLAIM_INTERVAL_SEC=30
TE_STUCK_EXECUTION_TIMEOUT_SEC=300

# Encryption
TE_ENCRYPTION_KEY=0000000000000000000000000000000000000000000000000000000000000000

# API Key
TE_API_KEY=dev-api-key

# Dashboard (compile-time env vars for WASM build)
# TE_DASHBOARD_PATH_PREFIX=/dashboard
# TE_API_BASE_URL=http://localhost:8080/kronos

# CLI / Test Scripts
# KRONOS_URL=http://localhost:8080
# KRONOS_API_KEY=dev-api-key
# KRONOS_ORG_ID=<org uuid>
# KRONOS_WORKSPACE_ID=<workspace uuid>
```

## See also

- [Docker](../deployment/docker) — Docker build and compose configuration
- [Production Deployment](../deployment/production) — production tuning and scaling
- [AWS KMS Integration](../deployment/kms) — encrypting sensitive variables
- [Dashboard](../deployment/dashboard) — dashboard path prefix configuration
- [Development Setup](../development/setup) — setting up a dev environment
