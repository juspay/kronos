# Invokr CLI Test

Tests the Invokr API using the Smithy-generated TypeScript SDK.

## Prerequisites

All services must be running. From the repo root, `just dev` starts the API,
worker, and mock server together; to drive them yourself:

```bash
# Terminal 1: PostgreSQL + pg_cron (port 5434)
docker compose up -d postgres

# Terminal 2: Invokr API
cargo run -p invokr-api

# Terminal 3: Invokr Worker
cargo run -p invokr-worker

# Terminal 4: Mock server
cargo run -p invokr-mock-server
```

The tests run against a tenant, so create one first (writes the ids into `.env`):

```bash
./scripts/setup-dev-tenant.sh
```

## Setup

```bash
# Generate the SDK from Smithy models
cd ../smithy && smithy build

# Build the SDK
cd ../smithy/build/smithy/source/typescript-client-codegen && npm install && npm run build

# Install CLI deps
cd ../cli && npm install
```

## Run Tests

```bash
# Test immediate job execution (end-to-end)
npx tsx src/test-immediate.ts
```

### Environment Variables

| Variable | Default | Description |
|---|---|---|
| `INVOKR_URL` | `http://localhost:8080` | Invokr API base URL |
| `MOCK_URL` | `http://localhost:9999` | Mock server base URL |
| `INVOKR_API_KEY` | `dev-api-key` | Bearer token for API auth |
| `INVOKR_ORG_ID` | _(required)_ | Org id sent as `X-Org-Id`; set by `scripts/setup-dev-tenant.sh` |
| `INVOKR_WORKSPACE_ID` | _(required)_ | Workspace id sent as `X-Workspace-Id`; set by `scripts/setup-dev-tenant.sh` |
