# Kronos for Aarokya — Integration Plan & Contribution Roadmap

## Context

**Aarokya** is a health savings + insurance platform for Namma Yatri drivers. It piggybacks on the existing Namma Yatri autopay mandate (₹25/day subscription, ₹35 max) to enable daily micro-savings (e.g., ₹30/day) into a Transcorp wallet.

**The daily flow:**
1. **Trigger**: Daily, for each enrolled driver, Aarokya must initiate autopay via Namma Yatri
2. **Execute**: Namma Yatri processes the autopay and calls back Aarokya with success/failure
3. **Load wallet**: On success, Aarokya calls Transcorp to credit the driver's wallet

**The question:** Can Kronos power this? What's missing?

---

## 1. Recommended Architecture (Option A — Kronos + Aarokya Backend)

```
┌─────────────────────────────────────────────────────────────┐
│                     DAILY FLOW                              │
│                                                             │
│  Kronos CRON (daily per user)                               │
│    │                                                        │
│    ▼                                                        │
│  HTTP Dispatch → Aarokya Backend /trigger-autopay/{user}    │
│                    │                                        │
│                    ▼                                        │
│                  Aarokya calls Namma Yatri Autopay API      │
│                    │                                        │
│                    ▼ (async, NY calls back later)           │
│                  NY webhook → Aarokya /webhooks/autopay     │
│                    │                                        │
│                    ├─ ON SUCCESS:                            │
│                    │   Aarokya creates IMMEDIATE job        │
│                    │   in Kronos → Transcorp wallet load    │
│                    │   (with retry + idempotency!)          │
│                    │                                        │
│                    └─ ON FAILURE:                            │
│                        Log failure, notify user, retry next │
│                        day                                  │
└─────────────────────────────────────────────────────────────┘
```

### What Kronos handles:
- **Daily scheduling** — CRON job per enrolled user
- **Reliable HTTP delivery** — Retry with backoff if Aarokya backend or Transcorp is down
- **Exactly-once** — Idempotency key `aarokya_{user_id}_{YYYY-MM-DD}` prevents double-charge
- **Observability** — Metrics + logs for every autopay attempt and wallet load

### What Aarokya backend handles:
- **Webhook receiver** — Receives async callback from Namma Yatri
- **Orchestration** — Decides whether to trigger wallet load based on callback result
- **User enrollment** — Creates/cancels Kronos CRON jobs when users opt in/out

---

## 2. Kronos Setup for Aarokya

### Step 1: Create workspace
```
POST /v1/orgs/{juspay_org}/workspaces
{ "name": "aarokya-prod" }
```

### Step 2: Create secrets
```
POST /v1/secrets
{ "name": "ny-api-key", "value": "<namma-yatri-api-key>" }

POST /v1/secrets
{ "name": "transcorp-api-key", "value": "<transcorp-api-key>" }
```

### Step 3: Create configs
```
POST /v1/configs
{ "name": "aarokya-config", "values": {
    "aarokya_base_url": "https://aarokya-backend.juspay.in",
    "transcorp_base_url": "https://api.transcorp.in"
}}
```

### Step 4: Create endpoints

**Endpoint A — Trigger autopay (daily CRON target)**
```
POST /v1/endpoints
{
  "name": "trigger-autopay",
  "endpoint_type": "HTTP",
  "config_ref": "aarokya-config",
  "spec": {
    "url": "{{config.aarokya_base_url}}/api/v1/autopay/trigger",
    "method": "POST",
    "headers": {
      "Content-Type": "application/json",
      "Authorization": "Bearer {{secret.ny-api-key}}"
    },
    "body_template": {
      "user_id": "{{input.user_id}}",
      "amount": "{{input.amount}}",
      "mandate_id": "{{input.mandate_id}}",
      "date": "{{input.date}}"
    },
    "timeout_ms": 10000,
    "expected_status_codes": [200, 201, 202]
  },
  "retry_policy": {
    "max_attempts": 3,
    "backoff": "exponential",
    "initial_delay_ms": 2000,
    "max_delay_ms": 30000
  }
}
```

**Endpoint B — Load Transcorp wallet (triggered on autopay success)**
```
POST /v1/endpoints
{
  "name": "load-wallet",
  "endpoint_type": "HTTP",
  "config_ref": "aarokya-config",
  "spec": {
    "url": "{{config.transcorp_base_url}}/api/v1/wallet/credit",
    "method": "POST",
    "headers": {
      "Content-Type": "application/json",
      "Authorization": "Bearer {{secret.transcorp-api-key}}"
    },
    "body_template": {
      "user_id": "{{input.user_id}}",
      "amount": "{{input.amount}}",
      "reference_id": "{{input.reference_id}}",
      "source": "aarokya_autopay"
    },
    "timeout_ms": 10000
  },
  "retry_policy": {
    "max_attempts": 5,
    "backoff": "exponential",
    "initial_delay_ms": 1000,
    "max_delay_ms": 60000
  }
}
```

### Step 5: Create CRON jobs (per enrolled user)
```
POST /v1/jobs
{
  "endpoint": "trigger-autopay",
  "trigger": "CRON",
  "cron_expression": "0 8 * * *",
  "cron_timezone": "Asia/Kolkata",
  "input": {
    "user_id": "driver_12345",
    "amount": 3000,
    "mandate_id": "mandate_abc"
  }
}
```

### Step 6: Aarokya backend webhook handler (on NY callback success)
```
// In Aarokya backend code:
// When Namma Yatri calls POST /webhooks/autopay with success:

POST /v1/jobs   (to Kronos)
{
  "endpoint": "load-wallet",
  "trigger": "IMMEDIATE",
  "idempotency_key": "wallet_load_driver_12345_2026-04-03",
  "input": {
    "user_id": "driver_12345",
    "amount": 3000,
    "reference_id": "ny_txn_xyz789"
  }
}
```

---

## 3. Scale Considerations

| Factor | Estimate | Kronos handles? |
|--------|----------|----------------|
| Enrolled drivers | 10K → 100K+ | One CRON job per user could be heavy at 100K+ |
| Daily executions | 10K-100K (autopay) + 10K-100K (wallet load) | Worker scales horizontally |
| Idempotency | Critical — no double-charging | Built-in |
| Failure rate | NY API may fail 1-5% | Retry with backoff |
| Timing | All fire at same time (8 AM IST) | Thundering herd — needs staggering |

### Thundering herd problem
If 100K CRON jobs all fire at `0 8 * * *`, the worker gets 100K executions at once. Solutions:
- **Stagger CRON times**: `0 8 * * *`, `5 8 * * *`, `10 8 * * *` across user cohorts
- **OR: Bulk trigger pattern** (better): One CRON job that triggers a batch endpoint in Aarokya, which then fans out

---

## 4. Contributions Needed in Kronos

### Must-have for Aarokya (Tier 1 priorities)

| # | Feature | Why | Effort | Key files |
|---|---------|-----|--------|-----------|
| 1 | **Batch job creation** | Enroll 1000+ users at once; `POST /v1/jobs/batch` | 1-2 days | `handlers/jobs.rs`, `db/jobs.rs` |
| 2 | **Bulk CRON (fan-out)** | Single CRON that triggers one batch endpoint instead of N individual CRONs | 2-3 days | `handlers/jobs.rs`, `pipeline.rs` |
| 3 | **Job cancellation by endpoint** | When user opts out, cancel their CRON job by user_id lookup | 1 day | `handlers/jobs.rs`, `db/jobs.rs` — add query by endpoint + input filter |

### Nice-to-have (Tier 2)

| # | Feature | Why | Effort |
|---|---------|-----|--------|
| 4 | **Job chaining** | On success of autopay job → auto-create wallet-load job (eliminates Aarokya backend orchestration) | 1 week |
| 5 | **Webhook receiver** | `POST /v1/webhooks/{id}/callback` — Kronos receives NY callback directly and chains | 1 week |
| 6 | **Execution dashboard filtering** | Filter by user_id in input, date range, status — for ops debugging | 2-3 days |
| 7 | **Metrics by endpoint** | Track success rate per endpoint (trigger-autopay vs load-wallet separately) | 1 day |

### If scale exceeds 100K users (Tier 3)

| # | Feature | Why |
|---|---------|-----|
| 8 | **Batch execution** | Single execution that processes N users instead of N executions |
| 9 | **Execution priorities** | Retries should be higher priority than new daily triggers |
| 10 | **Worker sharding** | Shard by workspace or endpoint to avoid contention |

---

## 5. Is Kronos the Right Tool?

### Verdict: YES, with Option A architecture

Kronos is the right tool because this problem is fundamentally about:
- **Reliable, scheduled HTTP delivery** (Kronos's core strength)
- **Retry with exactly-once guarantees** (critical for financial transactions)
- **Observability** (you need to know which autopays failed and why)

### What Kronos is NOT (and you shouldn't try to make it):
- A workflow orchestrator (use Aarokya backend for the callback choreography)
- A payment gateway (Kronos triggers the calls, doesn't process payments)
- A user database (user enrollment state lives in Aarokya's DB)

### Alternatives considered:
| Tool | Why not |
|------|---------|
| Raw cron + curl | No retry, no idempotency, no observability, no multi-tenant |
| Temporal/Cadence | Overkill for this — heavyweight workflow engine for a 2-step flow |
| AWS Step Functions | Cloud lock-in, doesn't run on-prem, cost at scale |
| Custom queue (Redis/Kafka) | You'd rebuild half of Kronos — scheduling, retry, idempotency |

Kronos gives you all of the above out of the box, and it's already in-house at Juspay.

---

## 6. Implementation Order

```
Phase 1: Get it working (1 week)
  ├── Set up Aarokya workspace in Kronos
  ├── Create endpoints (trigger-autopay, load-wallet)
  ├── Create CRON jobs for pilot users (100 drivers)
  ├── Build webhook handler in Aarokya backend
  └── E2E test with mock NY + Transcorp APIs

Phase 2: Scale contributions (1-2 weeks)
  ├── Contribute batch job creation to Kronos
  ├── Contribute job lookup/cancel by endpoint+input
  ├── Implement bulk CRON or staggered scheduling
  └── Load test with 10K simulated users

Phase 3: Production hardening
  ├── Contribute execution filtering for ops
  ├── Set up Grafana alerts (failure rate > 5%)
  ├── Add per-endpoint metrics
  └── Document runbook for autopay failures
```

---

## Verification

1. `just dev` — Start Kronos locally
2. Create Aarokya workspace + endpoints + test CRON job
3. Run `just test-e2e` to verify Kronos basics
4. Mock Namma Yatri + Transcorp APIs using `mock-server`
5. Verify full flow: CRON fires → autopay triggered → callback received → wallet loaded
6. Check idempotency: re-trigger same day → no duplicate wallet load
7. Check retry: mock NY API failure → Kronos retries with backoff
