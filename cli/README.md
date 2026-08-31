# Invokr CLI Test

Tests the Invokr API using the Smithy-generated TypeScript SDK.

## Prerequisites

All services must be running:

```bash
# Terminal 1: CockroachDB
docker-compose up -d

# Terminal 2: Invokr API
cargo run -p invokr-api

# Terminal 3: Invokr Worker
cargo run -p invokr-worker

# Terminal 4: Mock server
cargo run -p invokr-mock-server
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
