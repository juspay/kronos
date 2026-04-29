# Kronos Embedded Mode — Design

**Date:** 2026-04-29
**Status:** Draft, pending user approval
**Branch:** `feat/embedded-mode`

## Motivation

Kronos today is a network service: an actix-web API server plus a worker process, both backed by PostgreSQL with `pg_cron`. Teams that want job scheduling without operating two extra deployable services have no good option short of running the full stack.

The goal is to expose Kronos's job-management functionality as a Rust library that a host application embeds directly. The host app writes jobs into the same PostgreSQL schema the service uses, and runs the worker pipeline inside its own async runtime — no API server, no separate worker process.

## Scope

**In scope (v1):**

- A Rust crate that performs job/endpoint/config/secret CRUD against the same PostgreSQL schema the service uses.
- A Rust crate that runs the worker pipeline (claim → resolve templates → dispatch → record) inside the host process.
- Refactoring the existing service crates so they sit *on top of* these new library crates — one source of truth for behavior.
- Single-tenant ergonomics for embedded users (workspace pinned at construction time), preserving the underlying schema-per-workspace layout.

**Out of scope (v1):**

- Non-Rust language bindings. Other languages continue to use the HTTP API and the generated Smithy SDK.
- An in-process CRON scheduler. CRON triggers continue to require `pg_cron`. Document the prerequisite; library users who can't install `pg_cron` get IMMEDIATE and DELAYED triggers only.
- Stripping multi-tenancy or moving tables to `public`. Tables stay in tenant schemas; embedded mode just pins a default workspace.
- An embedded dashboard.
- A manual `tick()` API for non-Tokio schedulers.

## High-level architecture

Two new crates and a refactor of the existing service crates:

```
crates/
├── client/             NEW  — kronos-client: enqueue + CRUD library API
├── embedded-worker/    NEW  — kronos-embedded-worker: worker pipeline as a library
├── common/                   unchanged — shared models, db, crypto, template, tenant, cache
├── api/                      refactor — actix-web shell over kronos-client
├── worker/                   refactor — thin binary over kronos-embedded-worker
├── mock-server/              unchanged
└── dashboard/                unchanged
```

The architectural commitment is **one code path**: whatever the embedded library does is exactly what the service does, by construction. The service crates do not duplicate business logic — they translate HTTP/CLI into library calls.

## Crate: `kronos-client`

### Purpose

Public Rust API for everything the HTTP API does *except* the HTTP layer: create/list/get/update/delete for organizations, workspaces, payload specs, configs, secrets, endpoints, jobs; read paths for executions and attempts.

### Construction

Two builder modes, sharing the same struct and code paths:

```rust
// Embedded use: workspace pinned, all calls scoped to it
let kronos = KronosClient::builder(pool)
    .workspace(org_id, workspace_id)
    .encryption_key(key_bytes)
    .config_cache_ttl(Duration::from_secs(60))
    .secret_cache_ttl(Duration::from_secs(300))
    .build()
    .await?;

// Service use: no default workspace, derive a scoped client per request
let kronos = KronosClient::builder(pool).build().await?;
let scoped = kronos.for_workspace(org_id, workspace_id);
```

`for_workspace` returns a cheap, cloneable scoped view backed by the same pool and caches. The actix handlers use this to translate tenant headers into a scoped client without rebuilding state.

### Public API surface

```rust
// Organizations / workspaces (only available on the unscoped client)
kronos.orgs().create(name, slug).await?;
kronos.orgs().list(page).await?;
kronos.orgs().workspaces(&org_id).create(name, slug).await?;

// Setup (scoped)
kronos.payload_specs().create(name, schema).await?;
kronos.configs().create(name, values).await?;
kronos.secrets().create(name, value).await?;        // encrypts on insert
kronos.endpoints().create(EndpointSpec { ... }).await?;

// Jobs
let job = kronos.jobs().create(CreateJob {
    endpoint: "send-welcome-email".into(),
    trigger: Trigger::Immediate,
    idempotency_key: Some("order-1234".into()),
    input: Some(json!({ "order_id": "1234" })),
    ..Default::default()
}).await?;
kronos.jobs().status(&job.id).await?;
kronos.jobs().cancel(&job.id).await?;
kronos.jobs().list_executions(&job.id, page).await?;

// Executions / attempts (read paths)
kronos.executions().get(&exec_id).await?;
kronos.executions().attempts(&exec_id).await?;
kronos.executions().logs(&exec_id).await?;
```

### Types

Request and response types are the existing models from `kronos-common::models` (already derived from the Smithy IDL). The library does not introduce a parallel type system.

### Errors

A single typed error enum, with variants that map cleanly to HTTP status codes for the actix layer:

```rust
pub enum ClientError {
    ValidationFailed { field: String, reason: String },   // 400
    NotFound { resource: &'static str, id: String },      // 404
    Conflict { reason: String },                          // 409 (idempotency, slug clash)
    EncryptionNotConfigured,                              // 400 — secret op without key
    Database(sqlx::Error),                                // 500
    Crypto(crypto::Error),                                // 500
    SchemaResolution(String),                             // 500
}
```

`ApiError: From<ClientError>` lives in `kronos-api` and produces the HTTP responses the existing API contract requires.

### Behavior preserved exactly

- JSON Schema validation on job input runs on create (currently lives in the API handler).
- Idempotency-key uniqueness enforced via the same DB unique constraints.
- CRON job updates create a new immutable version (current behavior in handlers).
- Template resolution rules unchanged.
- pg_cron registration on CRON job create unchanged.

## Crate: `kronos-embedded-worker`

### Purpose

The worker pipeline (poll → claim → resolve → dispatch → record) as a library that runs inside the host's Tokio runtime.

### Construction & lifecycle

```rust
let worker = Worker::builder(pool)
    .workspace(org_id, workspace_id)            // matches client pinning
    .max_concurrent(50)
    .poll_interval(Duration::from_millis(200))
    .shutdown_timeout(Duration::from_secs(30))
    .encryption_key(key_bytes)
    .config_cache_ttl(Duration::from_secs(60))
    .secret_cache_ttl(Duration::from_secs(300))
    .install_metrics_recorder(false)            // host owns metrics
    .build()
    .await?;

// Builder + handle (primary API)
let handle: WorkerHandle = worker.start();
// ... host runs ...
handle.shutdown().await;       // honors the configured shutdown_timeout

// One-line convenience for simple cases
Worker::builder(pool).workspace(o, w).build().await?
    .run_until_ctrl_c().await?;
```

`WorkerHandle` exposes:

- `shutdown()` — drains in-flight executions up to `shutdown_timeout`, then returns.
- `is_idle()` — for host healthchecks / readiness probes.
- A `JoinHandle`-like `wait()` for hosts that want to await the loop's completion.

### Service use

For service mode with multiple tenants, `Worker::builder(pool).build()` (no `.workspace()` call) preserves today's behavior: iterate all active schemas via `SchemaRegistry`, claim across them. The pinned-workspace path is a short-circuit: the registry returns just the configured schema.

### Behavior preserved exactly

- `SELECT FOR UPDATE SKIP LOCKED` claim semantics — multiple workers (embedded, service, or both) coexist on the same DB without duplicate execution.
- Same retry/backoff machinery (fixed/linear/exponential with ±25% jitter, capped at `max_delay_ms`).
- Same dispatchers behind the same feature flags (`kafka`, `redis-stream`).
- Same per-execution attempt and log writes.

## Refactor: existing service crates

### `kronos-api`

Each handler becomes an HTTP↔library translator only. Example:

```rust
// Before: handler contains validation, DB writes, idempotency, response shaping (~100 lines)
// After:
async fn create_job(
    state: web::Data<AppState>,
    tenant: TenantHeaders,
    body: web::Json<CreateJob>,
) -> Result<HttpResponse, ApiError> {
    let kronos = state.kronos.for_workspace(tenant.org_id, tenant.workspace_id);
    let job = kronos.jobs().create(body.into_inner()).await?;
    Ok(HttpResponse::Created().json(job))
}
```

### `kronos-worker`

The binary's `main.rs` becomes ~15 lines:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();
    let config = AppConfig::from_env().await?;
    let pool = sqlx::PgPool::connect(&config.db.url).await?;

    Worker::builder(pool)
        .from_app_config(&config)
        .install_metrics_recorder(true)
        .build()
        .await?
        .run_until_ctrl_c()
        .await
}
```

`kronos-worker` becomes a binary-only crate (no `lib.rs`).

### Module migration table

| Module today | Moves to |
|---|---|
| `crates/worker/src/poller.rs` | `crates/embedded-worker/src/poller.rs` |
| `crates/worker/src/pipeline.rs` | `crates/embedded-worker/src/pipeline.rs` |
| `crates/worker/src/backoff.rs` | `crates/embedded-worker/src/backoff.rs` |
| `crates/worker/src/dispatcher/*` | `crates/embedded-worker/src/dispatcher/*` |
| `crates/api/src/handlers/*` (DB write logic) | `crates/client/src/{jobs,endpoints,...}.rs` |
| `crates/api/src/handlers/*` (HTTP response shape) | stays in `crates/api/` |
| `crates/common/src/*` | unchanged |

## Cross-cutting concerns

### Configuration

Library code never reads environment variables. Both builders accept values directly. The `TE_*` env contract and `AppConfig::from_env()` become the binaries' concern only.

A thin `Worker::builder.from_app_config(&AppConfig)` adapter bridges env-derived config into the builder, used by the `kronos-worker` binary. Embedded users construct their own `Worker::builder(...)` with whatever values they prefer.

### Encryption key & secrets

The `secrets` table and `crypto` module are unchanged. The encryption key is a builder parameter (`encryption_key(key_bytes)`) — the caller sources it from wherever (literal, env, AWS Secrets Manager, Vault).

KMS support remains gated behind the existing `kms` feature flag. With the feature on, the builder accepts a `KmsKeyResolver` (existing trait) instead of raw bytes.

If a host omits the encryption key entirely, secret operations return `ClientError::EncryptionNotConfigured`. Hosts that don't use Kronos secrets (sourcing credentials elsewhere and passing via `input` or `config`) pay no setup cost.

### Metrics

The library uses the `metrics` crate facade only. It does not install a recorder and does not start an HTTP listener. If the host has any `metrics` recorder installed, Kronos counters and histograms flow into it. If not, calls are no-ops.

The service binaries opt into the existing Prometheus recorder + listener via `.install_metrics_recorder(true)`, preserving today's `:9090` behavior.

### Tracing

Same pattern. Library uses `tracing` macros; never calls `tracing_subscriber::*`. Hosts get events flowing into their existing subscriber. Binaries continue to call `tracing_subscriber::fmt().json().init()` in `main`.

### Migrations

App-managed. The library exposes:

```rust
pub fn migrations() -> &'static [Migration];   // SQL files compiled in
pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError>;
```

The README documents three integration paths:

1. Call `kronos_client::migrate(&pool)` once at app startup (one-liner, no extra tool).
2. Inline the SQL files into the host's existing migration tool (sqlx, refinery, Atlas, Liquibase).
3. Use a small `kronos-migrate` CLI we ship for ad-hoc / CI use.

`pg_cron` extension creation and `cron.schedule()` calls happen on the existing CRON registration code path. The library detects whether `pg_cron` is installed and warns rather than fails when it isn't, so non-CRON users aren't blocked.

## Testing strategy

Three layers, with the existing integration tests as the load-bearing contract.

### 1. Existing integration tests must pass unchanged

`just test-immediate`, `just test-delayed`, `just test-cron`, `just test-e2e`, and the load test all hit the running HTTP API and the running worker. They are the contract that the refactor preserves service behavior.

### 2. New `kronos-client` unit tests

Each public method gets tests against a real PostgreSQL (sqlx test fixtures, schema-per-test isolation). Coverage:

- Validation paths (bad inputs return `ClientError::ValidationFailed` with the right field).
- Idempotency-key conflicts return `ClientError::Conflict`.
- Tenant scoping — a scoped client cannot read or write across workspaces.
- Error mapping — every internal error maps to a `ClientError` variant.

### 3. New embedded-mode end-to-end test

A new test binary that does *no HTTP* and *no actix*: builds a `KronosClient` and a `Worker` against a fresh schema, creates a job via the client, lets the worker process it, asserts the execution row reaches `SUCCESS`. This is the canary for the embedded use case — if it breaks, embedding is broken.

The mock-server stays as-is for HTTP dispatcher integration tests.

## Backwards-compatibility commitments

The refactor is structural only. The following do not change:

- HTTP request and response shapes for any endpoint.
- The DB schema and migration files.
- The `TE_*` environment variable contract for the binaries.
- The Prometheus metric names, labels, and the `:9090` listener for service mode.
- The Smithy SDK contract (TypeScript / Haskell consumers see no change).

If integration tests pass, the service is preserved.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Refactor introduces subtle behavior drift between library and existing handler logic. | The integration test suite is the contract. No PR merges if `test-e2e` fails. The new embedded-mode E2E test independently exercises the library path. |
| Library users without `pg_cron` are surprised at runtime when CRON jobs silently don't fire. | Library detects `pg_cron` presence on `migrate()` / first CRON job create and emits a warning. README documents the prerequisite prominently. |
| Library users discover `metrics`/`tracing` are silent because they have no recorder/subscriber. | README's "first run" section calls this out and shows minimal setup snippets. |
| Multi-tenancy code paths atrophy because most embedded users pin a workspace. | Service mode (`kronos-api` + multi-tenant `kronos-worker`) exercises the multi-tenant path on every CI run via the existing integration tests. |
| Encryption-key handling regresses (e.g., key passed by reference vs. owned, lifetimes). | Builder takes `Vec<u8>` (owned); never holds a borrow across awaits. KMS path tested under the `kms` feature flag in CI. |
| Embedded worker's `ctrl_c` handler conflicts with host's signal handling. | `start()` does not install a signal handler. Only the `run_until_ctrl_c()` convenience does, and its name is the documentation. Hosts that need their own shutdown story use `WorkerHandle::shutdown()`. |

## Implementation phasing (rough)

This document is the design; the implementation plan (sequence of PRs, test gates per step) is a separate artifact produced by the writing-plans skill.

Rough sketch of the phases the plan will likely cover:

1. Create empty `kronos-client` and `kronos-embedded-worker` crates, wire workspace.
2. Move worker modules (`poller`, `pipeline`, `backoff`, `dispatcher`) into `kronos-embedded-worker`. `kronos-worker` binary calls into it. Integration tests must pass.
3. Build out `kronos-client` API surface incrementally, one resource at a time (orgs → workspaces → payload_specs → configs → secrets → endpoints → jobs → executions). Each handler in `kronos-api` cuts over as its resource is implemented. Integration tests must pass at each cutover.
4. Add the embedded-mode E2E test.
5. Documentation: README updates, embedded mode quickstart, migration integration guide.

## Open questions

None at this time. Ready for review.
