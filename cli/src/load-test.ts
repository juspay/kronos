/**
 * Kronos Load Test — Create N jobs of each type and track completion
 *
 * Usage:
 *   npx tsx src/load-test.ts [N] [--no-wait]
 *
 *   N         Number of jobs per type (default: 10)
 *   --no-wait Fire-and-forget mode: create jobs without polling for completion
 *
 * Examples:
 *   npx tsx src/load-test.ts 50
 *   npx tsx src/load-test.ts 100 --no-wait
 *
 * Prerequisites:
 *   - Kronos API, worker, scheduler, mock-server all running (just dev)
 *   - Database migrated with at least one org+workspace
 */

import {
  KronosServiceClient,
  CreateEndpointCommand,
  CreateJobCommand,
  ListJobExecutionsCommand,
  GetExecutionCommand,
  CancelJobCommand,
  DeleteEndpointCommand,
} from "kronos-sdk";

// ─── Config ──────────────────────────────────────────────────────

const KRONOS_URL = process.env.KRONOS_URL ?? "http://localhost:8080";
const MOCK_URL = process.env.MOCK_URL ?? "http://localhost:9999";
const API_KEY = process.env.KRONOS_API_KEY ?? "dev-api-key";
const ORG_ID = process.env.KRONOS_ORG_ID ?? "e336afd5-c291-477a-80f4-2cc1c9a072f6";
const WORKSPACE_ID = process.env.KRONOS_WORKSPACE_ID ?? "60f0a1ef-0e07-4fb6-a590-d41ae1068785";
const CONCURRENCY = parseInt(process.env.LOAD_TEST_CONCURRENCY ?? "10", 10);
const POLL_INTERVAL_MS = 500;
const POLL_TIMEOUT_MS = 120_000;

const TERMINAL = new Set(["SUCCESS", "FAILED", "CANCELLED"]);

// ─── Args ────────────────────────────────────────────────────────

const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const flags = new Set(process.argv.slice(2).filter((a) => a.startsWith("--")));
const N = parseInt(args[0] ?? "10", 10);
const NO_WAIT = flags.has("--no-wait");

// ─── Helpers ─────────────────────────────────────────────────────

function ts(): string {
  return new Date().toISOString().slice(11, 23);
}

function log(msg: string) {
  console.log(`[${ts()}] ${msg}`);
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

interface JobRecord {
  type: "IMMEDIATE" | "DELAYED" | "CRON";
  jobId: string;
  createdAt: number;
  completedAt?: number;
  status?: string;
}

// Run promises in batches of `concurrency`
async function pooled<T>(
  tasks: (() => Promise<T>)[],
  concurrency: number,
): Promise<T[]> {
  const results: T[] = [];
  let idx = 0;

  async function next(): Promise<void> {
    while (idx < tasks.length) {
      const i = idx++;
      results[i] = await tasks[i]();
    }
  }

  const workers = Array.from({ length: Math.min(concurrency, tasks.length) }, () => next());
  await Promise.all(workers);
  return results;
}

// ─── Main ────────────────────────────────────────────────────────

async function main() {
  const client = new KronosServiceClient({
    endpoint: KRONOS_URL,
    token: { token: API_KEY },
  });

  const tenant = { org_id: ORG_ID, workspace_id: WORKSPACE_ID };
  const runId = Date.now().toString(36);
  const epImmediate = `load-imm-${runId}`;
  const epDelayed = `load-del-${runId}`;
  const epCron = `load-cron-${runId}`;

  console.log("═".repeat(60));
  console.log("  KRONOS LOAD TEST");
  console.log("═".repeat(60));
  log(`Jobs per type:  ${N}`);
  log(`Total jobs:     ${N * 3}`);
  log(`Concurrency:    ${CONCURRENCY}`);
  log(`Wait for exec:  ${!NO_WAIT}`);
  log(`Run ID:         ${runId}`);
  console.log("═".repeat(60));

  // ── Step 1: Create endpoints ─────────────────────────────────
  log("Creating endpoints...");

  const endpointSpec = (name: string) =>
    new CreateEndpointCommand({
      ...tenant,
      name,
      endpoint_type: "HTTP",
      spec: {
        method: "POST",
        url: `${MOCK_URL}/success`,
        headers: { "Content-Type": "application/json" },
      },
      retry_policy: {
        max_attempts: 3,
        backoff: "exponential",
        initial_delay_ms: 500,
        max_delay_ms: 5000,
      },
    });

  await Promise.all([
    client.send(endpointSpec(epImmediate)),
    client.send(endpointSpec(epDelayed)),
    client.send(endpointSpec(epCron)),
  ]);
  log(`Endpoints created: ${epImmediate}, ${epDelayed}, ${epCron}`);

  // ── Step 2: Create jobs ──────────────────────────────────────
  const jobs: JobRecord[] = [];
  const cronJobIds: string[] = [];

  // IMMEDIATE jobs
  log(`Creating ${N} IMMEDIATE jobs...`);
  const immStart = Date.now();

  const immTasks = Array.from({ length: N }, (_, i) => async () => {
    const resp = await client.send(
      new CreateJobCommand({
        ...tenant,
        endpoint: epImmediate,
        trigger: "IMMEDIATE",
        input: {
          type: "load-test",
          run_id: runId,
          seq: i,
          trigger: "IMMEDIATE",
          ts: new Date().toISOString(),
        },
      }),
    );
    const rec: JobRecord = {
      type: "IMMEDIATE",
      jobId: resp.data!.job_id!,
      createdAt: Date.now(),
    };
    jobs.push(rec);
    return rec;
  });
  await pooled(immTasks, CONCURRENCY);

  const immElapsed = Date.now() - immStart;
  log(`  ${N} IMMEDIATE jobs created in ${immElapsed}ms (${(N / (immElapsed / 1000)).toFixed(1)} jobs/s)`);

  // DELAYED jobs — stagger run_at 1-10s in the future
  log(`Creating ${N} DELAYED jobs...`);
  const delStart = Date.now();

  const delTasks = Array.from({ length: N }, (_, i) => async () => {
    const delaySec = 1 + (i % 10);
    const runAt = new Date(Date.now() + delaySec * 1000);
    const resp = await client.send(
      new CreateJobCommand({
        ...tenant,
        endpoint: epDelayed,
        trigger: "DELAYED",
        run_at: runAt,
        idempotency_key: `load-del-${runId}-${i}`,
        input: {
          type: "load-test",
          run_id: runId,
          seq: i,
          trigger: "DELAYED",
          delay_sec: delaySec,
          ts: new Date().toISOString(),
        },
      }),
    );
    const rec: JobRecord = {
      type: "DELAYED",
      jobId: resp.data!.job_id!,
      createdAt: Date.now(),
    };
    jobs.push(rec);
    return rec;
  });
  await pooled(delTasks, CONCURRENCY);

  const delElapsed = Date.now() - delStart;
  log(`  ${N} DELAYED jobs created in ${delElapsed}ms (${(N / (delElapsed / 1000)).toFixed(1)} jobs/s)`);

  // CRON jobs — every 10s, auto-cancel after one execution
  log(`Creating ${N} CRON jobs...`);
  const cronStart = Date.now();

  const cronTasks = Array.from({ length: N }, (_, i) => async () => {
    const resp = await client.send(
      new CreateJobCommand({
        ...tenant,
        endpoint: epCron,
        trigger: "CRON",
        cron: "0/10 * * * * *",
        timezone: "UTC",
        ends_at: new Date(Date.now() + 30_000),
        input: {
          type: "load-test",
          run_id: runId,
          seq: i,
          trigger: "CRON",
          ts: new Date().toISOString(),
        },
      }),
    );
    const rec: JobRecord = {
      type: "CRON",
      jobId: resp.data!.job_id!,
      createdAt: Date.now(),
    };
    jobs.push(rec);
    cronJobIds.push(rec.jobId);
    return rec;
  });
  await pooled(cronTasks, CONCURRENCY);

  const cronElapsed = Date.now() - cronStart;
  log(`  ${N} CRON jobs created in ${cronElapsed}ms (${(N / (cronElapsed / 1000)).toFixed(1)} jobs/s)`);

  const totalCreateMs = Date.now() - immStart;
  log(`\nAll ${N * 3} jobs created in ${totalCreateMs}ms`);

  // ── Step 3: Poll for completion ──────────────────────────────
  if (NO_WAIT) {
    log("--no-wait: skipping execution polling");
  } else {
    log("\nPolling for execution completion...\n");
    const pollStart = Date.now();
    const pending = new Map<string, JobRecord>();
    for (const j of jobs) pending.set(j.jobId, j);

    let succeeded = 0;
    let failed = 0;
    let timedOut = 0;
    let lastLog = 0;

    while (pending.size > 0 && Date.now() - pollStart < POLL_TIMEOUT_MS) {
      // Poll in batches
      const batch = Array.from(pending.values()).slice(0, CONCURRENCY * 2);

      const checks = batch.map(async (j) => {
        try {
          const resp = await client.send(
            new ListJobExecutionsCommand({ ...tenant, job_id: j.jobId }),
          );
          const execs = resp.data ?? [];
          if (execs.length > 0) {
            const exec = execs[0];
            if (TERMINAL.has(exec.status!)) {
              j.status = exec.status!;
              j.completedAt = Date.now();
              pending.delete(j.jobId);
              if (exec.status === "SUCCESS") succeeded++;
              else failed++;
            }
          }
        } catch {
          // transient error, retry next round
        }
      });

      await Promise.all(checks);

      // Progress log every 2s
      const now = Date.now();
      if (now - lastLog > 2000) {
        const elapsed = ((now - pollStart) / 1000).toFixed(1);
        log(
          `  [${elapsed}s] completed: ${succeeded + failed}/${N * 3} ` +
            `(${succeeded} ok, ${failed} fail, ${pending.size} pending)`,
        );
        lastLog = now;
      }

      if (pending.size > 0) await sleep(POLL_INTERVAL_MS);
    }

    timedOut = pending.size;
    for (const j of pending.values()) {
      j.status = "TIMEOUT";
    }

    const totalPollMs = Date.now() - pollStart;

    // ── Step 4: Print results ────────────────────────────────
    console.log("\n" + "═".repeat(60));
    console.log("  LOAD TEST RESULTS");
    console.log("═".repeat(60));

    const byType = (type: string) => jobs.filter((j) => j.type === type);

    for (const type of ["IMMEDIATE", "DELAYED", "CRON"] as const) {
      const group = byType(type);
      const ok = group.filter((j) => j.status === "SUCCESS").length;
      const fail = group.filter((j) => j.status === "FAILED").length;
      const to = group.filter((j) => j.status === "TIMEOUT").length;
      const latencies = group
        .filter((j) => j.completedAt)
        .map((j) => j.completedAt! - j.createdAt);
      const avgMs = latencies.length > 0
        ? (latencies.reduce((a, b) => a + b, 0) / latencies.length).toFixed(0)
        : "N/A";
      const p95Ms = latencies.length > 0
        ? latencies.sort((a, b) => a - b)[Math.floor(latencies.length * 0.95)].toFixed(0)
        : "N/A";
      const maxMs = latencies.length > 0 ? Math.max(...latencies).toFixed(0) : "N/A";

      console.log(`\n  ${type} (${N} jobs):`);
      console.log(`    Success:   ${ok}`);
      console.log(`    Failed:    ${fail}`);
      console.log(`    Timed out: ${to}`);
      console.log(`    Avg e2e:   ${avgMs}ms`);
      console.log(`    p95 e2e:   ${p95Ms}ms`);
      console.log(`    Max e2e:   ${maxMs}ms`);
    }

    console.log(`\n  TOTALS:`);
    console.log(`    Created:   ${N * 3} jobs in ${totalCreateMs}ms`);
    console.log(`    Succeeded: ${succeeded}`);
    console.log(`    Failed:    ${failed}`);
    console.log(`    Timed out: ${timedOut}`);
    console.log(`    Poll time: ${totalPollMs}ms`);
    console.log(`    Throughput: ${((succeeded + failed) / (totalPollMs / 1000)).toFixed(1)} exec/s`);
    console.log("═".repeat(60));
  }

  // ── Cleanup ──────────────────────────────────────────────────
  log("\nCleaning up...");

  // Cancel remaining cron jobs
  const cancelTasks = cronJobIds.map((id) => async () => {
    try {
      await client.send(new CancelJobCommand({ ...tenant, job_id: id }));
    } catch {
      // already cancelled or retired
    }
  });
  await pooled(cancelTasks, CONCURRENCY);

  // Delete endpoints
  for (const ep of [epImmediate, epDelayed, epCron]) {
    try {
      await client.send(new DeleteEndpointCommand({ ...tenant, name: ep }));
    } catch {
      // may have active references
    }
  }

  log("Done!");

  // Exit code: 0 if all succeeded, 1 if any failures/timeouts
  const allOk = jobs.every((j) => j.status === "SUCCESS" || j.status === undefined);
  process.exit(allOk ? 0 : 1);
}

main().catch((err) => {
  console.error("\nLoad test crashed:");
  console.error(`  ${err.name}: ${err.message}`);
  if (err.stack) console.error(err.stack);
  process.exit(1);
});
