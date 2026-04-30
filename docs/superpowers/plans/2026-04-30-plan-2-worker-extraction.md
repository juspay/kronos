# Plan 2 — Worker Extraction: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the worker pipeline (`poller`, `pipeline`, `backoff`, `dispatcher`) from the `kronos-worker` binary into the `kronos-embedded-worker` library crate, introduce a `Worker::builder(pool)` + `WorkerHandle` API, and shrink `kronos-worker/src/main.rs` to a ~15-line binary that drives the new builder. Behavior must be byte-identical to today's worker — same poll cadence, same claim semantics, same retry/backoff math, same dispatcher logic, same metrics names and labels. All existing integration tests (`just test-immediate`, `just test-delayed`, `just test-cron`, `just test-e2e`) pass at the end.

**Architecture:** A clean library/binary split. `kronos-embedded-worker` owns every async process that runs the pipeline; it depends only on `kronos-common`, sqlx, tokio, reqwest, etc. — no env, no `dotenvy`, no `tracing-subscriber`, no metrics-recorder install except behind an opt-in builder flag the binary calls. The poller's main loop is refactored once to take an `impl Future<Output = ()>` shutdown future instead of hard-coding `tokio::signal::ctrl_c()`; the existing service path is preserved by `Worker::run_until_ctrl_c()`, and embedded callers get cooperative shutdown via `Worker::start() -> WorkerHandle` plus `WorkerHandle::shutdown()`. Schema namespacing flows through the builder's `system_schema` / `tenant_schema_prefix` setters (defaults: `kronos` / `kronos_` for library, overridden by the binary via `from_app_config(&AppConfig)`). `Worker::build()` validates the `SchemaConfig` and confirms the system schema's `organizations` and `workspaces` tables exist, failing fast on misconfiguration.

**Tech Stack:** Rust 2021, sqlx 0.7 (Postgres), tokio (async runtime + `signal::ctrl_c`, `sync::oneshot`, `sync::Semaphore`), `tracing` facade only (no subscriber), `metrics` facade only (recorder install is binary-only, behind `install_metrics_recorder(true)`), reqwest for HTTP dispatcher, optional rdkafka/redis behind cargo features.

**Spec reference:** `docs/superpowers/specs/2026-04-29-kronos-embedded-mode-design.md` — see "Plan 2 — Worker extraction", "Module migration table", "kronos-worker" (~15-line `main`), "Cross-cutting concerns" (configuration, encryption key, metrics, tracing), and the risk-table rows on `ctrl_c` handling and schema-name parameter consistency.

**Branch:** `feat/worker-extraction` (already created off `feat/embedded-mode` tip). PR will be opened as a stacked draft against `feat/embedded-mode` once commits exist.

---

## File Structure

**New files (created):**

- `crates/embedded-worker/src/builder.rs` — `WorkerBuilder` struct, per-field setters, `from_app_config(&AppConfig)`, `build()` with `SchemaConfig::validate()` + system-schema existence probe.
- `crates/embedded-worker/src/handle.rs` — `WorkerHandle` plus the shutdown signal (`tokio::sync::oneshot::Sender<()>`).
- `crates/embedded-worker/src/error.rs` — `BuildError` enum (`InvalidSchemaConfig`, `SystemSchemaMissing`, `Database`).
- `crates/embedded-worker/tests/builder.rs` — unit tests for builder defaults, `from_app_config` extraction, `build()` validation paths.

**Files moved (mechanical, contents unchanged unless called out):**

- `crates/worker/src/backoff.rs` → `crates/embedded-worker/src/backoff.rs`
- `crates/worker/src/dispatcher.rs` → `crates/embedded-worker/src/dispatcher.rs`
- `crates/worker/src/dispatcher/http.rs` → `crates/embedded-worker/src/dispatcher/http.rs`
- `crates/worker/src/dispatcher/kafka.rs` → `crates/embedded-worker/src/dispatcher/kafka.rs`
- `crates/worker/src/dispatcher/redis_stream.rs` → `crates/embedded-worker/src/dispatcher/redis_stream.rs`
- `crates/worker/src/pipeline.rs` → `crates/embedded-worker/src/pipeline.rs` (no API change)
- `crates/worker/src/poller.rs` → `crates/embedded-worker/src/poller.rs` (refactored: `run` becomes a private `run_loop(pool, cfg, shutdown_fut)` taking an external shutdown future; the `WorkerConfig` struct replaces direct `AppConfig` reads).

**Modified files:**

- `crates/embedded-worker/Cargo.toml` — gain `kafka`, `redis-stream`, `kms` features; add the dependencies the moved modules need (`reqwest`, `serde`, `serde_json`, `uuid`, `chrono`, `rand`, `metrics`, optional `rdkafka` and `redis`).
- `crates/embedded-worker/src/lib.rs` — replace the inert shell with module declarations and public API re-exports (`Worker`, `WorkerBuilder`, `WorkerHandle`, `BuildError`).
- `crates/worker/Cargo.toml` — depend on `kronos-embedded-worker` (with feature pass-through), drop deps that moved (`reqwest`, `serde`, `serde_json`, `uuid`, `chrono`, `rand`, `metrics`, `rdkafka`, `redis`).
- `crates/worker/src/main.rs` — rewritten to ~15-line builder driver.

**Files deleted:**

- `crates/worker/src/lib.rs` — no library code remains in `kronos-worker`; it's binary-only.

---

## Design notes for the implementer

### `WorkerConfig` (private struct inside embedded-worker)

```rust
pub(crate) struct WorkerConfig {
    pub(crate) system_schema: String,
    pub(crate) tenant_schema_prefix: String,
    pub(crate) max_concurrent: usize,
    pub(crate) poll_interval_ms: u64,
    pub(crate) config_cache_ttl_sec: u64,
    pub(crate) secret_cache_ttl_sec: u64,
    pub(crate) shutdown_timeout_sec: u64,
    pub(crate) encryption_key: String,
}
```

This is built by `WorkerBuilder::build()` from per-field setters. The poller and pipeline take this rather than reading from `AppConfig` directly, severing the env-config dependency.

### Builder defaults (library-mode)

| Field | Default | Source |
|---|---|---|
| `system_schema` | `"kronos"` | `SchemaConfig::library_default()` |
| `tenant_schema_prefix` | `"kronos_"` | `SchemaConfig::library_default()` |
| `max_concurrent` | `50` | matches today's `WorkerEnv` default |
| `poll_interval_ms` | `200` | matches today's default |
| `config_cache_ttl_sec` | `60` | matches today's default |
| `secret_cache_ttl_sec` | `300` | matches today's default |
| `shutdown_timeout_sec` | `30` | matches today's default |
| `encryption_key` | required (no default) — `build()` returns an error if unset and any flow needs it; for v1 we require it always to avoid surprise at first secret use | spec: encryption-key handling |
| `install_metrics_recorder` | `false` | library purity; binary opts in |
| `metrics_port` | `9090` | only consulted when the recorder is installed |

The binary calls `from_app_config(&config)` which copies every field above (including `system_schema` from `config.schema.system_schema`, preserving today's `public` / `""` for service mode).

### Poller refactor

`pub async fn run(pool: PgPool, config: AppConfig)` becomes:

```rust
pub(crate) async fn run_loop<F>(pool: PgPool, cfg: WorkerConfig, shutdown: F)
where
    F: std::future::Future<Output = ()>,
```

`Worker::run_until_ctrl_c` passes `async { let _ = tokio::signal::ctrl_c().await; }`.

`Worker::start` creates `let (tx, rx) = tokio::sync::oneshot::channel::<()>();`, spawns `run_loop(pool, cfg, async move { let _ = rx.await; })`, returns `WorkerHandle { shutdown_tx: Some(tx), join: handle }`.

`WorkerHandle::shutdown(mut self) -> impl Future<Output = ()>` sends on `shutdown_tx` and awaits `self.join` so the caller knows in-flight tasks have drained.

### Schema-existence validation in `build()`

Probe via `to_regclass`, which is null-safe and avoids a parsing error when the schema is missing:

```rust
let probe: (Option<String>, Option<String>) = sqlx::query_as(
    "SELECT to_regclass($1)::text, to_regclass($2)::text",
)
.bind(format!("\"{}\".organizations", system_schema))
.bind(format!("\"{}\".workspaces", system_schema))
.fetch_one(&pool)
.await
.map_err(BuildError::Database)?;

if probe.0.is_none() {
    return Err(BuildError::SystemSchemaMissing {
        schema: system_schema.clone(),
        table: "organizations".into(),
    });
}
if probe.1.is_none() {
    return Err(BuildError::SystemSchemaMissing {
        schema: system_schema.clone(),
        table: "workspaces".into(),
    });
}
```

`system_schema` is already validated by `SchemaConfig::validate()` (ASCII alphanumeric/underscore only), so quoting it is safe.

### Metrics recorder installation

The library never installs a recorder by itself. The `install_metrics_recorder(true)` builder flag is the sole exception: when set, `build()` calls `kronos_common::metrics::install_recorder_with_listener(metrics_port)` exactly once, before returning the `Worker`. The flag's name is the documentation: it tells the reader this is service-binary territory.

### Feature-flag plumbing

`crates/worker/Cargo.toml` propagates features through to `kronos-embedded-worker` so `cargo build -p kronos-worker --features kafka` continues to enable the Kafka dispatcher end-to-end:

```toml
[features]
default = []
kafka = ["kronos-embedded-worker/kafka"]
redis-stream = ["kronos-embedded-worker/redis-stream"]
kms = ["kronos-embedded-worker/kms", "kronos-common/kms"]
```

---

## Tasks

### Task 1: Expand `kronos-embedded-worker` Cargo.toml

**Files:**
- Modify: `crates/embedded-worker/Cargo.toml`

- [ ] **Step 1: Replace the manifest** so it can host the moved modules in subsequent tasks. After this task, `lib.rs` is still inert; the new deps are unused warnings only — that is expected.

```toml
[package]
name = "kronos-embedded-worker"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[features]
default = []
kafka = ["dep:rdkafka"]
redis-stream = ["dep:redis"]
kms = ["kronos-common/kms"]

[dependencies]
kronos-common = { path = "../common" }
sqlx = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
reqwest = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
rand = { workspace = true }
metrics = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }

rdkafka = { workspace = true, optional = true }
redis = { workspace = true, optional = true }
```

- [ ] **Step 2: Verify the workspace still builds**

Run: `cargo build --workspace --all-features`
Expected: success. (`kronos-embedded-worker` builds with unused-dep warnings — fine.)

- [ ] **Step 3: Commit**

```bash
git add crates/embedded-worker/Cargo.toml
git commit -m "feat(embedded-worker): grow manifest deps and feature flags"
```

---

### Task 2: Move `dispatcher`, `backoff`, `pipeline`, `poller` into `kronos-embedded-worker`

This is a single mechanical relocation. The four modules form a tight call graph (poller → pipeline → dispatcher + backoff), so moving them together avoids cross-crate cycles and re-export shims. After this task, `kronos-worker` still has its old `lib.rs` re-exporting nothing useful and a `main.rs` that now imports the new path.

**Files:**
- Move (git mv): `crates/worker/src/backoff.rs` → `crates/embedded-worker/src/backoff.rs`
- Move (git mv): `crates/worker/src/dispatcher.rs` → `crates/embedded-worker/src/dispatcher.rs`
- Move (git mv): `crates/worker/src/dispatcher/http.rs` → `crates/embedded-worker/src/dispatcher/http.rs`
- Move (git mv): `crates/worker/src/dispatcher/kafka.rs` → `crates/embedded-worker/src/dispatcher/kafka.rs`
- Move (git mv): `crates/worker/src/dispatcher/redis_stream.rs` → `crates/embedded-worker/src/dispatcher/redis_stream.rs`
- Move (git mv): `crates/worker/src/pipeline.rs` → `crates/embedded-worker/src/pipeline.rs`
- Move (git mv): `crates/worker/src/poller.rs` → `crates/embedded-worker/src/poller.rs`
- Modify: `crates/worker/src/lib.rs` — change to a single line: `pub use kronos_embedded_worker::poller;` (temporary shim so `main.rs` keeps compiling; deleted in Task 7).
- Modify: `crates/embedded-worker/src/lib.rs` — declare the four moved modules and re-export `poller`.

- [ ] **Step 1: Move the files via `git mv`** so history is preserved.

```bash
git mv crates/worker/src/backoff.rs crates/embedded-worker/src/backoff.rs
git mv crates/worker/src/pipeline.rs crates/embedded-worker/src/pipeline.rs
git mv crates/worker/src/poller.rs crates/embedded-worker/src/poller.rs
git mv crates/worker/src/dispatcher.rs crates/embedded-worker/src/dispatcher.rs
mkdir -p crates/embedded-worker/src/dispatcher
git mv crates/worker/src/dispatcher/http.rs crates/embedded-worker/src/dispatcher/http.rs
git mv crates/worker/src/dispatcher/kafka.rs crates/embedded-worker/src/dispatcher/kafka.rs
git mv crates/worker/src/dispatcher/redis_stream.rs crates/embedded-worker/src/dispatcher/redis_stream.rs
```

- [ ] **Step 2: Replace `crates/embedded-worker/src/lib.rs`** with module declarations:

```rust
//! Kronos worker pipeline as an embeddable library. Moved from `kronos-worker`
//! in Plan 2 of the embedded-mode initiative; the public builder/handle API is
//! introduced in Tasks 3-5.

pub mod backoff;
pub mod dispatcher;
pub mod pipeline;
pub mod poller;
```

- [ ] **Step 3: Update the in-pipeline imports** so `pipeline.rs` and `poller.rs` reference sibling modules in the new crate. The original files used `crate::backoff` and `crate::dispatcher`; those paths still resolve because the modules are now siblings under `kronos-embedded-worker`. No edit needed unless rust-analyzer reports otherwise — verify with the build below.

- [ ] **Step 4: Replace `crates/worker/src/lib.rs`** with a temporary re-export shim:

```rust
//! Temporary shim for Task 2 of Plan 2. `kronos-worker` becomes binary-only in
//! Task 7, at which point this file is deleted.

pub use kronos_embedded_worker::poller;
```

- [ ] **Step 5: Update `crates/worker/Cargo.toml`** to depend on `kronos-embedded-worker`:

```toml
[dependencies]
kronos-common = { path = "../common" }
kronos-embedded-worker = { path = "../embedded-worker" }
tokio.workspace = true
sqlx.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
dotenvy.workspace = true
anyhow.workspace = true
```

(All other deps that the moved modules pulled in — `reqwest`, `serde`, `serde_json`, `uuid`, `chrono`, `rand`, `metrics`, `rdkafka`, `redis` — are dropped from the worker manifest. Features are rewritten in Task 7.)

- [ ] **Step 6: Verify `kronos-worker/src/main.rs` still calls `kronos_worker::poller::run`** (via the shim). The line `kronos_worker::poller::run(pool, config).await?;` resolves through `pub use kronos_embedded_worker::poller;`.

- [ ] **Step 7: Build the workspace**

Run: `cargo build --workspace --all-features`
Expected: success. Any rust-analyzer "file not in module tree" complaints are reindex lag, not real errors — the cargo build is authoritative.

- [ ] **Step 8: Run dispatcher unit tests** (the in-file tests moved with the files; they must still pass).

Run: `cargo test -p kronos-embedded-worker --lib -- --skip kafka --skip redis_stream`
Expected: HTTP dispatcher tests that don't require the mock server are skipped/ignored as before; non-network tests pass; no compilation errors.

- [ ] **Step 9: Run the existing integration smoke** — the worker binary still boots end-to-end against a fresh DB.

Run: `just db-reset && cargo build -p kronos-worker`
Expected: migrations apply, worker binary builds.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "refactor(embedded-worker): move poller, pipeline, backoff, dispatcher from worker"
```

---

### Task 3: Add `BuildError` enum and `WorkerBuilder` skeleton

**Files:**
- Create: `crates/embedded-worker/src/error.rs`
- Create: `crates/embedded-worker/src/builder.rs`
- Modify: `crates/embedded-worker/src/lib.rs`
- Create: `crates/embedded-worker/tests/builder.rs`

- [ ] **Step 1: Write failing tests first**

Create `crates/embedded-worker/tests/builder.rs`:

```rust
//! Builder defaults and `from_app_config` extraction. These tests do NOT touch
//! Postgres — they only exercise pure value construction. Schema-existence
//! validation is covered separately in Task 4.

use kronos_embedded_worker::WorkerBuilder;

// Helper: dummy pool that's never connected. Builder construction must not
// require a live DB.
fn dummy_pool() -> sqlx::PgPool {
    sqlx::PgPool::connect_lazy("postgres://example:example@127.0.0.1:1/none")
        .expect("connect_lazy should not error on a syntactically valid url")
}

#[test]
fn library_defaults_use_kronos_namespace() {
    let b = WorkerBuilder::new(dummy_pool());
    assert_eq!(b.system_schema_for_test(), "kronos");
    assert_eq!(b.tenant_schema_prefix_for_test(), "kronos_");
    assert_eq!(b.max_concurrent_for_test(), 50);
    assert_eq!(b.poll_interval_ms_for_test(), 200);
    assert_eq!(b.config_cache_ttl_sec_for_test(), 60);
    assert_eq!(b.secret_cache_ttl_sec_for_test(), 300);
    assert_eq!(b.shutdown_timeout_sec_for_test(), 30);
}

#[test]
fn setters_override_defaults() {
    let b = WorkerBuilder::new(dummy_pool())
        .system_schema("acme".into())
        .tenant_schema_prefix("acme_".into())
        .max_concurrent(7)
        .poll_interval_ms(1234)
        .config_cache_ttl_sec(11)
        .secret_cache_ttl_sec(22)
        .shutdown_timeout_sec(33)
        .encryption_key("0".repeat(64));

    assert_eq!(b.system_schema_for_test(), "acme");
    assert_eq!(b.tenant_schema_prefix_for_test(), "acme_");
    assert_eq!(b.max_concurrent_for_test(), 7);
    assert_eq!(b.poll_interval_ms_for_test(), 1234);
    assert_eq!(b.config_cache_ttl_sec_for_test(), 11);
    assert_eq!(b.secret_cache_ttl_sec_for_test(), 22);
    assert_eq!(b.shutdown_timeout_sec_for_test(), 33);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kronos-embedded-worker --test builder`
Expected: FAIL with "cannot find type WorkerBuilder" or equivalent.

- [ ] **Step 3: Add `crates/embedded-worker/src/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("invalid schema config: {0}")]
    InvalidSchemaConfig(String),

    #[error("system schema {schema:?} is missing the required table {table:?}; \
             run kronos-migrate or call kronos_client::migrate before starting the worker")]
    SystemSchemaMissing { schema: String, table: String },

    #[error("encryption_key is required but was not provided")]
    EncryptionKeyMissing,

    #[error("database error during worker build: {0}")]
    Database(#[from] sqlx::Error),
}
```

- [ ] **Step 4: Add `crates/embedded-worker/src/builder.rs`**

```rust
use kronos_common::config::AppConfig;
use kronos_common::schema_config::SchemaConfig;
use sqlx::PgPool;

/// Builder for a [`Worker`]. See `Worker::builder`.
pub struct WorkerBuilder {
    pub(crate) pool: PgPool,
    pub(crate) system_schema: String,
    pub(crate) tenant_schema_prefix: String,
    pub(crate) max_concurrent: usize,
    pub(crate) poll_interval_ms: u64,
    pub(crate) config_cache_ttl_sec: u64,
    pub(crate) secret_cache_ttl_sec: u64,
    pub(crate) shutdown_timeout_sec: u64,
    pub(crate) encryption_key: Option<String>,
    pub(crate) install_metrics_recorder: bool,
    pub(crate) metrics_port: u16,
}

impl WorkerBuilder {
    pub fn new(pool: PgPool) -> Self {
        let defaults = SchemaConfig::library_default();
        Self {
            pool,
            system_schema: defaults.system_schema,
            tenant_schema_prefix: defaults.tenant_schema_prefix,
            max_concurrent: 50,
            poll_interval_ms: 200,
            config_cache_ttl_sec: 60,
            secret_cache_ttl_sec: 300,
            shutdown_timeout_sec: 30,
            encryption_key: None,
            install_metrics_recorder: false,
            metrics_port: 9090,
        }
    }

    pub fn system_schema(mut self, v: String) -> Self {
        self.system_schema = v;
        self
    }
    pub fn tenant_schema_prefix(mut self, v: String) -> Self {
        self.tenant_schema_prefix = v;
        self
    }
    pub fn max_concurrent(mut self, v: usize) -> Self {
        self.max_concurrent = v;
        self
    }
    pub fn poll_interval_ms(mut self, v: u64) -> Self {
        self.poll_interval_ms = v;
        self
    }
    pub fn config_cache_ttl_sec(mut self, v: u64) -> Self {
        self.config_cache_ttl_sec = v;
        self
    }
    pub fn secret_cache_ttl_sec(mut self, v: u64) -> Self {
        self.secret_cache_ttl_sec = v;
        self
    }
    pub fn shutdown_timeout_sec(mut self, v: u64) -> Self {
        self.shutdown_timeout_sec = v;
        self
    }
    pub fn encryption_key(mut self, v: String) -> Self {
        self.encryption_key = Some(v);
        self
    }
    pub fn install_metrics_recorder(mut self, v: bool) -> Self {
        self.install_metrics_recorder = v;
        self
    }
    pub fn metrics_port(mut self, v: u16) -> Self {
        self.metrics_port = v;
        self
    }

    /// Adapter that copies env-derived config into the builder. Used by the
    /// `kronos-worker` binary to preserve service-mode defaults
    /// (`system_schema = "public"`, `tenant_schema_prefix = ""`).
    pub fn from_app_config(mut self, cfg: &AppConfig) -> Self {
        self.system_schema = cfg.schema.system_schema.clone();
        self.tenant_schema_prefix = cfg.schema.tenant_schema_prefix.clone();
        self.max_concurrent = cfg.worker.max_concurrent;
        self.poll_interval_ms = cfg.worker.poll_interval_ms;
        self.config_cache_ttl_sec = cfg.worker.config_cache_ttl_sec;
        self.secret_cache_ttl_sec = cfg.worker.secret_cache_ttl_sec;
        self.shutdown_timeout_sec = cfg.worker.shutdown_timeout_sec;
        self.encryption_key = Some(cfg.crypto.encryption_key.clone());
        self.metrics_port = cfg.metrics.port;
        self
    }

    // Test-only accessors so unit tests don't have to go through `build()`.
    #[doc(hidden)]
    pub fn system_schema_for_test(&self) -> &str { &self.system_schema }
    #[doc(hidden)]
    pub fn tenant_schema_prefix_for_test(&self) -> &str { &self.tenant_schema_prefix }
    #[doc(hidden)]
    pub fn max_concurrent_for_test(&self) -> usize { self.max_concurrent }
    #[doc(hidden)]
    pub fn poll_interval_ms_for_test(&self) -> u64 { self.poll_interval_ms }
    #[doc(hidden)]
    pub fn config_cache_ttl_sec_for_test(&self) -> u64 { self.config_cache_ttl_sec }
    #[doc(hidden)]
    pub fn secret_cache_ttl_sec_for_test(&self) -> u64 { self.secret_cache_ttl_sec }
    #[doc(hidden)]
    pub fn shutdown_timeout_sec_for_test(&self) -> u64 { self.shutdown_timeout_sec }
}
```

- [ ] **Step 5: Wire the new modules into `crates/embedded-worker/src/lib.rs`**

```rust
//! Kronos worker pipeline as an embeddable library. Moved from `kronos-worker`
//! in Plan 2 of the embedded-mode initiative.

pub mod backoff;
pub mod dispatcher;
pub mod pipeline;
pub mod poller;

mod builder;
mod error;

pub use builder::WorkerBuilder;
pub use error::BuildError;
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p kronos-embedded-worker --test builder`
Expected: PASS for both tests.

- [ ] **Step 7: Add an `from_app_config` regression test**

Append to `crates/embedded-worker/tests/builder.rs`:

```rust
#[tokio::test]
async fn from_app_config_pulls_through_service_defaults() {
    // Drive the binary's env-derived path with the canonical service defaults.
    std::env::set_var("TE_DATABASE_URL", "postgres://e:e@127.0.0.1:1/none");
    std::env::remove_var("TE_SYSTEM_SCHEMA");
    std::env::remove_var("TE_TENANT_SCHEMA_PREFIX");
    std::env::set_var("TE_ENCRYPTION_KEY", "0".repeat(64));
    let cfg = kronos_common::config::AppConfig::from_env()
        .await
        .expect("AppConfig::from_env should succeed with a syntactically valid TE_DATABASE_URL");

    let b = WorkerBuilder::new(dummy_pool()).from_app_config(&cfg);
    assert_eq!(b.system_schema_for_test(), "public");
    assert_eq!(b.tenant_schema_prefix_for_test(), "");
    assert_eq!(b.max_concurrent_for_test(), 50);
    assert_eq!(b.poll_interval_ms_for_test(), 200);

    std::env::remove_var("TE_DATABASE_URL");
    std::env::remove_var("TE_ENCRYPTION_KEY");
}
```

- [ ] **Step 8: Run the new test**

Run: `cargo test -p kronos-embedded-worker --test builder from_app_config_pulls_through_service_defaults -- --test-threads=1`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/embedded-worker/src/builder.rs crates/embedded-worker/src/error.rs crates/embedded-worker/src/lib.rs crates/embedded-worker/tests/builder.rs
git commit -m "feat(embedded-worker): add WorkerBuilder with library defaults and from_app_config"
```

---

### Task 4: `Worker::build()` validates schema config and probes system schema

**Files:**
- Modify: `crates/embedded-worker/src/builder.rs` (add `build()` method)
- Create: `crates/embedded-worker/src/worker.rs` (`Worker` struct + private `WorkerConfig`)
- Modify: `crates/embedded-worker/src/lib.rs` (re-export `Worker`, add `Worker::builder(pool)`)
- Modify: `crates/embedded-worker/tests/builder.rs` (add DB-backed validation tests)

- [ ] **Step 1: Write the failing tests**

Append to `crates/embedded-worker/tests/builder.rs`:

```rust
//! These tests need a running Postgres at TE_DATABASE_URL.
//! Run with: `cargo test -p kronos-embedded-worker --test builder -- --test-threads=1`
use kronos_embedded_worker::BuildError;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

#[tokio::test]
#[ignore]
async fn build_rejects_invalid_schema_config() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let err = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public; DROP TABLE x".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect_err("build must reject SQL-injection-style schema names");
    assert!(matches!(err, BuildError::InvalidSchemaConfig(_)));
}

#[tokio::test]
#[ignore]
async fn build_rejects_missing_system_schema() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    // Use an identifier that's syntactically valid but extremely unlikely to exist.
    let err = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("kronos_does_not_exist_42".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect_err("build must reject when system schema is missing");
    match err {
        BuildError::SystemSchemaMissing { schema, table } => {
            assert_eq!(schema, "kronos_does_not_exist_42");
            assert_eq!(table, "organizations");
        }
        other => panic!("expected SystemSchemaMissing, got {other:?}"),
    }
}

#[tokio::test]
#[ignore]
async fn build_succeeds_with_default_public_schema() {
    // Service-mode defaults: system_schema = "public" with the migrated DB.
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let _worker = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public".into())
        .tenant_schema_prefix("".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect("build should succeed against a migrated public schema");
}

#[tokio::test]
#[ignore]
async fn build_rejects_missing_encryption_key() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let err = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public".into())
        .tenant_schema_prefix("".into())
        .build()
        .await
        .expect_err("build must reject when encryption_key is unset");
    assert!(matches!(err, BuildError::EncryptionKeyMissing));
}
```

- [ ] **Step 2: Run the new tests with `--ignored` to verify they fail compilation/runtime**

Run: `cargo test -p kronos-embedded-worker --test builder -- --ignored --test-threads=1`
Expected: FAIL with "cannot find Worker" or "cannot find function builder".

- [ ] **Step 3: Add `crates/embedded-worker/src/worker.rs`**

```rust
use sqlx::PgPool;

/// A configured Kronos worker. Construct via [`Worker::builder`] and run with
/// [`Worker::run_until_ctrl_c`] (added in Task 5) or [`Worker::start`] (Task 5).
pub struct Worker {
    pub(crate) pool: PgPool,
    pub(crate) cfg: WorkerConfig,
}

/// Internal config built by [`crate::builder::WorkerBuilder::build`]. Holds
/// validated values; intentionally not public — callers shape it via the builder.
#[derive(Clone)]
pub(crate) struct WorkerConfig {
    pub(crate) system_schema: String,
    pub(crate) tenant_schema_prefix: String,
    pub(crate) max_concurrent: usize,
    pub(crate) poll_interval_ms: u64,
    pub(crate) config_cache_ttl_sec: u64,
    pub(crate) secret_cache_ttl_sec: u64,
    pub(crate) shutdown_timeout_sec: u64,
    pub(crate) encryption_key: String,
}

impl Worker {
    /// Start a builder for a Worker bound to `pool`.
    pub fn builder(pool: PgPool) -> crate::builder::WorkerBuilder {
        crate::builder::WorkerBuilder::new(pool)
    }
}
```

- [ ] **Step 4: Add `build()` to `WorkerBuilder`**

Append to `crates/embedded-worker/src/builder.rs`:

```rust
use kronos_common::schema_config::SchemaConfig;
use crate::error::BuildError;
use crate::worker::{Worker, WorkerConfig};

impl WorkerBuilder {
    /// Validate the config, probe the system schema, and produce a [`Worker`].
    /// When `install_metrics_recorder(true)` was called, the metrics recorder
    /// is installed on `metrics_port` exactly once before returning.
    pub async fn build(self) -> Result<Worker, BuildError> {
        // 1. Schema-name shape validation (no DB call).
        let cfg = SchemaConfig {
            system_schema: self.system_schema.clone(),
            tenant_schema_prefix: self.tenant_schema_prefix.clone(),
        };
        cfg.validate().map_err(BuildError::InvalidSchemaConfig)?;

        // 2. Encryption key required for v1.
        let encryption_key = self
            .encryption_key
            .clone()
            .ok_or(BuildError::EncryptionKeyMissing)?;

        // 3. System-schema existence probe via to_regclass (null-safe; no parse error
        //    when schema or table is missing). system_schema is already shape-validated,
        //    so quoting it is safe.
        let qualified_orgs = format!("\"{}\".organizations", self.system_schema);
        let qualified_ws = format!("\"{}\".workspaces", self.system_schema);
        let probe: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT to_regclass($1)::text, to_regclass($2)::text",
        )
        .bind(&qualified_orgs)
        .bind(&qualified_ws)
        .fetch_one(&self.pool)
        .await?;

        if probe.0.is_none() {
            return Err(BuildError::SystemSchemaMissing {
                schema: self.system_schema.clone(),
                table: "organizations".into(),
            });
        }
        if probe.1.is_none() {
            return Err(BuildError::SystemSchemaMissing {
                schema: self.system_schema.clone(),
                table: "workspaces".into(),
            });
        }

        // 4. Optional metrics recorder install — service-binary opt-in.
        if self.install_metrics_recorder {
            kronos_common::metrics::install_recorder_with_listener(self.metrics_port);
        }

        Ok(Worker {
            pool: self.pool,
            cfg: WorkerConfig {
                system_schema: self.system_schema,
                tenant_schema_prefix: self.tenant_schema_prefix,
                max_concurrent: self.max_concurrent,
                poll_interval_ms: self.poll_interval_ms,
                config_cache_ttl_sec: self.config_cache_ttl_sec,
                secret_cache_ttl_sec: self.secret_cache_ttl_sec,
                shutdown_timeout_sec: self.shutdown_timeout_sec,
                encryption_key,
            },
        })
    }
}
```

- [ ] **Step 5: Re-export `Worker`** from `crates/embedded-worker/src/lib.rs`:

```rust
mod worker;
pub use worker::Worker;
```

(Add this beside the existing `mod builder;` / `pub use builder::WorkerBuilder;` block.)

- [ ] **Step 6: Run the validation tests against a migrated DB**

Run: `just db-reset && cargo test -p kronos-embedded-worker --test builder -- --ignored --test-threads=1`
Expected: all four ignored tests PASS.

- [ ] **Step 7: Run the workspace build with all features**

Run: `cargo build --workspace --all-features`
Expected: success.

- [ ] **Step 8: Commit**

```bash
git add crates/embedded-worker/src/builder.rs crates/embedded-worker/src/error.rs crates/embedded-worker/src/worker.rs crates/embedded-worker/src/lib.rs crates/embedded-worker/tests/builder.rs
git commit -m "feat(embedded-worker): Worker::build validates SchemaConfig and probes system schema"
```

---

### Task 5: Refactor poller to take an external shutdown future; add `Worker::run_until_ctrl_c` and `Worker::start` + `WorkerHandle`

The existing `poller::run(pool, config)` hard-codes `tokio::signal::ctrl_c()`. Refactor to a private `run_loop(pool, cfg, shutdown_fut)`. The service binary path goes through `Worker::run_until_ctrl_c`, which passes a `ctrl_c()` future. Embedded callers go through `Worker::start`, which passes a oneshot receiver.

**Files:**
- Modify: `crates/embedded-worker/src/poller.rs` (split `run` into `run_loop` taking shutdown future + `WorkerConfig`)
- Create: `crates/embedded-worker/src/handle.rs` (`WorkerHandle` + shutdown channel)
- Modify: `crates/embedded-worker/src/worker.rs` (add `run_until_ctrl_c` and `start`)
- Modify: `crates/embedded-worker/src/lib.rs` (re-export `WorkerHandle`)

- [ ] **Step 1: Refactor `poller.rs`** — change the public signature so it takes the new private config + an external shutdown future. The body is otherwise identical: same loop, same select, same `claim_and_process` helper.

Replace `pub async fn run(pool: PgPool, config: AppConfig) -> anyhow::Result<()>` with:

```rust
use crate::worker::WorkerConfig;

pub(crate) async fn run_loop<F>(
    pool: sqlx::PgPool,
    cfg: WorkerConfig,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()>,
{
    let worker_id = format!("worker_{}", uuid::Uuid::new_v4().simple());
    let semaphore = Arc::new(Semaphore::new(cfg.max_concurrent));
    let poll_interval = Duration::from_millis(cfg.poll_interval_ms);
    let schema_registry = SchemaRegistry::new(
        pool.clone(),
        cfg.system_schema.clone(),
        30,
    );

    let ctx = Arc::new(PipelineContext {
        pool: pool.clone(),
        http_client: Client::new(),
        config_cache: ConfigCache::new(cfg.config_cache_ttl_sec),
        secret_cache: SecretCache::new(cfg.secret_cache_ttl_sec),
        encryption_key: cfg.encryption_key.clone(),
    });

    tracing::info!(worker_id = %worker_id, "Worker polling started");

    let idle = Arc::new(AtomicBool::new(false));

    tokio::pin!(shutdown);

    loop {
        if idle.load(Ordering::Relaxed) {
            tokio::time::sleep(poll_interval).await;
            idle.store(false, Ordering::Relaxed);
        }

        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("Shutting down worker, waiting for in-flight tasks...");
                let timeout = Duration::from_secs(cfg.shutdown_timeout_sec);
                let _ = tokio::time::timeout(timeout, async {
                    let _all = semaphore.acquire_many(cfg.max_concurrent as u32).await;
                }).await;
                tracing::info!("Worker shutdown complete");
                return Ok(());
            }
            permit = semaphore.clone().acquire_owned() => {
                let permit = permit?;

                let schemas = match schema_registry.get_active_schemas().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to fetch active schemas: {}", e);
                        drop(permit);
                        tokio::time::sleep(poll_interval).await;
                        continue;
                    }
                };

                let pool = pool.clone();
                let ctx = ctx.clone();
                let wid = worker_id.clone();
                let idle = idle.clone();

                tokio::spawn(async move {
                    let found = claim_and_process(&pool, &ctx, &schemas, &wid).await;
                    if !found {
                        metrics::counter!(m::WORKER_POLL_IDLE_TOTAL,
                            "worker_id" => wid,
                        )
                        .increment(1);
                        idle.store(true, Ordering::Relaxed);
                    }
                    drop(permit);
                });
            }
        }
    }
}
```

Drop the `use kronos_common::config::AppConfig;` import — it's no longer needed inside `poller.rs`. The `claim_and_process` helper below it is unchanged.

- [ ] **Step 2: Add `crates/embedded-worker/src/handle.rs`**

```rust
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Handle to a running worker. The worker continues until `shutdown()` is
/// called (graceful) or the handle is dropped (immediate task abort).
pub struct WorkerHandle {
    pub(crate) shutdown_tx: Option<oneshot::Sender<()>>,
    pub(crate) join: JoinHandle<anyhow::Result<()>>,
}

impl WorkerHandle {
    /// Send the shutdown signal and wait for the worker loop to drain in-flight
    /// tasks. Returns the worker's final result (`Ok(())` on clean exit).
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            // Receiver may have been dropped already; that's fine.
            let _ = tx.send(());
        }
        match self.join.await {
            Ok(res) => res,
            Err(join_err) => Err(anyhow::anyhow!("worker task panicked: {join_err}")),
        }
    }
}
```

- [ ] **Step 3: Add `Worker::run_until_ctrl_c` and `Worker::start`** to `crates/embedded-worker/src/worker.rs`:

```rust
use crate::handle::WorkerHandle;
use tokio::sync::oneshot;

impl Worker {
    /// Run the worker loop until SIGINT (Ctrl-C). Service-binary convenience.
    /// Embedded hosts that need their own shutdown story should use [`Worker::start`].
    pub async fn run_until_ctrl_c(self) -> anyhow::Result<()> {
        let shutdown = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        crate::poller::run_loop(self.pool, self.cfg, shutdown).await
    }

    /// Spawn the worker loop on the current Tokio runtime and return a handle.
    /// The handle's `shutdown()` triggers a graceful drain bounded by
    /// `shutdown_timeout_sec`.
    pub fn start(self) -> WorkerHandle {
        let (tx, rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            let shutdown = async move {
                let _ = rx.await;
            };
            crate::poller::run_loop(self.pool, self.cfg, shutdown).await
        });
        WorkerHandle {
            shutdown_tx: Some(tx),
            join,
        }
    }
}
```

- [ ] **Step 4: Re-export `WorkerHandle`** from `crates/embedded-worker/src/lib.rs`:

```rust
mod handle;
pub use handle::WorkerHandle;
```

- [ ] **Step 5: Update the temporary shim** in `crates/worker/src/lib.rs` so it still compiles. The shim now needs to expose `poller::run` with the old signature, since `main.rs` still calls it. Replace the shim body with a wrapper that constructs the builder internally — this keeps Task 2's main.rs passing until Task 7 rewrites it.

```rust
//! Temporary shim for Plan 2. Deleted in Task 7.
use kronos_common::config::AppConfig;
use sqlx::PgPool;

pub mod poller {
    use super::*;
    pub async fn run(pool: PgPool, config: AppConfig) -> anyhow::Result<()> {
        kronos_embedded_worker::Worker::builder(pool)
            .from_app_config(&config)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!(e))?
            .run_until_ctrl_c()
            .await
    }
}
```

- [ ] **Step 6: Build and run integration tests**

Run: `cargo build --workspace --all-features`
Expected: success.

Run: `just db-reset && just test-immediate`
Expected: PASS. (This is the load-bearing contract — the refactored poller must claim, dispatch, and complete a job exactly as before.)

- [ ] **Step 7: Add a graceful-shutdown integration test**

Create `crates/embedded-worker/tests/shutdown.rs`:

```rust
//! Verifies `Worker::start()` + `WorkerHandle::shutdown()` drains and returns
//! Ok(()) under a normal start/stop cycle. Requires a migrated DB.
//! Run with: `cargo test -p kronos-embedded-worker --test shutdown -- --ignored --test-threads=1`

use std::time::Duration;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

#[tokio::test]
#[ignore]
async fn start_then_shutdown_returns_clean() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let worker = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public".into())
        .tenant_schema_prefix("".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect("build against migrated public schema");

    let handle = worker.start();
    // Let the loop spin a few times so we exercise the shutdown branch from
    // an active poll cycle, not just the first iteration.
    tokio::time::sleep(Duration::from_millis(500)).await;
    handle.shutdown().await.expect("graceful shutdown returns Ok");
}
```

- [ ] **Step 8: Run the new test**

Run: `cargo test -p kronos-embedded-worker --test shutdown -- --ignored --test-threads=1`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/embedded-worker/src/poller.rs crates/embedded-worker/src/handle.rs crates/embedded-worker/src/worker.rs crates/embedded-worker/src/lib.rs crates/worker/src/lib.rs crates/embedded-worker/tests/shutdown.rs
git commit -m "feat(embedded-worker): poller takes shutdown future; add run_until_ctrl_c and start/handle"
```

---

### Task 6: Shrink `kronos-worker` to a binary-only crate driving the builder

**Files:**
- Modify: `crates/worker/src/main.rs` (rewrite to ~15-line builder driver)
- Modify: `crates/worker/Cargo.toml` (rewrite features to pass through to embedded-worker; drop the now-unused `[lib]` autodiscovery by deleting `lib.rs`)
- Delete: `crates/worker/src/lib.rs`

- [ ] **Step 1: Rewrite `crates/worker/src/main.rs`**

```rust
use kronos_common::config::AppConfig;
use kronos_embedded_worker::Worker;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("kronos=debug".parse()?))
        .json()
        .init();

    let config = AppConfig::from_env().await?;
    let pool = sqlx::PgPool::connect(&config.db.url).await?;

    tracing::info!("Worker starting (metrics on port {})", config.metrics.port);

    Worker::builder(pool)
        .from_app_config(&config)
        .install_metrics_recorder(true)
        .build()
        .await?
        .run_until_ctrl_c()
        .await
}
```

- [ ] **Step 2: Delete the shim**

```bash
git rm crates/worker/src/lib.rs
```

- [ ] **Step 3: Rewrite `crates/worker/Cargo.toml`** features to pass through to embedded-worker. Final manifest:

```toml
[package]
name = "kronos-worker"
version.workspace = true
edition.workspace = true

[[bin]]
name = "kronos-worker"
path = "src/main.rs"

[features]
default = []
kafka = ["kronos-embedded-worker/kafka"]
redis-stream = ["kronos-embedded-worker/redis-stream"]
kms = ["kronos-embedded-worker/kms", "kronos-common/kms"]

[dependencies]
kronos-common = { path = "../common" }
kronos-embedded-worker = { path = "../embedded-worker" }
tokio.workspace = true
sqlx.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
dotenvy.workspace = true
anyhow.workspace = true
```

- [ ] **Step 4: Build the workspace**

Run: `cargo build --workspace --all-features`
Expected: success.

- [ ] **Step 5: Verify `kronos-worker` builds with each feature individually**

Run:
```bash
cargo build -p kronos-worker
cargo build -p kronos-worker --features kafka
cargo build -p kronos-worker --features redis-stream
cargo build -p kronos-worker --features kafka,redis-stream
```
Expected: all succeed. (The features must reach the dispatcher modules in `kronos-embedded-worker` via the pass-through.)

- [ ] **Step 6: Confirm the binary still runs end-to-end**

Run: `just db-reset && just test-immediate`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/worker/src/main.rs crates/worker/Cargo.toml
git commit -m "refactor(worker): shrink to binary-only crate driving the embedded-worker builder"
```

---

### Task 7: Verify behavior preservation across the full integration suite

**Files:** none modified — this task is verification only.

- [ ] **Step 1: Reset the database to a clean migrated state**

Run: `just db-reset`
Expected: all migrations apply cleanly (Plan 1 already exercises this path).

- [ ] **Step 2: Run the immediate-execution integration test**

Run: `just test-immediate`
Expected: PASS. Worker claims the job, dispatches via HTTP, marks SUCCESS within the test's deadline.

- [ ] **Step 3: Run the delayed-execution integration test**

Run: `just test-delayed`
Expected: PASS. Worker waits for `run_at`, then claims and dispatches.

- [ ] **Step 4: Run the cron-trigger integration test**

Run: `just test-cron`
Expected: PASS. pg_cron schedules execution rows; worker drains them.

- [ ] **Step 5: Run the end-to-end suite**

Run: `just test-e2e`
Expected: PASS for the components on this branch. (Note: the spec called out that `just test-e2e` references a not-yet-extant `kronos-scheduler` crate on this branch; if `test-e2e` fails on that ground alone — same failure as on the parent `feat/embedded-mode` branch — record it as a known pre-existing issue rather than a Plan 2 regression.)

- [ ] **Step 6: Build with each cargo feature combination once more**

Run:
```bash
cargo build --workspace
cargo build --workspace --all-features
cargo build -p kronos-worker
cargo build -p kronos-worker --features kafka
cargo build -p kronos-worker --features redis-stream
cargo build -p kronos-embedded-worker
cargo build -p kronos-embedded-worker --all-features
```
Expected: all succeed.

- [ ] **Step 7: Run all unit tests**

Run: `cargo test --workspace --lib`
Expected: PASS.

- [ ] **Step 8: Sanity-check metrics names didn't drift** by grepping for the constants the moved files use:

Run: `cargo build --workspace --all-features 2>&1 | rg -i 'metrics::counter|metrics::histogram|metrics::gauge' || true`
(There should be no compile errors. The metric *names* are constants in `kronos_common::metrics` — unchanged.)

- [ ] **Step 9: Commit a verification note** if any pre-existing issue surfaced; otherwise no commit.

---

### Task 8: Open the stacked draft PR

**Files:** none.

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/worker-extraction
```

- [ ] **Step 2: Create a stacked draft PR** against `feat/embedded-mode` (the Plan 1 PR), not `main`. This makes the diff readable as the W1 phase only.

```bash
gh pr create \
  --base feat/embedded-mode \
  --head feat/worker-extraction \
  --draft \
  --title "Worker extraction (Plan 2)" \
  --body "$(cat <<'EOF'
## Summary
- Moves the worker pipeline (`poller`, `pipeline`, `backoff`, `dispatcher`) from `crates/worker/` into `crates/embedded-worker/`.
- Introduces `Worker::builder(pool)` + `WorkerHandle`, with library defaults `system_schema = "kronos"` / `tenant_schema_prefix = "kronos_"` and binary-side `from_app_config(&AppConfig)` preserving service-mode `public` / `""`.
- `kronos-worker` becomes a binary-only crate (~15 lines).
- `Worker::build()` validates `SchemaConfig` and probes the system schema's `organizations` and `workspaces` tables, failing fast on misconfiguration.
- Behavior is preserved: same poll cadence, same claim semantics, same retry/backoff math, same dispatcher logic, same metrics names and labels.

## Stacked PR context
This PR targets `feat/embedded-mode` (Plan 1). It will be merged after the Plan 1 PR; reviewers should diff against `feat/embedded-mode`, not `main`.

## Test plan
- [x] `cargo build --workspace --all-features`
- [x] `cargo test --workspace --lib`
- [x] `cargo test -p kronos-embedded-worker --test builder -- --ignored --test-threads=1`
- [x] `cargo test -p kronos-embedded-worker --test shutdown -- --ignored --test-threads=1`
- [x] `just test-immediate`
- [x] `just test-delayed`
- [x] `just test-cron`
- [x] `just test-e2e` (any failure mode pre-existing on `feat/embedded-mode` is noted, not a regression)

## Spec
`docs/superpowers/specs/2026-04-29-kronos-embedded-mode-design.md` — Plan 2.
EOF
)"
```

- [ ] **Step 2: Capture the PR URL** in the task notes.

---

## Self-review notes

- All seven implementation tasks include exact code, exact commands, and expected outputs.
- The poller refactor is the only behavior-touching change; it preserves the loop body verbatim and only externalizes the shutdown future. The `run_until_ctrl_c` path passes the same `tokio::signal::ctrl_c()` future the original code used.
- Metric names and labels are not edited — the moved files import `kronos_common::metrics as m` and reference the same constants.
- Schema validation reuses `SchemaConfig::validate()` from Plan 1 and a `to_regclass` probe; no new validation logic is introduced.
- The `from_app_config` adapter copies `cfg.schema.system_schema` / `cfg.schema.tenant_schema_prefix` directly, so service-mode defaults (`public` / `""` from `SchemaEnv`) reach the worker unchanged.
- Feature pass-through (`kafka`, `redis-stream`, `kms`) flows from `kronos-worker` → `kronos-embedded-worker`, so existing build commands keep working.
- `WorkerBuilder::build()` is `async` because the schema-existence probe needs a DB roundtrip; the spec snippet (`.build().await?`) matches.
- `WorkerHandle::shutdown()` drains by sending on the oneshot and awaiting the join handle — bounded by `shutdown_timeout_sec` inside the loop's existing timeout block.

## Execution

After saving this plan, the controller (you) chooses execution mode. Recommended: **Subagent-Driven Development** in this session — fresh implementer subagent per task, two-stage review (spec compliance → code quality) after each, fix loops until both pass.
