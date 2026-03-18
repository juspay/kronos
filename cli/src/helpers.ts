import {
  KronosServiceClient,
  CreateEndpointCommand,
  CancelJobCommand,
  ListJobExecutionsCommand,
  GetExecutionCommand,
  ListExecutionAttemptsCommand,
  GetJobStatusCommand,
  DeleteEndpointCommand,
} from "kronos-sdk";

// ─── Config ──────────────────────────────────────────────────────

export const KRONOS_URL = process.env.KRONOS_URL ?? "http://localhost:8080";
export const MOCK_URL = process.env.MOCK_URL ?? "http://localhost:9999";
export const API_KEY = process.env.KRONOS_API_KEY ?? "dev-api-key";
export const POLL_INTERVAL_MS = 500;
export const POLL_TIMEOUT_MS = 30_000;
export const TERMINAL_STATUSES = new Set(["SUCCESS", "FAILED", "CANCELLED"]);

// ─── Helpers ─────────────────────────────────────────────────────

export function log(msg: string) {
  const ts = new Date().toISOString().slice(11, 23);
  console.log(`[${ts}] ${msg}`);
}

export function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

export function createClient(): KronosServiceClient {
  return new KronosServiceClient({
    endpoint: KRONOS_URL,
    token: { token: API_KEY },
  });
}

export async function createTestEndpoint(
  client: KronosServiceClient,
  name: string,
  mockPath: string = "/success",
) {
  const resp = await client.send(
    new CreateEndpointCommand({
      name,
      endpoint_type: "HTTP",
      spec: {
        method: "POST",
        url: `${MOCK_URL}${mockPath}`,
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
  return resp.data!;
}

export async function pollExecution(
  client: KronosServiceClient,
  jobId: string,
  timeoutMs: number = POLL_TIMEOUT_MS,
): Promise<any> {
  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    const execsResp = await client.send(
      new ListJobExecutionsCommand({ job_id: jobId }),
    );

    const executions = execsResp.data ?? [];

    if (executions.length > 0) {
      const exec = executions[0];

      if (TERMINAL_STATUSES.has(exec.status!)) {
        const full = await client.send(
          new GetExecutionCommand({ execution_id: exec.execution_id! }),
        );
        log(`Execution reached terminal state: ${exec.status}`);
        return full.data;
      }

      log(
        `  Execution ${exec.execution_id?.slice(0, 8)}... status: ${exec.status}, attempts: ${exec.attempt_count}/${exec.max_attempts}`,
      );
    } else {
      log("  No executions yet, waiting...");
    }

    await sleep(POLL_INTERVAL_MS);
  }

  return null;
}

export async function printExecutionResult(
  client: KronosServiceClient,
  jobId: string,
  execution: any,
) {
  console.log("\n" + "═".repeat(60));
  console.log("  EXECUTION RESULT");
  console.log("═".repeat(60));
  console.log(`  Job ID:        ${jobId}`);
  console.log(`  Execution ID:  ${execution.execution_id}`);
  console.log(`  Status:        ${execution.status}`);
  console.log(`  Endpoint:      ${execution.endpoint}`);
  console.log(
    `  Attempts:      ${execution.attempt_count}/${execution.max_attempts}`,
  );
  console.log(`  Duration:      ${execution.duration_ms ?? "N/A"}ms`);
  console.log(`  Started:       ${execution.started_at}`);
  console.log(`  Completed:     ${execution.completed_at}`);

  if (execution.output) {
    console.log(
      `  Output:        ${JSON.stringify(execution.output, null, 2)}`,
    );
  }
  console.log("═".repeat(60));

  // Attempts
  const attemptsResp = await client.send(
    new ListExecutionAttemptsCommand({
      execution_id: execution.execution_id!,
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

  // Job status
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
}

export async function cleanup(
  client: KronosServiceClient,
  jobId: string | null,
  endpointName: string,
) {
  log("Cleaning up...");
  if (jobId) {
    try {
      await client.send(new CancelJobCommand({ job_id: jobId }));
    } catch {
      // ignore
    }
  }
  try {
    await client.send(new DeleteEndpointCommand({ name: endpointName }));
  } catch {
    // ignore
  }
}
