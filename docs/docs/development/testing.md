---
id: testing
title: Testing
---

# Testing

Invokr has several layers of tests: end-to-end (E2E) integration tests, dispatcher unit tests, Rust inline unit tests, and load tests. This page covers how to run each.

## End-to-end (E2E) tests

E2E tests are TypeScript scripts in `cli/src/` that exercise the full API → worker → dispatch pipeline. They require the API server, worker, and mock server to be running.

### Individual E2E tests

| Command | Test | Description |
|---------|------|-------------|
| `just test-immediate` | `test-immediate.ts` | Creates an IMMEDIATE job and verifies execution completes successfully |
| `just test-delayed` | `test-delayed.ts` | Creates a DELAYED job and verifies it fires at the scheduled time (requires scheduler running) |
| `just test-cron` | `test-cron.ts` | Creates a CRON job and verifies it fires on schedule (requires scheduler running) |
| `just test-internal-guards` | `test-internal-guards.ts` | Verifies public API guards for INTERNAL jobs/endpoints (dogfooded reaper). API-only — no worker/scheduler required. |

:::note
`test-delayed` and `test-cron` require the scheduler to be running (`just scheduler`) in addition to the API and worker. The scheduler handles CRON materialization and delayed job promotion.
:::

### Full E2E test suite

```bash
just test-e2e
```

This runs a complete integration test that:

1. Builds all Rust crates (`just build`)
2. Starts the API server, worker, scheduler, and mock server as background processes
3. Waits for services to be ready (polls `/health` endpoints for up to 30 seconds)
4. Runs all four E2E test scripts in sequence:
   - `test-immediate.ts`
   - `test-delayed.ts`
   - `test-cron.ts`
   - `test-internal-guards.ts`
5. Shuts down all services
6. Returns the exit code from the test run

:::tip
`just test-e2e` is self-contained — it starts and stops all services automatically. You don't need to run `just dev` beforehand.
:::

### Prerequisites for E2E tests

E2E tests require:
- The database to be running and migrated (`just db-up && just db-migrate`)
- The SDK to be built (`just build-sdk`)
- The CLI to be installed (`just cli-install`)
- Environment variables set in `.env`:
  - `INVOKR_URL` — API base URL (e.g. `http://localhost:8080`)
  - `INVOKR_API_KEY` — API key (e.g. `dev-api-key`)
  - `INVOKR_ORG_ID` — Organization UUID (created via the API)
  - `INVOKR_WORKSPACE_ID` — Workspace UUID (created via the API)

:::info
`just test-e2e` and `just test-haskell` both run `just build` first, so you don't need to build manually. For the individual test commands (`just test-immediate`, etc.), ensure the services are running via `just dev`.
:::

### Haskell SDK test

```bash
just test-haskell
```

Runs the Haskell SDK example (`haskell-example/`). This:

1. Builds all Rust crates
2. Starts the API server, worker, and mock server
3. Waits for services to be ready
4. Uses Nix to build and run the Haskell example (installs GHC 9.6 and required packages)
5. Shuts down all services

This test verifies that the generated Haskell SDK works correctly against the Invokr API.

## Dispatcher unit tests

Dispatcher tests are Rust unit tests in `crates/worker/src/dispatcher/`. They test individual dispatchers (HTTP, Kafka, Redis Stream) and require their respective infrastructure.

| Command | Test | Requires |
|---------|------|----------|
| `just test-http` | HTTP dispatcher tests | Mock server running (`just mock-server`) |
| `just test-kafka` | Kafka dispatcher tests | Kafka running (`docker compose --profile kafka up -d`) |
| `just test-redis` | Redis Stream dispatcher tests | Redis running (`docker compose --profile redis up -d`) |
| `just test-dispatchers` | All dispatcher tests | Mock server + Kafka + Redis |

### Running dispatcher tests

```bash
# Start the mock server first
just mock-server &

# Run HTTP dispatcher tests
just test-http

# Start Kafka
docker compose --profile kafka up -d

# Run Kafka dispatcher tests (single-threaded)
just test-kafka

# Start Redis
docker compose --profile redis up -d

# Run Redis Stream dispatcher tests (single-threaded)
just test-redis

# All dispatcher tests at once (requires all infrastructure running)
just infra-up
just mock-server &
just test-dispatchers
```

:::note
Kafka and Redis dispatcher tests use `--test-threads=1` to avoid race conditions when multiple tests interact with the same Kafka topic or Redis stream.
:::

### What the dispatcher tests cover

| Dispatcher | Tests |
|------------|-------|
| HTTP | Successful dispatch (200), failed dispatch (500), timeout handling, header injection, template resolution |
| Kafka | Message production to Kafka topic, header propagation, error handling for connection failures |
| Redis Stream | Stream entry production, field mapping, error handling for connection failures |

## Rust unit tests

In addition to the dispatcher tests, Invokr has inline `#[cfg(test)]` unit tests throughout the codebase. These run without external infrastructure.

### Running all Rust unit tests

```bash
cargo test --workspace
```

### Key test modules

| Module | Location | Tests |
|--------|----------|-------|
| Crypto | `crates/common/src/crypto.rs` | AES-256-GCM encryption/decryption, nonce handling, key validation |
| Template | `crates/worker/src/` (template module) | Template resolution for `{{input.*}}`, `{{config.*}}`, `{{secret.*}}` — full replacement vs. string interpolation, nested objects, error cases |
| Tenant | `crates/common/src/tenant.rs` | Slug validation, schema name construction, table prefix validation |
| PgCronExpr | `crates/common/src/models/pg_cron_expr.rs` | 5-field cron expression parsing and validation |
| Job | `crates/common/src/models/job.rs` | Job model serialization/deserialization |
| DB (jobs) | `crates/common/src/db/` | Database query construction and execution |

### Running specific test modules

```bash
# Run tests in a specific crate
cargo test -p invokr-common

# Run tests in a specific module
cargo test -p invokr-common --lib crypto::tests

# Run a specific test by name
cargo test -p invokr-common --lib crypto::tests::test_encrypt_decrypt
```

## Load testing

Invokr includes load testing scripts that create batches of jobs and track completion.

### Creating load

```bash
# Create 50 jobs of each type (IMMEDIATE, DELAYED, CRON) and track completion
just load-test 50

# Fire-and-forget (create jobs without polling for completion)
just load-test-nw 50
```

The default count is 10 if not specified:

```bash
just load-test        # Creates 10 jobs of each type
just load-test 100    # Creates 100 jobs of each type
```

### What load testing does

The load test script (`cli/src/load-test.ts`):

1. Creates jobs of all three trigger types (IMMEDIATE, DELAYED, CRON)
2. For `load-test` (default): polls job status until all executions complete or timeout
3. For `load-test-nw` (`--no-wait`): fires jobs and exits immediately without waiting for completion

:::tip
Use `just load-test-nw` for high-throughput testing where you want to saturate the worker without the overhead of polling. Monitor completion via Prometheus metrics or the dashboard.
:::

### Prerequisites

Load tests require:
- All services running (`just dev`)
- CLI environment variables set (`INVOKR_URL`, `INVOKR_API_KEY`, `INVOKR_ORG_ID`, `INVOKR_WORKSPACE_ID`)
- An endpoint configured (the test scripts create their own test endpoint pointing at the mock server)

## Mock server

The mock HTTP server (`invokr-mock-server` crate) is a test fixture that provides predefined HTTP responses on port 9999.

### Starting the mock server

```bash
just mock-server
```

### Endpoints

| Path | Method | Response | Use case |
|------|--------|----------|----------|
| `/success` | GET | `200 OK` | Simulate a successful HTTP dispatch |
| `/fail` | GET | `500 Internal Server Error` | Simulate a failed dispatch (triggers retry) |
| `/slow` | GET | `200 OK` (delayed) | Simulate a slow endpoint (for timeout testing) |
| `/echo` | POST | `200 OK` with request body | Verify request body and headers |
| `/flaky` | GET | Alternates 200/500 | Simulate intermittent failures |
| `/health` | GET | `200 OK` | Health check |

:::tip
Point your HTTP endpoint's `url` field to the mock server for local testing:
```json
{
  "spec": {
    "url": "http://localhost:9999/success",
    "method": "POST",
    "timeout_ms": 5000,
    "expected_status_codes": [200]
  }
}
```
:::

## Test workflow summary

| What to test | Command | Infrastructure needed |
|--------------|---------|----------------------|
| Quick local check | `cargo test --workspace` | None |
| HTTP dispatchers | `just test-http` | Mock server |
| Kafka dispatchers | `just test-kafka` | Kafka |
| Redis dispatchers | `just test-redis` | Redis |
| All dispatchers | `just test-dispatchers` | Mock + Kafka + Redis |
| Immediate E2E | `just test-immediate` | `just dev` running |
| Delayed E2E | `just test-delayed` | `just dev` + scheduler |
| CRON E2E | `just test-cron` | `just dev` + scheduler |
| Internal guards | `just test-internal-guards` | API only |
| Full E2E | `just test-e2e` | Self-contained (starts services) |
| Haskell SDK | `just test-haskell` | Self-contained (starts services) |
| Load test | `just load-test 50` | `just dev` running |

## See also

- [Development Setup](./setup) — starting services and managing the database
- [Building](./building) — building with feature flags
- [SDK Code Generation](./sdk-codegen) — Smithy model workflow
- [HTTP Endpoints](../guides/http-endpoints) — using the mock server for endpoint testing
