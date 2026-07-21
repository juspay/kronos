---
id: intro
title: Introduction
---

# Introduction

**Kronos is `setTimeout` and `setInterval` as a service.**

It is a distributed, durable, retriable, and observable delivery engine for jobs sent to HTTP endpoints, Kafka topics, and Redis Streams — with type-safety guarantees. Built in Rust on top of PostgreSQL with the `pg_cron` extension, Kronos survives crashes, retries on failure, never fires the same job twice, and makes every execution observable.

---

## The mental model

If you've written JavaScript, you already know the API.

| What you want | JavaScript | Kronos |
|---|---|---|
| Fire now | `setTimeout(fn, 0)` | `POST /v1/jobs { trigger: IMMEDIATE }` |
| Fire later | `setTimeout(fn, 5000)` | `POST /v1/jobs { trigger: DELAYED, run_at: "..." }` |
| Fire repeatedly | `setInterval(fn, 60000)` | `POST /v1/jobs { trigger: CRON, cron: "* * * * *" }` |
| Cancel | `clearTimeout(id)` | `POST /v1/jobs/{id}/cancel` |

Except: it survives crashes, retries on failure, never fires twice, and every execution is observable.

---

## Key guarantees

| Guarantee | How it's achieved |
|-----------|-------------------|
| **Exactly-once** | Idempotency keys + DB unique constraints + `SELECT FOR UPDATE SKIP LOCKED` |
| **Durable** | Every job persisted to PostgreSQL before acknowledgment |
| **Retry with backoff** | Configurable per endpoint: fixed, linear, or exponential with jitter |
| **Sub-second** | Immediate: ~300ms. Delayed: within ~200ms of `run_at` (worker poll interval) |
| **Observable** | Every execution has a lifecycle. Every attempt recorded with duration, output, and error |
| **Type-safe** | JSON Schema validation on job input at creation time |
| **Multi-tenant** | Schema-per-workspace isolation. Shared nothing between tenants |

---

## Architecture overview

```
                              ┌─────────────────────────┐
                              │        Client / SDK      │
                              └────────────┬────────────┘
                                           │
                                    POST /v1/jobs
                                           │
                              ┌────────────▼────────────┐
                              │   API Server (actix-web) │
                              │   port 8080 + /metrics   │
                              └────────────┬────────────┘
                                           │
                             INSERT job + execution (txn)
                                           │
                 ┌─────────────────────────▼──────────────────────────┐
                 │               PostgreSQL + pg_cron                  │
                 │                                                     │
                 │  Source of truth          CRON scheduling natively  │
                 │  FOR UPDATE SKIP LOCKED   via pg_cron extension    │
                 │  Txn-based job pickup     (no external scheduler)  │
                 └───────┬──────────────────────────────┬─────────────┘
                         │                              │
              ┌──────────▼───────────┐    ┌─────────────▼─────────────┐
              │     Worker Pool      │    │    Dashboard (WASM)       │
              │                      │    │    Leptos + Trunk         │
              │  Semaphore-gated     │    │    port 3000              │
              │  50 concurrent jobs  │    └───────────────────────────┘
              │                      │
              │  ┌────────────────┐  │
              │  │ HTTP  (reqwest)│  │
              │  │ Kafka (rdkafka)│  │
              │  │ Redis (redis)  │  │
              │  └────────────────┘  │
              │  metrics on :9090    │
              └──────────────────────┘
```

### How scheduling works

Kronos uses **PostgreSQL pg_cron** for CRON materialization and **transaction-based pickup** for all job types. No separate scheduler process is needed — the database handles all scheduling concerns.

- **IMMEDIATE** jobs: Execution is created as `QUEUED` in the same transaction as the job. Workers pick it up directly.
- **DELAYED** jobs: Execution is created as `PENDING` with a `run_at` timestamp. Workers pick up PENDING executions once `run_at <= now()`.
- **CRON** jobs: Registered with pg_cron at creation time. pg_cron inserts a new `QUEUED` execution on each tick. Workers pick it up directly.

---

## Crates overview

Kronos is organized as a Cargo workspace with the following crates:

| Crate | Description |
|-------|-------------|
| `kronos-common` | Shared library — models, DB layer, config, tenant management, caching, metrics |
| `kronos-api` | REST API server (actix-web). CRUD for all resources, job invocation, Prometheus metrics at `/metrics` |
| `kronos-worker` | Execution engine. Polls DB for QUEUED/RETRYING/PENDING executions, resolves templates, dispatches to endpoints. Exposes metrics via HTTP listener |
| `kronos-mock-server` | Test fixture — HTTP server on port 9999 for integration tests |
| `kronos-dashboard` | Web UI — Leptos/WASM, shows jobs, executions, attempts. Excluded from workspace build |

---

## Multi-tenancy overview

Kronos uses **schema-per-tenant** isolation. Each workspace gets its own PostgreSQL schema with isolated tables. Shared tables live in the `public` schema.

```
public schema:        organizations, workspaces
tenant schema:        payload_specs, configs, secrets, endpoints,
(org_workspace):      jobs, executions, attempts, execution_logs
```

Tenant-scoped API requests require `X-Org-Id` and `X-Workspace-Id` headers. The worker iterates all active workspace schemas via a cached `SchemaRegistry` (30s TTL). See [Multi-Tenancy](./core-concepts/multi-tenancy) for details.

:::info
The schema-per-tenant model means each workspace has complete isolation — jobs, executions, endpoints, and all resources are scoped to the workspace's own database schema. Organizations live in the `public` schema and can contain multiple workspaces.
:::

---

## Deployment modes

Kronos runs in two deployment modes:

| Mode | Description | Use case |
|------|-------------|----------|
| **Library mode** (embedded) | Kronos embedded directly in your Rust application process. No HTTP overhead, no separate server. | Single Rust app that needs durable scheduling |
| **Service mode** (standalone) | Kronos runs as a standalone REST API. Multiple apps share one deployment. | Multiple apps, or decoupled operational lifecycle |

Both modes expose the same API through the `KronosClient` trait. The [Quickstart](./quickstart) uses service mode. For library mode setup, see [Library Mode Setup](./deployment/library-mode). For the conceptual comparison, see [Dual Deployment Modes](./architecture/dual-deployment).

---

## Next steps

- [Quickstart](./quickstart) — get a job firing in under 5 minutes
- [Core Concepts](./core-concepts/overview) — understand the three-step workflow
- [Jobs](./core-concepts/jobs) — trigger types and job lifecycle
- [Executions](./core-concepts/executions) — execution lifecycle and retry behavior
