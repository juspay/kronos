# Kronos CLI Test

Tests the Kronos API using the Smithy-generated TypeScript SDK.

## Prerequisites

All services must be running:

```bash
# Terminal 1: CockroachDB
docker-compose up -d

# Terminal 2: Kronos API
cargo run -p kronos-api

# Terminal 3: Kronos Worker
cargo run -p kronos-worker

# Terminal 4: Mock server
cargo run -p kronos-mock-server
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
| `KRONOS_URL` | `http://localhost:8080` | Kronos API base URL |
| `MOCK_URL` | `http://localhost:9999` | Mock server base URL |
| `KRONOS_API_KEY` | `dev-api-key` | Bearer token for API auth |
