# Task Executor — Health Checker & Regional Failover Design

**Version:** 2.0.0
**Date:** March 15, 2026

---

## Overview

The Task Executor runs in an active-active configuration across `ap-south-1` and `ap-south-2`. Under normal operation, each region processes only its own jobs. When a region fails, the surviving region adopts the dead region's workload. When the failed region recovers, it resumes ownership.

The health checker detects failure, triggers adoption, and manages recovery.

---

## Problem Statement

**Distribute N jobs among M workers across 2 regions such that:**

1. Under normal operation, a job is only processed by workers in its home region.
2. When a region fails, the surviving region picks up orphaned jobs.
3. When a region recovers, ownership is restored without double processing.
4. Split-brain (both regions think the other is dead) must not cause duplicate execution.

---

## Failure Modes

| Failure | Impact | Detection |
|---------|--------|-----------|
| **Full region down** | All components unreachable. Jobs pile up as QUEUED. | All heartbeats stale. |
| **Partial failure** | API down but workers running, or vice versa. | Per-component heartbeats show mixed state. |
| **Network partition** | Both regions healthy internally, can't reach each other. | Both see the other as stale. Split-brain risk. |
| **Temporary blip** | 2-5 second network hiccup. | Brief heartbeat staleness. Must not trigger failover. |
| **CockroachDB node loss** | Individual CRDB nodes fail within a region. | CRDB handles via Raft. Not a health checker concern unless quorum is lost. |
| **Transport failure** | Kafka brokers or Redis down, but workers/API alive. | Executions fail but heartbeats are healthy. Not a regional failover — retry handles this. |

---

## Architecture

```
┌─────────────────────────────────────────┐
│            Each Region Runs:            │
│                                         │
│  ┌─────────────────┐                    │
│  │ Heartbeat Writer │ (per component)   │
│  │  - API server    │                   │
│  │  - Worker pool   │                   │
│  │  - Scheduler     │                   │
│  └────────┬────────┘                    │
│           │ UPSERT every 5s             │
│           ▼                             │
│  ┌─────────────────┐                    │
│  │ CockroachDB      │                   │
│  │ region_heartbeats│ (GLOBAL table)    │
│  │ region_status    │ (GLOBAL table)    │
│  └────────┬────────┘                    │
│           │ READ every 10s              │
│           ▼                             │
│  ┌─────────────────┐                    │
│  │ Health Evaluator  │ (1 per region)   │
│  │  - Reads OTHER    │                  │
│  │    region's beats │                  │
│  │  - Decides alive  │                  │
│  │    or dead        │                  │
│  │  - Writes region_ │                  │
│  │    status         │                  │
│  └─────────────────┘                    │
└─────────────────────────────────────────┘
```

---

## Component 1: Heartbeat Writer

Each application component writes a heartbeat every 5 seconds.

### Table

```sql
CREATE TABLE region_heartbeats (
    region        STRING      NOT NULL,
    component     STRING      NOT NULL,       -- 'api', 'worker', 'scheduler'
    last_beat_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    status        STRING      NOT NULL DEFAULT 'ALIVE',
    metadata      JSONB,

    CONSTRAINT pk_region_heartbeats PRIMARY KEY (region, component)
);

ALTER TABLE region_heartbeats SET LOCALITY GLOBAL;
```

### Behavior

```
every 5 seconds:
    UPSERT INTO region_heartbeats (region, component, last_beat_at, status, metadata)
    VALUES (
        <my_region>,
        <my_component>,
        now(),
        'ALIVE',
        { ... }
    );
```

### Metadata by component

| Component | Metadata fields |
|-----------|----------------|
| `api` | `instance_count`, `requests_per_second`, `error_rate` |
| `worker` | `active_workers`, `queue_depth`, `jobs_processed_1m`, `avg_latency_ms`, `transports` |
| `scheduler` | `cron_jobs_managed`, `ticks_materialized_1m`, `delayed_jobs_promoted_1m` |

The worker metadata now includes `transports` — which transport types this worker pool handles (e.g., `["HTTP", "KAFKA"]` or `["HTTP", "KAFKA", "REDIS_STREAM"]`).

---

## Component 2: Health Evaluator

### State Machine

```
                   heartbeats fresh
            ┌──────────────────────────┐
            ▼                          │
        ┌───────┐   beats stale    ┌───────┐   3 consecutive    ┌──────┐
        │ ALIVE │ ───────────────→ │SUSPECT│ ───────────────── → │ DEAD │
        └───────┘                  └───────┘   suspect checks    └──────┘
            ▲                          │                             │
            │       beats resume       │                             │
            │ ◄────────────────────────┘                             │
            │                                                        │
            │         3 consecutive healthy checks                   │
            └────────────────────────────────────────────────────────┘
```

### Algorithm

```
Config:
    HEARTBEAT_INTERVAL     = 5 seconds
    EVAL_INTERVAL          = 10 seconds
    STALENESS_THRESHOLD    = 30 seconds
    SUSPECT_CHECKS_NEEDED  = 3
    RECOVERY_CHECKS_NEEDED = 3

State:
    suspect_count = 0
    recovery_count = 0
    current_state = ALIVE

every EVAL_INTERVAL:
    heartbeats = SELECT component, last_beat_at, status
                 FROM region_heartbeats
                 WHERE region = <other_region>

    stale_components = heartbeats.filter(
        hb => now() - hb.last_beat_at > STALENESS_THRESHOLD
    )

    all_stale = (stale_components.count == heartbeats.count)

    SWITCH current_state:

        CASE ALIVE:
            IF all_stale:
                suspect_count += 1
                current_state = SUSPECT
            ELSE:
                suspect_count = 0

        CASE SUSPECT:
            IF all_stale:
                suspect_count += 1
                IF suspect_count >= SUSPECT_CHECKS_NEEDED:
                    current_state = DEAD
                    suspect_count = 0
                    trigger_failover(<other_region>)
            ELSE:
                current_state = ALIVE
                suspect_count = 0

        CASE DEAD:
            IF NOT all_stale:
                recovery_count += 1
                IF recovery_count >= RECOVERY_CHECKS_NEEDED:
                    current_state = ALIVE
                    recovery_count = 0
                    trigger_recovery(<other_region>)
            ELSE:
                recovery_count = 0
```

### Failover Trigger

```
function trigger_failover(dead_region):
    -- GLOBAL table write — requires cross-region quorum
    -- This IS the split-brain protection
    UPDATE region_status
    SET alive = false,
        failed_at = now(),
        adopted_by = <my_region>,
        updated_at = now()
    WHERE region = dead_region
      AND alive = true;

    IF rows_affected > 0:
        log("FAILOVER: {dead_region} declared dead. Adopted by {my_region}.")
        notify_workers("ADOPT", dead_region)
```

### Recovery Trigger

```
function trigger_recovery(recovered_region):
    UPDATE region_status
    SET alive = true,
        failed_at = null,
        adopted_by = null,
        updated_at = now()
    WHERE region = recovered_region
      AND alive = false;

    IF rows_affected > 0:
        log("RECOVERY: {recovered_region} is back. Releasing adoption.")
        notify_workers("RELEASE", recovered_region)
```

---

## Component 3: Worker Region Awareness

Workers cache `region_status` locally and refresh every 10 seconds.

### Normal mode

```sql
SELECT crdb_region, execution_id
FROM executions
WHERE crdb_region = 'ap-south-1'
  AND status IN ('QUEUED', 'RETRYING')
  AND run_at <= now()
ORDER BY run_at ASC
LIMIT 1
FOR UPDATE SKIP LOCKED;
```

### Failover mode

```sql
SELECT crdb_region, execution_id
FROM executions
WHERE crdb_region IN ('ap-south-1', 'ap-south-2')
  AND status IN ('QUEUED', 'RETRYING')
  AND run_at <= now()
ORDER BY run_at ASC
LIMIT 1
FOR UPDATE SKIP LOCKED;
```

### Switching logic

```
every 10 seconds:
    dead_regions = SELECT region FROM region_status WHERE alive = false

on each poll cycle:
    my_regions = [<my_region>] + dead_regions
    -- use in WHERE crdb_region IN (...) clause
```

---

## Component 4: Scheduler Region Awareness

Same pattern as workers. The CRON tick materializer and DELAYED job promoter filter by `crdb_region` and expand the scope during failover.

CRON tick deduplication (`idx_executions_cron_dedup`) prevents double ticks during recovery transitions.

---

## Split-Brain Protection

`region_status` is a `GLOBAL` table. Writes require Raft consensus across regions.

**Even split (3+3 nodes):** neither side achieves quorum during partition. Both block. Safe but both stall.

**Odd split with tie-breaker (3+3+1 nodes):** one side gets 4/7 quorum and can proceed. Recommended.

```
Partition with tie-breaker:

  ap-south-1 (3 nodes)     ✗     ap-south-2 (3 nodes) + tie-breaker (1 node)
       │                              │
       │ 3/7 = no quorum              │ 4/7 = HAS quorum
       │ UPDATE → BLOCKS              │ UPDATE → SUCCEEDS
       │ Cannot adopt                 │ Adopts ap-south-1's jobs
```

---

## Timing Summary

| Event | Time from failure |
|-------|------------------|
| Region goes down | T+0s |
| Staleness threshold reached | T+30s |
| First SUSPECT check | T+30s |
| 3rd SUSPECT check → DEAD | T+60s |
| Workers refresh cache | T+60s to T+70s |
| First orphaned job picked up | T+60s to T+70s |

| Event | Time from recovery |
|-------|--------------------|
| Region comes back | T+0s |
| Fresh heartbeats appear | T+5s |
| 3 healthy checks → ALIVE | T+40s |
| Workers release adopted jobs | T+40s to T+50s |

---

## Configuration Reference

| Parameter | Default | Env var | Description |
|-----------|---------|---------|-------------|
| `HEARTBEAT_INTERVAL` | 5s | `TE_HEARTBEAT_INTERVAL_SEC` | Heartbeat write frequency. |
| `EVAL_INTERVAL` | 10s | `TE_EVAL_INTERVAL_SEC` | Health evaluation frequency. |
| `STALENESS_THRESHOLD` | 30s | `TE_STALENESS_THRESHOLD_SEC` | Stale heartbeat threshold. |
| `SUSPECT_CHECKS_NEEDED` | 3 | `TE_SUSPECT_CHECKS` | Checks before declaring dead. |
| `RECOVERY_CHECKS_NEEDED` | 3 | `TE_RECOVERY_CHECKS` | Checks before declaring recovered. |
| `WORKER_STATUS_REFRESH` | 10s | `TE_WORKER_STATUS_REFRESH_SEC` | Worker region_status cache TTL. |
| `STUCK_EXECUTION_TIMEOUT` | 5m | `TE_STUCK_EXECUTION_TIMEOUT_SEC` | RUNNING timeout before reclaim. |

### Tuning tradeoffs

| Profile | Detection time | Risk |
|---------|---------------|------|
| **Aggressive** (15s stale, 2 checks, 5s eval) | ~25s | False positives from blips |
| **Default** (30s stale, 3 checks, 10s eval) | ~60s | Balanced |
| **Conservative** (60s stale, 5 checks, 10s eval) | ~120s | Very safe, slow failover |

---

## Monitoring & Alerts

| Metric | Alert threshold | Severity |
|--------|----------------|----------|
| Heartbeat write failures | > 3 consecutive | WARN |
| Heartbeat staleness (own region) | > 15s | CRITICAL |
| Region marked SUSPECT | Any | WARN |
| Region marked DEAD | Any | CRITICAL |
| Failover triggered | Any | CRITICAL |
| Recovery triggered | Any | INFO |
| Split-brain (both dead) | Any | EMERGENCY |
| Orphaned jobs (QUEUED in dead region) | > 0 for > 5 min | WARN |
| Adopted jobs count | > 0 | INFO |
