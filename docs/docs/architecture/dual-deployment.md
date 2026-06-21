---
id: dual-deployment
title: Dual Deployment Modes
---

# Dual Deployment Modes

Kronos supports two deployment modes: **library mode** (embedded in-process) and **service mode** (standalone REST API). Both modes expose the same API through the `KronosClient` trait, so call sites are transparent to the deployment mode — switching requires only environment variable changes, no code changes.

## KronosClient Trait

The `KronosClient` trait abstracts over both deployment modes. It defines the full set of operations for managing Kronos resources:

```rust
#[async_trait]
pub trait KronosClient: Send + Sync {
    async fn upsert_secret(&self, schema_name: &str, name: &str, plaintext: &str) -> anyhow::Result<()>;
    async fn delete_secret(&self, schema_name: &str, name: &str) -> anyhow::Result<()>;
    async fn register_endpoint(&self, schema_name: &str, name: &str, endpoint_type: &str, spec: serde_json::Value, retry_policy: Option<serde_json::Value>) -> anyhow::Result<()>;
    async fn delete_endpoint(&self, schema_name: &str, name: &str) -> anyhow::Result<()>;
    async fn create_job(&self, schema_name: &str, endpoint: &str, input: serde_json::Value, max_attempts: i64, trigger: JobTrigger, idempotency_key: Option<&str>) -> anyhow::Result<String>;
    async fn provision_workspace(&self, schema_name: &str) -> anyhow::Result<()>;
    async fn cancel_job(&self, schema_name: &str, job_id: &str) -> anyhow::Result<()>;
    async fn get_execution(&self, schema_name: &str, execution_id: &str) -> anyhow::Result<Option<Execution>>;
}
```

| Method | Library Mode | Service Mode |
|--------|-------------|-------------|
| `upsert_secret` | Encrypts and writes directly to DB | POST/PUT to `/v1/secrets` |
| `delete_secret` | Deletes from DB | DELETE to `/v1/secrets/{name}` |
| `register_endpoint` | Upserts in DB | PUT/POST to `/v1/endpoints` |
| `delete_endpoint` | Deletes from DB | DELETE to `/v1/endpoints/{name}` |
| `create_job` | Inserts job + execution directly | POST to `/v1/jobs` |
| `provision_workspace` | Applies `workspace_v1.sql` template | POST to `/v1/orgs/{id}/workspaces` |
| `cancel_job` | Updates DB + unschedules pg_cron | POST to `/v1/jobs/{id}/cancel` |
| `get_execution` | Queries DB directly | GET to `/v1/executions/{id}` |

## JobTrigger Enum

Both modes accept the same `JobTrigger` enum for job creation:

```rust
pub enum JobTrigger {
    /// Fire immediately, create a QUEUED execution right away.
    Immediate,
    /// Fire at a specific future time.
    Delayed { run_at: DateTime<Utc> },
    /// Recurring CRON schedule.
    Cron {
        expression: String,
        timezone: String,
        starts_at: Option<DateTime<Utc>>,
        ends_at: Option<DateTime<Utc>>,
        first_run_at: DateTime<Utc>,
    },
}
```

## Library Mode (KronosLibraryClient)

Library mode embeds Kronos directly into your application process. It holds a caller-provided `PgPool` and accesses the database directly — no HTTP overhead.

### Creating a Library Client

```rust
use kronos_worker::KronosLibraryClient;

let client = KronosLibraryClient::new(
    pool,                    // caller-owned sqlx PgPool
    "sched",                 // table prefix (e.g. "sched" → sched_jobs)
    "64_hex_chars...",       // AES encryption key for secrets
    Some(http_client),       // optional reqwest client to reuse
)?;
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `pool` | `PgPool` | Caller-owned connection pool pointing at the same PostgreSQL instance |
| `table_prefix` | `&str` | Prefix for all Kronos tables (e.g. `"sched"` → `sched_jobs`). Empty string means no prefix |
| `encryption_key` | `&str` | 64 hex-char AES-256 key for secrets; pass zeros if not using secrets |
| `http_client` | `Option<Client>` | Optional reqwest client to reuse the caller's connection pool |

### Provisioning a Workspace

In library mode, `provision_workspace()` applies the `workspace_v1.sql` template directly to the database, creating all workspace-scoped tables:

```rust
async fn provision_workspace(&self, schema_name: &str) -> anyhow::Result<()> {
    const TEMPLATE: &str = include_str!("../../../migrations/workspace_v1.sql");
    let p = if prefix.is_empty() { String::new() }
            else { format!("{}_", prefix) };
    let ddl = TEMPLATE.replace("{p}", &p);
    // Execute each statement...
}
```

### Starting the Worker

The library client can start a background worker directly via `start_worker()`:

```rust
use kronos_worker::{KronosLibraryClient, WorkerConfig};
use kronos_common::tenant::SchemaRegistry;
use tokio_util::sync::CancellationToken;

let client = KronosLibraryClient::new(pool, "sched", &key, None)?;

let schema_provider = SchemaRegistry::new(pool.clone(), 30);
let cancel = CancellationToken::new();

let handle = client.start_worker(schema_provider, cancel, WorkerConfig::default());
// Worker runs as a tokio task — await handle on shutdown
```

`start_worker()` returns a `tokio::task::JoinHandle` that the caller should await or drop on shutdown.

### WorkerConfig

```rust
pub struct WorkerConfig {
    pub max_concurrent: usize,      // default: 50
    pub poll_interval_ms: u64,       // default: 200
    pub config_cache_ttl_sec: u64,  // default: 60
    pub secret_cache_ttl_sec: u64,  // default: 300
    pub shutdown_timeout_sec: u64,  // default: 30
}
```

## Service Mode (KronosHttpClient)

Service mode communicates with Kronos via the REST API. It's used when Kronos runs as a standalone service:

```rust
use kronos_worker::KronosHttpClient;

let client = KronosHttpClient::new(
    "http://localhost:8080".to_string(),  // base URL
    "dev-api-key".to_string(),            // API key
    "org_id".to_string(),                 // org ID
);
```

### Workspace Routing

Each request includes `x-org-id` and `x-workspace-id` headers for tenant routing:

```rust
fn with_workspace(&self, req: reqwest::RequestBuilder, schema_name: &str) -> reqwest::RequestBuilder {
    req.header("x-org-id", &self.org_id)
       .header("x-workspace-id", schema_name)
}
```

Kronos resolves the workspace by slug (the `schema_name` is used as the workspace slug). The `resolve_schema` function in `db/workspaces.rs` accepts both slug and workspace UUID.

### Provisioning in Service Mode

In service mode, `provision_workspace()` registers the workspace with Kronos by creating it via the API. The org must already exist (created by the operator):

```rust
async fn provision_workspace(&self, schema_name: &str) -> anyhow::Result<()> {
    let resp = self.authed(
        self.http_client.post(self.url(&format!("/orgs/{}/workspaces", self.org_id)))
    )
    .json(&serde_json::json!({ "name": schema_name, "slug": schema_name }))
    .send().await?;

    if status.is_success() || status.as_u16() == 409 {
        return Ok(());  // created or already exists
    }
    // ...
}
```

## When to Use Which Mode

| Criteria | Library Mode | Service Mode |
|----------|-------------|-------------|
| **Latency** | Lowest (no HTTP overhead) | Higher (HTTP round-trip) |
| **Isolation** | Shared process with your app | Fully isolated service |
| **Scalability** | Scales with your app | Scales independently |
| **DB access** | Direct (shared pool) | Via API (Kronos owns its DB) |
| **Deployment** | Embedded in your binary | Separate process/container |
| **Multi-app** | Each app embeds Kronos | Single Kronos serves multiple apps |
| **Worker** | Started in-process via `start_worker()` | Kronos runs its own workers |

:::tip
**Library mode** is ideal when you want to add durable job scheduling to a single Rust application with minimal infrastructure. **Service mode** is better when multiple applications need to share a single Kronos deployment, or when you want to decouple Kronos's operational lifecycle from your application.
:::

## SchemaProvider Trait

The `SchemaProvider` trait tells the worker where to find the list of active workspace schemas. Kronos ships `SchemaRegistry` as the default implementation:

```rust
#[async_trait]
pub trait SchemaProvider: Send + Sync + 'static {
    async fn get_active_schemas(&self) -> Result<Vec<String>, sqlx::Error>;
}
```

### SchemaRegistry (Default)

`SchemaRegistry` queries Kronos's own `public.workspaces` table with a 30-second TTL cache:

```rust
let schemas: Vec<(String,)> = sqlx::query_as(
    "SELECT schema_name FROM public.workspaces WHERE status = 'ACTIVE'"
).fetch_all(&self.pool).await?;
```

### Custom SchemaProvider

For embedding scenarios where the host application maintains its own list of schemas, you can implement `SchemaProvider` to return schemas from your own source:

```rust
struct MyAppSchemaProvider {
    my_config: MyConfig,
}

#[async_trait]
impl SchemaProvider for MyAppSchemaProvider {
    async fn get_active_schemas(&self) -> Result<Vec<String>, sqlx::Error> {
        // Return schemas from your own configuration
        Ok(self.my_config.active_schemas.clone())
    }
}
```

## Table Prefix System

Both deployment modes support a table prefix to avoid collisions when Kronos tables share a schema with other application tables.

### The `tbl()` Function

In the codebase, table names are constructed using a prefix-aware `DbContext`:

```rust
let mut db = DbContext::new(&mut *conn, prefix);
// All queries use db.prefix to construct table names
```

### The `{p}` Placeholder in workspace_v1.sql

The `workspace_v1.sql` template uses `{p}` as a placeholder for the table prefix:

```sql
CREATE TABLE IF NOT EXISTS {p}jobs (
    job_id  TEXT  NOT NULL DEFAULT gen_random_uuid()::TEXT,
    ...
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_{p}jobs_idempotency
    ON {p}jobs (endpoint, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
```

At provisioning time, `{p}` is replaced with either:
- Empty string (no prefix → `jobs`, `idx_jobs_idempotency`)
- `{prefix}_` (e.g. `sched_` → `sched_jobs`, `idx_sched_jobs_idempotency`)

```rust
let p = if prefix.is_empty() { String::new() }
        else { format!("{}_", prefix) };
let ddl = TEMPLATE.replace("{p}", &p);
```

:::info
The table prefix is validated to contain only alphanumeric characters and underscores. An empty prefix is valid and means no prefix is applied.
:::

## Related Pages

- [Architecture Overview](./overview) — System architecture and process topology
- [Worker Pipeline](./worker-pipeline) — How the worker poller operates (used by `start_worker()`)
- [Database Schema](./database-schema) — Full schema layout including the table prefix system
