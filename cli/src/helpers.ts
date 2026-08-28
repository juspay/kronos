import {
  InvokrServiceClient,
  CreateEndpointCommand,
  CancelJobCommand,
  ListJobExecutionsCommand,
  GetExecutionCommand,
  ListExecutionAttemptsCommand,
  GetJobStatusCommand,
  DeleteEndpointCommand,
} from "invokr-sdk";

// ─── Config ──────────────────────────────────────────────────────

export const INVOKR_URL = process.env.INVOKR_URL ?? "http://localhost:8080";
export const MOCK_URL = process.env.MOCK_URL ?? "http://localhost:9999";
export const API_KEY = process.env.INVOKR_API_KEY ?? "dev-api-key";
export const ORG_ID = process.env.INVOKR_ORG_ID!;
export const WORKSPACE_ID = process.env.INVOKR_WORKSPACE_ID!;
export const tenant = { org_id: ORG_ID, workspace_id: WORKSPACE_ID };
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

export function createClient(): InvokrServiceClient {
  return new InvokrServiceClient({
    endpoint: INVOKR_URL,
    token: { token: API_KEY },
  });
}

export async function createTestEndpoint(
  client: InvokrServiceClient,
  name: string,
  mockPath: string = "/success",
) {
  const resp = await client.send(
    new CreateEndpointCommand({
      ...tenant,
      name,
      endpoint_type: "HTTP",
      spec: {
        method: "POST",
        url: `${MOCK_URL}${mockPath}`,
        headers: {
          "Content-Type": "application/json",
          "X-Test-Source": "invokr-cli",
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
  client: InvokrServiceClient,
  jobId: string,
  timeoutMs: number = POLL_TIMEOUT_MS,
): Promise<any> {
  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    const execsResp = await client.send(
      new ListJobExecutionsCommand({ ...tenant, job_id: jobId }),
    );

    const executions = execsResp.data ?? [];

    if (executions.length > 0) {
      const exec = executions[0];

      if (TERMINAL_STATUSES.has(exec.status!)) {
        const full = await client.send(
          new GetExecutionCommand({ ...tenant, execution_id: exec.execution_id! }),
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
  client: InvokrServiceClient,
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
      ...tenant,
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
    new GetJobStatusCommand({ ...tenant, job_id: jobId }),
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
  client: InvokrServiceClient,
  jobId: string | null,
  endpointName: string,
) {
  log("Cleaning up...");
  if (jobId) {
    try {
      await client.send(new CancelJobCommand({ ...tenant, job_id: jobId }));
    } catch {
      // ignore
    }
  }
  try {
    await client.send(new DeleteEndpointCommand({ ...tenant, name: endpointName }));
  } catch {
    // ignore
  }
}
