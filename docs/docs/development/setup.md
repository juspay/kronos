---
id: setup
title: Development Setup
---

# Development Setup

This page covers setting up a local development environment for Kronos.

## Prerequisites

- [Nix](https://nixos.org/download) with flakes enabled
- [Docker](https://docs.docker.com/get-docker/) (for PostgreSQL and optional infrastructure)

### Enabling Nix flakes

If you haven't used Nix flakes before, enable them in your Nix configuration:

**`~/.config/nix/nix.conf`** (or `/etc/nix/nix.conf`):
```ini
experimental-features = nix-command flakes
```

:::note
On macOS with Nix Darwin, the config file may be at `~/.config/nix-darwin/nix.conf` or managed via your `flake.nix`.
:::

## Enter the development shell

```bash
nix develop
```

The Nix flake (`flake.nix`) provides a complete development environment with all required tools pre-installed:

| Tool | Purpose |
|------|---------|
| Rust toolchain (stable) | Compiling Rust crates, includes `rust-src` and `rust-analyzer` |
| `wasm32-unknown-unknown` target | Building the dashboard WASM bundle |
| `pkg-config` + `openssl` | OpenSSL bindings for Rust |
| `postgresql` | `psql` client for database access |
| `docker-compose` | Managing infrastructure containers |
| `sqlx-cli` | Database migrations |
| `nodejs_22` + `yarn` | TypeScript SDK and CLI |
| `smithy-cli` | Smithy model validation and code generation |
| `just` | Task runner |
| `wasm-pack` + `wasm-bindgen-cli` | Building the WASM dashboard |
| `tailwindcss` | Dashboard CSS generation |
| `awscli2` | KMS scripts and LocalStack interaction |

The shell also sets `DATABASE_URL` for `sqlx` compile-time checking:

```bash
export DATABASE_URL="postgresql://kronos:kronos@localhost:5432/taskexecutor"
```

### direnv integration

For automatic shell loading, add [direnv](https://direnv.net/) with a `.envrc` file at the project root:

```bash
# .envrc
use flake
```

This automatically enters the Nix development shell whenever you `cd` into the project.

## One-time setup

After entering the development shell, run:

```bash
just setup
```

This runs four steps in sequence:

1. **`just db-up`** — Starts the PostgreSQL container (with `pg_cron`) and waits for it to be ready
2. **`just db-migrate`** — Applies all four SQL migrations to the `taskexecutor` database
3. **`just build-sdk`** — Validates Smithy models, generates SDKs, builds the TypeScript SDK
4. **`just cli-install`** — Installs CLI dependencies (links to the built SDK)

### Verifying the setup

```bash
# Check the database is running
docker compose ps postgres

# Check migrations applied
just db-shell
# \dt  (should show organizations, workspaces tables)

# Check the API responds
curl http://localhost:8080/health
# OK
```

## Starting services

### All services at once

```bash
just dev
```

This starts the API server, worker, and mock HTTP server in parallel:

| Service | Port | Metrics |
|---------|------|---------|
| API server | 8080 | `/metrics` |
| Worker | — | `:9090` |
| Mock server | 9999 | — |

All services are managed as background processes — press `Ctrl+C` to stop all of them.

:::note
`just dev` does not start the scheduler. The scheduler runs CRON materialization, delayed job promotion, and stuck execution reclaiming. To run it separately:
```bash
just scheduler
```
:::

### Individual services

```bash
just api          # API server only (port 8080)
just worker       # Worker only (metrics on :9090)
just scheduler    # Scheduler only (metrics on :9091)
just mock-server  # Mock HTTP server only (port 9999)
```

## Manual setup alternative

If you prefer to drive each step yourself (e.g., to run with a path prefix and the dashboard), the flow below mirrors what `just setup` / `just dev` automate.

### 1. Start PostgreSQL

```bash
docker compose up -d postgres
```

The container is named `kronos-postgres-1` with host port **5434** mapped to the container's `5432`.

### 2. (Re)create the database

```bash
docker exec -i kronos-postgres-1 psql -U kronos -d postgres -c \
  "DROP DATABASE IF EXISTS taskexecutor WITH (FORCE);"
docker exec -i kronos-postgres-1 psql -U kronos -d postgres -c \
  "CREATE DATABASE taskexecutor;"
```

### 3. Apply migrations

```bash
for f in migrations/20260317000000_initial.sql \
         migrations/20260318000000_multi_tenancy.sql \
         migrations/20260322000000_txn_based_pickup.sql \
         migrations/20260322000001_pg_cron.sql; do
  echo ">> applying $f"
  docker exec -i kronos-postgres-1 psql -U kronos -d taskexecutor -v ON_ERROR_STOP=1 < "$f"
done
```

### 4. Run the API server

```bash
TE_DATABASE_URL="postgres://kronos:kronos@localhost:5434/taskexecutor" \
TE_LISTEN_ADDR="0.0.0.0:8090" \
TE_MODE="both" \
TE_PATH_PREFIX="/api" \
TE_DASHBOARD_PATH_PREFIX="/dashboard" \
TE_DASHBOARD_DIST_DIR="crates/dashboard/pkg" \
cargo run -p kronos-api
```

:::tip
Building the dashboard bundle first (`just dashboard-build`) is required for `TE_MODE=both` to serve `crates/dashboard/pkg`.
:::

### 5. Run the worker

In a separate shell:

```bash
TE_DATABASE_URL="postgres://kronos:kronos@localhost:5434/taskexecutor" \
TE_METRICS_PORT="9090" \
cargo run -p kronos-worker
```

### Verify

```bash
# Root path (just dev):
curl http://localhost:8080/health
# OK

# Path prefix /api on port 8090:
curl http://localhost:8090/api/health
# OK
```

## Database management

### Start and stop PostgreSQL

```bash
just db-up      # Start PostgreSQL container
just db-down    # Stop all Docker Compose services
```

### Run migrations

```bash
just db-migrate
```

Applies all four migration files in order:

1. `20260317000000_initial.sql` — Base schema
2. `20260318000000_multi_tenancy.sql` — Multi-tenancy (organizations, workspaces, schema-per-tenant)
3. `20260322000000_txn_based_pickup.sql` — Transaction-based job pickup
4. `20260322000001_pg_cron.sql` — pg_cron integration

### Reset the database

```bash
just db-reset
```

Drops and recreates the `taskexecutor` database, then runs all migrations. Uses `sqlx database drop` and `sqlx database create`.

### Open a SQL shell

```bash
just db-shell
```

Opens a `psql` shell connected to the `taskexecutor` database:

```
psql (16.x)
Type "help" for help.

taskexecutor=#
```

## Infrastructure services

### All infrastructure

```bash
just infra-up    # DB + Kafka + Redis
just infra-down  # Stop all infrastructure
```

### Everything including monitoring

```bash
just all-up      # DB + Kafka + Redis + Prometheus + Grafana
just all-down    # Stop everything
```

After `just all-up`:

| Service | URL | Credentials |
|---------|-----|-------------|
| Prometheus | `http://localhost:9099` | — |
| Grafana | `http://localhost:3001` | `admin` / `kronos` |

### Individual services

```bash
# Kafka (for kafka dispatcher feature)
docker compose --profile kafka up -d

# Redis (for redis-stream dispatcher feature)
docker compose --profile redis up -d

# LocalStack KMS (for kms feature)
just kms-up

# Monitoring only
just monitoring-up
```

See [Building](./building) for enabling feature flags like `kafka`, `redis-stream`, and `kms`.

## Environment configuration

Copy `.env.example` to `.env` and customize as needed:

```bash
cp .env.example .env
```

Or use the `just` helper:

```bash
just init-env
```

Key defaults for local development:

| Variable | Default | Notes |
|----------|---------|-------|
| `TE_DATABASE_URL` | `postgresql://kronos:kronos@localhost:5432/taskexecutor` | Note: port 5432 in .env.example, but Docker maps to 5434. The justfile overrides this to `localhost:5432`. |
| `TE_API_KEY` | `dev-api-key` | Development API key |
| `TE_ENCRYPTION_KEY` | 64 zeros | Development encryption key (no security) |

:::warning
The `docker-compose.yml` maps PostgreSQL host port **5434** to container port **5432**. If you use `just db-up` and the justfile defaults, the database is accessible at `localhost:5432` (the justfile sets `TE_DATABASE_URL` to port 5432). If you manually configure the connection string, use port **5434** for the host-side mapping: `postgresql://kronos:kronos@localhost:5434/taskexecutor`.
:::

## See also

- [Building](./building) — build commands, feature flags, and Docker builds
- [Testing](./testing) — E2E tests, unit tests, and load testing
- [SDK Code Generation](./sdk-codegen) — Smithy model workflow
- [Environment Variables](../configuration/environment-variables) — full configuration reference
