---
id: dashboard
title: Dashboard
---

# Dashboard

Kronos includes a web-based dashboard built with [Leptos 0.7](https://leptos.dev/) compiled to WebAssembly (WASM). The dashboard provides a visual interface for browsing organizations, workspaces, jobs, executions, and attempts.

## Architecture

The dashboard is a WASM application that runs in the browser. When `TE_MODE=both`, the API server serves both the REST API and the dashboard from the same process — the dashboard's WASM bundle and static assets are served from the `TE_DASHBOARD_DIST_DIR` directory.

```
Browser ──→ API Server (actix-web, TE_MODE=both)
              ├── /v1/*          → REST API
              ├── /dashboard/*   → Dashboard WASM + assets
              └── /metrics       → Prometheus metrics
```

### Server modes

The `TE_MODE` environment variable controls what the API server serves:

| Mode | Value | Serves |
|------|-------|--------|
| API only | `api` (default) | REST API only |
| Dashboard only | `dashboard` | Dashboard only |
| Both | `both` | REST API + Dashboard (SSR) |

```bash
# API only (default)
TE_MODE=api ./kronos-api

# API + Dashboard
TE_MODE=both TE_DASHBOARD_DIST_DIR=crates/dashboard/pkg ./kronos-api
```

## Setup

### Prerequisites

The dashboard requires `wasm-pack` and the `wasm32-unknown-unknown` Rust target:

```bash
just dashboard-setup
```

This runs:
```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

:::note
The Nix development shell (`nix develop`) already includes `wasm-pack` and the `wasm32-unknown-unknown` target. If you're using Nix, you can skip `just dashboard-setup`.
:::

### Building the dashboard

```bash
# Release build (optimized, smaller WASM)
just dashboard-build

# Dev build (faster compilation, larger WASM, no optimizations)
just dashboard-build-dev
```

The build process:

1. **`wasm-pack build`** — compiles the `crates/dashboard` crate to WASM with the `hydrate` feature:
   ```bash
   cd crates/dashboard && wasm-pack build --target web --release -- --features hydrate
   ```

2. **`tailwindcss`** — generates the CSS bundle:
   ```bash
   cd crates/dashboard && tailwindcss -i input.css -o pkg/tailwind-output.css --minify
   ```

The output is placed in `crates/dashboard/pkg/`, which is the directory you point `TE_DASHBOARD_DIST_DIR` to.

:::warning
The dashboard bundle **must** be built before running the API server in `both` mode. If `TE_MODE=both` is set but `TE_DASHBOARD_DIST_DIR` points to a non-existent or empty directory, the dashboard routes will return 404.
:::

## Running the dashboard

### Local development

```bash
just dashboard
```

This is equivalent to:

```bash
TE_MODE=both TE_DASHBOARD_DIST_DIR=crates/dashboard/pkg cargo run -p kronos-api
```

The API server starts on port 8080, serving both the API and the dashboard. Access the dashboard at `http://localhost:8080/dashboard/` (when `TE_DASHBOARD_PATH_PREFIX=/dashboard` is set) or at the root path.

### With a path prefix

When running behind a reverse proxy or alongside other services, you typically want the dashboard under a path prefix:

```bash
TE_MODE=both \
TE_PATH_PREFIX=/kronos \
TE_DASHBOARD_PATH_PREFIX=/dashboard \
TE_API_BASE_URL=http://localhost:8080/kronos \
TE_DASHBOARD_DIST_DIR=crates/dashboard/pkg \
  cargo run -p kronos-api
```

This serves:
- API at `http://localhost:8080/kronos/v1/...`
- Dashboard at `http://localhost:8080/dashboard/`

## Path prefix configuration

The dashboard uses **compile-time** environment variables that are baked into the WASM binary. This means you must rebuild the dashboard if you change the prefix.

| Variable | Default | Description |
|----------|---------|-------------|
| `TE_DASHBOARD_PATH_PREFIX` | *(empty)* | URL prefix for dashboard routes (e.g. `/dashboard`) |
| `TE_API_BASE_URL` | *(empty)* | Full API base URL including path prefix (e.g. `http://localhost:8080/kronos`) |

:::important
`TE_API_BASE_URL` must include `TE_PATH_PREFIX` if it is set. For example, if `TE_PATH_PREFIX=/kronos`, then `TE_API_BASE_URL` should be `http://localhost:8080/kronos` (not `http://localhost:8080`).
:::

### Building with path prefix

```bash
TE_DASHBOARD_PATH_PREFIX=/dashboard \
TE_API_BASE_URL=http://localhost:8080/kronos \
  just dashboard-build
```

Then run the API server:

```bash
TE_MODE=both \
TE_PATH_PREFIX=/kronos \
TE_DASHBOARD_PATH_PREFIX=/dashboard \
TE_DASHBOARD_DIST_DIR=crates/dashboard/pkg \
  cargo run -p kronos-api
```

### Using .env

Since the justfile has `set dotenv-load`, you can set these in `.env`:

```env
# .env
TE_PATH_PREFIX=/kronos
TE_DASHBOARD_PATH_PREFIX=/dashboard
TE_API_BASE_URL=http://localhost:8080/kronos
```

```bash
just dashboard-build   # bakes prefix into WASM
just dashboard         # runs with prefix
```

## Docker

When building a Docker image with `INCLUDE_DASHBOARD=true`, the Dockerfile automatically builds the dashboard WASM bundle and copies it into the image:

```bash
docker build \
  --build-arg BINARY=kronos-api \
  --build-arg INCLUDE_DASHBOARD=true \
  -t kronos-api-with-dashboard .
```

The built image sets `TE_DASHBOARD_DIST_DIR=/app/dashboard-dist`, so you only need to set `TE_MODE=both` at runtime:

```bash
docker run -e TE_MODE=both -p 8080:8080 kronos-api-with-dashboard
```

For Docker builds with path prefix, pass the compile-time env vars as build args:

```dockerfile
# In your Dockerfile or docker-compose.yml build section:
ARG TE_DASHBOARD_PATH_PREFIX=/dashboard
ARG TE_API_BASE_URL=http://localhost:8080/kronos
ENV TE_DASHBOARD_PATH_PREFIX=$TE_DASHBOARD_PATH_PREFIX
ENV TE_API_BASE_URL=$TE_API_BASE_URL
```

:::note
The `docker-compose.prod.yml` already configures the dashboard with `TE_PATH_PREFIX=/kronos`, `TE_DASHBOARD_PATH_PREFIX=/dashboard`, and `INCLUDE_DASHBOARD=true`. See [Production Deployment](./production).
:::

## Dashboard pages

The dashboard provides the following pages:

| Page | Route | Description |
|------|-------|-------------|
| Organizations | `/dashboard/` | Lists all organizations with status badges |
| Organization detail | `/dashboard/orgs/{org_id}` | Organization details with its workspaces |
| Workspace detail | `/dashboard/orgs/{org_id}/workspaces/{workspace_id}` | Workspace details with jobs, executions, and attempts |

### Components

The dashboard is built with these reusable Leptos components:

| Component | Description |
|-----------|-------------|
| `sidebar` | Navigation sidebar with org/workspace context |
| `status_badge` | Color-coded status indicator (active, queued, running, success, failed, etc.) |
| `modal` | Modal dialog for confirmations and details |
| `loading` | Loading spinner / skeleton state |
| `confirm` | Confirmation dialog for destructive actions (cancel job, delete endpoint, etc.) |

## See also

- [Production Deployment](./production) — running the dashboard in Docker with KMS
- [Docker](./docker) — building images with `INCLUDE_DASHBOARD=true`
- [Environment Variables](../configuration/environment-variables) — `TE_MODE`, `TE_DASHBOARD_PATH_PREFIX`, `TE_API_BASE_URL`
