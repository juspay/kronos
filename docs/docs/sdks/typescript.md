---
id: typescript
title: TypeScript SDK
---

# TypeScript SDK

The TypeScript SDK is generated from Smithy IDL models and provides a fully typed client for the Kronos REST API. It's published as the `kronos-sdk` npm package.

## Installation

The TypeScript SDK is generated and compiled via the justfile:

```bash
# Generate from Smithy models and compile the npm package
just build-sdk

# Install CLI dependencies (links to the built SDK)
just cli-install
```

This produces the `kronos-sdk` package in `smithy/build/smithy/source/typescript-client-codegen/`, which is then linked into the `cli/` project.

## Client Setup

Create a `KronosServiceClient` with the API endpoint and bearer token:

```typescript
import { KronosServiceClient, CreateJobCommand } from "kronos-sdk";

const client = new KronosServiceClient({
  endpoint: "http://localhost:8080",
  token: { token: "dev-api-key" },
});
```

| Parameter | Description | Default |
|-----------|-------------|---------|
| `endpoint` | Kronos API base URL | — |
| `token.token` | Bearer token for authentication | — |

For tenant-scoped operations, pass `org_id` and `workspace_id` with each command:

```typescript
const tenant = {
  org_id: process.env.KRONOS_ORG_ID!,
  workspace_id: process.env.KRONOS_WORKSPACE_ID!,
};
```

## Creating a Job

```typescript
import { KronosServiceClient, CreateJobCommand } from "kronos-sdk";

const client = new KronosServiceClient({
  endpoint: "http://localhost:8080",
  token: { token: "dev-api-key" },
});

const response = await client.send(
  new CreateJobCommand({
    ...tenant,
    endpoint: "send-welcome-email",
    trigger: "IMMEDIATE",
    idempotency_key: "order-1234-welcome",
    input: { order_id: "order-1234", user_id: "u_abc" },
  }),
);

console.log(response.data!.job_id);
```

### Response Shape

For `IMMEDIATE` and `DELAYED` triggers, the response includes an `execution` object:

```typescript
console.log(response.data!.job_id);           // "job_8f3a..."
console.log(response.data!.execution!.execution_id);  // "exec_2b7c..."
console.log(response.data!.execution!.status);         // "QUEUED"
```

For `CRON` triggers, the response includes scheduling metadata:

```typescript
console.log(response.data!.job_id);        // "job_c72f..."
console.log(response.data!.cron);         // "0 9 * * MON"
console.log(response.data!.next_run_at);  // "2026-03-16T09:00:00+05:30"
```

## Other Operations

### Create an Endpoint

```typescript
import { CreateEndpointCommand } from "kronos-sdk";

const endpointResp = await client.send(
  new CreateEndpointCommand({
    ...tenant,
    name: "send-welcome-email",
    endpoint_type: "HTTP",
    spec: {
      method: "POST",
      url: "https://api.myapp.com/emails/welcome",
      headers: {
        "Content-Type": "application/json",
      },
    },
    retry_policy: {
      max_attempts: 3,
      backoff: "exponential",
      initial_delay_ms: 500,
      max_delay_ms: 5000,
    },
  }),
);
```

### Create a Payload Spec

```typescript
import { CreatePayloadSpecCommand } from "kronos-sdk";

await client.send(
  new CreatePayloadSpecCommand({
    ...tenant,
    name: "order-input",
    schema: {
      type: "object",
      properties: {
        order_id: { type: "string" },
        user_id: { type: "string" },
      },
      required: ["order_id"],
    },
  }),
);
```

### Get an Execution

```typescript
import { GetExecutionCommand } from "kronos-sdk";

const execResp = await client.send(
  new GetExecutionCommand({
    ...tenant,
    execution_id: "exec_2b7c...",
  }),
);

console.log(execResp.data!.status);        // "SUCCESS"
console.log(execResp.data!.attempt_count); // 1
console.log(execResp.data!.duration_ms);   // 340
```

### Cancel a Job

```typescript
import { CancelJobCommand } from "kronos-sdk";

await client.send(
  new CancelJobCommand({
    ...tenant,
    job_id: "job_c72f...",
  }),
);
```

### List Executions for a Job

```typescript
import { ListJobExecutionsCommand } from "kronos-sdk";

const execsResp = await client.send(
  new ListJobExecutionsCommand({
    ...tenant,
    job_id: "job_8f3a...",
  }),
);

const executions = execsResp.data ?? [];
for (const exec of executions) {
  console.log(`${exec.execution_id}: ${exec.status}`);
}
```

### List Execution Attempts

```typescript
import { ListExecutionAttemptsCommand } from "kronos-sdk";

const attemptsResp = await client.send(
  new ListExecutionAttemptsCommand({
    ...tenant,
    execution_id: "exec_2b7c...",
  }),
);

const attempts = attemptsResp.data ?? [];
for (const attempt of attempts) {
  console.log(`#${attempt.attempt_number} — ${attempt.status} (${attempt.duration_ms}ms)`);
}
```

## CLI Test Scripts

The `cli/src/` directory contains test scripts that exercise the full Kronos lifecycle using the TypeScript SDK:

| Script | Just Recipe | Description |
|--------|------------|-------------|
| `test-immediate.ts` | `just test-immediate` | Create an endpoint, fire an IMMEDIATE job, poll until terminal state |
| `test-delayed.ts` | `just test-delayed` | Test delayed job execution with `run_at` |
| `test-cron.ts` | `just test-cron` | Test CRON job scheduling and execution |
| `load-test.ts` | `just load-test 50` | Create 50 jobs of each type and track completion |
| `test-internal-guards.ts` | — | Verify API guards reject user jobs targeting INTERNAL endpoints |

### Running Tests

```bash
# Prerequisites: all services running
just dev

# Set tenant environment variables
export KRONOS_ORG_ID="<your_org_id>"
export KRONOS_WORKSPACE_ID="<your_workspace_id>"

# Run individual tests
just test-immediate    # Test immediate job execution
just test-delayed      # Test delayed job execution
just test-cron         # Test CRON job execution

# Load testing
just load-test 50      # Create 50 jobs of each type
just load-test-nw 50   # Fire-and-forget (no polling)
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `KRONOS_URL` | `http://localhost:8080` | Kronos API base URL |
| `MOCK_URL` | `http://localhost:9999` | Mock server base URL |
| `KRONOS_API_KEY` | `dev-api-key` | Bearer token for API authentication |
| `KRONOS_ORG_ID` | *(required)* | Organization ID for tenant routing |
| `KRONOS_WORKSPACE_ID` | *(required)* | Workspace ID for tenant routing |

### Test Script Example (test-immediate.ts)

The test scripts follow a common pattern:

1. Create an HTTP endpoint pointing to the mock server
2. Create a job targeting that endpoint
3. Poll `ListJobExecutions` until the execution reaches a terminal state (`SUCCESS`, `FAILED`, `CANCELLED`)
4. Print execution details, attempts, and job status
5. Clean up (cancel job, delete endpoint)

```typescript
const TERMINAL_STATUSES = new Set(["SUCCESS", "FAILED", "CANCELLED"]);

while (Date.now() - startTime < POLL_TIMEOUT_MS) {
  const execsResp = await client.send(
    new ListJobExecutionsCommand({ ...tenant, job_id: jobId }),
  );

  const executions = execsResp.data ?? [];
  if (executions.length > 0) {
    const exec = executions[0];
    if (TERMINAL_STATUSES.has(exec.status!)) {
      // Fetch full execution details
      const fullExec = await client.send(
        new GetExecutionCommand({ ...tenant, execution_id: exec.execution_id! }),
      );
      break;
    }
  }
  await sleep(POLL_INTERVAL_MS);
}
```

## Error Handling

The SDK throws errors with metadata for non-success responses:

```typescript
try {
  await client.send(new CreateJobCommand({ ... }));
} catch (err: any) {
  console.error(`${err.name}: ${err.message}`);
  if (err.$metadata) {
    console.error(`HTTP ${err.$metadata.httpStatusCode}`);
  }
}
```

:::tip
The SDK uses the AWS Smithy TypeScript runtime, which provides the same command/response pattern as the AWS SDK for JavaScript. If you're familiar with `aws-sdk` v3, the Kronos SDK will feel immediately familiar.
:::

## Related Pages

- [SDK Overview](./overview) — All SDKs and the Smithy codegen pipeline
- [Rust SDK](./rust) — Generated Rust SDK for Rust consumers
- [Haskell SDK](./haskell) — Generated Haskell SDK
