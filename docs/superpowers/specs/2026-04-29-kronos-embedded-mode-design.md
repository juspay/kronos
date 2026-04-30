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
- A connection-agnostic `KronosExecutor` trait that would let hosts pass non-sqlx connections (e.g., Diesel) into Kronos. Hosts using other DB libraries run with two pools against the same Postgres database; for atomicity-critical flows, the documented outbox pattern applies.
- A forced migration of existing service deployments to a non-`public` system schema. Service-mode defaults preserve today's `public.organizations` / `public.workspaces` exactly. Operators who want the safer namespace get a separate, optional migration guide outside this spec.
- Shipping a `kronos-outbox-relay` helper crate. The outbox pattern is documented as a blueprint in v1; a helper crate is a fast-follow if real users adopt the pattern.

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

Two builder modes, sharing the same struct and code paths. The pool argument is `sqlx::PgPool`, taken by value (the type is internally `Arc<PoolInner>`, so passing by value is just a refcount bump):

```rust
impl KronosClient {
    pub fn builder(pool: sqlx::PgPool) -> KronosClientBuilder { /* ... */ }
}

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

The host owns the pool's lifecycle and sizing; the library never connects on its own. Hosts that already have a `sqlx::PgPool` for their own queries can pass the same pool. `for_workspace` returns a cheap, cloneable scoped view backed by the same pool and caches. The actix handlers use this to translate tenant headers into a scoped client without rebuilding state.

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

The pool argument is `sqlx::PgPool` (same type and ownership semantics as `KronosClient::builder`):

```rust
impl Worker {
    pub fn builder(pool: sqlx::PgPool) -> WorkerBuilder { /* ... */ }
}

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
pub fn migrations(opts: &MigrationOpts) -> Vec<Migration>;   // rendered for the chosen schemas
pub async fn migrate(pool: &PgPool, opts: &MigrationOpts) -> Result<(), MigrateError>;
```

`MigrationOpts` carries the `system_schema` name and the `tenant_schema_prefix` (see "Schema namespacing" below). Migration files are templates over these names, rendered at apply time.

The README documents three integration paths:

1. Call `kronos_client::migrate(&pool, &opts)` once at app startup (one-liner, no extra tool).
2. Inline the rendered SQL into the host's existing migration tool (sqlx, refinery, Atlas, Liquibase). The `kronos-migrate` CLI prints rendered SQL to stdout for this case.
3. Use the `kronos-migrate` CLI directly for ad-hoc / CI use.

`pg_cron` extension creation and `cron.schedule()` calls happen on the existing CRON registration code path. The library detects whether `pg_cron` is installed and warns rather than fails when it isn't, so non-CRON users aren't blocked.

### Schema namespacing and table-name collisions

Today's deployment puts shared tables (`organizations`, `workspaces`) in `public` and per-tenant tables in `{org_slug}_{workspace_slug}` schemas. For an embedded host whose own application tables also live in `public`, the names `organizations` and `workspaces` collide with very common host-app table names.

The library treats schema names as configuration. Both builders accept:

```rust
KronosClient::builder(pool)
    .system_schema("kronos")              // shared tables go here
    .tenant_schema_prefix("kronos_")      // per-workspace schemas become "kronos_{org}_{ws}"
    .workspace(o, w)
    .build()
```

**Defaults differ by mode, by design:**

- **Service binaries (`kronos-api`, `kronos-worker`):** default to `system_schema = "public"` and `tenant_schema_prefix = ""`. This preserves every existing deployment exactly. Operators can opt into the safer namespace by setting `TE_SYSTEM_SCHEMA` and `TE_TENANT_SCHEMA_PREFIX` and running a one-time migration (documented separately, not part of v1's scope).
- **Library defaults (`KronosClient`, `Worker`):** default to `system_schema = "kronos"` and `tenant_schema_prefix = "kronos_"`. Embedded users get isolation from the host's `public.organizations` (or whatever) by default. They can override if they have a reason to.

**Implementation:**

- Migration SQL files reference schema names as template placeholders (`{{system_schema}}`, `{{tenant_schema_prefix}}`). The migration runner substitutes at apply time.
- All runtime SQL in `kronos-common::db::*` already takes a schema parameter (the `scoped_transaction` path) — that pattern extends to the system schema for `organizations` / `workspaces` queries.
- `SchemaRegistry` already iterates schemas; it gains a prefix filter so it picks up only Kronos-owned tenant schemas, not arbitrary host schemas that happen to exist.
- `pg_cron` registrations include the schema name in the inserted execution row (no change to `pg_cron` itself, just the SQL it executes on tick).

This is a configuration choice, not a schema rewrite — the table layout is identical, only the qualifier changes.

### Coexistence with other database libraries (Diesel, etc.)

Hosts may use Diesel, sea-orm, or another DB layer for their own tables. Two sub-questions matter, with different answers.

**Connection pool coexistence.** The library is internally sqlx-only — both `KronosClient::builder` and `Worker::builder` accept a `sqlx::PgPool`. The host's ORM keeps its own pool (e.g., `r2d2`/`bb8` holding `diesel::PgConnection`s for Diesel) against the same Postgres database. Two pools, one database — wasteful by a few connections, but functionally fine. v1 stance: document this; do not introduce a connection-agnostic abstraction.

**Cross-system transactional atomicity.** The harder case is when the host wants to commit a domain change *and* enqueue a Kronos job atomically (e.g., "create order + enqueue welcome email — both or neither"). Two pools cannot share a transaction.

The v1 stance is two-tiered:

1. **Default: rely on idempotency keys.** Most embedded use cases tolerate "enqueue eventually succeeds" because Kronos already enforces idempotency via DB unique constraints — a retry of the enqueue after a domain commit is safe. The host pattern is:

   ```rust
   // Inside the host's diesel transaction:
   let order = orders::insert(...).execute(&mut diesel_conn)?;
   let idem_key = format!("welcome-email-{}", order.id);
   diesel_conn.commit()?;

   // After commit, enqueue with the order id as the idempotency key.
   // If this fails, a retry (manual or via a sweeper) is safe — Kronos dedupes.
   kronos.jobs().create(CreateJob {
       endpoint: "welcome-email".into(),
       idempotency_key: Some(idem_key),
       input: Some(json!({ "order_id": order.id })),
       ..Default::default()
   }).await?;
   ```

   Failure mode: domain commits, enqueue crashes before retry, no email sent until the next sweep. Acceptable for most flows.

2. **For hard-atomic needs: a transactional outbox.** For flows where the enqueue truly must be atomic with the domain commit, document the outbox pattern:

   - Host adds its own `kronos_outbox` table to its schema, holding `{create_job_payload jsonb, attempted_at timestamptz}`.
   - Inside the domain transaction (using the host's ORM), insert the outbox row.
   - A small relay process polls `kronos_outbox`, calls `kronos.jobs().create()` for each pending row, and marks it sent. The relay uses idempotency keys so re-running after a crash is safe.

   We document this pattern in the README and (as a fast-follow, not v1) optionally ship a `kronos-outbox-relay` helper crate that handles the polling/marking loop.

**Out of scope for v1:** a connection-agnostic `KronosExecutor` trait that would let the host pass a Diesel connection where Kronos expects a sqlx connection. This is a substantial internal refactor (every internal query would have to route through the trait), and Diesel's type-state-heavy API does not abstract cleanly behind a runtime trait. We will revisit only if real users ask for it.

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

The test runs against the **non-default** `system_schema = "kronos"` and `tenant_schema_prefix = "kronos_"`, so it independently exercises the schema-parameterization machinery that service-mode integration tests don't reach (because they default to `public` / `""`).

The mock-server stays as-is for HTTP dispatcher integration tests.

## Backwards-compatibility commitments

The refactor is structural only. For existing service deployments, the following do not change:

- HTTP request and response shapes for any endpoint.
- The DB table layout (column types, constraints, indices). Migration files become parametric on schema names but, with service-mode defaults (`system_schema = "public"`, `tenant_schema_prefix = ""`), produce identical SQL output to today's migrations.
- The `TE_*` environment variable contract for the binaries. New optional vars (`TE_SYSTEM_SCHEMA`, `TE_TENANT_SCHEMA_PREFIX`) default to today's values when unset.
- The Prometheus metric names, labels, and the `:9090` listener for service mode.
- The Smithy SDK contract (TypeScript / Haskell consumers see no change).

If integration tests pass with default config, the service is preserved.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Refactor introduces subtle behavior drift between library and existing handler logic. | The integration test suite is the contract. No PR merges if `test-e2e` fails. The new embedded-mode E2E test independently exercises the library path. |
| Library users without `pg_cron` are surprised at runtime when CRON jobs silently don't fire. | Library detects `pg_cron` presence on `migrate()` / first CRON job create and emits a warning. README documents the prerequisite prominently. |
| Library users discover `metrics`/`tracing` are silent because they have no recorder/subscriber. | README's "first run" section calls this out and shows minimal setup snippets. |
| Multi-tenancy code paths atrophy because most embedded users pin a workspace. | Service mode (`kronos-api` + multi-tenant `kronos-worker`) exercises the multi-tenant path on every CI run via the existing integration tests. |
| Encryption-key handling regresses (e.g., key passed by reference vs. owned, lifetimes). | Builder takes `Vec<u8>` (owned); never holds a borrow across awaits. KMS path tested under the `kms` feature flag in CI. |
| Embedded worker's `ctrl_c` handler conflicts with host's signal handling. | `start()` does not install a signal handler. Only the `run_until_ctrl_c()` convenience does, and its name is the documentation. Hosts that need their own shutdown story use `WorkerHandle::shutdown()`. |
| Schema-name parameter inconsistency between client and worker (e.g., client writes to `kronos.organizations`, worker looks in `public.organizations`) silently breaks job execution. | Both builders share a `SchemaConfig { system_schema, tenant_schema_prefix }` value type. Mismatched config fails fast at `Worker::build()` by validating that the configured schemas exist and contain the expected migration version. The embedded-mode E2E test exercises the non-default `kronos`/`kronos_` namespace. |
| Host adopts the outbox pattern incorrectly (e.g., commits the outbox row but never drains it; or drains without idempotency keys, producing duplicates). | README outbox section is prescriptive: it shows the exact relay loop, requires an idempotency key derived from a stable domain id, and recommends a periodic sweeper for the "outbox row written, relay process died before it ran" case. We may ship the relay as a helper crate later to reduce footguns. |

## Implementation phasing and rollout

This document is the design. The detailed step-by-step implementation tasks live in separate plan files under `docs/superpowers/plans/` (one per rollout plan below), produced by the writing-plans skill.

The work ships as **four sequential plans, each merging as its own PR**. The boundaries are chosen so that every PR leaves the system in a green, runnable state — existing integration tests pass with default config, the service runs unchanged, and any merge can be deferred or rolled back without blocking the others.

### Plan 1 — Foundation

**Covers:** new crate scaffolding + schema parameterization (phases below labeled F1, F2).

| Phase | Detail |
|---|---|
| F1 | Create empty `kronos-client` and `kronos-embedded-worker` crates and wire them into the Cargo workspace. No public API yet — the crates compile but are inert. |
| F2 | Parameterize migration files and runtime SQL on `system_schema` / `tenant_schema_prefix`. Add a `MigrationOpts` value type and the `migrate(&pool, &opts)` entry point on `kronos-client`. Service binaries default to `system_schema = "public"`, `tenant_schema_prefix = ""` (preserving today exactly). Add `TE_SYSTEM_SCHEMA` / `TE_TENANT_SCHEMA_PREFIX` env vars defaulting to today's values. |

**Ships when:** `cargo build --workspace` succeeds, all existing integration tests (`just test-immediate`, `just test-delayed`, `just test-cron`, `just test-e2e`) pass with default config, and a smoke test confirms migrations applied with non-default `system_schema = "kronos"` produce a working schema.

**Depends on:** nothing. This is the root.

### Plan 2 — Worker extraction

**Covers:** moving the worker pipeline into the new library crate (phase W1).

| Phase | Detail |
|---|---|
| W1 | Move `poller`, `pipeline`, `backoff`, and `dispatcher` modules from `crates/worker/` to `crates/embedded-worker/`. Introduce the `Worker::builder(pool: sqlx::PgPool)` + `WorkerHandle` API and the `run_until_ctrl_c()` convenience. `kronos-worker` becomes a binary-only crate (~15-line `main`). |

**Ships when:** all existing integration tests pass with the new worker shell. Behavior is byte-identical to the pre-extraction worker (same poll cadence, same claim semantics, same retry/backoff, same metrics labels).

**Depends on:** Plan 1 (needs the empty `kronos-embedded-worker` crate to move into).

### Plan 3 — Client extraction

**Covers:** building the `kronos-client` API surface and cutting over `kronos-api` handlers, one resource at a time (phase C1).

| Phase | Detail |
|---|---|
| C1 | For each resource (orgs → workspaces → payload-specs → configs → secrets → endpoints → jobs → executions/attempts/logs): implement the resource's API in `kronos-client` with unit tests against a real Postgres; cut over the corresponding `kronos-api` handler to translate HTTP into a `kronos-client` call; ensure integration tests still pass at each cutover. |

**Ships when:** every `kronos-api` handler is a thin HTTP↔library translator; no DB-write logic remains in handler code; the integration test suite passes; the `KronosClient::for_workspace` per-request scoping pattern is in place.

**Depends on:** Plan 1 (schema parameterization is a prerequisite for client SQL). Plan 2 is recommended-before but not strictly required (client extraction does not touch worker code).

### Plan 4 — Embedded validation and documentation

**Covers:** the embedded-mode E2E test and user-facing documentation (phases V1, D1).

| Phase | Detail |
|---|---|
| V1 | Add the embedded-mode E2E test: a test binary that does no HTTP and no actix, builds a `KronosClient` and a `Worker` against a fresh schema using the non-default `system_schema = "kronos"` / `tenant_schema_prefix = "kronos_"`, creates a job via the client, lets the worker process it, asserts `SUCCESS`. |
| D1 | README updates: embedded mode quickstart, the `sqlx::PgPool` setup snippet, the migration integration guide for `kronos-client::migrate(&pool, &opts)`, the transactional outbox pattern blueprint for hosts using Diesel/sea-orm/etc., and the explicit pg_cron prerequisite call-out. |

**Ships when:** the embedded-mode E2E test is green in CI; documentation has been reviewed by a teammate who has not been part of the embedded-mode design.

**Depends on:** Plan 3 (V1 needs the full `kronos-client` API and `Worker` library to exercise).

### Out-of-band: optional follow-ups (not in v1)

These are explicitly *not* in any of the four plans; they're tracked here so reviewers know the design has considered them:

- One-time migration guide for service operators who want to move existing `public.organizations` / `public.workspaces` deployments into the `kronos` schema.
- `kronos-outbox-relay` helper crate for hosts adopting the outbox pattern.
- In-process CRON scheduler for hosts that can't install `pg_cron`.

## Open questions

None at this time. Ready for review.
