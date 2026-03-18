/**
 * Kronos CLI — Test Immediate Job Execution
 *
 * This script:
 * 1. Creates an endpoint pointing to the mock-server's /success route
 * 2. Creates an IMMEDIATE job targeting that endpoint
 * 3. Polls the job's execution until it reaches a terminal state (SUCCESS/FAILED)
 * 4. Prints the execution result and attempts
 *
 * Prerequisites:
 *   - Kronos API running at KRONOS_URL (default: http://localhost:8080)
 *   - Mock server running at MOCK_URL (default: http://localhost:9999)
 *   - CockroachDB + migrations applied
 *   - Kronos worker running
 */

import {
  KronosServiceClient,
  CreateEndpointCommand,
  CreateJobCommand,
  CancelJobCommand,
  GetJobStatusCommand,
  ListJobExecutionsCommand,
  GetExecutionCommand,
  ListExecutionAttemptsCommand,
  DeleteEndpointCommand,
} from "kronos-sdk";

// ─── Config ──────────────────────────────────────────────────────

const KRONOS_URL = process.env.KRONOS_URL ?? "http://localhost:8080";
const MOCK_URL = process.env.MOCK_URL ?? "http://localhost:9999";
const API_KEY = process.env.KRONOS_API_KEY ?? "dev-api-key";
const POLL_INTERVAL_MS = 500;
const POLL_TIMEOUT_MS = 30_000;

// ─── Helpers ─────────────────────────────────────────────────────

function log(msg: string) {
  const ts = new Date().toISOString().slice(11, 23);
  console.log(`[${ts}] ${msg}`);
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

const TERMINAL_STATUSES = new Set(["SUCCESS", "FAILED", "CANCELLED"]);

// ─── Main ────────────────────────────────────────────────────────

async function main() {
  const client = new KronosServiceClient({
    endpoint: KRONOS_URL,
    token: { token: API_KEY },
  });

  const endpointName = `test-endpoint-${Date.now()}`;
  const testPayload = {
    message: "Hello from Kronos CLI test",
    timestamp: new Date().toISOString(),
    test_id: Math.random().toString(36).slice(2),
  };

  try {
    // ── Step 1: Create an endpoint pointing to mock-server ─────
    log(`Creating endpoint "${endpointName}" → ${MOCK_URL}/success`);

    const endpointResp = await client.send(
      new CreateEndpointCommand({
        name: endpointName,
        endpoint_type: "HTTP",
        spec: {
          method: "POST",
          url: `${MOCK_URL}/success`,
          headers: {
            "Content-Type": "application/json",
            "X-Test-Source": "kronos-cli",
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

    log(`Endpoint created: ${endpointResp.data?.name}`);

    // ── Step 2: Create an IMMEDIATE job ────────────────────────
    log("Creating IMMEDIATE job...");

    const jobResp = await client.send(
      new CreateJobCommand({
        endpoint: endpointName,
        trigger: "IMMEDIATE",
        input: testPayload,
      }),
    );

    const jobId = jobResp.data!.job_id!;
    log(`Job created: ${jobId} (status: ${jobResp.data?.status})`);

    // ── Step 3: Poll for execution completion ──────────────────
    log("Polling for execution completion...");

    const startTime = Date.now();
    let finalExecution: any = null;

    while (Date.now() - startTime < POLL_TIMEOUT_MS) {
      // List executions for this job
      const execsResp = await client.send(
        new ListJobExecutionsCommand({ job_id: jobId }),
      );

      const executions = execsResp.data ?? [];

      if (executions.length > 0) {
        const exec = executions[0];
        const status = exec.status;

        if (TERMINAL_STATUSES.has(status!)) {
          // Fetch full execution details
          finalExecution = (
            await client.send(
              new GetExecutionCommand({
                execution_id: exec.execution_id!,
              }),
            )
          ).data;

          log(`Execution reached terminal state: ${status}`);
          break;
        }

        log(
          `  Execution ${exec.execution_id?.slice(0, 8)}... status: ${status}, attempts: ${exec.attempt_count}/${exec.max_attempts}`,
        );
      } else {
        log("  No executions yet, waiting...");
      }

      await sleep(POLL_INTERVAL_MS);
    }

    if (!finalExecution) {
      log("ERROR: Timed out waiting for execution to complete");
      process.exit(1);
    }

    // ── Step 4: Print results ──────────────────────────────────
    console.log("\n" + "═".repeat(60));
    console.log("  EXECUTION RESULT");
    console.log("═".repeat(60));
    console.log(`  Job ID:        ${jobId}`);
    console.log(`  Execution ID:  ${finalExecution.execution_id}`);
    console.log(`  Status:        ${finalExecution.status}`);
    console.log(`  Endpoint:      ${finalExecution.endpoint}`);
    console.log(
      `  Attempts:      ${finalExecution.attempt_count}/${finalExecution.max_attempts}`,
    );
    console.log(`  Duration:      ${finalExecution.duration_ms ?? "N/A"}ms`);
    console.log(`  Started:       ${finalExecution.started_at}`);
    console.log(`  Completed:     ${finalExecution.completed_at}`);

    if (finalExecution.output) {
      console.log(
        `  Output:        ${JSON.stringify(finalExecution.output, null, 2)}`,
      );
    }
    console.log("═".repeat(60));

    // ── Step 5: Fetch and print attempts ───────────────────────
    const attemptsResp = await client.send(
      new ListExecutionAttemptsCommand({
        execution_id: finalExecution.execution_id!,
      }),
    );

    const attempts = attemptsResp.data ?? [];
    if (attempts.length > 0) {
      console.log(`\n  ATTEMPTS (${attempts.length}):`);
      for (const attempt of attempts) {
        console.log(
          `    #${attempt.attempt_number} — ${attempt.status} (${attempt.duration_ms ?? "?"}ms)`,
        );
        if (attempt.error) {
          console.log(`      Error: ${JSON.stringify(attempt.error)}`);
        }
      }
    }

    // ── Step 6: Check job status ───────────────────────────────
    const statusResp = await client.send(
      new GetJobStatusCommand({ job_id: jobId }),
    );

    console.log(`\n  Job Status: ${statusResp.data?.job_status}`);
    if (statusResp.data?.latest_execution) {
      console.log(
        `  Latest Execution: ${statusResp.data.latest_execution.status}`,
      );
    }
    console.log("═".repeat(60) + "\n");

    // ── Cleanup ────────────────────────────────────────────────
    log("Cleaning up...");
    try {
      await client.send(new CancelJobCommand({ job_id: jobId }));
    } catch {
      // Job may already be in a non-cancellable state
    }
    try {
      await client.send(new DeleteEndpointCommand({ name: endpointName }));
    } catch {
      // Endpoint may still have references
    }
    log("Done!");

    process.exit(finalExecution.status === "SUCCESS" ? 0 : 1);
  } catch (err: any) {
    console.error("\nTest failed with error:");
    console.error(`  ${err.name}: ${err.message}`);
    if (err.$metadata) {
      console.error(`  HTTP ${err.$metadata.httpStatusCode}`);
    }

    process.exit(1);
  }
}

main();
