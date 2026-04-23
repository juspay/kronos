/**
 * Kronos CLI — Test Delayed Job Execution
 *
 * This script:
 * 1. Creates an endpoint pointing to the mock-server's /success route
 * 2. Creates a DELAYED job with run_at set 5 seconds in the future
 * 3. Verifies the job starts in PENDING status
 * 4. Polls until the delayed_promoter promotes it and it executes
 * 5. Prints the execution result and attempts
 *
 * Prerequisites:
 *   - Kronos API running at KRONOS_URL (default: http://localhost:8080)
 *   - Mock server running at MOCK_URL (default: http://localhost:9999)
 *   - Kronos worker running
 *   - Kronos scheduler running (delayed_promoter)
 */

import {
  CreateJobCommand,
  GetJobStatusCommand,
} from "kronos-sdk";

import {
  log,
  sleep,
  createClient,
  createTestEndpoint,
  pollExecution,
  printExecutionResult,
  cleanup,
  tenant,
  POLL_TIMEOUT_MS,
} from "./helpers.js";

async function main() {
  const client = createClient();
  const endpointName = `test-delayed-${Date.now()}`;
  let jobId: string | null = null;

  try {
    // ── Step 1: Create an endpoint pointing to mock-server ─────
    log(`Creating endpoint "${endpointName}"`);
    await createTestEndpoint(client, endpointName);
    log(`Endpoint created: ${endpointName}`);

    // ── Step 2: Create a DELAYED job (runs 5s from now) ────────
    const delaySeconds = 5;
    const runAt = new Date(Date.now() + delaySeconds * 1000);
    log(`Creating DELAYED job, run_at = ${runAt.toISOString()} (${delaySeconds}s from now)`);

    const idempotencyKey = `delayed-test-${Date.now()}`;
    const jobResp = await client.send(
      new CreateJobCommand({
        ...tenant,
        endpoint: endpointName,
        trigger: "DELAYED",
        run_at: runAt,
        idempotency_key: idempotencyKey,
        input: {
          message: "Hello from Kronos delayed job test",
          timestamp: new Date().toISOString(),
          test_id: Math.random().toString(36).slice(2),
        },
      }),
    );

    jobId = jobResp.data!.job_id!;
    log(`Job created: ${jobId} (status: ${jobResp.data?.status})`);

    // ── Step 3: Verify initial state is PENDING ────────────────
    const statusResp = await client.send(
      new GetJobStatusCommand({ ...tenant, job_id: jobId }),
    );
    const initialStatus = statusResp.data?.job_status;
    log(`Initial job status: ${initialStatus}`);

    if (initialStatus !== "PENDING") {
      log(`WARNING: Expected PENDING status, got ${initialStatus}`);
    }

    // ── Step 4: Wait for delayed_promoter to promote & execute ─
    log(`Waiting for delayed_promoter to promote job after ${runAt.toISOString()}...`);

    // Use a longer timeout to account for the delay + promoter interval
    const totalTimeout = (delaySeconds + 30) * 1000;
    const finalExecution = await pollExecution(client, jobId, totalTimeout);

    if (!finalExecution) {
      log("ERROR: Timed out waiting for delayed execution to complete");
      process.exit(1);
    }

    // ── Step 5: Print results ──────────────────────────────────
    await printExecutionResult(client, jobId, finalExecution);

    // ── Cleanup ────────────────────────────────────────────────
    await cleanup(client, jobId, endpointName);
    log("Done!");

    process.exit(finalExecution.status === "SUCCESS" ? 0 : 1);
  } catch (err: any) {
    console.error("\nTest failed with error:");
    console.error(`  ${err.name}: ${err.message}`);
    if (err.$metadata) {
      console.error(`  HTTP ${err.$metadata.httpStatusCode}`);
    }

    await cleanup(client, jobId, endpointName);
    process.exit(1);
  }
}

main();
