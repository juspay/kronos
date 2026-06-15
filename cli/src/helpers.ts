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
export const ORG_ID = process.env.KRONOS_ORG_ID!;
export const WORKSPACE_ID = process.env.KRONOS_WORKSPACE_ID!;
export const tenant = { org_id: ORG_ID, workspace_id: WORKSPACE_ID };
export const POLL_INTERVAL_MS = 500;
export const POLL_TIMEOUT_MS = 30_000;
export const TERMINAL_STATUSES = new Set(["SUCCESS", "FAILED", "CANCELLED"]);

// ─── Auth ─────────────────────────────────────────────────────────
//
// Priority:
//   KRONOS_CLIENT_ID + KRONOS_CLIENT_SECRET  →  HTTP Basic
//   KRONOS_BEARER_TOKEN                      →  HTTP Bearer
//   (neither)                                →  no Authorization header (TE_AUTH_MODE=disabled)
//
// KRONOS_CLIENT_ID/SECRET and KRONOS_BEARER_TOKEN are mutually exclusive.
//
// SDK limitation: smithy-typescript 0.26.0 does not emit a first-class
// `basic` config field even though @httpBasicAuth is declared in the model.
// We work around this by supplying a custom `httpAuthSchemes` array that
// includes a hand-rolled Basic signer registered under the scheme ID
// "smithy.api#httpBasicAuth".  The @smithy/types and @smithy/core packages
// are not direct CLI deps, so we type the scheme entries as `any` and
// reproduce the minimal Bearer signer inline rather than importing it.

/** Hand-rolled Basic signer: sets Authorization: Basic <base64(user:pass)>. */
const makeBasicScheme = (username: string, password: string): any => ({
  schemeId: "smithy.api#httpBasicAuth",
  identityProvider: () => async () => ({ username, password }),
  signer: {
    async sign(httpRequest: any): Promise<any> {
      const encoded = Buffer.from(`${username}:${password}`).toString("base64");
      httpRequest.headers["authorization"] = `Basic ${encoded}`;
      return httpRequest;
    },
  },
});

/** Minimal Bearer signer that mirrors HttpBearerAuthSigner from @smithy/core. */
const bearerScheme = (ipc: any): any => ({
  schemeId: "smithy.api#httpBearerAuth",
  identityProvider: (cfg: any) => cfg.getIdentityProvider("smithy.api#httpBearerAuth"),
  signer: {
    async sign(httpRequest: any, identity: any): Promise<any> {
      httpRequest.headers["authorization"] = `Bearer ${identity.token}`;
      return httpRequest;
    },
  },
});

/**
 * Returns the auth fragment to spread into KronosServiceClient config.
 *
 * - KRONOS_CLIENT_ID + KRONOS_CLIENT_SECRET  →  Basic via custom httpAuthSchemes
 * - KRONOS_BEARER_TOKEN                      →  Bearer via the standard `token` field
 * - Both set simultaneously                  →  throws (mutually exclusive)
 * - Neither set                              →  empty object (no Authorization header)
 */
function authFromEnv(): Record<string, unknown> {
  const clientId = process.env.KRONOS_CLIENT_ID;
  const clientSecret = process.env.KRONOS_CLIENT_SECRET;
  const bearer = process.env.KRONOS_BEARER_TOKEN;

  if (clientId && clientSecret) {
    if (bearer) {
      throw new Error(
        "KRONOS_CLIENT_ID/KRONOS_CLIENT_SECRET and KRONOS_BEARER_TOKEN are mutually exclusive — " +
        "unset one auth method before continuing"
      );
    }
    // Override httpAuthSchemes so the Basic scheme is tried first.
    // The default runtimeConfig only registers Bearer; we add Basic here.
    return {
      httpAuthSchemes: [
        makeBasicScheme(clientId, clientSecret),
        // Keep Bearer registered so the scheme list stays complete.
        bearerScheme(null),
      ],
    };
  }

  if (bearer) {
    return { token: { token: bearer } };
  }

  // No credentials → works against TE_AUTH_MODE=disabled (no header sent).
  return {};
}

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
    ...authFromEnv(),
  } as any);
}

export async function createTestEndpoint(
  client: KronosServiceClient,
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
  client: KronosServiceClient,
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
