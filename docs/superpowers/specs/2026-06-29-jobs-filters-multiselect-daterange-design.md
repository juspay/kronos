# Jobs list: multi-select filters, date-range filter, cancel UX, scroll fix

**Date:** 2026-06-29
**Branch:** `feat/jobs-filters-multiselect-daterange` (stacked on `fix/jobs-list-pagination-filters`)
**Status:** Approved design — pending spec review, then implementation plan.

## Background

PR #35 added single-select filters (status / trigger_type / endpoint_type) and an
endpoint substring search to the dashboard Jobs tab, with cursor pagination.
PR #49 (`fix/jobs-list-pagination-filters`) hardened that work (composite cursor,
LIKE escaping, typed `JobFilters`, Smithy `INTERNAL`). This feature builds on #49.

UX reference (pattern only — it's React/TS, ours is Leptos/Rust):
`aarokya-app/dashboard/src/components/FilterPanel/*`.

## Goals

1. Convert the three enum filters (Status, Trigger, Endpoint Type) from
   single-select to **multi-select**. Endpoint stays a text search.
2. Add a **date-range filter** on `created_at` with a hand-built popover:
   preset list (Today, Last 2 days, Last 7 days, This month, Last month) beside a
   clickable month-grid calendar for custom ranges.
3. Add a **confirmation dialog** before cancelling a job, and make the row
   actions safer/clearer (separate the destructive Cancel from Status/Versions).
4. Fix the **scroll-jump** on refetch (page-size, filter, pagination, cancel) by
   switching the jobs list from `<Suspense>` to `<Transition>`.

Non-goals: changing pagination semantics; filtering on fields other than the four
above + created_at; a date-picker dependency (hand-built).

## Wire contract

All filters are ANDed. Empty = unconstrained.

| Query param | Form | Example |
|---|---|---|
| `status` | comma-separated enum | `?status=ACTIVE,RETIRED` |
| `trigger_type` | comma-separated enum | `?trigger_type=CRON,DELAYED` |
| `endpoint_type` | comma-separated enum | `?endpoint_type=HTTP,INTERNAL` |
| `endpoint` | substring (unchanged) | `?endpoint=notify` |
| `created_after` | RFC-3339 datetime | `?created_after=2026-06-18T00:00:00Z` |
| `created_before` | RFC-3339 datetime | `?created_before=2026-06-24T23:59:59Z` |

Comma-separated (not repeated params) chosen to match the aarokya pattern and
because actix `web::Query` (serde_urlencoded) doesn't deserialize repeated keys
into a `Vec` cleanly. Date bounds are inclusive; the UI snaps presets/picks to
`T00:00:00Z` (after) and `T23:59:59Z` (before).

## Architecture by layer

### DB layer — `crates/common/src/db/jobs.rs`

`JobFilters` becomes:

```rust
pub struct JobFilters {
    pub status: Vec<JobStatus>,          // empty = unconstrained
    pub trigger: Vec<TriggerType>,
    pub endpoint: Option<String>,        // substring (unchanged)
    pub endpoint_type: Vec<EndpointType>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
}
```

`build_list_query` stays pure (returns `(String, Vec<BindValue>)`) so it remains
unit-testable. New bind representation:

```rust
enum BindValue { Scalar(String), Array(Vec<String>) }
```

- Non-empty enum list → `status = ANY($n)`, bind `BindValue::Array(values)`
  (one placeholder per filter, stable prepared-statement shape — preferred over
  expanded `IN (...)`).
- `endpoint` → `endpoint ILIKE '%' || $n || '%' ESCAPE '\'` (unchanged), `Scalar`.
- `created_after` → `created_at >= $n::timestamptz`, `Scalar(rfc3339)`.
- `created_before` → `created_at <= $n::timestamptz`, `Scalar(rfc3339)`.
- Cursor + LIMIT unchanged (composite cursor from #49 retained).

`list()` matches each `BindValue` and binds a `&str` (scalar) or `&[String]`
(array) accordingly, then binds `limit`.

### API layer — `crates/api/src/handlers/jobs.rs`

`JobListFilters` keeps `Option<String>` per key (raw comma string for enums; raw
RFC-3339 for dates). `into_db_filters`:

- Generalize `parse_filter` into `parse_filter_list`: split on `,`, trim, drop
  blanks, dedupe (stable order), `from_str_val` each; any invalid token → 400
  (`AppError::InvalidRequest("Invalid status: X")`).
- `created_after`/`created_before`: parse RFC-3339 (`DateTime::parse_from_rfc3339`)
  → 400 on failure. Reject `created_after > created_before` with a 400.
- Blank/absent → empty vec / `None`.

### Smithy — `smithy/model/jobs.smithy`, `common.smithy`

- `ListJobsInput`: `status`, `trigger_type`, `endpoint_type` become lists of the
  enums; add `created_after`/`created_before` as `Timestamp` query members.
- Caveat: Smithy's default list-query serialization is repeated params, not
  comma-joined. We document the comma-separated form in member docs; the contract
  approximates it. (Accepted — full custom protocol modeling is out of scope.)

### Dashboard — `crates/dashboard/`

**New component `components/multi_select.rs` — `MultiSelectFilter`**, mirroring
aarokya's `MultiSelectFilter`:
- Props: `label: String`, `options: Vec<(value: &str, label: &str)>`,
  `selected: ReadSignal<Vec<String>>`, `set_selected: WriteSignal<Vec<String>>`.
- Trigger button text: `All {label}` (empty) / `A, B` (≤2) / `N selected` (>2).
- Popover: top "All" row (clears), then checkable rows (✓ when selected); inline
  ✕ clear in the trigger; closes on outside click via `window_event_listener`
  (mousedown) gated on an `open` signal.

**New component `components/date_range.rs` — `DateRangeFilter`**:
- Props: `from: ReadSignal<Option<DateTime<Utc>>>`, `to: ...`, setters, `label`.
- Popover with two columns: preset list (Today / Last 2 days / Last 7 days /
  This month / Last month / Custom) + a month-grid calendar (prev/next month nav,
  click start then end → highlighted inclusive range), Clear / Apply footer.
- Presets compute concrete `DateTime<Utc>` client-side. Requires chrono `wasmbind`
  feature for `Utc::now()` in WASM (add to dashboard Cargo.toml).
- Trigger button text reflects the active range ("Last 7 days", or
  "Jun 18 – Jun 24", or "Created" when empty).

**`pages/workspace_detail.rs` — `JobsTab`**:
- Replace the three `<select>` filters with `MultiSelectFilter`; signals change
  from `String` to `Vec<String>`. Add the `DateRangeFilter`. Update `any_filter`,
  the Clear-filters button, and `reset_pagination` wiring (any filter change still
  resets to page 1).
- `JobListQueryParams` / `api::list_jobs`: enum filters serialized as comma-joined
  values; add `created_after`/`created_before` (RFC-3339) params.
- **`<Suspense>` → `<Transition>`** at the jobs list (~line 1126) — fixes the
  scroll jump for page-size, filters, pagination, and cancel.

**Cancel UX — `JobsTable` actions cell**:
- Layout: Status + Versions (benign, left); a divider; **Cancel** pushed right,
  destructive styling (red, bordered button). Cancel shown only for `ACTIVE` jobs.
- Confirmation: clicking Cancel opens a `ConfirmDialog` ("Cancel this job? It will
  be retired and stop running."). Cancel fires only on confirm.
- `ConfirmDialog` enhancement (`components/confirm.rs`): add
  `#[prop(into, optional)] confirm_label: Option<String>` (default `"Delete"`, so
  existing delete call-sites are unchanged) and an optional `confirm_variant`
  (red=delete, amber=cancel). Job dialog: confirm = "Cancel job", dismiss = "Keep
  job" — avoids the current "Cancel / Cancel" collision.

## Testing

- `build_list_query` unit tests: ANY-array for each enum filter, combined filters
  + cursor placeholder ordering, date bounds with `::timestamptz`, empty vecs emit
  no condition. (Asserts exact SQL + `BindValue` vec.)
- `into_db_filters` unit tests: multi-value parse + dedupe, blank handling,
  invalid token → 400, RFC-3339 parse + `after > before` → 400.
- Dashboard: WASM `cargo check`; manual smoke after `just dashboard-build-dev`.

## Rollout

- Stacked on `fix/jobs-list-pagination-filters` (PR #49). Separate PR.
- Rebuild the WASM bundle (`just dashboard-build`) on deploy — the running bundle
  is otherwise stale and won't show new UI.
