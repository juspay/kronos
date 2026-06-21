---
id: production
title: Production Deployment
---

# Production Deployment

This page covers deploying Kronos in a production-like configuration using Docker Compose, with guidance on scaling, tuning, and health checks.

## docker-compose.prod.yml

The `docker-compose.prod.yml` file defines a complete prod-like environment with all services on a shared Docker network (`kronos-net`) and KMS encryption enabled:

| Service | Image / Build | Port | Purpose |
|---------|---------------|------|---------|
| `postgres` | `docker/postgres` (custom, pg_cron) | 5432 | PostgreSQL database with health check |
| `localstack` | `localstack/localstack:3` | 4566 | LocalStack KMS for at-rest encryption of sensitive env vars |
| `kronos-mock-server` | Built from `Dockerfile` | 9999 | Mock HTTP server (used for testing) |
| `kronos-server` | Built from `Dockerfile` (API + dashboard + KMS) | 8080 | API server in `both` mode with path prefix and dashboard |
| `kronos-worker` | Built from `Dockerfile` (KMS) | 9090 | Worker with metrics listener |

### Key differences from dev compose

- All services are on a shared `kronos-net` bridge network (no `host.docker.internal` needed)
- PostgreSQL uses port **5432** (not 5434)
- The API server is built with `FEATURES=kms` and `INCLUDE_DASHBOARD=true`
- The worker is built with `FEATURES=kms`
- The API server runs in `TE_MODE=both` with `TE_PATH_PREFIX=/kronos` and `TE_DASHBOARD_PATH_PREFIX=/dashboard`
- All services have health checks with retry logic
- Service dependencies use `condition: service_healthy` for ordered startup

## Running the prod-like environment

### Automated startup

```bash
just docker-prod-up
```

This runs `scripts/docker-prod.sh`, which performs a two-phase startup:

**Phase 1 — Infrastructure:**
1. Builds all Docker images
2. Starts PostgreSQL, LocalStack, and the mock server
3. Waits for PostgreSQL and LocalStack to become healthy
4. Runs all database migrations

**Phase 2 — KMS + Application:**
1. Creates a KMS key on LocalStack
2. Encrypts `TE_DATABASE_URL`, `TE_API_KEY`, and `TE_ENCRYPTION_KEY` using the KMS key
3. Writes the encrypted values to `.env.prod.kms`
4. Starts the API server and worker
5. Waits for the API and dashboard health checks to pass

### Manual startup

If you prefer to drive each step:

```bash
# Build all images
docker compose -f docker-compose.prod.yml build

# Start infrastructure
docker compose -f docker-compose.prod.yml up -d postgres localstack kronos-mock-server

# Wait for PostgreSQL
docker compose -f docker-compose.prod.yml exec -T postgres pg_isready -U kronos -d taskexecutor

# Run migrations (migrations are mounted at /migrations inside the postgres container)
docker compose -f docker-compose.prod.yml exec -T postgres sh -c '
  PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor -f /migrations/20260317000000_initial.sql &&
  PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor -f /migrations/20260318000000_multi_tenancy.sql &&
  PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor -f /migrations/20260322000000_txn_based_pickup.sql &&
  PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor -f /migrations/20260322000001_pg_cron.sql
'

# Create KMS key and encrypt values (see KMS Integration page for details)
# Then start app services:
docker compose -f docker-compose.prod.yml up -d kronos-server kronos-worker
```

### Teardown

```bash
just docker-prod-down
```

Or manually:

```bash
docker compose -f docker-compose.prod.yml down
```

### Accessing services

After startup, the following endpoints are available:

| Service | URL |
|---------|-----|
| API | `http://localhost:8080/kronos` |
| Dashboard | `http://localhost:8080/dashboard` |
| Mock Server | `http://localhost:9999` |
| Worker Metrics | `http://localhost:9090/metrics` |
| PostgreSQL | `localhost:5432` |
| LocalStack KMS | `http://localhost:4566` |

## Production checklist

Before deploying to production, ensure the following are configured:

### Security

- [ ] **`TE_API_KEY`** — set to a strong, unique API key (not `dev-api-key`)
- [ ] **`TE_ENCRYPTION_KEY`** — set to a valid 32-byte (64 hex character) AES-256 key (not the default all-zeros key)
- [ ] **`TE_DATABASE_URL`** — point to your production PostgreSQL instance with appropriate credentials
- [ ] **KMS enabled** — set `TE_KMS_ENABLED=true` and encrypt `TE_DATABASE_URL`, `TE_API_KEY`, and `TE_ENCRYPTION_KEY` via AWS KMS. See [AWS KMS Integration](./kms).
- [ ] **PostgreSQL credentials** — use strong passwords, not the default `kronos:kronos`

### Database

- [ ] PostgreSQL 16+ with `pg_cron` extension installed
- [ ] `shared_preload_libraries=pg_cron` set in PostgreSQL config
- [ ] `cron.database_name` set to your database name
- [ ] All four migrations applied in order
- [ ] Connection pool sized appropriately (see below)

### Application

- [ ] `TE_MODE` set to `both` if serving the dashboard, or `api` for API-only
- [ ] `TE_PATH_PREFIX` configured if running behind a reverse proxy
- [ ] `TE_DASHBOARD_PATH_PREFIX` and `TE_API_BASE_URL` set if using the dashboard
- [ ] Health checks configured for all services
- [ ] Prometheus scraping configured (see [Monitoring](../guides/monitoring))
- [ ] Grafana dashboards imported

### Worker tuning

- [ ] `TE_WORKER_MAX_CONCURRENT` tuned for your workload
- [ ] `TE_WORKER_POLL_INTERVAL_MS` set (default 200ms is good for most cases)
- [ ] `TE_WORKER_SHUTDOWN_TIMEOUT_SEC` set to allow graceful draining
- [ ] Multiple worker instances deployed for high availability

## Scaling workers

### Horizontal scaling

Workers are stateless and can be scaled horizontally. Simply run multiple worker instances pointing at the same database:

```bash
# Worker instance 1
TE_DATABASE_URL=postgresql://kronos:kronos@db:5432/taskexecutor \
TE_METRICS_PORT=9090 \
  ./kronos-worker

# Worker instance 2
TE_DATABASE_URL=postgresql://kronos:kronos@db:5432/taskexecutor \
TE_METRICS_PORT=9091 \
  ./kronos-worker
```

Job distribution is handled by PostgreSQL's `SELECT FOR UPDATE SKIP LOCKED`, which ensures each execution is claimed by exactly one worker — no coordination layer needed.

:::tip
When running multiple workers behind a load balancer or in Kubernetes, give each worker a unique `TE_METRICS_PORT` (or use a sidecar pattern) so Prometheus can scrape each instance individually.
:::

### Vertical scaling (concurrency tuning)

Each worker uses a semaphore to limit concurrent in-flight executions. Tune `TE_WORKER_MAX_CONCURRENT` based on:

- **CPU/memory** of the worker host
- **Database connection pool size** — each concurrent execution may hold a DB connection
- **Downstream endpoint capacity** — don't overwhelm the services you're calling

```bash
# Higher concurrency for lightweight HTTP endpoints
TE_WORKER_MAX_CONCURRENT=100 ./kronos-worker

# Lower concurrency for heavy Kafka/Redis workloads
TE_WORKER_MAX_CONCURRENT=25 ./kronos-worker
```

## Connection pool sizing

The database connection pool is controlled by `TE_DB_POOL_SIZE` (default: 50). Each connection is a persistent PostgreSQL connection managed by `sqlx`.

### Guidelines

| Deployment | Pool Size | Rationale |
|------------|----------|-----------|
| Dev (single API + worker) | 20 | Sufficient for local testing |
| Small prod (1 API + 1 worker) | 50 | Default, handles moderate load |
| Scaled prod (1 API + N workers) | 20–30 per instance | Avoid exceeding PostgreSQL's `max_connections` |

:::warning
When running multiple API and worker instances, the total number of connections across all instances must not exceed PostgreSQL's `max_connections` setting (default 100). Calculate: `(API instances × TE_DB_POOL_SIZE) + (worker instances × TE_WORKER_MAX_CONCURRENT)` and ensure it stays within limits.
:::

## Path prefix for reverse proxy deployments

When running behind a reverse proxy (e.g., Nginx, Traefik, AWS ALB), set `TE_PATH_PREFIX` to serve all API routes under a prefix:

```bash
TE_PATH_PREFIX=/kronos ./kronos-api
```

All routes become:
- `GET /kronos/health`
- `POST /kronos/v1/jobs`
- `GET /kronos/metrics`
- etc.

When a prefix is configured, `GET /` returns a `302` redirect to `{prefix}/health`.

:::info
When using the dashboard alongside a path prefix, the dashboard's compile-time env vars must also be set. See [Dashboard](./dashboard) for details.
:::

## Health checks

### API server health

```bash
curl http://localhost:8080/health          # no prefix
curl http://localhost:8080/kronos/health  # with prefix
```

Returns `OK` (200) when the server is ready.

### Worker health

The worker does not expose a dedicated health endpoint, but its metrics listener serves Prometheus metrics:

```bash
curl http://localhost:9090/metrics
```

If the metrics endpoint responds, the worker is alive.

### Docker health checks

The prod compose file includes health checks for all services:

| Service | Health check command | Interval | Timeout | Retries |
|---------|----------------------|----------|---------|---------|
| `postgres` | `pg_isready -U kronos -d taskexecutor` | 5s | 3s | 10 |
| `localstack` | `curl -sf http://localhost:4566/_localstack/health` | 5s | 3s | 10 |
| `kronos-mock-server` | `curl -sf http://localhost:9999/health` | 5s | 3s | 10 |
| `kronos-server` | `curl -sf http://localhost:8080/kronos/health` | 5s | 3s | 15 |
| `kronos-worker` | *(no health check — relies on metrics port)* | — | — | — |

:::tip
When using a path prefix, update the health check URL in `docker-compose.prod.yml` to include the prefix (e.g., `http://localhost:8080/kronos/health` instead of `http://localhost:8080/health`).
:::

## Graceful shutdown

Workers handle `SIGTERM`/`SIGINT` by draining in-flight executions before exiting. The `TE_WORKER_SHUTDOWN_TIMEOUT_SEC` environment variable controls how long the worker waits for active executions to complete before forcing shutdown:

```bash
# Allow 60 seconds for graceful drain
TE_WORKER_SHUTDOWN_TIMEOUT_SEC=60 ./kronos-worker
```

During shutdown:
1. The worker stops polling for new executions
2. In-flight executions are allowed to complete
3. If the timeout is reached, remaining executions are abandoned (they will be reclaimed by other workers via the stuck execution reclaimer)

:::warning
Set `TE_WORKER_SHUTDOWN_TIMEOUT_SEC` high enough for your longest-running job to complete. If a job takes 45 seconds and the timeout is 30 seconds, the job will be interrupted and retried by another worker.
:::

## Monitoring in production

See the [Monitoring guide](../guides/monitoring) for details on Prometheus metrics and Grafana dashboards. Key production considerations:

- Scrape the API at `{prefix}/metrics` (e.g., `/kronos/metrics`)
- Scrape each worker instance at its `TE_METRICS_PORT`
- Set up alerts for `kronos_worker_poll_idle_total` (no work found for extended periods may indicate a problem)
- Monitor `kronos_worker_inflight_executions` to ensure concurrency isn't maxed out

## See also

- [Docker](./docker) — Dockerfile and dev compose details
- [AWS KMS Integration](./kms) — encrypting sensitive environment variables
- [Dashboard](./dashboard) — building and serving the WASM dashboard
- [Environment Variables](../configuration/environment-variables) — full configuration reference
- [Monitoring](../guides/monitoring) — Prometheus and Grafana setup
