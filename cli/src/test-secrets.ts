/**
 * Kronos CLI — Test Secrets via External KMS (LocalStack)
 *
 * This script tests the full secrets flow:
 * 1. Seeds a secret in LocalStack's Secrets Manager
 * 2. Registers the secret reference in Kronos
 * 3. Creates an endpoint that uses {{secret.*}} in its spec (Authorization header)
 * 4. Creates an IMMEDIATE job targeting the /echo endpoint on the mock server
 * 5. Polls until execution completes
 * 6. Verifies the echoed response contains the resolved secret value
 *
 * Prerequisites:
 *   - LocalStack running (docker compose --profile kms up -d)
 *   - Kronos API + Worker running with TE_KMS_AWS_ENDPOINT_URL=http://localhost:4566
 *   - Mock server running
 *   - AWS env vars set: AWS_ACCESS_KEY_ID=test, AWS_SECRET_ACCESS_KEY=test
 */

import {
  KronosServiceClient,
  CreateEndpointCommand,
  CreateJobCommand,
  DeleteEndpointCommand,
  CancelJobCommand,
} from "kronos-sdk";

import {
  KRONOS_URL,
  MOCK_URL,
  API_KEY,
  log,
  sleep,
  pollExecution,
  printExecutionResult,
} from "./helpers.js";

// ─── Config ──────────────────────────────────────────────────────

const LOCALSTACK_URL =
  process.env.LOCALSTACK_URL ?? "http://localhost:4566";
const AWS_REGION = process.env.AWS_DEFAULT_REGION ?? "us-east-1";

const TEST_SECRET_NAME = `test-secret-${Date.now()}`;
const TEST_SECRET_VALUE = `sk-live-test-${Math.random().toString(36).slice(2)}`;

// ─── LocalStack Helpers ─────────────────────────────────────────

async function createLocalStackSecret(
  name: string,
  value: string,
): Promise<void> {
  const resp = await fetch(`${LOCALSTACK_URL}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": "secretsmanager.CreateSecret",
      Authorization:
        "AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/secretsmanager/aws4_request, SignedHeaders=content-type;host;x-amz-target, Signature=fake",
    },
    body: JSON.stringify({
      Name: name,
      SecretString: value,
    }),
  });

  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(
      `Failed to create LocalStack secret: ${resp.status} ${body}`,
    );
  }

  log(`LocalStack secret created: ${name}`);
}

async function deleteLocalStackSecret(name: string): Promise<void> {
  try {
    await fetch(`${LOCALSTACK_URL}/`, {
      method: "POST",
      headers: {
        "Content-Type": "application/x-amz-json-1.1",
        "X-Amz-Target": "secretsmanager.DeleteSecret",
        Authorization:
          "AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/secretsmanager/aws4_request, SignedHeaders=content-type;host;x-amz-target, Signature=fake",
      },
      body: JSON.stringify({
        SecretId: name,
        ForceDeleteWithoutRecovery: true,
      }),
    });
  } catch {
    // ignore cleanup failures
  }
}

// ─── Kronos Secret API Helpers ──────────────────────────────────

async function kronosCreateSecret(
  name: string,
  provider: string,
  reference: string,
): Promise<void> {
  const resp = await fetch(`${KRONOS_URL}/v1/secrets`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${API_KEY}`,
    },
    body: JSON.stringify({ name, provider, reference }),
  });

  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(`Failed to create Kronos secret: ${resp.status} ${body}`);
  }

  log(`Kronos secret registered: ${name} → ${provider}:${reference}`);
}

async function kronosDeleteSecret(name: string): Promise<void> {
  try {
    await fetch(`${KRONOS_URL}/v1/secrets/${encodeURIComponent(name)}`, {
      method: "DELETE",
      headers: { Authorization: `Bearer ${API_KEY}` },
    });
  } catch {
    // ignore cleanup failures
  }
}

// ─── Main ────────────────────────────────────────────────────────

async function main() {
  const client = new KronosServiceClient({
    endpoint: KRONOS_URL,
    token: { token: API_KEY },
  });

  const endpointName = `test-secrets-endpoint-${Date.now()}`;
  let jobId: string | null = null;

  try {
    // ── Step 1: Seed secret in LocalStack ───────────────────────
    log("Step 1: Creating secret in LocalStack...");
    await createLocalStackSecret(TEST_SECRET_NAME, TEST_SECRET_VALUE);

    // ── Step 2: Register secret reference in Kronos ─────────────
    log("Step 2: Registering secret in Kronos...");
    await kronosCreateSecret(TEST_SECRET_NAME, "aws", TEST_SECRET_NAME);

    // ── Step 3: Create endpoint with {{secret.*}} in headers ────
    log(
      `Step 3: Creating endpoint "${endpointName}" → ${MOCK_URL}/echo with secret in header`,
    );

    await client.send(
      new CreateEndpointCommand({
        name: endpointName,
        endpoint_type: "HTTP",
        spec: {
          method: "POST",
          url: `${MOCK_URL}/echo`,
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer {{secret.${TEST_SECRET_NAME}}}`,
            "X-Test-Source": "kronos-secrets-test",
          },
        },
        retry_policy: {
          max_attempts: 1,
          backoff: "exponential",
          initial_delay_ms: 500,
          max_delay_ms: 5000,
        },
      }),
    );

    log(`Endpoint created: ${endpointName}`);

    // ── Step 4: Create IMMEDIATE job ────────────────────────────
    log("Step 4: Creating IMMEDIATE job...");

    const jobResp = await client.send(
      new CreateJobCommand({
        endpoint: endpointName,
        trigger: "IMMEDIATE",
        input: {
          test: "secrets-flow",
          timestamp: new Date().toISOString(),
        },
      }),
    );

    jobId = jobResp.data!.job_id!;
    log(`Job created: ${jobId}`);

    // ── Step 5: Poll for execution completion ───────────────────
    log("Step 5: Polling for execution completion...");

    const execution = await pollExecution(client, jobId);

    if (!execution) {
      console.error("ERROR: Timed out waiting for execution");
      process.exit(1);
    }

    // Print full execution details
    await printExecutionResult(client, jobId, execution);

    // ── Step 6: Verify the secret was resolved ──────────────────
    log("Step 6: Verifying secret resolution...");

    if (execution.status !== "SUCCESS") {
      console.error(`ERROR: Execution status is ${execution.status}, expected SUCCESS`);
      process.exit(1);
    }

    // The mock /echo endpoint returns the request headers in output.headers
    const output = execution.output;
    if (!output) {
      console.error("ERROR: Execution has no output");
      process.exit(1);
    }

    const echoedAuthHeader = output.headers?.authorization;
    const expectedAuthHeader = `Bearer ${TEST_SECRET_VALUE}`;

    if (echoedAuthHeader === expectedAuthHeader) {
      console.log("\n  ✅ SECRET RESOLUTION VERIFIED!");
      console.log(
        `     Authorization header: "${echoedAuthHeader}"`,
      );
      console.log(
        `     Matches expected:     "${expectedAuthHeader}"`,
      );
    } else {
      console.error("\n  ❌ SECRET RESOLUTION FAILED!");
      console.error(`     Expected: "${expectedAuthHeader}"`);
      console.error(`     Got:      "${echoedAuthHeader}"`);
      process.exit(1);
    }

    console.log("\n" + "═".repeat(60));
    console.log("  SECRETS E2E TEST PASSED");
    console.log("═".repeat(60) + "\n");
  } catch (err: any) {
    console.error("\nTest failed with error:");
    console.error(`  ${err.name}: ${err.message}`);
    if (err.$metadata) {
      console.error(`  HTTP ${err.$metadata.httpStatusCode}`);
    }
    process.exit(1);
  } finally {
    // ── Cleanup ─────────────────────────────────────────────────
    log("Cleaning up...");
    if (jobId) {
      try {
        await client.send(new CancelJobCommand({ job_id: jobId }));
      } catch {
        // ignore
      }
    }
    try {
      await client.send(
        new DeleteEndpointCommand({ name: endpointName }),
      );
    } catch {
      // ignore
    }
    await kronosDeleteSecret(TEST_SECRET_NAME);
    await deleteLocalStackSecret(TEST_SECRET_NAME);
    log("Done!");
  }
}

main();
