/**
 * Kronos CLI — Test CRON Job Execution
 *
 * This script:
 * 1. Creates an endpoint pointing to the mock-server's /success route
 * 2. Creates a CRON job that fires every 10 seconds
 * 3. Waits for 2 executions to complete (verifying recurring behavior)
 * 4. Updates the CRON job (new schedule + input) and verifies versioning
 * 5. Cancels the job and verifies it stops
 *
 * Prerequisites:
 *   - Kronos API running at KRONOS_URL (default: http://localhost:8080)
 *   - Mock server running at MOCK_URL (default: http://localhost:9999)
 *   - Kronos worker running
 *   - Kronos scheduler running (cron_materializer)
 */

import {
  CreateJobCommand,
  GetJobCommand,
  GetJobStatusCommand,
  UpdateJobCommand,
  CancelJobCommand,
  GetJobVersionsCommand,
  ListJobExecutionsCommand,
  GetExecutionCommand,
  ListExecutionAttemptsCommand,
} from "kronos-sdk";

import {
  log,
  sleep,
  createClient,
  createTestEndpoint,
  cleanup,
  POLL_INTERVAL_MS,
  TERMINAL_STATUSES,
} from "./helpers.js";

const CRON_EVERY_10S = "0/10 * * * * *"; // every 10 seconds
const CRON_EVERY_15S = "0/15 * * * * *"; // every 15 seconds (for update test)

async function waitForExecutions(
  client: ReturnType<typeof createClient>,
  jobId: string,
  targetCount: number,
  timeoutMs: number,
): Promise<any[]> {
  const startTime = Date.now();
  const completed: Map<string, any> = new Map();

  while (Date.now() - startTime < timeoutMs) {
    const execsResp = await client.send(
      new ListJobExecutionsCommand({ job_id: jobId }),
    );

    const executions = execsResp.data ?? [];

    for (const exec of executions) {
      if (TERMINAL_STATUSES.has(exec.status!) && !completed.has(exec.execution_id!)) {
        const full = await client.send(
          new GetExecutionCommand({ execution_id: exec.execution_id! }),
        );
        completed.set(exec.execution_id!, full.data);
        log(`  Execution ${exec.execution_id?.slice(0, 8)}... completed: ${exec.status} (${completed.size}/${targetCount})`);
      }
    }

    if (completed.size >= targetCount) {
      return Array.from(completed.values());
    }

    const pending = executions.filter((e: any) => !TERMINAL_STATUSES.has(e.status!));
    if (pending.length > 0) {
      log(`  ${pending.length} execution(s) in progress, ${completed.size} completed, waiting for ${targetCount}...`);
    }

    await sleep(POLL_INTERVAL_MS);
  }

  return Array.from(completed.values());
}

async function main() {
  const client = createClient();
  const endpointName = `test-cron-${Date.now()}`;
  let jobId: string | null = null;

  try {
    // ── Step 1: Create endpoint ──────────────────────────────
    log(`Creating endpoint "${endpointName}"`);
    await createTestEndpoint(client, endpointName);
    log(`Endpoint created: ${endpointName}`);

    // ── Step 2: Create CRON job (every 10s) ──────────────────
    log(`Creating CRON job with schedule: ${CRON_EVERY_10S}`);

    const jobResp = await client.send(
      new CreateJobCommand({
        endpoint: endpointName,
        trigger: "CRON",
        cron: CRON_EVERY_10S,
        timezone: "UTC",
        input: {
          message: "Hello from Kronos CRON test",
          iteration: "v1",
        },
      }),
    );

    jobId = jobResp.data!.job_id!;
    log(`Job created: ${jobId}`);
    log(`  Status:      ${jobResp.data?.status}`);
    log(`  Next run at: ${jobResp.data?.cron_next_run_at}`);
    log(`  Version:     ${jobResp.data?.version}`);

    // ── Step 3: Wait for 2 executions ────────────────────────
    log("Waiting for 2 CRON executions to complete...");

    const executions = await waitForExecutions(client, jobId, 2, 60_000);

    if (executions.length < 2) {
      log(`ERROR: Only ${executions.length}/2 executions completed within timeout`);
      process.exit(1);
    }

    log(`\n  ${executions.length} executions completed successfully!`);

    // Print execution summaries
    console.log("\n" + "═".repeat(60));
    console.log("  CRON EXECUTIONS");
    console.log("═".repeat(60));
    for (const exec of executions) {
      console.log(`  Execution: ${exec.execution_id}`);
      console.log(`    Status:    ${exec.status}`);
      console.log(`    Attempts:  ${exec.attempt_count}/${exec.max_attempts}`);
      console.log(`    Duration:  ${exec.duration_ms ?? "N/A"}ms`);
      console.log(`    Started:   ${exec.started_at}`);
      console.log(`    Completed: ${exec.completed_at}`);

      // Fetch attempts for each execution
      const attemptsResp = await client.send(
        new ListExecutionAttemptsCommand({
          execution_id: exec.execution_id!,
        }),
      );
      const attempts = attemptsResp.data ?? [];
      if (attempts.length > 0) {
        for (const attempt of attempts) {
          console.log(
            `    Attempt #${attempt.attempt_number} — ${attempt.status} (${attempt.duration_ms ?? "?"}ms)`,
          );
        }
      }
      console.log("");
    }
    console.log("═".repeat(60));

    // ── Step 4: Update the CRON job ──────────────────────────
    log(`Updating CRON job to new schedule: ${CRON_EVERY_15S}`);

    const updateResp = await client.send(
      new UpdateJobCommand({
        job_id: jobId,
        cron: CRON_EVERY_15S,
        input: {
          message: "Updated CRON payload",
          iteration: "v2",
        },
      }),
    );

    const newJobId = updateResp.data!.job_id!;
    log(`Job updated — new version created`);
    log(`  New Job ID:          ${newJobId}`);
    log(`  Version:             ${updateResp.data?.version}`);
    log(`  Previous Version ID: ${updateResp.data?.previous_version_id}`);
    log(`  New schedule:        ${updateResp.data?.cron_expression}`);
    log(`  Next run at:         ${updateResp.data?.cron_next_run_at}`);

    // ── Step 5: Verify versioning ────────────────────────────
    log("Fetching version history...");

    const versionsResp = await client.send(
      new GetJobVersionsCommand({ job_id: newJobId }),
    );
    const versions = versionsResp.data ?? [];
    console.log(`\n  VERSION HISTORY (${versions.length} versions):`);
    for (const v of versions) {
      console.log(`    v${v.version}: ${v.job_id?.slice(0, 8)}... — ${v.status} (cron: ${v.cron_expression ?? "N/A"})`);
    }

    // Verify original job is RETIRED
    const oldJobResp = await client.send(
      new GetJobCommand({ job_id: jobId }),
    );
    log(`Original job status: ${oldJobResp.data?.status} (expected: RETIRED)`);

    if (oldJobResp.data?.status !== "RETIRED") {
      log(`WARNING: Expected original job to be RETIRED, got ${oldJobResp.data?.status}`);
    }

    // ── Step 6: Wait for 1 execution on the new version ──────
    log("Waiting for 1 execution on updated job...");

    const newExecs = await waitForExecutions(client, newJobId, 1, 30_000);
    if (newExecs.length > 0) {
      log(`Updated job executed successfully: ${newExecs[0].status}`);
    } else {
      log("WARNING: No execution on updated job within timeout");
    }

    // ── Step 7: Cancel the job ───────────────────────────────
    log("Cancelling CRON job...");

    const cancelResp = await client.send(
      new CancelJobCommand({ job_id: newJobId }),
    );
    log(`Job cancelled: ${cancelResp.data?.status}`);

    // Verify status
    const statusResp = await client.send(
      new GetJobStatusCommand({ job_id: newJobId }),
    );
    log(`Final job status: ${statusResp.data?.job_status}`);

    if (statusResp.data?.job_status !== "CANCELLED") {
      log(`WARNING: Expected CANCELLED, got ${statusResp.data?.job_status}`);
    }

    // ── Cleanup ──────────────────────────────────────────────
    await cleanup(client, null, endpointName);
    log("Done!");

    const allSucceeded = executions.every((e: any) => e.status === "SUCCESS");
    process.exit(allSucceeded ? 0 : 1);
  } catch (err: any) {
    console.error("\nTest failed with error:");
    console.error(`  ${err.name}: ${err.message}`);
    if (err.$metadata) {
      console.error(`  HTTP ${err.$metadata.httpStatusCode}`);
    }

    if (jobId) {
      try {
        await client.send(new CancelJobCommand({ job_id: jobId }));
      } catch { /* ignore */ }
    }
    await cleanup(client, null, endpointName);
    process.exit(1);
  }
}

main();
