---
id: kafka-endpoints
title: Kafka Endpoints
---

# Kafka Endpoints

Kafka endpoints deliver messages to Apache Kafka topics. When a job fires, the worker resolves templates, creates a `rdkafka::FutureProducer`, produces the message, and awaits broker acknowledgement. The partition and offset are recorded as the execution output.

## Feature flag

Kafka support is behind a compile-time feature flag. Build the worker with the `kafka` feature:

```bash
# Build with Kafka support
cargo build --features kronos-worker/kafka

# Or build the entire workspace with Kafka
cargo build --workspace --features kronos-worker/kafka
```

:::warning
Without the `kafka` feature flag, Kafka endpoints cannot be dispatched. The worker will fail to compile the Kafka dispatcher module. If you register a Kafka endpoint but run a worker built without the feature, executions will fail.
:::

## Starting Kafka for local development

Kafka is an optional infrastructure service in the Docker Compose file. Start it with the `kafka` profile:

```bash
docker compose --profile kafka up -d
```

This starts a single-node Kafka broker (Bitnami Kafka 3.7) on `localhost:9092` in KRaft mode (no Zookeeper required).

To stop Kafka:

```bash
docker compose --profile kafka down
```

## Endpoint spec fields

The `spec` object for a Kafka endpoint (`type: "KAFKA"`):

| Field | Type | Required | Default | Description |
|-------|------|:--------:|:-------:|-------------|
| `bootstrap_servers` | string | yes | | Kafka broker address(es). Supports `{{config.*}}`, `{{secret.*}}`. |
| `topic` | string | yes | | Target topic. Supports `{{config.*}}`. |
| `key_template` | string | no | | Message key. Supports `{{input.*}}`, `{{config.*}}`. |
| `value_template` | object | yes | | Message value (serialized to JSON). Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `headers` | object | no | | Kafka message headers (key-value pairs). Supports `{{config.*}}`, `{{secret.*}}`. |
| `acks` | string | no | `"all"` | Acknowledgement level: `"0"`, `"1"`, or `"all"`. |
| `timeout_ms` | integer | no | `10000` | Producer timeout in milliseconds. |

:::info
If `value_template` is omitted, the worker defaults to an empty JSON object (`{}`) as the message value.
:::

## Template resolution

Kafka endpoint specs support the same three template namespaces as HTTP endpoints:

| Namespace | Source | Usable in |
|-----------|--------|-----------|
| `{{input.*}}` | Per-job input payload | `key_template`, `value_template` |
| `{{config.*}}` | Endpoint's referenced config | `bootstrap_servers`, `topic`, `key_template`, `value_template`, `headers` |
| `{{secret.*}}` | Encrypted secret store | `bootstrap_servers`, `value_template`, `headers` |

The worker resolves `{{config.*}}` and `{{secret.*}}` first (from cache), then `{{input.*}}` from the per-execution input. Unresolvable variables cause immediate execution failure.

Example spec with templates:

```json
{
  "bootstrap_servers": "{{config.bootstrap_servers}}",
  "topic": "{{config.topic}}",
  "key_template": "{{input.order_id}}",
  "value_template": {
    "event_type": "{{input.event_type}}",
    "order_id": "{{input.order_id}}",
    "amount": "{{input.amount}}"
  },
  "headers": {
    "ce-type": "order.{{input.event_type}}",
    "ce-source": "task-executor"
  },
  "acks": "all",
  "timeout_ms": 10000
}
```

## rdkafka FutureProducer

The Kafka dispatcher uses [`rdkafka`](https://docs.rs/rdkafka)'s `FutureProducer` to produce messages asynchronously:

1. A `FutureProducer` is created with `bootstrap.servers`, `message.timeout.ms`, and `acks` from the resolved spec.
2. A `FutureRecord` is constructed with the resolved `topic`, `key` (from `key_template` if present), `payload` (the serialized `value_template`), and optional headers.
3. The producer's `send()` method is awaited with a timeout of `timeout_ms`.
4. On success, the partition and offset are returned as the execution output.
5. On failure, the error is classified as `BROKER_ERROR` or `TIMEOUT`.

:::note
Each dispatch creates a new `FutureProducer` instance. In production, Kafka endpoints typically share a connection via config values, but the producer lifecycle is per-dispatch in the current implementation.
:::

## Response output

On success, the attempt output contains the Kafka delivery metadata:

```json
{
  "partition": 3,
  "offset": 12847
}
```

## Error handling

| Error type | When |
|------------|------|
| `BROKER_ERROR` | Failed to create producer or broker rejected the message (non-timeout) |
| `TIMEOUT` | Message delivery exceeded `timeout_ms` |

Error shape:

```json
{
  "type": "BROKER_ERROR",
  "message": "Failed to create producer: ..."
}
```

Failed dispatches follow the endpoint's retry policy. See the [retry policy documentation](../core-concepts/retry-policy) for backoff strategies.

## Full example: end-to-end Kafka job

:::info Prerequisites
- API server running at `http://localhost:8080`
- Worker running with Kafka feature: `cargo run --features kronos-worker/kafka -p kronos-worker`
- Kafka running: `docker compose --profile kafka up -d`
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
    "name": "order-event-input",
    "schema": {
      "type": "object",
      "properties": {
        "order_id": { "type": "string" },
        "event_type": { "type": "string" },
        "amount": { "type": "number" }
      },
      "required": ["order_id", "event_type"]
    }
  }'
```

### 2. Create a config with Kafka connection details

```bash
curl -X POST http://localhost:8080/v1/configs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "order-events",
    "values": {
      "bootstrap_servers": "localhost:9092",
      "topic": "order.events.v1"
    }
  }'
```

### 3. Create a Kafka endpoint

```bash
curl -X POST http://localhost:8080/v1/endpoints \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "publish-order-event",
    "type": "KAFKA",
    "payload_spec": "order-event-input",
    "config": "order-events",
    "spec": {
      "bootstrap_servers": "{{config.bootstrap_servers}}",
      "topic": "{{config.topic}}",
      "key_template": "{{input.order_id}}",
      "value_template": {
        "event_type": "{{input.event_type}}",
        "order_id": "{{input.order_id}}",
        "amount": "{{input.amount}}"
      },
      "headers": {
        "ce-type": "order.{{input.event_type}}",
        "ce-source": "kronos"
      },
      "acks": "all",
      "timeout_ms": 10000
    },
    "retry_policy": {
      "max_attempts": 5,
      "backoff": "exponential",
      "initial_delay_ms": 500,
      "max_delay_ms": 15000
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
    "endpoint": "publish-order-event",
    "trigger": "IMMEDIATE",
    "idempotency_key": "order-5678-event",
    "input": {
      "order_id": "order-5678",
      "event_type": "created",
      "amount": 99.99
    }
  }'
```

Response (`201 Created`):

```json
{
  "job_id": "job_a1b2...",
  "endpoint": "publish-order-event",
  "endpoint_type": "KAFKA",
  "trigger": "IMMEDIATE",
  "status": "ACTIVE",
  "version": 1,
  "idempotency_key": "order-5678-event",
  "execution": {
    "execution_id": "exec_c3d4...",
    "status": "QUEUED",
    "created_at": "2026-03-15T10:00:00Z"
  },
  "created_at": "2026-03-15T10:00:00Z"
}
```

### 5. Verify the message in the topic

Use the Kafka CLI consumer to verify the message was delivered:

```bash
# Create the topic first if it doesn't exist
docker exec -it $(docker ps -qf "ancestor=bitnamilegacy/kafka:3.7") \
  kafka-topics.sh --create \
  --bootstrap-server localhost:9092 \
  --topic order.events.v1 \
  --partitions 1 \
  --replication-factor 1

# Consume messages from the beginning
docker exec -it $(docker ps -qf "ancestor=bitnamilegacy/kafka:3.7") \
  kafka-console-consumer.sh \
  --bootstrap-server localhost:9092 \
  --topic order.events.v1 \
  --from-beginning
```

You should see:

```json
{"event_type":"created","order_id":"order-5678","amount":99.99}
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
    "partition": 0,
    "offset": 0
  }
}
```

## Running Kafka dispatcher tests

The Kafka dispatcher has integration tests that require a running Kafka broker:

```bash
# Start Kafka
docker compose --profile kafka up -d

# Run Kafka dispatcher tests (single-threaded to avoid consumer group conflicts)
just test-kafka
```

Tests verify:
- Successful message production with partition/offset returned
- Messages with keys and custom headers
- Failure on bad broker address (`BROKER_ERROR` / `TIMEOUT`)
- Monotonically increasing offsets for sequential messages

## See also

- [HTTP endpoints](./http-endpoints) — delivering to HTTP URLs
- [Redis Stream endpoints](./redis-stream-endpoints) — delivering to Redis Streams
- [Template resolution](../core-concepts/templates) — how templates are resolved
- [Retry policy](../core-concepts/retry-policy) — backoff strategies for failed dispatches
