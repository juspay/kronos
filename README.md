# Kronos

**`setTimeout` and `setInterval` as a service.**

Distributed, durable, retriable, observable delivery of jobs to HTTP endpoints, Kafka topics, and Redis Streams — with type-safety guarantees.

---

## The mental model

If you've written JavaScript, you already know the API.

| What you want | JS | Kronos |
|---|---|---|
| Fire now | `setTimeout(fn, 0)` | `POST /v1/jobs { trigger: IMMEDIATE }` |
| Fire later | `setTimeout(fn, 5000)` | `POST /v1/jobs { trigger: DELAYED, run_at: "..." }` |
| Fire repeatedly | `setInterval(fn, 60000)` | `POST /v1/jobs { trigger: CRON, cron: "* * * * *" }` |
| Cancel | `clearTimeout(id)` | `POST /v1/jobs/{id}/cancel` |

Except: it survives crashes, retries on failure, never fires twice, and every execution is observable.

---

## Architecture

```
                              ┌─────────────────────────┐
                              │        Client / SDK      │
                              └────────────┬────────────┘
                                           │
                                    POST /v1/jobs
                                           │
                              ┌────────────▼────────────┐
                              │    API Server (actix)    │
                              │       port 8080          │
                              └────────────┬────────────┘
                                           │
                             INSERT job + execution (txn)
                                           │
                              ┌────────────▼────────────┐
                              │      PostgreSQL          │
                              │  (source of truth)       │
                              └──┬──────────┬──────────┬┘
                                 │          │          │
            ┌────────────────────▼┐  ┌──────▼───────┐  ┌▼────────────────────┐
            │     Scheduler       │  │   Worker      │  │   Dashboard (WASM)  │
            │  (3 loops)          │  │   Pool        │  │   Leptos + Trunk    │
            │                     │  │               │  └─────────────────────┘
            │  CRON materializer  │  │  SELECT FOR   │
            │    (every 1s)       │  │  UPDATE SKIP  │
            │                     │  │  LOCKED       │
            │  Delayed promoter   │  │               │
            │    (every 500ms)    │  │  ┌──────────┐ │
            │                     │  │  │ HTTP     │ │
            │  Stuck reclaimer    │  │  │ Kafka    │ │
            │    (every 30s)      │  │  │ Redis    │ │
            └─────────────────────┘  │  └──────────┘ │
                                     └───────────────┘
```

### Crates

| Crate | Description |
|-------|-------------|
| `kronos-common` | Shared library — models, DB layer, config, tenant management, caching |
| `kronos-api` | REST API server (actix-web). Handles all CRUD and job invocations |
| `kronos-worker` | Execution engine. Polls DB, resolves templates, dispatches to endpoints |
| `kronos-scheduler` | Three background loops: CRON materializer, delayed promoter, stuck reclaimer |
| `kronos-mock-server` | Test fixture — HTTP server on port 9999 for integration tests |
| `kronos-dashboard` | Web UI — Leptos/WASM, shows jobs, executions, attempts |

### Multi-tenancy

Kronos uses **schema-per-tenant** isolation. Each workspace gets its own PostgreSQL schema with isolated tables for endpoints, jobs, executions, etc. Shared tables (organizations, workspaces) live in the `public` schema.

```
public schema:        organizations, workspaces
tenant schema:        payload_specs, configs, secrets, endpoints,
(org_workspace):      jobs, executions, attempts, execution_logs
```

---

## Quickstart

### Prerequisites

- [Nix](https://nixos.org/download) with flakes enabled
- [Docker](https://docs.docker.com/get-docker/) (for PostgreSQL)

### Setup

```bash
# Enter the dev shell (installs Rust, Node.js, smithy-cli, just, etc.)
nix develop

# One-time setup: start DB, run migrations, build SDK, install CLI deps
just setup

# Run all services (API + worker + scheduler + mock-server)
just dev
```

The API is now running at `http://localhost:8080`.

### Verify

```bash
curl http://localhost:8080/health
# OK
```

---

## Usage

All endpoints require `Authorization: Bearer <api_key>` (default: `dev-api-key`).

### 1. Setup — define input contracts, configs, and secrets

```bash
# Create a JSON Schema for input validation
curl -X POST http://localhost:8080/v1/payload-specs \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "order-input",
    "schema": {
      "type": "object",
      "properties": {
        "order_id": { "type": "string" },
        "user_id": { "type": "string" }
      },
      "required": ["order_id"]
    }
  }'

# Create configs (static variables)
curl -X POST http://localhost:8080/v1/configs \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "email-service",
    "values": {
      "api_base_url": "https://api.myapp.com",
      "sender": "noreply@myapp.com"
    }
  }'

# Create secrets (encrypted at rest, write-only)
curl -X POST http://localhost:8080/v1/secrets \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "email_api_key",
    "value": "sk-your-api-key"
  }'
```

### 2. Register — tell Kronos where to deliver

```bash
curl -X POST http://localhost:8080/v1/endpoints \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "send-welcome-email",
    "type": "HTTP",
    "payload_spec": "order-input",
    "config": "email-service",
    "spec": {
      "url": "{{config.api_base_url}}/emails/welcome",
      "method": "POST",
      "headers": {
        "Authorization": "Bearer {{secret.email_api_key}}",
        "Content-Type": "application/json"
      },
      "body_template": {
        "order_id": "{{input.order_id}}",
        "sender": "{{config.sender}}"
      },
      "timeout_ms": 5000,
      "expected_status_codes": [200, 201, 202, 204]
    },
    "retry_policy": {
      "max_attempts": 3,
      "backoff": "exponential",
      "initial_delay_ms": 1000,
      "max_delay_ms": 30000
    }
  }'
```

Endpoint types: `HTTP`, `KAFKA`, `REDIS_STREAM`. Same template resolution, same retry policy, same guarantees — regardless of transport.

### 3. Invoke — fire it

**Immediate** — fires now:
```bash
curl -X POST http://localhost:8080/v1/jobs \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "endpoint": "send-welcome-email",
    "trigger": "IMMEDIATE",
    "idempotency_key": "order-1234-welcome",
    "input": { "order_id": "order-1234", "user_id": "u_abc" }
  }'
```

**Delayed** — fires at a specific time:
```bash
curl -X POST http://localhost:8080/v1/jobs \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "endpoint": "send-welcome-email",
    "trigger": "DELAYED",
    "idempotency_key": "order-1234-reminder",
    "run_at": "2026-03-20T18:00:00Z",
    "input": { "order_id": "order-1234" }
  }'
```

**CRON** — fires on a schedule:
```bash
curl -X POST http://localhost:8080/v1/jobs \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "endpoint": "send-welcome-email",
    "trigger": "CRON",
    "cron": "0 9 * * MON",
    "timezone": "Asia/Kolkata",
    "input": { "order_id": "all" }
  }'
```

### 4. Observe

```bash
# Job details
curl http://localhost:8080/v1/jobs/{job_id} -H "Authorization: Bearer dev-api-key"

# Job health status
curl http://localhost:8080/v1/jobs/{job_id}/status -H "Authorization: Bearer dev-api-key"

# List executions
curl http://localhost:8080/v1/jobs/{job_id}/executions -H "Authorization: Bearer dev-api-key"

# Execution details
curl http://localhost:8080/v1/executions/{execution_id} -H "Authorization: Bearer dev-api-key"

# Attempt history
curl http://localhost:8080/v1/executions/{execution_id}/attempts -H "Authorization: Bearer dev-api-key"
```

---

## Using the TypeScript SDK

Kronos generates a TypeScript SDK from Smithy models.

```bash
just build-sdk    # Generate and compile the SDK
just cli-install  # Install CLI deps (links to built SDK)
```

```typescript
import { KronosServiceClient, CreateJobCommand } from "kronos-sdk";

const client = new KronosServiceClient({
  endpoint: "http://localhost:8080",
  token: { token: "dev-api-key" },
});

const response = await client.send(
  new CreateJobCommand({
    endpoint: "send-welcome-email",
    trigger: "IMMEDIATE",
    idempotency_key: "order-1234-welcome",
    input: { order_id: "order-1234" },
  }),
);

console.log(response.data.job_id);
```

---

## Template resolution

Endpoint specs support three template namespaces, resolved at execution time:

| Namespace | Source | Example |
|---|---|---|
| `{{input.*}}` | Per-job payload | `{{input.user_id}}` → `"u_abc"` |
| `{{config.*}}` | Centrally managed config | `{{config.api_base_url}}` → `"https://api.myapp.com"` |
| `{{secret.*}}` | Encrypted secret store | `{{secret.email_api_key}}` → resolved at runtime, never exposed |

Configs are cached (60s TTL). Secrets are encrypted at rest, decrypted in memory (300s TTL). Template resolution failures reject the execution immediately — no wasted retries.

---

## Execution lifecycle

```
PENDING ──→ QUEUED ──→ RUNNING ──→ SUCCESS
                │          │
                │          ├──→ RETRYING ──→ RUNNING (next attempt)
                │          │
                │          └──→ FAILED (retries exhausted)
                │
                └──→ CANCELLED
```

- **IMMEDIATE** jobs skip PENDING and go directly to QUEUED.
- **DELAYED** jobs start as PENDING and are promoted to QUEUED by the scheduler when `run_at` arrives.
- **CRON** jobs are materialized by the scheduler on each tick, creating a new execution per tick.

### Retry policy

Configurable per endpoint with three backoff strategies:

| Strategy | Formula | Use case |
|----------|---------|----------|
| `fixed` | `initial_delay_ms` | Consistent retry interval |
| `linear` | `initial_delay_ms * attempt` | Gradually increasing delay |
| `exponential` | `initial_delay_ms * 2^(attempt-1)` | Back off quickly under pressure |

All strategies apply ±25% jitter and are capped at `max_delay_ms`.

### CRON versioning

CRON jobs are immutable. Updates create a new version and retire the old one. The full version chain is preserved for audit:

```bash
# Update a CRON job (creates new version)
curl -X PUT http://localhost:8080/v1/jobs/{job_id} \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{ "cron": "0 */2 * * *", "input": { "mode": "v2" } }'

# View version history
curl http://localhost:8080/v1/jobs/{job_id}/versions \
  -H "Authorization: Bearer dev-api-key"
```

---

## Guarantees

| Guarantee | How |
|-----------|-----|
| **Exactly-once** | Idempotency keys + DB unique constraints + `SELECT FOR UPDATE SKIP LOCKED` |
| **Durable** | Every job persisted to PostgreSQL before acknowledgment |
| **Retry with backoff** | Configurable per endpoint: fixed, linear, or exponential with jitter |
| **Sub-second** | Immediate: ~300ms. Delayed: ~700ms of `run_at`. CRON: within 1s of tick |
| **Observable** | Every execution has a lifecycle. Every attempt recorded with duration, output, error |
| **Type-safe** | JSON Schema validation on job input at creation time |
| **Multi-tenant** | Schema-per-workspace isolation. Shared nothing between tenants |

---

## API reference

### Organizations & Workspaces
| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/orgs` | Create organization |
| `GET` | `/v1/orgs` | List organizations |
| `GET` | `/v1/orgs/{org_id}` | Get organization |
| `PUT` | `/v1/orgs/{org_id}` | Update organization |
| `POST` | `/v1/orgs/{org_id}/workspaces` | Create workspace |
| `GET` | `/v1/orgs/{org_id}/workspaces` | List workspaces |
| `GET` | `/v1/orgs/{org_id}/workspaces/{id}` | Get workspace |

### Setup
| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/payload-specs` | Create input schema |
| `GET/PUT/DELETE` | `/v1/payload-specs/{name}` | Manage |
| `POST` | `/v1/configs` | Create config |
| `GET/PUT/DELETE` | `/v1/configs/{name}` | Manage |
| `POST` | `/v1/secrets` | Create secret (write-only) |
| `GET/PUT/DELETE` | `/v1/secrets/{name}` | Manage (value never returned) |

### Endpoints
| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/endpoints` | Register HTTP / Kafka / Redis Stream endpoint |
| `GET/PUT/DELETE` | `/v1/endpoints/{name}` | Manage |

### Jobs
| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/jobs` | Create a job (IMMEDIATE / DELAYED / CRON) |
| `GET` | `/v1/jobs` | List jobs (filterable by endpoint, trigger, status) |
| `GET` | `/v1/jobs/{id}` | Get job details |
| `PUT` | `/v1/jobs/{id}` | Update CRON job (new immutable version) |
| `POST` | `/v1/jobs/{id}/cancel` | Cancel job |
| `GET` | `/v1/jobs/{id}/status` | Job health and stats |
| `GET` | `/v1/jobs/{id}/versions` | Version history (CRON) |
| `GET` | `/v1/jobs/{id}/executions` | List executions |

### Executions
| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/executions/{id}` | Execution details |
| `POST` | `/v1/executions/{id}/cancel` | Cancel execution |
| `GET` | `/v1/executions/{id}/attempts` | Attempt history |
| `GET` | `/v1/executions/{id}/logs` | Structured execution logs |

All list endpoints support cursor-based pagination via `?limit=N&cursor=...`.

---

## Configuration

All configuration is via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_DATABASE_URL` | *required* | PostgreSQL connection string |
| `TE_LISTEN_ADDR` | `0.0.0.0:8080` | API server bind address |
| `TE_API_KEY` | `dev-api-key` | Bearer token for authentication |
| `TE_ENCRYPTION_KEY` | 64 zeros | AES key for secret encryption (hex, 32+ bytes) |
| `TE_DB_POOL_SIZE` | `20` | Database connection pool size |
| `TE_WORKER_MAX_CONCURRENT` | `50` | Max concurrent job executions per worker |
| `TE_WORKER_POLL_INTERVAL_MS` | `200` | Worker DB polling interval |
| `TE_CRON_TICK_INTERVAL_SEC` | `1` | CRON materializer tick interval |
| `TE_CRON_BATCH_SIZE` | `100` | CRON jobs processed per tick |
| `TE_PROMOTE_INTERVAL_MS` | `500` | Delayed job promoter interval |
| `TE_RECLAIM_INTERVAL_SEC` | `30` | Stuck execution reclaimer interval |
| `TE_STUCK_EXECUTION_TIMEOUT_SEC` | `300` | Timeout before reclaiming a running execution |
| `TE_CONFIG_CACHE_TTL_SEC` | `60` | Config cache TTL in worker |
| `TE_SECRET_CACHE_TTL_SEC` | `300` | Secret cache TTL in worker |

---

## Development

### Just recipes

```bash
just                    # List all recipes
just setup              # One-time setup (DB + migrations + SDK + CLI)
just dev                # Run all 4 services in parallel

# Individual services
just api                # API server (port 8080)
just worker             # Worker
just scheduler          # Scheduler (cron, delayed, stuck)
just mock-server        # Mock HTTP server (port 9999)

# Database
just db-up              # Start PostgreSQL
just db-down            # Stop PostgreSQL
just db-migrate         # Run migrations
just db-reset           # Drop + recreate + migrate
just db-shell           # Open psql shell

# SDK
just smithy-build       # Generate from Smithy models
just build-sdk          # Build TypeScript SDK
just sdk-refresh        # Regenerate + rebuild + reinstall CLI
just cli-install        # Install CLI dependencies

# Tests
just test-immediate     # Test immediate job execution
just test-delayed       # Test delayed job execution
just test-cron          # Test CRON job execution
just test-e2e           # Full integration test (starts services, runs all tests)

# Build
just build              # Build all Rust crates
just build-release      # Release build
just check              # Type-check without building
just lint               # Run clippy
just fmt                # Format code

# Dashboard
just dashboard          # Run dashboard dev server
just dashboard-build    # Build WASM dashboard
```

### Project structure

```
kronos/
├── crates/
│   ├── common/          # Shared: models, DB, config, tenant, cache
│   ├── api/             # REST API server (actix-web)
│   ├── worker/          # Job execution engine
│   ├── scheduler/       # CRON materializer, delayed promoter, stuck reclaimer
│   ├── mock-server/     # Test HTTP server
│   └── dashboard/       # Web UI (Leptos/WASM)
├── migrations/          # SQL migration files
├── smithy/
│   ├── model/           # Smithy IDL definitions
│   └── smithy-build.json
├── cli/                 # TypeScript CLI for testing (uses generated SDK)
├── haskell-example/     # Example Haskell client
├── nix/                 # Custom Nix derivations (smithy-cli)
├── docker-compose.yml   # PostgreSQL, Kafka (opt), Redis (opt)
├── flake.nix            # Nix dev environment
└── justfile             # Task runner
```

### Adding a new endpoint type

The worker dispatches to endpoint types via `crates/worker/src/dispatcher/`. Kafka and Redis Stream are behind feature flags:

```bash
# Build with Kafka support
cargo build --workspace --features kronos-worker/kafka

# Build with Redis Stream support
cargo build --workspace --features kronos-worker/redis-stream
```

To start Kafka or Redis for local dev:

```bash
docker compose --profile kafka up -d
docker compose --profile redis up -d
```

---

## How it works internally

### Worker pipeline

When a worker claims an execution:

1. Load endpoint definition
2. Load config (cached 60s) and secrets (cached 300s, encrypted at rest)
3. Resolve `{{input.*}}`, `{{config.*}}`, `{{secret.*}}` templates
4. Validate resolved payload against JSON Schema (if payload spec is attached)
5. Dispatch to endpoint (HTTP / Kafka / Redis)
6. Record attempt (status, duration, output/error)
7. On success: mark execution `SUCCESS`
8. On failure: compute backoff, mark `RETRYING` (or `FAILED` if retries exhausted)

### Scheduler components

- **CRON materializer** (every 1s): Finds CRON jobs where `next_run_at <= now`, creates an execution for each tick, advances the tick pointer via CAS.
- **Delayed promoter** (every 500ms): Promotes `PENDING` executions to `QUEUED` when `run_at` arrives.
- **Stuck reclaimer** (every 30s): Finds `RUNNING` executions that have been running longer than the timeout (default 5min), resets them to `QUEUED` for re-pickup.

All three loops are idempotent — safe to run multiple instances for redundancy.
