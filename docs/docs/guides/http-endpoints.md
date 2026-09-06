---
id: http-endpoints
title: HTTP Endpoints
---

# HTTP Endpoints

HTTP is the most common endpoint type in Invokr. An HTTP endpoint defines a target URL, HTTP method, headers, body template, timeout, and the status codes that indicate success. When a job fires, the worker resolves all templates, sends the request via `reqwest`, and records the result.

## Endpoint spec fields

The `spec` object for an HTTP endpoint (`type: "HTTP"`):

| Field | Type | Required | Default | Description |
|-------|------|:--------:|:-------:|-------------|
| `url` | string | yes | | Target URL. Supports `{{config.*}}`, `{{secret.*}}`. |
| `method` | string | yes | `POST` | One of `GET`, `POST`, `PUT`, `PATCH`, `DELETE`. |
| `headers` | object | no | | Key-value header map. Supports `{{config.*}}`, `{{secret.*}}`. |
| `body_template` | object | no | | JSON body. Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `body` | object | no | | Static JSON body (no template resolution). Used if `body_template` is absent. |
| `timeout_ms` | integer | yes | `5000` | Request timeout in milliseconds. |
| `expected_status_codes` | integer[] | no | `[200, 201, 202, 204]` | Status codes treated as success. Any non-matching code is a failure. |

:::note
`body_template` takes precedence over `body`. If neither is specified, the worker injects the job's `input` object as the JSON request body. This lets you fire jobs with arbitrary input without pre-defining a body template — useful for generic webhook-style endpoints.
:::

## Template resolution

HTTP endpoint specs support three template namespaces, resolved at execution time by the worker:

| Namespace | Source | Example |
|-----------|--------|---------|
| `{{input.*}}` | Per-job input payload | `{{input.user_id}}` → `"u_abc"` |
| `{{config.*}}` | Endpoint's referenced config | `{{config.api_base_url}}` → `"https://api.myapp.com"` |
| `{{secret.*}}` | Encrypted secret store | `{{secret.email_api_key}}` → resolved at runtime, never exposed |

Templates can appear in:

- **`url`** — e.g. `"{{config.api_base_url}}/emails/welcome"`
- **`headers`** (values) — e.g. `"Authorization": "Bearer {{secret.email_api_key}}"`
- **`body_template`** (any value, recursively) — e.g. `"user_id": "{{input.user_id}}"`

The template engine walks the entire JSON tree — objects, arrays, and nested keys — replacing every `{{namespace.key}}` occurrence. If a variable is unresolvable, the execution fails immediately with `TEMPLATE_RESOLUTION_FAILED` (no retry, since it would fail identically).

:::tip
Templates support both **full replacement** (when the entire string is a single `{{var}}`, the native JSON type is preserved) and **string interpolation** (when templates are embedded in surrounding text, the result is always a string).
:::

## Auto-injected idempotency header

For every HTTP dispatch, the worker automatically injects an `x-invokr-idempotency-key` header containing the execution's idempotency key. This allows downstream services to deduplicate retries safely.

- For `IMMEDIATE` and `DELAYED` jobs, the key is the client-provided `idempotency_key`.
- For `CRON` jobs, the key is system-generated: `cron_{job_id}_{epoch_ms}`.

If you already set a header named `x-invokr-idempotency-key` (case-insensitive) in your endpoint's `headers`, the worker respects your value and does not override it.

## Timeout handling

The `timeout_ms` field is applied to each individual HTTP request via `reqwest`'s per-request timeout. If the request exceeds the timeout, the attempt is marked as failed with error type `TIMEOUT`, and the execution follows the retry policy (if retries remain).

```json
{
  "type": "TIMEOUT",
  "message": "..."
}
```

## Expected status codes

The `expected_status_codes` array defines which HTTP response codes are treated as success. If the response status is **not** in this list, the attempt fails with error type `HTTP_ERROR`:

```json
{
  "type": "HTTP_ERROR",
  "status_code": 503,
  "message": "Unexpected status code: 503"
}
```

If `expected_status_codes` is omitted, the default is `[200, 201, 202, 204]`.

## Response handling

On success, the attempt output is:

```json
{
  "status_code": 200,
  "body": "OK"
}
```

The full response body is captured as a string. On failure, the error object includes the type and message:

| Error type | When |
|------------|------|
| `HTTP_ERROR` | Response status not in `expected_status_codes` |
| `TIMEOUT` | Request exceeded `timeout_ms` |
| `CONNECTION_ERROR` | Could not connect to the target (DNS failure, refused, etc.) |

## Full example: end-to-end HTTP job

This example creates a payload spec, config, secret, HTTP endpoint, fires a job, and checks the result.

:::info Prerequisites
- API server running at `http://localhost:8080`
- Worker running (`just worker`)
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
    "name": "order-input",
    "schema": {
      "type": "object",
      "properties": {
        "order_id": { "type": "string" },
        "user_id": { "type": "string" }
      },
      "required": ["order_id"]
    }
  }'
```

### 2. Create a config

```bash
curl -X POST http://localhost:8080/v1/configs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "email-service",
    "values": {
      "api_base_url": "https://api.myapp.com",
      "sender": "noreply@myapp.com"
    }
  }'
```

### 3. Create a secret

```bash
curl -X POST http://localhost:8080/v1/secrets \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "email_api_key",
    "value": "sk-your-api-key"
  }'
```

:::note
Secrets are write-only. The `value` is never returned in API responses — only metadata (`name`, `created_at`, `updated_at`).
:::

### 4. Create an HTTP endpoint

```bash
curl -X POST http://localhost:8080/v1/endpoints \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "send-welcome-email",
    "type": "HTTP",
    "payload_spec": "order-input",
    "config": "email-service",
    "spec": {
      "url": "{{config.api_base_url}}/emails/welcome",
      "method": "POST",
      "headers": {
        "Authorization": "Bearer {{secret.email_api_key}}",
        "Content-Type": "application/json"
      },
      "body_template": {
        "order_id": "{{input.order_id}}",
        "sender": "{{config.sender}}"
      },
      "timeout_ms": 5000,
      "expected_status_codes": [200, 201, 202, 204]
    },
    "retry_policy": {
      "max_attempts": 3,
      "backoff": "exponential",
      "initial_delay_ms": 1000,
      "max_delay_ms": 30000
    }
  }'
```

### 5. Fire a job

```bash
curl -X POST http://localhost:8080/v1/jobs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>" \
  -H "Content-Type: application/json" \
  -d '{
    "endpoint": "send-welcome-email",
    "trigger": "IMMEDIATE",
    "idempotency_key": "order-1234-welcome",
    "input": { "order_id": "order-1234", "user_id": "u_abc" }
  }'
```

Response (`201 Created`):

```json
{
  "job_id": "job_8f3a...",
  "endpoint": "send-welcome-email",
  "endpoint_type": "HTTP",
  "trigger": "IMMEDIATE",
  "status": "ACTIVE",
  "version": 1,
  "idempotency_key": "order-1234-welcome",
  "input": { "order_id": "order-1234", "user_id": "u_abc" },
  "execution": {
    "execution_id": "exec_2b7c...",
    "status": "QUEUED",
    "created_at": "2026-03-15T10:00:00Z"
  },
  "created_at": "2026-03-15T10:00:00Z"
}
```

### 6. Check the result

```bash
# Get execution details
curl http://localhost:8080/v1/executions/{execution_id} \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"

# List attempts for the execution
curl http://localhost:8080/v1/executions/{execution_id}/attempts \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"
```

The attempt output will contain the HTTP response:

```json
{
  "output": {
    "status_code": 200,
    "body": "OK"
  }
}
```

:::tip
For local testing without an external API, use the bundled mock HTTP server (`just mock-server`, port 9999). Set your endpoint's `url` to `http://localhost:9999/success` to simulate a successful response, or `http://localhost:9999/fail` to trigger a 500.
:::

## Using the mock server for testing

The `invokr-mock-server` crate provides a test HTTP server on port 9999 with predefined endpoints:

| Path | Response |
|------|----------|
| `/success` | `200 OK` |
| `/fail` | `500 Internal Server Error` |
| `/health` | `200 OK` |

Start it with:

```bash
just mock-server
```

Run the HTTP dispatcher tests:

```bash
just test-http
```

## See also

- [Template resolution](../core-concepts/templates) — how `{{input.*}}`, `{{config.*}}`, and `{{secret.*}}` are resolved
- [Kafka endpoints](./kafka-endpoints) — delivering to Kafka topics
- [Redis Stream endpoints](./redis-stream-endpoints) — delivering to Redis Streams
- [Cron jobs](./cron-jobs) — scheduling recurring HTTP deliveries
