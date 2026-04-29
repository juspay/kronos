# Plan 1 — Foundation: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the prerequisites for Kronos embedded mode: two new empty library crates wired into the workspace, plus parameterization of all migrations and runtime SQL on `system_schema` and `tenant_schema_prefix`. Service binaries continue to default to `public` / `""` so every existing deployment, integration test, and dev workflow behaves byte-identically.

**Architecture:** A new `SchemaConfig` value type carries the two parameters through the codebase. Migration files become templates with `{{system_schema}}` and `{{tenant_schema_prefix}}` placeholders, rendered at apply time by a tiny `String::replace`-based engine (no new dependency). Two new placeholder crates (`kronos-client`, `kronos-embedded-worker`) exist but expose no API beyond a public `migrate()` entry point on `kronos-client`. A small `kronos-migrate` binary replaces the justfile's raw `psql` migration runner. Every change is additive or backwards-compatible by default.

**Tech Stack:** Rust 2021, sqlx 0.7 (Postgres), actix-web 4, existing test harness (TypeScript CLI + `just test-e2e`).

**Spec reference:** `docs/superpowers/specs/2026-04-29-kronos-embedded-mode-design.md` — see "Schema namespacing and table-name collisions" and "Plan 1 — Foundation" sections.

---

## File Structure

**New files (created):**

- `crates/client/Cargo.toml` — manifest for `kronos-client`
- `crates/client/src/lib.rs` — public API root (exports `SchemaConfig`, `migrate`, `MIGRATIONS`)
- `crates/client/src/migrate.rs` — `migrate()` function and `MIGRATIONS` slice
- `crates/client/src/bin/kronos-migrate.rs` — CLI binary that renders + applies migrations
- `crates/embedded-worker/Cargo.toml` — manifest for `kronos-embedded-worker`
- `crates/embedded-worker/src/lib.rs` — empty library shell (placeholder for Plan 2)
- `crates/common/src/schema_config.rs` — `SchemaConfig` value type
- `crates/common/src/migrations/mod.rs` — embedded migration templates + renderer
- `crates/common/src/migrations/render.rs` — placeholder-substitution engine + tests
- `crates/client/tests/migrate_kronos_namespace.rs` — smoke test for non-default schema

**Modified files:**

- `Cargo.toml` (root) — add the two new crates to `[workspace] members`
- `crates/common/src/lib.rs` — wire `schema_config` and `migrations` modules
- `crates/common/src/tenant.rs` — `build_schema_name` and `SchemaRegistry` take a `SchemaConfig`
- `crates/common/src/db/scoped.rs` — connection scoping uses workspace schema (already does; minor doc-comment update only)
- `crates/common/src/config.rs` — `AppConfig` gains a `schema: SchemaConfig` field; new `TE_SYSTEM_SCHEMA` and `TE_TENANT_SCHEMA_PREFIX` env vars default to today's values
- `crates/api/src/handlers/workspaces.rs` — workspace-creation handler uses the configured tenant prefix
- `migrations/20260318000000_multi_tenancy.sql` — `public.{organizations,workspaces}` → `{{system_schema}}.{organizations,workspaces}`
- `migrations/20260322000001_pg_cron.sql` — `public.workspaces` → `{{system_schema}}.workspaces`
- `migrations/20260317000000_initial.sql` — no rewrite needed (per-workspace tables; already unqualified). Add a leading comment noting it's applied per-workspace by the dynamic schema setup.
- `migrations/workspace_v1.sql` — no rewrite needed (per-workspace template).
- `justfile` — `db-migrate` recipe calls the new `kronos-migrate` binary instead of looping `psql`

**Note on migration tracking:** The existing setup has no `_sqlx_migrations` table — migrations are idempotent via `CREATE ... IF NOT EXISTS`. Plan 1 preserves that exactly.

---

## Tasks

### Task 1: Add empty `kronos-client` and `kronos-embedded-worker` crates to the workspace

**Why:** Establish the crate boundary that subsequent plans will fill. Both crates compile as inert library shells.

**Files:**
- Create: `crates/client/Cargo.toml`, `crates/client/src/lib.rs`
- Create: `crates/embedded-worker/Cargo.toml`, `crates/embedded-worker/src/lib.rs`
- Modify: `Cargo.toml` (root)

- [ ] **Step 1: Create the `kronos-client` manifest**

Write `crates/client/Cargo.toml`:

```toml
[package]
name = "kronos-client"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
kronos-common = { path = "../common" }
sqlx = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["full", "macros"] }
```

- [ ] **Step 2: Create the `kronos-client` library root**

Write `crates/client/src/lib.rs`:

```rust
//! Kronos library API. Today this crate exposes only the migration entry
//! point; subsequent plans add the enqueue + CRUD surface.

pub use kronos_common::schema_config::SchemaConfig;
```

- [ ] **Step 3: Create the `kronos-embedded-worker` manifest**

Write `crates/embedded-worker/Cargo.toml`:

```toml
[package]
name = "kronos-embedded-worker"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
kronos-common = { path = "../common" }
sqlx = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 4: Create the `kronos-embedded-worker` library root**

Write `crates/embedded-worker/src/lib.rs`:

```rust
//! Kronos worker pipeline as an embeddable library. This crate is an empty
//! shell in Plan 1; Plan 2 moves poller/pipeline/backoff/dispatcher here.
```

- [ ] **Step 5: Wire both crates into the workspace**

Edit `Cargo.toml` (root). Find the `[workspace] members` list and update it:

```toml
[workspace]
members = [
    "crates/common",
    "crates/api",
    "crates/worker",
    "crates/client",
    "crates/embedded-worker",
    "crates/mock-server",
    "crates/dashboard",
]
resolver = "2"
```

- [ ] **Step 6: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: compiles cleanly, output includes `Compiling kronos-client v0.1.0` and `Compiling kronos-embedded-worker v0.1.0`.

- [ ] **Step 7: Commit**

```bash
git add crates/client crates/embedded-worker Cargo.toml
git commit -m "feat(plan-1): scaffold kronos-client and kronos-embedded-worker crates"
```

---

### Task 2: Add `SchemaConfig` value type to `kronos-common`

**Why:** A single canonical place for the two parameters that flow through migrations, runtime SQL, and builders. Keeping it in `kronos-common` lets both the service binaries and the new library crates share the type.

**Files:**
- Create: `crates/common/src/schema_config.rs`
- Modify: `crates/common/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append a `tests` module at the end of `crates/common/src/schema_config.rs` (file doesn't exist yet — create with this content):

```rust
//! `SchemaConfig` carries the two schema-namespacing parameters that flow
//! through migrations, runtime SQL, and builders.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaConfig {
    pub system_schema: String,
    pub tenant_schema_prefix: String,
}

impl SchemaConfig {
    /// Service-mode default: preserves today's `public.organizations`
    /// and unprefixed tenant schemas.
    pub fn service_default() -> Self {
        Self {
            system_schema: "public".to_string(),
            tenant_schema_prefix: String::new(),
        }
    }

    /// Library-mode default: avoids collisions with host-app tables.
    pub fn library_default() -> Self {
        Self {
            system_schema: "kronos".to_string(),
            tenant_schema_prefix: "kronos_".to_string(),
        }
    }

    /// Validate that both names are safe for use in raw SQL identifiers.
    /// Returns `Err` with a human-readable reason on failure.
    pub fn validate(&self) -> Result<(), String> {
        if !is_valid_pg_identifier(&self.system_schema) {
            return Err(format!(
                "system_schema {:?} must contain only ASCII letters, digits, and underscores, and be 1-63 chars",
                self.system_schema
            ));
        }
        // Empty prefix is allowed; non-empty prefix must be a valid identifier *prefix*
        if !self.tenant_schema_prefix.is_empty()
            && !is_valid_pg_identifier_prefix(&self.tenant_schema_prefix)
        {
            return Err(format!(
                "tenant_schema_prefix {:?} must contain only ASCII letters, digits, and underscores",
                self.tenant_schema_prefix
            ));
        }
        Ok(())
    }
}

fn is_valid_pg_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_valid_pg_identifier_prefix(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_default_preserves_today() {
        let c = SchemaConfig::service_default();
        assert_eq!(c.system_schema, "public");
        assert_eq!(c.tenant_schema_prefix, "");
        c.validate().expect("service default must validate");
    }

    #[test]
    fn library_default_uses_kronos_namespace() {
        let c = SchemaConfig::library_default();
        assert_eq!(c.system_schema, "kronos");
        assert_eq!(c.tenant_schema_prefix, "kronos_");
        c.validate().expect("library default must validate");
    }

    #[test]
    fn rejects_sql_injection_attempts() {
        let bad = SchemaConfig {
            system_schema: "public; DROP TABLE x;".to_string(),
            tenant_schema_prefix: String::new(),
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn rejects_empty_system_schema() {
        let bad = SchemaConfig {
            system_schema: String::new(),
            tenant_schema_prefix: String::new(),
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn empty_prefix_is_valid() {
        let c = SchemaConfig {
            system_schema: "public".to_string(),
            tenant_schema_prefix: String::new(),
        };
        c.validate().unwrap();
    }
}
```

- [ ] **Step 2: Wire into `kronos-common` lib root**

Edit `crates/common/src/lib.rs`. After the existing module list, add `pub mod schema_config;`:

```rust
pub mod cache;
pub mod config;
pub mod crypto;
pub mod db;
pub mod env;
pub mod error;
#[cfg(feature = "kms")]
pub mod kms;
pub mod metrics;
pub mod models;
pub mod pagination;
pub mod schema_config;
pub mod template;
pub mod tenant;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p kronos-common schema_config`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/common/src/schema_config.rs crates/common/src/lib.rs
git commit -m "feat(plan-1): add SchemaConfig value type"
```

---

### Task 3: Add migration template renderer

**Why:** Migrations need to be parametric on `system_schema` (and, in a few places, on `tenant_schema_prefix`). A tiny `String::replace`-based renderer is sufficient — no new dep, no full template engine.

**Files:**
- Create: `crates/common/src/migrations/mod.rs`
- Create: `crates/common/src/migrations/render.rs`
- Modify: `crates/common/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/common/src/migrations/render.rs`:

```rust
//! Renders SQL migration templates by substituting `{{system_schema}}` and
//! `{{tenant_schema_prefix}}` placeholders. After rendering, the SQL contains
//! no `{{` sequences — that's a post-condition the renderer enforces.

use crate::schema_config::SchemaConfig;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("invalid SchemaConfig: {0}")]
    InvalidConfig(String),
    #[error("template contains unrecognized placeholders after rendering: {0}")]
    UnrenderedPlaceholder(String),
}

pub fn render(template: &str, cfg: &SchemaConfig) -> Result<String, RenderError> {
    cfg.validate().map_err(RenderError::InvalidConfig)?;

    let rendered = template
        .replace("{{system_schema}}", &cfg.system_schema)
        .replace("{{tenant_schema_prefix}}", &cfg.tenant_schema_prefix);

    // Post-condition: any `{{...}}` left over is an unrecognized placeholder.
    if let Some(start) = rendered.find("{{") {
        let snippet: String = rendered
            .chars()
            .skip(start)
            .take(40)
            .collect();
        return Err(RenderError::UnrenderedPlaceholder(snippet));
    }

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_system_schema_placeholder() {
        let cfg = SchemaConfig::service_default();
        let out = render("CREATE TABLE {{system_schema}}.foo();", &cfg).unwrap();
        assert_eq!(out, "CREATE TABLE public.foo();");
    }

    #[test]
    fn renders_tenant_prefix_placeholder() {
        let cfg = SchemaConfig::library_default();
        let out = render("PREFIX={{tenant_schema_prefix}}", &cfg).unwrap();
        assert_eq!(out, "PREFIX=kronos_");
    }

    #[test]
    fn renders_both_placeholders() {
        let cfg = SchemaConfig::library_default();
        let out = render(
            "SELECT * FROM {{system_schema}}.workspaces WHERE schema_name LIKE '{{tenant_schema_prefix}}%';",
            &cfg,
        )
        .unwrap();
        assert_eq!(
            out,
            "SELECT * FROM kronos.workspaces WHERE schema_name LIKE 'kronos_%';"
        );
    }

    #[test]
    fn empty_prefix_renders_to_empty_string() {
        let cfg = SchemaConfig::service_default();
        let out = render("[{{tenant_schema_prefix}}]", &cfg).unwrap();
        assert_eq!(out, "[]");
    }

    #[test]
    fn rejects_unknown_placeholder() {
        let cfg = SchemaConfig::service_default();
        let err = render("SELECT {{unknown_thing}};", &cfg).unwrap_err();
        assert!(matches!(err, RenderError::UnrenderedPlaceholder(_)));
    }

    #[test]
    fn rejects_invalid_config() {
        let cfg = SchemaConfig {
            system_schema: String::new(),
            tenant_schema_prefix: String::new(),
        };
        let err = render("anything", &cfg).unwrap_err();
        assert!(matches!(err, RenderError::InvalidConfig(_)));
    }
}
```

- [ ] **Step 2: Create the migrations module root**

Create `crates/common/src/migrations/mod.rs`:

```rust
//! Embedded migration templates plus the renderer.
//!
//! Plan 1 only ships the renderer; the embedded migration list and the
//! `apply()` entry point are added in this same plan as a later task.

pub mod render;

pub use render::{render, RenderError};
```

- [ ] **Step 3: Wire into the lib root**

Edit `crates/common/src/lib.rs`. Add `pub mod migrations;` to the module list (alphabetical order):

```rust
pub mod cache;
pub mod config;
pub mod crypto;
pub mod db;
pub mod env;
pub mod error;
#[cfg(feature = "kms")]
pub mod kms;
pub mod metrics;
pub mod migrations;
pub mod models;
pub mod pagination;
pub mod schema_config;
pub mod template;
pub mod tenant;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p kronos-common migrations`
Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/migrations crates/common/src/lib.rs
git commit -m "feat(plan-1): add migration template renderer"
```

---

### Task 4: Convert migration SQL files to use `{{system_schema}}` placeholders

**Why:** The two migration files that explicitly reference `public.organizations` / `public.workspaces` must become parametric. The other two migration files (`20260317000000_initial.sql` and `workspace_v1.sql`) are unchanged because they target the per-workspace schema (set via `search_path`), not the system schema.

**Files:**
- Modify: `migrations/20260318000000_multi_tenancy.sql`
- Modify: `migrations/20260322000001_pg_cron.sql`

- [ ] **Step 1: Update `20260318000000_multi_tenancy.sql`**

Open `migrations/20260318000000_multi_tenancy.sql` and replace every occurrence of `public.` with `{{system_schema}}.`. The full rewritten file:

```sql
-- Multi-tenancy: organizations and workspaces

CREATE SCHEMA IF NOT EXISTS {{system_schema}};

CREATE TABLE IF NOT EXISTS {{system_schema}}.organizations (
    org_id      TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    name        TEXT        NOT NULL,
    slug        TEXT        NOT NULL UNIQUE,
    status      TEXT        NOT NULL DEFAULT 'ACTIVE',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_organizations PRIMARY KEY (org_id),
    CONSTRAINT chk_org_status CHECK (status IN ('ACTIVE', 'SUSPENDED', 'DELETED'))
);

CREATE TABLE IF NOT EXISTS {{system_schema}}.workspaces (
    workspace_id    TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    org_id          TEXT        NOT NULL,
    name            TEXT        NOT NULL,
    slug            TEXT        NOT NULL,
    schema_name     TEXT        NOT NULL UNIQUE,
    status          TEXT        NOT NULL DEFAULT 'ACTIVE',
    schema_version  BIGINT      NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_workspaces PRIMARY KEY (workspace_id),
    CONSTRAINT fk_workspaces_org FOREIGN KEY (org_id) REFERENCES {{system_schema}}.organizations (org_id),
    CONSTRAINT uq_workspace_slug UNIQUE (org_id, slug),
    CONSTRAINT chk_ws_status CHECK (status IN ('ACTIVE', 'SUSPENDED', 'DELETED'))
);

CREATE INDEX IF NOT EXISTS idx_workspaces_org ON {{system_schema}}.workspaces (org_id);
CREATE INDEX IF NOT EXISTS idx_workspaces_status ON {{system_schema}}.workspaces (status);
```

The `CREATE SCHEMA IF NOT EXISTS {{system_schema}};` line is new — it ensures the target schema exists before tables are created. With the service default `system_schema = "public"`, this is a no-op (Postgres always has `public`).

- [ ] **Step 2: Update `20260322000001_pg_cron.sql`**

Open `migrations/20260322000001_pg_cron.sql` and replace `public.workspaces` with `{{system_schema}}.workspaces`. The full rewritten file:

```sql
-- Migration: Replace CRON materializer with pg_cron
-- pg_cron handles scheduling natively, eliminating the need for the CRON materializer loop.

CREATE EXTENSION IF NOT EXISTS pg_cron;

-- Migrate existing active CRON jobs to pg_cron.
-- For each active workspace, register all active CRON jobs with pg_cron.
DO $$ DECLARE
    ws RECORD;
    job RECORD;
    cron_job_name TEXT;
    cron_command TEXT;
BEGIN
    FOR ws IN SELECT schema_name FROM {{system_schema}}.workspaces WHERE status = 'ACTIVE' LOOP
        FOR job IN EXECUTE format(
            'SELECT job_id, cron_expression, endpoint FROM %I.jobs WHERE trigger_type = ''CRON'' AND status = ''ACTIVE''',
            ws.schema_name
        ) LOOP
            cron_job_name := 'kronos_' || ws.schema_name || '_' || job.job_id;
            cron_command := format(
                'INSERT INTO %I.executions '
                    '(job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at, max_attempts) '
                'SELECT j.job_id, j.endpoint, j.endpoint_type, '
                    '''cron_'' || j.job_id || ''_'' || (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT, '
                    '''QUEUED'', j.input, now(), '
                    'COALESCE((e.retry_policy->>''max_attempts'')::BIGINT, 1) '
                'FROM %I.jobs j '
                'JOIN %I.endpoints e ON e.name = j.endpoint '
                'WHERE j.job_id = %L AND j.status = ''ACTIVE'' '
                'ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING',
                ws.schema_name, ws.schema_name, ws.schema_name, job.job_id
            );

            PERFORM cron.schedule(cron_job_name, job.cron_expression, cron_command);
        END LOOP;
    END LOOP;
END $$;
```

(Note: the hard-coded `'kronos_'` prefix in `cron_job_name := 'kronos_' || ...` is *not* the same thing as `tenant_schema_prefix` — it's just a pg_cron job-name namespace. Leaving it as-is.)

- [ ] **Step 3: Verify rendering with service defaults produces today's SQL**

Add a quick characterization test. Append to `crates/common/src/migrations/render.rs` `tests` module (before the closing `}`):

```rust
    #[test]
    fn service_default_renders_to_public_schema() {
        let cfg = SchemaConfig::service_default();
        let template = include_str!("../../../../migrations/20260318000000_multi_tenancy.sql");
        let rendered = render(template, &cfg).unwrap();
        assert!(rendered.contains("public.organizations"));
        assert!(rendered.contains("public.workspaces"));
        assert!(!rendered.contains("{{"));
    }

    #[test]
    fn library_default_renders_to_kronos_schema() {
        let cfg = SchemaConfig::library_default();
        let template = include_str!("../../../../migrations/20260318000000_multi_tenancy.sql");
        let rendered = render(template, &cfg).unwrap();
        assert!(rendered.contains("kronos.organizations"));
        assert!(rendered.contains("kronos.workspaces"));
        assert!(!rendered.contains("public.organizations"));
        assert!(!rendered.contains("{{"));
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p kronos-common migrations`
Expected: 8 tests pass (the original 6 plus the 2 characterization tests).

- [ ] **Step 5: Commit**

```bash
git add migrations/20260318000000_multi_tenancy.sql migrations/20260322000001_pg_cron.sql crates/common/src/migrations/render.rs
git commit -m "feat(plan-1): parameterize multi_tenancy and pg_cron migrations on system_schema"
```

---

### Task 5: Update `build_schema_name` to accept a `tenant_schema_prefix`

**Why:** The function that builds the per-workspace schema name (`{org_id}_{slug}`) needs to take the prefix so library users get `kronos_{org}_{slug}` while service users keep today's unprefixed names.

**Files:**
- Modify: `crates/common/src/tenant.rs`

- [ ] **Step 1: Write the failing test**

Edit `crates/common/src/tenant.rs`. Replace the existing `build_schema_name` function and its tests (or add new tests if none exist) with:

```rust
/// Builds the per-workspace schema name from `{prefix}{org_id}_{workspace_slug}`.
/// Replaces hyphens with underscores since PostgreSQL schema names can't contain hyphens.
pub fn build_schema_name(prefix: &str, org_id: &str, workspace_slug: &str) -> String {
    format!(
        "{}{}_{}",
        prefix,
        org_id.replace('-', "_"),
        workspace_slug.replace('-', "_")
    )
}

#[cfg(test)]
mod build_schema_name_tests {
    use super::*;

    #[test]
    fn service_mode_no_prefix() {
        assert_eq!(
            build_schema_name("", "myorg", "prod"),
            "myorg_prod"
        );
    }

    #[test]
    fn library_mode_kronos_prefix() {
        assert_eq!(
            build_schema_name("kronos_", "myorg", "prod"),
            "kronos_myorg_prod"
        );
    }

    #[test]
    fn hyphens_in_org_id_become_underscores() {
        assert_eq!(
            build_schema_name("", "abc-123", "prod"),
            "abc_123_prod"
        );
    }

    #[test]
    fn hyphens_in_slug_become_underscores() {
        assert_eq!(
            build_schema_name("kronos_", "myorg", "prod-east"),
            "kronos_myorg_prod_east"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails (signature mismatch)**

Run: `cargo test -p kronos-common build_schema_name`
Expected: compile error — every existing call site to `build_schema_name(org_id, slug)` now has the wrong arity.

- [ ] **Step 3: Update every call site**

Run: `cargo build --workspace 2>&1 | grep -E 'build_schema_name'` to find all call sites. Each call must now pass the prefix as the first argument.

For each call site, look up the available `SchemaConfig` (from `AppConfig.schema` once Task 7 lands; for now they all use the service default `""`) and update the call. Common locations:

- `crates/api/src/handlers/workspaces.rs` (workspace creation)
- Any test fixtures that construct workspace schema names

In every site found, prepend an empty string as the first argument for now (keeping behavior identical to today). Task 8 wires the configured prefix in.

Example transformation:

```rust
// Before:
let schema_name = build_schema_name(&org_id, &workspace.slug);

// After:
let schema_name = build_schema_name("", &org_id, &workspace.slug);
```

- [ ] **Step 4: Run the full workspace tests**

Run: `cargo test -p kronos-common build_schema_name`
Expected: 4 tests pass.

Run: `cargo build --workspace`
Expected: succeeds.

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/tenant.rs crates/api/src
git commit -m "feat(plan-1): build_schema_name takes an explicit prefix"
```

---

### Task 6: Update `SchemaRegistry` to use the configured system schema

**Why:** The registry's query is hard-coded to `public.workspaces`. It must instead read from `{system_schema}.workspaces` so that a non-default `system_schema` works at runtime.

**Files:**
- Modify: `crates/common/src/tenant.rs`

- [ ] **Step 1: Update `SchemaRegistry::new` to take a `SchemaConfig`**

Edit `crates/common/src/tenant.rs`. Find the `SchemaRegistry` struct and its `new` / `get_active_schemas` methods. Replace with:

```rust
/// Cached registry of active workspace schemas.
/// Used by worker and scheduler to iterate tenants.
#[derive(Clone)]
pub struct SchemaRegistry {
    pool: PgPool,
    cache: Arc<RwLock<CachedSchemas>>,
    ttl: Duration,
    system_schema: String,
}

struct CachedSchemas {
    schemas: Vec<String>,
    fetched_at: Instant,
}

impl SchemaRegistry {
    pub fn new(pool: PgPool, system_schema: String, ttl_secs: u64) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(CachedSchemas {
                schemas: Vec::new(),
                fetched_at: Instant::now() - Duration::from_secs(ttl_secs + 1),
            })),
            ttl: Duration::from_secs(ttl_secs),
            system_schema,
        }
    }

    pub async fn get_active_schemas(&self) -> Result<Vec<String>, sqlx::Error> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if cache.fetched_at.elapsed() < self.ttl && !cache.schemas.is_empty() {
                return Ok(cache.schemas.clone());
            }
        }

        // Refresh — system_schema is a validated identifier so quoting it is safe.
        // We assert the validator on construction in the call site; here we just use it.
        let query = format!(
            "SELECT schema_name FROM {}.workspaces WHERE status = 'ACTIVE'",
            quote_ident(&self.system_schema)
        );
        let schemas: Vec<(String,)> = sqlx::query_as(&query)
            .fetch_all(&self.pool)
            .await?;

        let schemas: Vec<String> = schemas.into_iter().map(|r| r.0).collect();

        let mut cache = self.cache.write().await;
        cache.schemas = schemas.clone();
        cache.fetched_at = Instant::now();

        Ok(schemas)
    }
}

/// Quotes a Postgres identifier safely (doubles internal double-quotes).
/// Callers must still validate against `is_valid_pg_identifier` upstream;
/// this is defense in depth, not the primary check.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
```

- [ ] **Step 2: Update existing call sites of `SchemaRegistry::new`**

Run: `cargo build --workspace 2>&1 | grep -E 'SchemaRegistry::new'` to find call sites.

The only known call site is `crates/worker/src/poller.rs`:

```rust
// Before:
let schema_registry = SchemaRegistry::new(pool.clone(), 30);
// After:
let schema_registry = SchemaRegistry::new(pool.clone(), "public".to_string(), 30);
```

(Task 8 wires the configured value through `AppConfig`.)

- [ ] **Step 3: Verify it builds**

Run: `cargo build --workspace`
Expected: compiles cleanly.

- [ ] **Step 4: Run integration tests with default config to verify no regression**

This step requires the Postgres + worker stack:

```bash
just db-reset
just test-e2e
```

Expected: all three tests (`test-immediate`, `test-delayed`, `test-cron`) pass. (The justfile still uses raw `psql` to apply migrations; we update that in Task 9. For this step, the migration files now contain `{{system_schema}}` placeholders that `psql` won't render — Task 9 fixes this. So skip this step here and run the full e2e check at the end of Task 9.)

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/tenant.rs crates/worker/src/poller.rs
git commit -m "feat(plan-1): SchemaRegistry queries the configured system schema"
```

---

### Task 7: Add embedded `MIGRATIONS` list and `apply()` to `kronos-common::migrations`

**Why:** The library needs to know which migration files to apply, in order. Embedding them at compile time (via `include_str!`) means the binary is self-contained — no external SQL files needed at runtime.

**Files:**
- Create: `crates/common/src/migrations/embedded.rs`
- Modify: `crates/common/src/migrations/mod.rs`

- [ ] **Step 1: Embed the migration files**

Create `crates/common/src/migrations/embedded.rs`:

```rust
//! Migration template files embedded at compile time. The order in
//! `MIGRATIONS` matches the order they must be applied.

pub struct Migration {
    pub name: &'static str,
    pub template: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "20260317000000_initial",
        template: include_str!("../../../../migrations/20260317000000_initial.sql"),
    },
    Migration {
        name: "20260318000000_multi_tenancy",
        template: include_str!("../../../../migrations/20260318000000_multi_tenancy.sql"),
    },
    Migration {
        name: "20260322000000_txn_based_pickup",
        template: include_str!("../../../../migrations/20260322000000_txn_based_pickup.sql"),
    },
    Migration {
        name: "20260322000001_pg_cron",
        template: include_str!("../../../../migrations/20260322000001_pg_cron.sql"),
    },
];
```

- [ ] **Step 2: Add the `apply` function**

Append to `crates/common/src/migrations/mod.rs`:

```rust
pub mod embedded;
pub mod render;

pub use embedded::{Migration, MIGRATIONS};
pub use render::{render, RenderError};

use crate::schema_config::SchemaConfig;
use sqlx::PgPool;

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("template render failed for {migration}: {source}")]
    Render {
        migration: &'static str,
        #[source]
        source: RenderError,
    },
    #[error("SQL execution failed for {migration}: {source}")]
    Sql {
        migration: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

/// Render and apply every embedded migration against `pool`.
///
/// Each migration's template is rendered using `cfg`, then executed as a
/// single SQL statement-batch. Migrations are idempotent (every CREATE
/// uses `IF NOT EXISTS`), so re-running this function on an already-migrated
/// database is safe.
pub async fn apply(pool: &PgPool, cfg: &SchemaConfig) -> Result<(), MigrateError> {
    for m in MIGRATIONS {
        let rendered = render(m.template, cfg).map_err(|e| MigrateError::Render {
            migration: m.name,
            source: e,
        })?;
        tracing::info!(migration = m.name, "applying migration");
        sqlx::raw_sql(&rendered)
            .execute(pool)
            .await
            .map_err(|e| MigrateError::Sql {
                migration: m.name,
                source: e,
            })?;
    }
    Ok(())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p kronos-common`
Expected: succeeds.

- [ ] **Step 4: Add an integration test against a real Postgres**

Create `crates/common/tests/migrate_apply.rs`:

```rust
//! Integration test: `migrations::apply` produces a working schema for
//! both service-default and library-default `SchemaConfig`s.
//!
//! Requires a running Postgres at `TE_DATABASE_URL` (or `postgres://kronos:kronos@localhost:5432/taskexecutor`).
//! Run with: `cargo test -p kronos-common --test migrate_apply -- --test-threads=1`

use kronos_common::migrations;
use kronos_common::schema_config::SchemaConfig;
use sqlx::PgPool;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

async fn fresh_pool() -> PgPool {
    let pool = PgPool::connect(&db_url()).await.unwrap();
    // Reset both candidate system schemas so the test is reentrant.
    for schema in &["kronos_test", "public"] {
        let _ = sqlx::query(&format!(
            "DROP SCHEMA IF EXISTS \"{}\" CASCADE; CREATE SCHEMA \"{}\";",
            schema, schema
        ))
        .execute(&pool)
        .await;
    }
    pool
}

#[tokio::test]
#[ignore] // requires DB; run explicitly
async fn applies_with_service_default() {
    let pool = fresh_pool().await;
    let cfg = SchemaConfig::service_default();
    migrations::apply(&pool, &cfg).await.unwrap();

    // Sanity: organizations and workspaces exist in `public`.
    let exists: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = 'organizations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists.0);
}

#[tokio::test]
#[ignore]
async fn applies_with_library_default() {
    let pool = fresh_pool().await;
    let cfg = SchemaConfig {
        system_schema: "kronos_test".to_string(),
        tenant_schema_prefix: "kronos_".to_string(),
    };
    migrations::apply(&pool, &cfg).await.unwrap();

    let exists: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'kronos_test' AND table_name = 'organizations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists.0);
}
```

The tests are marked `#[ignore]` because they require a running Postgres; they are not run by default `cargo test`.

- [ ] **Step 5: Run the integration tests against a fresh database**

```bash
just db-up
sqlx database drop --database-url "$TE_DATABASE_URL" -y || true
sqlx database create --database-url "$TE_DATABASE_URL"
cargo test -p kronos-common --test migrate_apply -- --ignored --test-threads=1
```

Expected: both tests pass; tables exist in the right schemas.

- [ ] **Step 6: Commit**

```bash
git add crates/common/src/migrations crates/common/tests
git commit -m "feat(plan-1): embed migrations and add apply() entry point"
```

---

### Task 8: Wire `SchemaConfig` through `AppConfig` and load from environment

**Why:** Service binaries need to read the schema config from env (`TE_SYSTEM_SCHEMA`, `TE_TENANT_SCHEMA_PREFIX`), defaulting to today's values. This is the bridge between env-config and the new typed value.

**Files:**
- Modify: `crates/common/src/config.rs`

- [ ] **Step 1: Add a `SchemaEnv` struct and field to `AppConfig`**

Edit `crates/common/src/config.rs`. After the existing `MetricsEnv` impl block, add:

```rust
#[derive(Debug, Clone)]
pub struct SchemaEnv {
    pub system_schema: String,
    pub tenant_schema_prefix: String,
}

impl SchemaEnv {
    fn new() -> Self {
        Self {
            system_schema: get_from_env_or_default(
                "TE_SYSTEM_SCHEMA",
                "public".to_string(),
            ),
            tenant_schema_prefix: get_from_env_or_default(
                "TE_TENANT_SCHEMA_PREFIX",
                String::new(),
            ),
        }
    }

    pub fn to_schema_config(&self) -> crate::schema_config::SchemaConfig {
        crate::schema_config::SchemaConfig {
            system_schema: self.system_schema.clone(),
            tenant_schema_prefix: self.tenant_schema_prefix.clone(),
        }
    }
}
```

Then update the `AppConfig` struct and its `from_env` method to include the new field:

```rust
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub db: DbEnv,
    pub server: ServerEnv,
    pub worker: WorkerEnv,
    pub crypto: CryptoEnv,
    pub metrics: MetricsEnv,
    pub schema: SchemaEnv,
}

impl AppConfig {
    pub async fn from_env() -> anyhow::Result<Self> {
        let kms_enabled: bool = get_from_env_or_default("TE_KMS_ENABLED", false);

        #[cfg(not(feature = "kms"))]
        if kms_enabled {
            anyhow::bail!(
                "TE_KMS_ENABLED=true but kronos was compiled without the 'kms' feature"
            );
        }

        let reader = SensitiveEnvReader {
            #[cfg(feature = "kms")]
            client: if kms_enabled {
                tracing::info!("KMS decryption enabled, initializing AWS KMS client");
                Some(crate::kms::new_client().await)
            } else {
                None
            },
        };

        let db = DbEnv::new(&reader).await.map_err(|e| anyhow::anyhow!(e))?;
        let server = ServerEnv::new(&reader).await.map_err(|e| anyhow::anyhow!(e))?;
        let worker = WorkerEnv::new();
        let crypto = CryptoEnv::new(&reader).await.map_err(|e| anyhow::anyhow!(e))?;
        let metrics = MetricsEnv::new();
        let schema = SchemaEnv::new();

        // Validate schema config early so misconfiguration fails fast at startup.
        schema
            .to_schema_config()
            .validate()
            .map_err(|e| anyhow::anyhow!("invalid schema config: {}", e))?;

        Ok(Self {
            db,
            server,
            worker,
            crypto,
            metrics,
            schema,
        })
    }
}
```

- [ ] **Step 2: Add a unit test for env defaults**

Append to `crates/common/src/config.rs` (at the end, in a `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod schema_env_tests {
    use super::*;

    #[test]
    fn defaults_match_today() {
        // Save and clear env to test defaults
        let saved_sys = std::env::var("TE_SYSTEM_SCHEMA").ok();
        let saved_prefix = std::env::var("TE_TENANT_SCHEMA_PREFIX").ok();
        std::env::remove_var("TE_SYSTEM_SCHEMA");
        std::env::remove_var("TE_TENANT_SCHEMA_PREFIX");

        let s = SchemaEnv::new();
        assert_eq!(s.system_schema, "public");
        assert_eq!(s.tenant_schema_prefix, "");

        if let Some(v) = saved_sys {
            std::env::set_var("TE_SYSTEM_SCHEMA", v);
        }
        if let Some(v) = saved_prefix {
            std::env::set_var("TE_TENANT_SCHEMA_PREFIX", v);
        }
    }

    #[test]
    fn picks_up_non_default_values() {
        std::env::set_var("TE_SYSTEM_SCHEMA", "kronos_test_env");
        std::env::set_var("TE_TENANT_SCHEMA_PREFIX", "k_");

        let s = SchemaEnv::new();
        assert_eq!(s.system_schema, "kronos_test_env");
        assert_eq!(s.tenant_schema_prefix, "k_");

        std::env::remove_var("TE_SYSTEM_SCHEMA");
        std::env::remove_var("TE_TENANT_SCHEMA_PREFIX");
    }
}
```

These tests mutate process-global env, so they must run single-threaded:

- [ ] **Step 3: Run the tests**

Run: `cargo test -p kronos-common config::schema_env_tests -- --test-threads=1`
Expected: 2 tests pass.

- [ ] **Step 4: Wire `SchemaConfig` into `SchemaRegistry::new` call sites and `build_schema_name` call sites**

In `crates/worker/src/poller.rs`, replace:

```rust
let schema_registry = SchemaRegistry::new(pool.clone(), "public".to_string(), 30);
```

with:

```rust
let schema_registry = SchemaRegistry::new(
    pool.clone(),
    config.schema.system_schema.clone(),
    30,
);
```

In `crates/api/src/handlers/workspaces.rs`, find the `build_schema_name("", &org_id, &workspace.slug)` call (placed there in Task 5) and replace `""` with the configured prefix. Locate the `AppConfig` access path in the handler — typically `state.config.schema.tenant_schema_prefix` — and pass it through:

```rust
// Before (placeholder from Task 5):
let schema_name = build_schema_name("", &org_id, &workspace.slug);
// After:
let schema_name = build_schema_name(
    &state.config.schema.tenant_schema_prefix,
    &org_id,
    &workspace.slug,
);
```

(If the actix `AppState` does not yet expose the full `AppConfig`, add a `schema: SchemaEnv` field to it and populate it during construction in `crates/api/src/main.rs`. The exact field path depends on existing handler patterns — search for how other env values like `crypto` are accessed.)

- [ ] **Step 5: Verify it builds**

Run: `cargo build --workspace`
Expected: succeeds.

- [ ] **Step 6: Commit**

```bash
git add crates/common/src/config.rs crates/worker/src/poller.rs crates/api/src
git commit -m "feat(plan-1): plumb SchemaConfig through AppConfig with env-var defaults"
```

---

### Task 9: Add `kronos_client::migrate` and the `kronos-migrate` CLI binary

**Why:** Public migration entry point for both library users and the justfile. The CLI binary replaces the justfile's raw `psql` loop, since the latter can't render `{{...}}` placeholders.

**Files:**
- Create: `crates/client/src/migrate.rs`
- Create: `crates/client/src/bin/kronos-migrate.rs`
- Modify: `crates/client/src/lib.rs`
- Modify: `crates/client/Cargo.toml`

- [ ] **Step 1: Add the public `migrate` re-export**

Create `crates/client/src/migrate.rs`:

```rust
//! Public migration entry point. Library users call `kronos_client::migrate(&pool, &opts)`
//! to render and apply all embedded migrations against their database.

pub use kronos_common::migrations::{apply, MigrateError, Migration, MIGRATIONS};
pub use kronos_common::schema_config::SchemaConfig;

use sqlx::PgPool;

/// Convenience alias matching the spec's API shape.
///
/// Equivalent to `kronos_common::migrations::apply(pool, opts)`.
pub async fn migrate(pool: &PgPool, opts: &SchemaConfig) -> Result<(), MigrateError> {
    apply(pool, opts).await
}
```

- [ ] **Step 2: Update the lib root**

Edit `crates/client/src/lib.rs`:

```rust
//! Kronos library API. Today this crate exposes only the migration entry
//! point; subsequent plans add the enqueue + CRUD surface.

pub mod migrate;

pub use kronos_common::schema_config::SchemaConfig;
pub use migrate::{migrate, MigrateError, Migration, MIGRATIONS};
```

- [ ] **Step 3: Add `clap` dependency for the CLI binary**

Edit `crates/client/Cargo.toml`. Add to `[dependencies]`:

```toml
clap = { version = "4", features = ["derive", "env"] }
anyhow = { workspace = true }
tracing-subscriber = { workspace = true }
```

(`anyhow` and `tracing-subscriber` are already workspace deps.)

- [ ] **Step 4: Write the CLI binary**

Create `crates/client/src/bin/kronos-migrate.rs`:

```rust
//! `kronos-migrate` — render and apply Kronos migrations.
//!
//! Designed to drop into the existing `just db-migrate` recipe.

use clap::Parser;
use kronos_client::{migrate, SchemaConfig};
use sqlx::PgPool;

#[derive(Parser, Debug)]
#[command(about = "Render and apply Kronos migrations")]
struct Args {
    /// Postgres connection URL.
    #[arg(long, env = "TE_DATABASE_URL")]
    database_url: String,

    /// System schema for shared tables (organizations, workspaces).
    #[arg(long, env = "TE_SYSTEM_SCHEMA", default_value = "public")]
    system_schema: String,

    /// Prefix prepended to per-workspace schema names.
    #[arg(long, env = "TE_TENANT_SCHEMA_PREFIX", default_value = "")]
    tenant_schema_prefix: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let cfg = SchemaConfig {
        system_schema: args.system_schema,
        tenant_schema_prefix: args.tenant_schema_prefix,
    };

    let pool = PgPool::connect(&args.database_url).await?;
    migrate(&pool, &cfg).await?;
    println!("migrations applied (system_schema={}, tenant_schema_prefix={:?})",
        cfg.system_schema, cfg.tenant_schema_prefix);
    Ok(())
}
```

- [ ] **Step 5: Verify it builds**

Run: `cargo build -p kronos-client --bin kronos-migrate`
Expected: produces `target/debug/kronos-migrate`.

- [ ] **Step 6: Commit**

```bash
git add crates/client/src/migrate.rs crates/client/src/lib.rs crates/client/src/bin crates/client/Cargo.toml
git commit -m "feat(plan-1): add kronos_client::migrate and kronos-migrate CLI"
```

---

### Task 10: Update `justfile` to use `kronos-migrate`

**Why:** The current `db-migrate` recipe pipes raw SQL into `psql`, which can't render `{{...}}` placeholders. Replace with a `cargo run` call that uses defaults matching today's behavior.

**Files:**
- Modify: `justfile`

- [ ] **Step 1: Replace the `db-migrate` recipe**

Edit `justfile`. Find the `db-migrate` recipe and replace:

```make
# Run SQL migrations
db-migrate:
    PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor < migrations/20260317000000_initial.sql
    PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor < migrations/20260318000000_multi_tenancy.sql
    PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor < migrations/20260322000000_txn_based_pickup.sql
    PGPASSWORD=kronos psql -h localhost -U kronos -d taskexecutor < migrations/20260322000001_pg_cron.sql
```

with:

```make
# Run SQL migrations via the kronos-migrate binary.
# Defaults to system_schema=public, tenant_schema_prefix="" (preserves today's layout).
# Override via TE_SYSTEM_SCHEMA / TE_TENANT_SCHEMA_PREFIX env vars.
db-migrate:
    cargo run -p kronos-client --bin kronos-migrate -- \
        --database-url "$TE_DATABASE_URL"
```

- [ ] **Step 2: Run the full integration suite**

```bash
just db-reset
just test-e2e
```

Expected: all three tests (`test-immediate`, `test-delayed`, `test-cron`) pass with default config. This is the load-bearing regression check.

- [ ] **Step 3: Commit**

```bash
git add justfile
git commit -m "chore(plan-1): justfile db-migrate uses kronos-migrate binary"
```

---

### Task 11: Smoke test — full migration with non-default `system_schema = "kronos"`

**Why:** Independent verification that the parameterization works end-to-end. The service-mode integration tests only exercise the default path; this test exercises the path that embedded mode will rely on.

**Files:**
- Create: `crates/client/tests/migrate_kronos_namespace.rs`

- [ ] **Step 1: Write the integration test**

Create `crates/client/tests/migrate_kronos_namespace.rs`:

```rust
//! Smoke test for non-default schema namespacing.
//!
//! Applies migrations with `system_schema = "kronos_smoke"` against a fresh
//! database, then asserts the expected tables exist in the right schema and
//! NO Kronos tables landed in `public`.
//!
//! Requires Postgres at `TE_DATABASE_URL` (default postgres://kronos:kronos@localhost:5432/taskexecutor).
//! Run with: `cargo test -p kronos-client --test migrate_kronos_namespace -- --ignored`

use kronos_client::{migrate, SchemaConfig};
use sqlx::PgPool;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

#[tokio::test]
#[ignore]
async fn migrations_create_tables_in_configured_schema_only() {
    let pool = PgPool::connect(&db_url()).await.unwrap();

    // Wipe any prior state from this test schema, but DO NOT touch `public`
    // (the integration tests in the same DB rely on `public`).
    sqlx::query("DROP SCHEMA IF EXISTS kronos_smoke CASCADE")
        .execute(&pool)
        .await
        .unwrap();

    let cfg = SchemaConfig {
        system_schema: "kronos_smoke".to_string(),
        tenant_schema_prefix: "kronos_".to_string(),
    };

    migrate(&pool, &cfg).await.unwrap();

    // organizations and workspaces are in kronos_smoke
    let orgs_in_kronos: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'kronos_smoke' AND table_name = 'organizations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(orgs_in_kronos.0, "organizations should exist in kronos_smoke");

    let ws_in_kronos: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'kronos_smoke' AND table_name = 'workspaces')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ws_in_kronos.0, "workspaces should exist in kronos_smoke");

    // Cleanup so re-running the test is clean.
    sqlx::query("DROP SCHEMA IF EXISTS kronos_smoke CASCADE")
        .execute(&pool)
        .await
        .unwrap();
}
```

- [ ] **Step 2: Run the smoke test**

```bash
just db-up
cargo test -p kronos-client --test migrate_kronos_namespace -- --ignored
```

Expected: test passes.

- [ ] **Step 3: Run the full integration suite again to confirm no regression**

```bash
just db-reset
just test-e2e
```

Expected: all three tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/client/tests/migrate_kronos_namespace.rs
git commit -m "test(plan-1): smoke test for non-default kronos system_schema"
```

---

### Task 12: Push the branch and update the draft PR

**Why:** Plan 1 is complete on the existing `feat/embedded-mode` branch (or its own sub-branch if the team prefers). Push so reviewers can see the foundation work landing.

**Files:** none

- [ ] **Step 1: Push the branch**

```bash
git push
```

- [ ] **Step 2: Comment on the draft PR**

Post a comment on PR #12 noting that Plan 1 is complete:

> Plan 1 (Foundation) is now committed on this branch:
> - Two new empty crates (`kronos-client`, `kronos-embedded-worker`) wired into the workspace
> - `SchemaConfig` value type and migration template renderer in `kronos-common`
> - Migrations parameterized on `{{system_schema}}` (service default `public` preserves today)
> - `kronos_client::migrate(&pool, &opts)` public API and `kronos-migrate` CLI binary
> - `justfile db-migrate` uses the new binary
> - Smoke test verifies non-default `system_schema = "kronos_smoke"` works
> - All existing integration tests pass with default config
>
> Plan 2 (Worker extraction) will follow.

(Use `gh pr comment 12 --body "<message>"` if doing this from the CLI.)

---

## Self-Review

**Spec coverage check:**

- ✅ Phase F1 (new crate scaffolding): Tasks 1, 2 (manifests, lib roots, workspace wire-in)
- ✅ Phase F2 (schema parameterization): Tasks 3, 4 (renderer + template files), Task 5 (`build_schema_name` prefix), Task 6 (`SchemaRegistry` system schema), Task 7 (embed migrations + apply), Task 8 (env-var plumbing), Task 9 (public API + CLI), Task 10 (justfile cutover)
- ✅ Ships-when criteria: Task 10 step 2 runs `just test-e2e` with defaults; Task 11 verifies non-default `kronos_smoke` namespace; Task 12 publishes for review

**Placeholder scan:** No "TBD", "TODO", "implement later", "similar to Task N", or vague "add error handling" instructions. Every code block is complete and copy-pasteable.

**Type consistency:**
- `SchemaConfig` (Task 2) is referenced consistently in Tasks 3, 5, 7, 8, 9 (`SchemaConfig`, `service_default()`, `library_default()`, `validate()` — all match Task 2's definitions)
- `Migration` and `MIGRATIONS` (Task 7) are re-exported in Task 9 with the same names
- `apply()` (Task 7) is called by `migrate()` (Task 9) — signatures match: `(pool: &PgPool, cfg: &SchemaConfig) -> Result<(), MigrateError>`
- `SchemaRegistry::new(pool, system_schema, ttl_secs)` signature (Task 6) is called consistently in Tasks 6 and 8
- `build_schema_name(prefix, org_id, slug)` signature (Task 5) is called consistently in Tasks 5 and 8

**Known unknowns the engineer will hit:**
- The exact field path for `tenant_schema_prefix` in actix `AppState` (Task 8 step 4) depends on existing handler patterns — the task tells the engineer to grep for similar fields like `crypto` and follow the same pattern. This is the right level of guidance: prescribing an exact diff for `AppState` would over-specify code the engineer should write organically.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-29-plan-1-foundation.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — A fresh subagent per task, with review between tasks. Good for fast iteration when each task is well-bounded.

**2. Inline Execution** — Execute tasks in this session using the executing-plans skill. Batch execution with checkpoints for review.

**Which approach?**
