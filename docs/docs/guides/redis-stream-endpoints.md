---
id: redis-stream-endpoints
title: Redis Stream Endpoints
---

# Redis Stream Endpoints

Redis Stream endpoints deliver messages to Redis Streams via the `XADD` command. When a job fires, the worker resolves templates, connects to Redis, and appends an entry to the specified stream. The generated message ID is recorded as the execution output. Optional `MAXLEN` trimming keeps streams bounded.

## Feature flag

Redis Stream support is behind a compile-time feature flag. Build the worker with the `redis-stream` feature:

```bash
# Build with Redis Stream support
cargo build --features kronos-worker/redis-stream

# Or build the entire workspace with Redis Stream
cargo build --workspace --features kronos-worker/redis-stream
```

:::warning
Without the `redis-stream` feature flag, Redis Stream endpoints cannot be dispatched. If you register a `REDIS_STREAM` endpoint but run a worker built without the feature, executions will fail.
:::

## Starting Redis for local development

Redis is an optional infrastructure service in the Docker Compose file. Start it with the `redis` profile:

```bash
docker compose --profile redis up -d
```

This starts a Redis 7 server on `localhost:6379` with persistent storage.

To stop Redis:

```bash
docker compose --profile redis down
```

## Endpoint spec fields

The `spec` object for a Redis Stream endpoint (`type: "REDIS_STREAM"`):

| Field | Type | Required | Default | Description |
|-------|------|:--------:|:-------:|-------------|
| `redis_url` | string | no | `redis://127.0.0.1:6379` | Redis connection URL. Supports `{{config.*}}`, `{{secret.*}}`. |
| `stream` | string | yes | | Target Redis Stream name. Supports `{{config.*}}`. |
| `fields_template` | object | yes | | Key-value pairs written as stream entry fields. Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `max_len` | integer | no | | `MAXLEN` for stream trimming. When set, old entries are trimmed. |
| `approximate_trimming` | boolean | no | `true` | If `true`, uses approximate trimming (`~`), which is faster. If `false`, uses exact trimming. |
| `timeout_ms` | integer | no | `3000` | Connection/dispatch timeout. |

:::note
The `fields_template` is required. If it is missing, the dispatch fails immediately with `STREAM_ERROR` and the message `"Missing fields_template in spec"`.
:::

## Template resolution

Redis Stream endpoint specs support the same three template namespaces:

| Namespace | Source | Usable in |
|-----------|--------|-----------|
| `{{input.*}}` | Per-job input payload | `fields_template` |
| `{{config.*}}` | Endpoint's referenced config | `redis_url`, `stream`, `fields_template` |
| `{{secret.*}}` | Encrypted secret store | `redis_url`, `fields_template` |

The worker resolves templates before constructing the `XADD` command. Non-string field values (numbers, booleans, objects) are serialized to JSON strings automatically.

Example spec with templates:

```json
{
  "redis_url": "{{config.redis_url}}",
  "stream": "{{config.stream_name}}",
  "fields_template": {
    "user_id": "{{input.user_id}}",
    "title": "{{input.title}}",
    "body": "{{input.body}}"
  },
  "max_len": "{{config.max_stream_length}}",
  "approximate_trimming": true,
  "timeout_ms": 3000
}
```

## XADD with optional MAXLEN trimming

The dispatcher constructs a Redis `XADD` command with the following logic:

**With `max_len` and `approximate_trimming: true` (default):**

```redis
XADD <stream> MAXLEN ~ <max_len> * <field1> <value1> <field2> <value2> ...
```

**With `max_len` and `approximate_trimming: false` (exact):**

```redis
XADD <stream> MAXLEN <max_len> * <field1> <value1> <field2> <value2> ...
```

**Without `max_len` (no trimming):**

```redis
XADD <stream> * <field1> <value1> <field2> <value2> ...
```

:::tip
Use approximate trimming (`approximate_trimming: true`) in production for better performance. Exact trimming (`false`) forces Redis to trim precisely to `max_len`, which is slower under high throughput.
:::

## Connection management

The dispatcher uses the `redis` crate's multiplexed async connection (`get_multiplexed_async_connection`). This provides efficient connection reuse over a single TCP connection with automatic pipelining.

## Response output

On success, the attempt output contains the Redis Stream message ID and stream name:

```json
{
  "message_id": "1710499801234-0",
  "stream": "notifications:outbound"
}
```

The message ID is a Redis-generated timestamp-sequence identifier (e.g. `1710499801234-0`).

## Error handling

| Error type | When |
|------------|------|
| `STREAM_ERROR` | Missing `fields_template`, or Redis returned a stream-level error (non-timeout) |
| `CONNECTION_ERROR` | Failed to open Redis connection or connect to the URL |
| `TIMEOUT` | Operation exceeded the timeout |

Error shapes:

```json
{ "type": "CONNECTION_ERROR", "message": "Redis connection failed: ..." }
{ "type": "STREAM_ERROR", "message": "Missing fields_template in spec" }
{ "type": "TIMEOUT", "message": "..." }
```

## Full example: end-to-end Redis Stream job

:::info Prerequisites
- API server running at `http://localhost:8080`
- Worker running with Redis Stream feature: `cargo run --features kronos-worker/redis-stream -p kronos-worker`
- Redis running: `docker compose --profile redis up -d`
- Replace `<org_id>` and `<workspace_id>` with your values
:::

### 1. Create a payload spec

```bash
curl -X POST http://localhost:8080/v1/payload-specs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "notification-input",
    "schema": {
      "type": "object",
      "properties": {
        "user_id": { "type": "string" },
        "title": { "type": "string" },
        "body": { "type": "string" }
      },
      "required": ["user_id", "title"]
    }
  }'
```

### 2. Create a config with Redis connection details

```bash
curl -X POST http://localhost:8080/v1/configs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "notification-stream",
    "values": {
      "redis_url": "redis://localhost:6379",
      "stream_name": "notifications:outbound",
      "max_stream_length": 100000
    }
  }'
```

### 3. Create a Redis Stream endpoint

```bash
curl -X POST http://localhost:8080/v1/endpoints \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "push-notification",
    "type": "REDIS_STREAM",
    "payload_spec": "notification-input",
    "config": "notification-stream",
    "spec": {
      "redis_url": "{{config.redis_url}}",
      "stream": "{{config.stream_name}}",
      "fields_template": {
        "user_id": "{{input.user_id}}",
        "title": "{{input.title}}",
        "body": "{{input.body}}"
      },
      "max_len": "{{config.max_stream_length}}",
      "approximate_trimming": true,
      "timeout_ms": 3000
    },
    "retry_policy": {
      "max_attempts": 3,
      "backoff": "exponential",
      "initial_delay_ms": 500,
      "max_delay_ms": 10000
    }
  }'
```

### 4. Fire a job

```bash
curl -X POST http://localhost:8080/v1/jobs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "endpoint": "push-notification",
    "trigger": "IMMEDIATE",
    "idempotency_key": "notif-001-push",
    "input": {
      "user_id": "u_abc",
      "title": "Welcome!",
      "body": "Thanks for signing up."
    }
  }'
```

Response (`201 Created`):

```json
{
  "job_id": "job_e5f6...",
  "endpoint": "push-notification",
  "endpoint_type": "REDIS_STREAM",
  "trigger": "IMMEDIATE",
  "status": "ACTIVE",
  "version": 1,
  "idempotency_key": "notif-001-push",
  "execution": {
    "execution_id": "exec_g7h8...",
    "status": "QUEUED",
    "created_at": "2026-03-15T10:00:00Z"
  },
  "created_at": "2026-03-15T10:00:00Z"
}
```

### 5. Verify the entry in the stream

Use `redis-cli` to read entries from the stream:

```bash
docker exec -it $(docker ps -qf "ancestor=redis:7-alpine") \
  redis-cli XRANGE notifications:outbound - +
```

You should see:

```
1) 1) "1710499801234-0"
   2) 1) "user_id"
      2) "u_abc"
      3) "title"
      4) "Welcome!"
      5) "body"
      6) "Thanks for signing up."
```

### 6. Check the execution result

```bash
curl http://localhost:8080/v1/executions/{execution_id} \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"
```

The attempt output:

```json
{
  "output": {
    "message_id": "1710499801234-0",
    "stream": "notifications:outbound"
  }
}
```

## Running Redis Stream dispatcher tests

The Redis Stream dispatcher has integration tests that require a running Redis server:

```bash
# Start Redis
docker compose --profile redis up -d

# Run Redis Stream dispatcher tests (single-threaded)
just test-redis
```

Tests verify:
- Successful `XADD` with message ID returned
- `MAXLEN` trimming with approximate and exact modes
- Failure on missing `fields_template` (`STREAM_ERROR`)
- Failure on bad Redis URL (`CONNECTION_ERROR`)
- Unique message IDs for multiple messages to the same stream
- Non-string field values (numbers, booleans, objects) are serialized correctly

## Field value serialization

When `fields_template` contains non-string values, they are serialized to their JSON representation:

| Input value | Stored in stream |
|-------------|-----------------|
| `"hello"` | `hello` |
| `42` | `42` |
| `true` | `true` |
| `{"nested": "value"}` | `{"nested":"value"}` |

## See also

- [HTTP endpoints](./http-endpoints) — delivering to HTTP URLs
- [Kafka endpoints](./kafka-endpoints) — delivering to Kafka topics
- [Template resolution](../core-concepts/templates) — how templates are resolved
- [Retry policy](../core-concepts/retry-policy) — backoff strategies for failed dispatches
