# Jobs Filters: Multi-Select + Date-Range + Cancel UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the dashboard Jobs filters to multi-select, add a date-range filter on `created_at`, add a confirmation + safer layout for the Cancel action, and stop the list from scrolling to the top on every refetch.

**Architecture:** Full-stack. The DB `JobFilters` carries typed `Vec<enum>` lists + optional date bounds; `build_list_query` stays a pure function emitting `= ANY($n)` / `created_at >=/<= $n::timestamptz`. The API parses comma-separated enum params and RFC-3339 dates into those filters. The dashboard gets two new hand-built Leptos popover components (`MultiSelectFilter`, `DateRangeFilter`) and switches the jobs list from `<Suspense>` to `<Transition>`.

**Tech Stack:** Rust, sqlx 0.7 (Postgres), actix-web 4, Smithy, Leptos 0.7 (WASM hydrate), chrono (+ `wasmbind` for WASM `now()`), Tailwind.

**Spec:** `docs/superpowers/specs/2026-06-29-jobs-filters-multiselect-daterange-design.md`

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/common/src/db/jobs.rs` | Modify | `JobFilters` (lists + dates), `BindValue`, `build_list_query`, `list`, tests |
| `crates/api/src/handlers/jobs.rs` | Modify | `JobListFilters` (+dates), `parse_filter_list`, `parse_datetime`, `into_db_filters`, tests |
| `smithy/model/common.smithy` | Modify | enum list shapes |
| `smithy/model/jobs.smithy` | Modify | `ListJobsInput` list + timestamp query members |
| `crates/dashboard/src/components/multi_select.rs` | Create | `MultiSelectFilter` popover component |
| `crates/dashboard/src/components/date_range.rs` | Create | `DateRangeFilter` popover (presets + month calendar) |
| `crates/dashboard/src/components/mod.rs` | Modify | export the two new components |
| `crates/dashboard/src/components/confirm.rs` | Modify | optional `confirm_label` / `confirm_variant` props |
| `crates/dashboard/src/pages/workspace_detail.rs` | Modify | JobsTab filter bar, query params, `<Transition>`, cancel UX |
| `crates/dashboard/Cargo.toml` | Modify | chrono `wasmbind` feature |

**Verification note:** The dashboard has no component test harness. Frontend tasks verify with `cargo check -p kronos-dashboard` (compile) + a manual smoke step at the end (`just dashboard-build-dev`, hard-refresh). Backend tasks are full TDD with `cargo test`.

---

## Phase 1 — DB layer

### Task 1: `JobFilters` becomes lists + date bounds; introduce `BindValue`

**Files:**
- Modify: `crates/common/src/db/jobs.rs`

- [ ] **Step 1: Replace the `JobFilters` struct**

Replace the existing `JobFilters` definition with:

```rust
/// Optional, server-side filters applied to [`list`]. All fields are ANDed
/// together. Empty `Vec`s / `None` fields are unconstrained. `endpoint` is a
/// case-insensitive substring; the enum lists match any of their values
/// (`= ANY`); the date bounds are inclusive.
#[derive(Debug, Default, Clone)]
pub struct JobFilters {
    pub status: Vec<JobStatus>,
    pub trigger: Vec<TriggerType>,
    pub endpoint: Option<String>,
    pub endpoint_type: Vec<EndpointType>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
}

/// One positional bind for the list query. `Scalar` is a single text value
/// (cursor, endpoint substring, an RFC-3339 timestamp); `Array` is bound as a
/// Postgres `text[]` for `= ANY($n)`.
#[derive(Debug, Clone, PartialEq)]
enum BindValue {
    Scalar(String),
    Array(Vec<String>),
}
```

- [ ] **Step 2: Confirm imports**

Ensure the top of the file imports `DateTime` and `Utc` (already used elsewhere) plus the enums:

```rust
use crate::{
    db::{tbl, DbContext},
    models::endpoint::EndpointType,
    models::job::{Job, JobStatus, TriggerType},
};
use chrono::{DateTime, Utc};
```

- [ ] **Step 3: Compile (expect errors in `build_list_query`/`list`/tests)**

Run: `cargo check -p kronos-common`
Expected: FAIL — `build_list_query` returns `Vec<String>` and binds mismatch. Fixed in Task 2.

### Task 2: `build_list_query` emits `= ANY` + date bounds; `list` binds `BindValue`

**Files:**
- Modify: `crates/common/src/db/jobs.rs`

- [ ] **Step 1: Replace `build_list_query` body**

```rust
/// Builds the `list` query and the ordered binds. Pure (no DB access) so the
/// placeholder/bind bookkeeping is unit-tested. The final `LIMIT` placeholder is
/// left for the caller to bind as an `i64`.
fn build_list_query(t: &str, cursor: Option<&str>, filters: &JobFilters) -> (String, Vec<BindValue>) {
    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<BindValue> = Vec::new();
    let mut n = 1;

    if let Some(c) = cursor {
        conditions.push(format!(
            "(created_at, job_id) < ((SELECT created_at FROM {t} WHERE job_id = ${n}), ${n})"
        ));
        binds.push(BindValue::Scalar(c.to_string()));
        n += 1;
    }
    if !filters.status.is_empty() {
        conditions.push(format!("status = ANY(${n})"));
        binds.push(BindValue::Array(filters.status.iter().map(|s| s.as_str().to_string()).collect()));
        n += 1;
    }
    if !filters.trigger.is_empty() {
        conditions.push(format!("trigger_type = ANY(${n})"));
        binds.push(BindValue::Array(filters.trigger.iter().map(|x| x.as_str().to_string()).collect()));
        n += 1;
    }
    if !filters.endpoint_type.is_empty() {
        conditions.push(format!("endpoint_type = ANY(${n})"));
        binds.push(BindValue::Array(filters.endpoint_type.iter().map(|x| x.as_str().to_string()).collect()));
        n += 1;
    }
    if let Some(endpoint) = &filters.endpoint {
        conditions.push(format!("endpoint ILIKE '%' || ${n} || '%' ESCAPE '\\'"));
        binds.push(BindValue::Scalar(escape_like(endpoint)));
        n += 1;
    }
    if let Some(after) = &filters.created_after {
        conditions.push(format!("created_at >= ${n}::timestamptz"));
        binds.push(BindValue::Scalar(after.to_rfc3339()));
        n += 1;
    }
    if let Some(before) = &filters.created_before {
        conditions.push(format!("created_at <= ${n}::timestamptz"));
        binds.push(BindValue::Scalar(before.to_rfc3339()));
        n += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT * FROM {t}{where_clause} ORDER BY created_at DESC, job_id DESC LIMIT ${n}"
    );
    (sql, binds)
}
```

- [ ] **Step 2: Replace `list` bind loop**

```rust
pub async fn list(
    db: &mut DbContext<'_>,
    cursor: Option<&str>,
    limit: i64,
    filters: &JobFilters,
) -> Result<Vec<Job>, sqlx::Error> {
    let t = tbl(db.prefix, "jobs");
    let (sql, binds) = build_list_query(&t, cursor, filters);
    let mut query = sqlx::query_as::<_, Job>(&sql);
    for bind in &binds {
        query = match bind {
            BindValue::Scalar(s) => query.bind(s),
            BindValue::Array(a) => query.bind(a.as_slice()),
        };
    }
    query.bind(limit).fetch_all(&mut *db.conn).await
}
```

- [ ] **Step 3: Compile**

Run: `cargo check -p kronos-common`
Expected: FAIL only in the `mod tests` block (old `JobFilters { status: Some(...) }` shape). Fixed in Task 3.

### Task 3: Update + extend `build_list_query` unit tests

**Files:**
- Modify: `crates/common/src/db/jobs.rs` (the `#[cfg(test)] mod tests` block)

- [ ] **Step 1: Replace the four `list_query_*` tests and add date/array tests**

```rust
    #[test]
    fn list_query_without_cursor_or_filters() {
        let (sql, binds) = build_list_query("jobs", None, &JobFilters::default());
        assert_eq!(
            sql,
            "SELECT * FROM jobs ORDER BY created_at DESC, job_id DESC LIMIT $1"
        );
        assert!(binds.is_empty());
    }

    #[test]
    fn list_query_with_cursor_only() {
        let (sql, binds) = build_list_query("jobs", Some("job-9"), &JobFilters::default());
        assert_eq!(
            sql,
            "SELECT * FROM jobs WHERE \
             (created_at, job_id) < ((SELECT created_at FROM jobs WHERE job_id = $1), $1) \
             ORDER BY created_at DESC, job_id DESC LIMIT $2"
        );
        assert_eq!(binds, vec![BindValue::Scalar("job-9".into())]);
    }

    #[test]
    fn list_query_uses_any_for_multi_value_enum_filters() {
        let filters = JobFilters {
            status: vec![JobStatus::ACTIVE, JobStatus::RETIRED],
            trigger: vec![TriggerType::CRON],
            endpoint_type: vec![EndpointType::HTTP, EndpointType::INTERNAL],
            ..Default::default()
        };
        let (sql, binds) = build_list_query("jobs", None, &filters);
        assert_eq!(
            sql,
            "SELECT * FROM jobs WHERE \
             status = ANY($1) AND trigger_type = ANY($2) AND endpoint_type = ANY($3) \
             ORDER BY created_at DESC, job_id DESC LIMIT $4"
        );
        assert_eq!(
            binds,
            vec![
                BindValue::Array(vec!["ACTIVE".into(), "RETIRED".into()]),
                BindValue::Array(vec!["CRON".into()]),
                BindValue::Array(vec!["HTTP".into(), "INTERNAL".into()]),
            ]
        );
    }

    #[test]
    fn list_query_combines_cursor_filters_endpoint_and_dates_in_order() {
        let filters = JobFilters {
            status: vec![JobStatus::ACTIVE],
            endpoint: Some("notify".into()),
            created_after: Some(
                chrono::DateTime::parse_from_rfc3339("2026-06-18T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            ),
            created_before: Some(
                chrono::DateTime::parse_from_rfc3339("2026-06-24T23:59:59Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            ),
            ..Default::default()
        };
        let (sql, binds) = build_list_query("jobs", Some("job-9"), &filters);
        assert_eq!(
            sql,
            "SELECT * FROM jobs WHERE \
             (created_at, job_id) < ((SELECT created_at FROM jobs WHERE job_id = $1), $1) AND \
             status = ANY($2) AND \
             endpoint ILIKE '%' || $3 || '%' ESCAPE '\\' AND \
             created_at >= $4::timestamptz AND created_at <= $5::timestamptz \
             ORDER BY created_at DESC, job_id DESC LIMIT $6"
        );
        assert_eq!(
            binds,
            vec![
                BindValue::Scalar("job-9".into()),
                BindValue::Array(vec!["ACTIVE".into()]),
                BindValue::Scalar("notify".into()),
                BindValue::Scalar("2026-06-18T00:00:00+00:00".into()),
                BindValue::Scalar("2026-06-24T23:59:59+00:00".into()),
            ]
        );
    }

    #[test]
    fn list_query_escapes_like_metacharacters_in_endpoint() {
        let filters = JobFilters { endpoint: Some("order_50%_v2".into()), ..Default::default() };
        let (_sql, binds) = build_list_query("jobs", None, &filters);
        assert_eq!(binds, vec![BindValue::Scalar(r"order\_50\%\_v2".into())]);
    }

    #[test]
    fn escape_like_passes_through_plain_text() {
        assert_eq!(escape_like("notify"), "notify");
        assert_eq!(escape_like("a_b"), r"a\_b");
        assert_eq!(escape_like("100%"), r"100\%");
        assert_eq!(escape_like(r"a\b"), r"a\\b");
    }
```

> Note: `DateTime::<Utc>::to_rfc3339()` renders the `+00:00` offset form (not `Z`), which is why the expected binds use `+00:00`. Postgres parses both.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p kronos-common --lib db::jobs`
Expected: PASS (all `list_query_*`, `escape_like`, plus the existing `cron_command_*`).

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/db/jobs.rs
git commit -m "feat(db): multi-value enum filters (= ANY) and created_at date bounds for jobs list"
```

---

## Phase 2 — API layer

### Task 4: Parse comma-separated enum lists + RFC-3339 dates (write tests first)

**Files:**
- Modify: `crates/api/src/handlers/jobs.rs`

- [ ] **Step 1: Write the failing tests**

In the existing `#[cfg(test)] mod tests` block, replace the `filters(...)` helper signature and the value tests with:

```rust
    fn filters(
        status: Option<&str>,
        trigger_type: Option<&str>,
        endpoint: Option<&str>,
        endpoint_type: Option<&str>,
        created_after: Option<&str>,
        created_before: Option<&str>,
    ) -> JobListFilters {
        JobListFilters {
            status: status.map(String::from),
            trigger_type: trigger_type.map(String::from),
            endpoint: endpoint.map(String::from),
            endpoint_type: endpoint_type.map(String::from),
            created_after: created_after.map(String::from),
            created_before: created_before.map(String::from),
        }
    }

    fn assert_invalid_request(result: Result<db::jobs::JobFilters, AppError>) {
        match result {
            Err(AppError::InvalidRequest(_)) => {}
            Err(_) => panic!("expected InvalidRequest"),
            Ok(_) => panic!("expected an error, got Ok"),
        }
    }

    #[test]
    fn into_db_filters_parses_comma_separated_enums() {
        let f = filters(Some("ACTIVE,RETIRED"), Some("CRON,DELAYED"), Some("notify"), Some("HTTP,INTERNAL"), None, None)
            .into_db_filters()
            .unwrap();
        assert_eq!(f.status, vec![JobStatus::ACTIVE, JobStatus::RETIRED]);
        assert_eq!(f.trigger, vec![TriggerType::CRON, TriggerType::DELAYED]);
        assert_eq!(f.endpoint, Some("notify".to_string()));
        assert_eq!(f.endpoint_type, vec![EndpointType::HTTP, EndpointType::INTERNAL]);
    }

    #[test]
    fn into_db_filters_trims_dedupes_and_drops_blanks() {
        let f = filters(Some(" ACTIVE , ACTIVE ,, RETIRED "), None, None, None, None, None)
            .into_db_filters()
            .unwrap();
        assert_eq!(f.status, vec![JobStatus::ACTIVE, JobStatus::RETIRED]);
    }

    #[test]
    fn into_db_filters_empty_lists_when_absent() {
        let f = filters(None, None, None, None, None, None).into_db_filters().unwrap();
        assert!(f.status.is_empty());
        assert!(f.trigger.is_empty());
        assert!(f.endpoint_type.is_empty());
        assert_eq!(f.endpoint, None);
        assert_eq!(f.created_after, None);
        assert_eq!(f.created_before, None);
    }

    #[test]
    fn into_db_filters_parses_rfc3339_dates() {
        let f = filters(None, None, None, None, Some("2026-06-18T00:00:00Z"), Some("2026-06-24T23:59:59Z"))
            .into_db_filters()
            .unwrap();
        assert!(f.created_after.is_some());
        assert!(f.created_before.is_some());
    }

    #[test]
    fn into_db_filters_rejects_bad_enum_token() {
        assert_invalid_request(filters(Some("ACTIVE,BOGUS"), None, None, None, None, None).into_db_filters());
    }

    #[test]
    fn into_db_filters_rejects_bad_date() {
        assert_invalid_request(filters(None, None, None, None, Some("yesterday"), None).into_db_filters());
    }

    #[test]
    fn into_db_filters_rejects_inverted_date_range() {
        assert_invalid_request(
            filters(None, None, None, None, Some("2026-06-24T00:00:00Z"), Some("2026-06-18T00:00:00Z")).into_db_filters(),
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kronos-api`
Expected: FAIL (compile error — `JobListFilters` lacks date fields; `into_db_filters` returns old types).

- [ ] **Step 3: Add date fields to `JobListFilters`**

```rust
#[derive(Debug, serde::Deserialize)]
pub struct JobListFilters {
    pub status: Option<String>,
    pub trigger_type: Option<String>,
    pub endpoint: Option<String>,
    pub endpoint_type: Option<String>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
}
```

- [ ] **Step 4: Replace `parse_filter` with `parse_filter_list` + add `parse_datetime`**

```rust
/// Splits a comma-separated query value into validated, de-duplicated enum
/// values. Blank tokens are skipped; an invalid token fails the whole request
/// with a 400 rather than silently dropping rows.
fn parse_filter_list<T: PartialEq>(
    value: Option<String>,
    parse: impl Fn(&str) -> Option<T>,
    label: &str,
) -> Result<Vec<T>, AppError> {
    let raw = match blank_to_none(value) {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };
    let mut out: Vec<T> = Vec::new();
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let parsed =
            parse(token).ok_or_else(|| AppError::InvalidRequest(format!("Invalid {label}: {token}")))?;
        if !out.contains(&parsed) {
            out.push(parsed);
        }
    }
    Ok(out)
}

/// Parses an optional RFC-3339 datetime query value to UTC; blank == absent.
fn parse_datetime(value: Option<String>, label: &str) -> Result<Option<DateTime<Utc>>, AppError> {
    match blank_to_none(value) {
        Some(s) => {
            let dt = DateTime::parse_from_rfc3339(&s)
                .map_err(|_| AppError::InvalidRequest(format!("Invalid {label}: {s}")))?;
            Ok(Some(dt.with_timezone(&Utc)))
        }
        None => Ok(None),
    }
}
```

- [ ] **Step 5: Replace `into_db_filters`**

```rust
    fn into_db_filters(self) -> Result<db::jobs::JobFilters, AppError> {
        let created_after = parse_datetime(self.created_after, "created_after")?;
        let created_before = parse_datetime(self.created_before, "created_before")?;
        if let (Some(a), Some(b)) = (created_after, created_before) {
            if a > b {
                return Err(AppError::InvalidRequest(
                    "created_after must not be after created_before".into(),
                ));
            }
        }
        Ok(db::jobs::JobFilters {
            status: parse_filter_list(self.status, JobStatus::from_str_val, "status")?,
            trigger: parse_filter_list(self.trigger_type, TriggerType::from_str_val, "trigger")?,
            endpoint: blank_to_none(self.endpoint),
            endpoint_type: parse_filter_list(self.endpoint_type, EndpointType::from_str_val, "endpoint_type")?,
            created_after,
            created_before,
        })
    }
```

- [ ] **Step 6: Fix imports**

Ensure the handler imports `DateTime` (add to the existing `use chrono::Utc;` → `use chrono::{DateTime, Utc};`). `JobStatus`, `TriggerType`, `EndpointType` are already imported.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p kronos-api`
Expected: PASS (all `into_db_filters_*`).

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/handlers/jobs.rs
git commit -m "feat(api): parse comma-separated enum filters and created_at date range for jobs list"
```

---

## Phase 3 — Smithy contract

### Task 5: Model list + timestamp query params

**Files:**
- Modify: `smithy/model/common.smithy`
- Modify: `smithy/model/jobs.smithy`

- [ ] **Step 1: Add list shapes to `common.smithy`**

After the enum definitions add:

```smithy
list JobStatusList { member: JobStatusEnum }
list TriggerTypeList { member: TriggerTypeEnum }
list EndpointTypeList { member: EndpointTypeEnum }
```

- [ ] **Step 2: Update `ListJobsInput` in `jobs.smithy`**

Replace the three single-enum members with lists and add the date bounds. Keep the `endpoint` member as-is.

```smithy
@input
structure ListJobsInput with [WorkspaceHeaders, PaginationQuery] {
    @httpQuery("endpoint")
    endpoint: String

    /// Comma-separated list of trigger types, e.g. `CRON,DELAYED`.
    @httpQuery("trigger_type")
    trigger_type: TriggerTypeList

    /// Comma-separated list of job statuses, e.g. `ACTIVE,RETIRED`.
    @httpQuery("status")
    status: JobStatusList

    /// Comma-separated list of endpoint types, e.g. `HTTP,INTERNAL`.
    @httpQuery("endpoint_type")
    endpoint_type: EndpointTypeList

    /// Inclusive lower bound on `created_at` (RFC-3339).
    @httpQuery("created_after")
    created_after: Timestamp

    /// Inclusive upper bound on `created_at` (RFC-3339).
    @httpQuery("created_before")
    created_before: Timestamp
}
```

> Known limitation (documented in the spec): Smithy serializes list query params as repeated keys; the live API uses comma-separated values. The member docs record the real wire format.

- [ ] **Step 3: Validate the model if the build tool is available**

Run: `just smithy-build` (or the project's Smithy build target if one exists; skip if none).
Expected: model builds, or no-op if no target.

- [ ] **Step 4: Commit**

```bash
git add smithy/model/common.smithy smithy/model/jobs.smithy
git commit -m "feat(smithy): list + date-range query params for ListJobs"
```

---

## Phase 4 — `MultiSelectFilter` component

### Task 6: Build the multi-select popover

**Files:**
- Create: `crates/dashboard/src/components/multi_select.rs`
- Modify: `crates/dashboard/src/components/mod.rs`

- [ ] **Step 1: Create the component**

```rust
use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Multi-select dropdown. Empty selection means "no filter". Selecting "All"
/// clears the selection; each option toggles. Closes on outside click.
#[component]
pub fn MultiSelectFilter(
    #[prop(into)] label: String,
    /// (value, display label) pairs.
    options: Vec<(&'static str, &'static str)>,
    selected: ReadSignal<Vec<String>>,
    set_selected: WriteSignal<Vec<String>>,
) -> impl IntoView {
    let (open, set_open) = signal(false);
    let node_ref = NodeRef::<leptos::html::Div>::new();

    // Close on outside click.
    let handle = window_event_listener(leptos::ev::mousedown, move |ev| {
        if !open.get_untracked() {
            return;
        }
        let inside = node_ref
            .get_untracked()
            .zip(ev.target())
            .and_then(|(el, target)| target.dyn_into::<web_sys::Node>().ok().map(|n| el.contains(Some(&n))))
            .unwrap_or(false);
        if !inside {
            set_open.set(false);
        }
    });
    on_cleanup(move || handle.remove());

    let label_for_button = label.clone();
    let options_for_button = options.clone();
    let button_text = move || {
        let sel = selected.get();
        if sel.is_empty() {
            format!("All {label_for_button}")
        } else {
            let labels: Vec<&str> = options_for_button
                .iter()
                .filter(|(v, _)| sel.iter().any(|s| s == v))
                .map(|(_, l)| *l)
                .collect();
            if labels.len() <= 2 {
                labels.join(", ")
            } else {
                format!("{} selected", labels.len())
            }
        }
    };

    let toggle = move |value: &'static str| {
        set_selected.update(|sel| {
            if let Some(pos) = sel.iter().position(|s| s == value) {
                sel.remove(pos);
            } else {
                sel.push(value.to_string());
            }
        });
    };

    let any_selected = move || !selected.get().is_empty();

    view! {
        <div node_ref=node_ref class="relative flex flex-col gap-1 min-w-[160px]">
            <label class="text-xs font-medium text-gray-500">{label.clone()}</label>
            <button type="button"
                on:click=move |_| set_open.update(|o| *o = !*o)
                class="flex h-9 items-center justify-between gap-2 rounded-lg border border-gray-300 bg-white px-3 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none">
                <span class="truncate" class:text-gray-400=move || !any_selected()>{button_text}</span>
                <span class="flex items-center gap-1 shrink-0">
                    <Show when=any_selected>
                        <span role="button" aria-label="Clear"
                            on:click=move |ev| { ev.stop_propagation(); set_selected.set(Vec::new()); }
                            class="rounded p-0.5 hover:bg-gray-100">"\u{2715}"</span>
                    </Show>
                    <span class="text-gray-400">"\u{25be}"</span>
                </span>
            </button>
            <Show when=move || open.get()>
                <div class="absolute left-0 top-full z-50 mt-1 w-full min-w-[200px] rounded-lg border border-gray-200 bg-white p-1 shadow-lg">
                    <button type="button"
                        on:click=move |_| { set_selected.set(Vec::new()); set_open.set(false); }
                        class="flex w-full items-center justify-between rounded px-2 py-1.5 text-sm text-left border-b border-gray-100 mb-1 hover:bg-gray-50"
                        class:font-medium=move || !any_selected()>
                        <span>{format!("All {label}")}</span>
                        <Show when=move || !any_selected()><span class="text-blue-600">"\u{2713}"</span></Show>
                    </button>
                    {options.into_iter().map(|(value, opt_label)| {
                        let checked = move || selected.get().iter().any(|s| s == value);
                        view! {
                            <button type="button"
                                on:click=move |_| toggle(value)
                                class="flex w-full items-center justify-between rounded px-2 py-1.5 text-sm text-left hover:bg-gray-50"
                                class:font-medium=checked>
                                <span>{opt_label}</span>
                                <Show when=checked><span class="text-blue-600">"\u{2713}"</span></Show>
                            </button>
                        }
                    }).collect_view()}
                </div>
            </Show>
        </div>
    }
}
```

> If `window_event_listener`'s handle API differs in this Leptos patch version, the compile step will flag it; the fix is to match the returned handle type (it exposes `.remove()`).

- [ ] **Step 2: Export from `components/mod.rs`**

Add: `pub mod multi_select;`

- [ ] **Step 3: Compile**

Run: `cargo check -p kronos-dashboard`
Expected: PASS (component compiles even though unused — allow the dead-code warning for now).

- [ ] **Step 4: Commit**

```bash
git add crates/dashboard/src/components/multi_select.rs crates/dashboard/src/components/mod.rs
git commit -m "feat(dashboard): MultiSelectFilter popover component"
```

---

## Phase 5 — `DateRangeFilter` component

### Task 7: chrono `wasmbind` + preset date math (pure helpers, unit-tested)

**Files:**
- Modify: `crates/dashboard/Cargo.toml`
- Create: `crates/dashboard/src/components/date_range.rs`

- [ ] **Step 1: Enable chrono `wasmbind` in the dashboard**

In `crates/dashboard/Cargo.toml`, ensure the chrono dependency enables `wasmbind` (needed for `Utc::now()` under WASM). If it inherits the workspace dep, override:

```toml
chrono = { workspace = true, features = ["wasmbind"] }
```

- [ ] **Step 2: Create `date_range.rs` with pure preset helpers + tests**

```rust
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};

/// A named quick-pick range. `range(now)` returns inclusive [after, before].
#[derive(Clone, Copy, PartialEq)]
pub enum Preset {
    Today,
    Last2Days,
    Last7Days,
    ThisMonth,
    LastMonth,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Preset::Today => "Today",
            Preset::Last2Days => "Last 2 days",
            Preset::Last7Days => "Last 7 days",
            Preset::ThisMonth => "This month",
            Preset::LastMonth => "Last month",
        }
    }

    pub const ALL: [Preset; 5] =
        [Preset::Today, Preset::Last2Days, Preset::Last7Days, Preset::ThisMonth, Preset::LastMonth];

    /// Inclusive [after, before] in UTC for the preset, relative to `now`.
    pub fn range(self, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        let today = now.date_naive();
        match self {
            Preset::Today => (start_of(today), end_of(today)),
            Preset::Last2Days => (start_of(today - Duration::days(1)), end_of(today)),
            Preset::Last7Days => (start_of(today - Duration::days(6)), end_of(today)),
            Preset::ThisMonth => (start_of(first_of_month(today)), end_of(today)),
            Preset::LastMonth => {
                let first_this = first_of_month(today);
                let last_prev = first_this - Duration::days(1);
                (start_of(first_of_month(last_prev)), end_of(last_prev))
            }
        }
    }
}

fn start_of(d: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
}
fn end_of(d: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&d.and_hms_opt(23, 59, 59).unwrap())
}
fn first_of_month(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        // Wed 2026-06-24 10:30:00 UTC
        Utc.from_utc_datetime(&NaiveDate::from_ymd_opt(2026, 6, 24).unwrap().and_hms_opt(10, 30, 0).unwrap())
    }

    #[test]
    fn today_is_full_day() {
        let (a, b) = Preset::Today.range(now());
        assert_eq!(a.to_rfc3339(), "2026-06-24T00:00:00+00:00");
        assert_eq!(b.to_rfc3339(), "2026-06-24T23:59:59+00:00");
    }

    #[test]
    fn last_7_days_spans_7_calendar_days() {
        let (a, b) = Preset::Last7Days.range(now());
        assert_eq!(a.to_rfc3339(), "2026-06-18T00:00:00+00:00");
        assert_eq!(b.to_rfc3339(), "2026-06-24T23:59:59+00:00");
    }

    #[test]
    fn this_month_starts_on_the_first() {
        let (a, b) = Preset::ThisMonth.range(now());
        assert_eq!(a.to_rfc3339(), "2026-06-01T00:00:00+00:00");
        assert_eq!(b.to_rfc3339(), "2026-06-24T23:59:59+00:00");
    }

    #[test]
    fn last_month_is_full_previous_month() {
        let (a, b) = Preset::LastMonth.range(now());
        assert_eq!(a.to_rfc3339(), "2026-05-01T00:00:00+00:00");
        assert_eq!(b.to_rfc3339(), "2026-05-31T23:59:59+00:00");
    }
}
```

- [ ] **Step 3: Run the preset tests**

Run: `cargo test -p kronos-dashboard --lib components::date_range`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/dashboard/Cargo.toml crates/dashboard/src/components/date_range.rs
git commit -m "feat(dashboard): date-range presets with chrono wasmbind"
```

### Task 8: Date-range popover view (presets + month calendar)

**Files:**
- Modify: `crates/dashboard/src/components/date_range.rs`
- Modify: `crates/dashboard/src/components/mod.rs`

- [ ] **Step 1: Add the `DateRangeFilter` component**

Append to `date_range.rs`:

```rust
use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Month-grid range picker with preset shortcuts. Writes inclusive [after,
/// before] UTC bounds; empty = no filter. `now` is injected so the component is
/// deterministic and the WASM `Utc::now()` call lives in the caller.
#[component]
pub fn DateRangeFilter(
    after: ReadSignal<Option<DateTime<Utc>>>,
    set_after: WriteSignal<Option<DateTime<Utc>>>,
    before: ReadSignal<Option<DateTime<Utc>>>,
    set_before: WriteSignal<Option<DateTime<Utc>>>,
) -> impl IntoView {
    let (open, set_open) = signal(false);
    // Month currently displayed in the grid (first day of that month).
    let (view_month, set_view_month) = signal(first_of_month(Utc::now().date_naive()));
    let node_ref = NodeRef::<leptos::html::Div>::new();

    let handle = window_event_listener(leptos::ev::mousedown, move |ev| {
        if !open.get_untracked() {
            return;
        }
        let inside = node_ref
            .get_untracked()
            .zip(ev.target())
            .and_then(|(el, t)| t.dyn_into::<web_sys::Node>().ok().map(|n| el.contains(Some(&n))))
            .unwrap_or(false);
        if !inside {
            set_open.set(false);
        }
    });
    on_cleanup(move || handle.remove());

    let button_text = move || match (after.get(), before.get()) {
        (Some(a), Some(b)) => format!("{} \u{2013} {}", a.format("%b %d"), b.format("%b %d")),
        (Some(a), None) => format!("From {}", a.format("%b %d")),
        (None, Some(b)) => format!("Until {}", b.format("%b %d")),
        (None, None) => "Created".to_string(),
    };
    let any = move || after.get().is_some() || before.get().is_some();

    let apply_preset = move |p: Preset| {
        let (a, b) = p.range(Utc::now());
        set_after.set(Some(a));
        set_before.set(Some(b));
        set_open.set(false);
    };

    // Click a day: first click sets `after` (00:00) and clears `before`; second
    // click sets `before` (23:59:59), swapping if before the start.
    let pick_day = move |d: NaiveDate| {
        let a = after.get();
        let b = before.get();
        if a.is_none() || b.is_some() {
            set_after.set(Some(start_of(d)));
            set_before.set(None);
        } else {
            let start = a.unwrap();
            let picked_end = end_of(d);
            if picked_end < start {
                set_after.set(Some(start_of(d)));
                set_before.set(Some(end_of(start.date_naive())));
            } else {
                set_before.set(Some(picked_end));
            }
        }
    };

    view! {
        <div node_ref=node_ref class="relative flex flex-col gap-1 min-w-[160px]">
            <label class="text-xs font-medium text-gray-500">"Created"</label>
            <button type="button"
                on:click=move |_| set_open.update(|o| *o = !*o)
                class="flex h-9 items-center justify-between gap-2 rounded-lg border border-gray-300 bg-white px-3 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none">
                <span class="truncate" class:text-gray-400=move || !any()>{button_text}</span>
                <span class="flex items-center gap-1 shrink-0">
                    <Show when=any>
                        <span role="button" aria-label="Clear"
                            on:click=move |ev| { ev.stop_propagation(); set_after.set(None); set_before.set(None); }
                            class="rounded p-0.5 hover:bg-gray-100">"\u{2715}"</span>
                    </Show>
                    <span class="text-gray-400">"\u{25be}"</span>
                </span>
            </button>
            <Show when=move || open.get()>
                <div class="absolute left-0 top-full z-50 mt-1 flex rounded-lg border border-gray-200 bg-white shadow-lg">
                    // Presets column
                    <div class="flex flex-col gap-0.5 border-r border-gray-100 p-2 w-36">
                        {Preset::ALL.iter().copied().map(|p| view! {
                            <button type="button"
                                on:click=move |_| apply_preset(p)
                                class="rounded px-2 py-1.5 text-sm text-left hover:bg-gray-50">
                                {p.label()}
                            </button>
                        }).collect_view()}
                        <button type="button"
                            on:click=move |_| { set_after.set(None); set_before.set(None); }
                            class="mt-1 rounded px-2 py-1.5 text-sm text-left text-gray-500 hover:bg-gray-50 border-t border-gray-100">
                            "Clear"
                        </button>
                    </div>
                    // Calendar column
                    <div class="p-3 w-64">
                        <div class="flex items-center justify-between mb-2">
                            <button type="button" class="px-2 rounded hover:bg-gray-100"
                                on:click=move |_| set_view_month.update(|m| *m = add_months(*m, -1))>"\u{2039}"</button>
                            <span class="text-sm font-medium">{move || view_month.get().format("%B %Y").to_string()}</span>
                            <button type="button" class="px-2 rounded hover:bg-gray-100"
                                on:click=move |_| set_view_month.update(|m| *m = add_months(*m, 1))>"\u{203a}"</button>
                        </div>
                        <div class="grid grid-cols-7 gap-0.5 text-center text-[10px] text-gray-400 mb-1">
                            {["Su","Mo","Tu","We","Th","Fr","Sa"].iter().map(|d| view! { <span>{*d}</span> }).collect_view()}
                        </div>
                        <div class="grid grid-cols-7 gap-0.5">
                            {move || {
                                month_cells(view_month.get()).into_iter().map(|cell| {
                                    match cell {
                                        None => view! { <span></span> }.into_any(),
                                        Some(d) => {
                                            let in_range = move || {
                                                let day = end_of(d);
                                                let lo = after.get();
                                                let hi = before.get();
                                                match (lo, hi) {
                                                    (Some(a), Some(b)) => start_of(d) >= a && day <= b,
                                                    (Some(a), None) => d == a.date_naive(),
                                                    _ => false,
                                                }
                                            };
                                            view! {
                                                <button type="button"
                                                    on:click=move |_| pick_day(d)
                                                    class="h-7 rounded text-sm hover:bg-blue-100"
                                                    class:bg-blue-600=in_range
                                                    class:text-white=in_range>
                                                    {d.day().to_string()}
                                                </button>
                                            }.into_any()
                                        }
                                    }
                                }).collect_view()
                            }}
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}

/// Days of `month` laid out in a 7-col grid: leading `None` pad for the weekday
/// offset (Sunday=0), then each day of the month.
fn month_cells(month: NaiveDate) -> Vec<Option<NaiveDate>> {
    let first = first_of_month(month);
    let lead = first.weekday().num_days_from_sunday() as usize;
    let days_in_month = {
        let next = add_months(first, 1);
        (next - Duration::days(1)).day()
    };
    let mut cells: Vec<Option<NaiveDate>> = vec![None; lead];
    for day in 1..=days_in_month {
        cells.push(NaiveDate::from_ymd_opt(first.year(), first.month(), day));
    }
    cells
}

/// Adds `delta` months (can be negative) to the first-of-month date `d`.
fn add_months(d: NaiveDate, delta: i32) -> NaiveDate {
    let zero = d.year() * 12 + (d.month() as i32 - 1) + delta;
    let year = zero.div_euclid(12);
    let month = zero.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1).unwrap()
}
```

- [ ] **Step 2: Add `month_cells` / `add_months` unit tests**

Append to the existing `mod tests`:

```rust
    #[test]
    fn add_months_wraps_year() {
        assert_eq!(add_months(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), -1),
                   NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        assert_eq!(add_months(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(), 1),
                   NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
    }

    #[test]
    fn month_cells_pads_and_counts() {
        // June 2026: 1st is a Monday -> 1 leading pad; 30 days.
        let cells = month_cells(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
        assert_eq!(cells[0], None);
        assert_eq!(cells[1], NaiveDate::from_ymd_opt(2026, 6, 1));
        assert_eq!(cells.iter().filter(|c| c.is_some()).count(), 30);
    }
```

- [ ] **Step 3: Export + compile + test**

Add `pub mod date_range;` to `components/mod.rs`.
Run: `cargo test -p kronos-dashboard --lib components::date_range`
Expected: PASS (6 tests).
Run: `cargo check -p kronos-dashboard`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/dashboard/src/components/date_range.rs crates/dashboard/src/components/mod.rs
git commit -m "feat(dashboard): DateRangeFilter popover with month calendar"
```

---

## Phase 6 — Wire filters into JobsTab + Transition fix

### Task 9: Replace the filter bar, query params, and Suspense

**Files:**
- Modify: `crates/dashboard/src/pages/workspace_detail.rs`
- Modify: `crates/dashboard/src/api/models.rs`
- Modify: `crates/dashboard/src/api/client.rs`

- [ ] **Step 1: Extend `JobListQueryParams` (models.rs)**

Change the enum filter fields to `Vec<String>` and add date bounds:

```rust
#[derive(Debug, Clone)]
pub struct JobListQueryParams {
    pub cursor: Option<String>,
    pub limit: i64,
    pub status: Vec<String>,
    pub trigger: Vec<String>,
    pub endpoint: Option<String>,
    pub endpoint_type: Vec<String>,
    pub created_after: Option<String>,  // RFC-3339
    pub created_before: Option<String>, // RFC-3339
}

impl Default for JobListQueryParams {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: 50,
            status: Vec::new(),
            trigger: Vec::new(),
            endpoint: None,
            endpoint_type: Vec::new(),
            created_after: None,
            created_before: None,
        }
    }
}
```

- [ ] **Step 2: Serialize lists as comma-joined params (client.rs)**

In `list_jobs` (the `inner` cfg-wasm impl), replace the per-filter pushes:

```rust
        if !params.status.is_empty() {
            push_query_param(&mut qs, "status", &params.status.join(","));
        }
        if !params.trigger.is_empty() {
            push_query_param(&mut qs, "trigger_type", &params.trigger.join(","));
        }
        if !params.endpoint_type.is_empty() {
            push_query_param(&mut qs, "endpoint_type", &params.endpoint_type.join(","));
        }
        if let Some(endpoint) = &params.endpoint {
            push_query_param(&mut qs, "endpoint", endpoint);
        }
        if let Some(after) = &params.created_after {
            push_query_param(&mut qs, "created_after", after);
        }
        if let Some(before) = &params.created_before {
            push_query_param(&mut qs, "created_before", before);
        }
```

(The SSR stub signature is unchanged — it already takes `JobListQueryParams`.)

- [ ] **Step 3: Update JobsTab signals + filter bar (workspace_detail.rs)**

Replace the four `String` filter signals with `Vec<String>` for the enums and `Option<DateTime<Utc>>` for the dates:

```rust
    let (status_filter, set_status_filter) = signal(Vec::<String>::new());
    let (trigger_filter, set_trigger_filter) = signal(Vec::<String>::new());
    let (endpoint_type_filter, set_endpoint_type_filter) = signal(Vec::<String>::new());
    let (endpoint_filter, set_endpoint_filter) = signal(String::new());
    let (created_after, set_created_after) = signal(Option::<chrono::DateTime<chrono::Utc>>::None);
    let (created_before, set_created_before) = signal(Option::<chrono::DateTime<chrono::Utc>>::None);
    let (page_size, set_page_size) = signal(50i64);
```

Replace the `any_filter` closure:

```rust
    let any_filter = move || {
        !status_filter.get().is_empty()
            || !trigger_filter.get().is_empty()
            || !endpoint_type_filter.get().is_empty()
            || !endpoint_filter.get().is_empty()
            || created_after.get().is_some()
            || created_before.get().is_some()
    };
```

Build the resource params:

```rust
        let params = JobListQueryParams {
            cursor,
            limit: page_size.get(),
            status: status_filter.get(),
            trigger: trigger_filter.get(),
            endpoint: filter_opt(endpoint_filter.get()),
            endpoint_type: endpoint_type_filter.get(),
            created_after: created_after.get().map(|d| d.to_rfc3339()),
            created_before: created_before.get().map(|d| d.to_rfc3339()),
        };
```

> Resetting pagination: the existing pattern wires `reset_pagination()` into each filter's change handler. The new `MultiSelectFilter`/`DateRangeFilter` mutate signals directly, so add an `Effect` that resets pagination when any filter signal changes:
>
> ```rust
> Effect::new(move |prev: Option<()>| {
>     // track all filters
>     let _ = (status_filter.get(), trigger_filter.get(), endpoint_type_filter.get(),
>              endpoint_filter.get(), created_after.get(), created_before.get());
>     if prev.is_some() { reset_pagination(); } // skip the initial run
> });
> ```

- [ ] **Step 4: Replace the filter-bar markup**

Replace the four `<select>`/`<input>` blocks (Status, Trigger, Endpoint Type, Endpoint) and keep the search box; add the two new components. Use the imports `use crate::components::multi_select::MultiSelectFilter; use crate::components::date_range::DateRangeFilter;`.

```rust
                <MultiSelectFilter label="Status"
                    options=vec![("ACTIVE","Active"),("RETIRED","Retired")]
                    selected=status_filter set_selected=set_status_filter />
                <MultiSelectFilter label="Trigger"
                    options=vec![("IMMEDIATE","Immediate"),("DELAYED","Delayed"),("CRON","CRON")]
                    selected=trigger_filter set_selected=set_trigger_filter />
                <MultiSelectFilter label="Endpoint Type"
                    options=vec![("HTTP","HTTP"),("KAFKA","Kafka"),("REDIS_STREAM","Redis Stream"),("INTERNAL","Internal")]
                    selected=endpoint_type_filter set_selected=set_endpoint_type_filter />
                <div class="flex flex-col gap-1">
                    <label class="text-xs font-medium text-gray-500">"Endpoint"</label>
                    <input type="search" prop:value=move || endpoint_filter.get()
                        on:change=move |ev| set_endpoint_filter.set(event_target_value(&ev))
                        class="h-9 rounded-lg border border-gray-300 px-3 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                        placeholder="Search by endpoint name..." />
                </div>
                <DateRangeFilter
                    after=created_after set_after=set_created_after
                    before=created_before set_before=set_created_before />
```

Update the "Clear filters" button to reset all six signals:

```rust
                        on:click=move |_| {
                            set_status_filter.set(Vec::new());
                            set_trigger_filter.set(Vec::new());
                            set_endpoint_type_filter.set(Vec::new());
                            set_endpoint_filter.set(String::new());
                            set_created_after.set(None);
                            set_created_before.set(None);
                        }
```

- [ ] **Step 5: Switch Suspense → Transition**

Change the jobs-list wrapper (~line 1126) from `<Suspense ...>`/`</Suspense>` to `<Transition ...>`/`</Transition>` (fallback prop unchanged).

- [ ] **Step 6: Compile**

Run: `cargo check -p kronos-dashboard`
Expected: PASS. Fix any signal-type / move-closure errors surfaced.

- [ ] **Step 7: Commit**

```bash
git add crates/dashboard/src/pages/workspace_detail.rs crates/dashboard/src/api/models.rs crates/dashboard/src/api/client.rs
git commit -m "feat(dashboard): multi-select + date-range jobs filters; Transition to keep scroll on refetch"
```

---

## Phase 7 — Cancel UX

### Task 10: `ConfirmDialog` configurable confirm button

**Files:**
- Modify: `crates/dashboard/src/components/confirm.rs`

- [ ] **Step 1: Add optional props (defaults keep existing delete call-sites intact)**

```rust
#[component]
pub fn ConfirmDialog(
    #[prop(into)] title: String,
    #[prop(into)] message: String,
    open: ReadSignal<bool>,
    set_open: WriteSignal<bool>,
    on_confirm: Callback<()>,
    /// Confirm button text. Defaults to "Delete".
    #[prop(into, optional)] confirm_label: Option<String>,
    /// Dismiss button text. Defaults to "Cancel".
    #[prop(into, optional)] dismiss_label: Option<String>,
    /// When true, the confirm button is amber (cancel-style) instead of red.
    #[prop(optional)] amber: bool,
) -> impl IntoView {
    let confirm_label = confirm_label.unwrap_or_else(|| "Delete".to_string());
    let dismiss_label = dismiss_label.unwrap_or_else(|| "Cancel".to_string());
    let confirm_class = if amber {
        "px-4 py-2 bg-amber-600 text-white rounded-lg hover:bg-amber-700 text-sm font-medium transition-colors"
    } else {
        "px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 text-sm font-medium transition-colors"
    };
    view! {
        <div class="fixed inset-0 z-50 flex items-center justify-center"
            style=move || if open.get() { "" } else { "display:none" }>
            <div class="absolute inset-0 bg-black/50" on:click=move |_| set_open.set(false)></div>
            <div class="relative bg-white rounded-xl shadow-xl max-w-sm w-full mx-4 p-6">
                <h3 class="text-lg font-semibold text-gray-900">{title}</h3>
                <p class="mt-2 text-sm text-gray-600">{message}</p>
                <div class="mt-4 flex justify-end gap-3">
                    <button on:click=move |_| set_open.set(false)
                        class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors">
                        {dismiss_label}
                    </button>
                    <button on:click=move |_| { set_open.set(false); on_confirm.run(()); }
                        class=confirm_class>
                        {confirm_label}
                    </button>
                </div>
            </div>
        </div>
    }
}
```

- [ ] **Step 2: Compile (existing call-sites still valid — new props optional)**

Run: `cargo check -p kronos-dashboard`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/dashboard/src/components/confirm.rs
git commit -m "feat(dashboard): ConfirmDialog supports custom confirm/dismiss labels and amber variant"
```

### Task 11: Confirm + reposition the Cancel action in JobsTable

**Files:**
- Modify: `crates/dashboard/src/pages/workspace_detail.rs` (the `JobsTable` component)

- [ ] **Step 1: Add per-row confirm state and reorder the actions cell**

In `JobsTable`, the actions cell currently renders Cancel (immediate), Status, Versions inline. Replace the cell so that: Status + Versions sit together on the left; a divider; the destructive Cancel sits on the right and opens a confirm dialog. Only render Cancel for `ACTIVE` jobs.

```rust
        // per-row cancel confirmation
        let (confirm_open, set_confirm_open) = signal(false);
        let oid_cancel = org_id.clone();
        let wid_cancel = workspace_id.clone();
        let jid_cancel = job.job_id.clone();
        let is_active = job.status == "ACTIVE";

        let on_cancel_confirmed = Callback::new(move |_| {
            let oid = oid_cancel.clone();
            let wid = wid_cancel.clone();
            let jid = jid_cancel.clone();
            leptos::task::spawn_local(async move {
                let _ = api::cancel_job(oid, wid, jid).await;
                set_refresh.update(|r| *r += 1);
            });
        });
```

Actions cell markup:

```rust
            <td class="px-6 py-4">
                <div class="flex items-center justify-end gap-3">
                    <button on:click=/* toggle status panel */
                        class="text-blue-600 hover:text-blue-800 text-xs font-medium">"Status"</button>
                    <button on:click=/* toggle versions panel */
                        class="text-teal-600 hover:text-teal-800 text-xs font-medium">"Versions"</button>
                    <Show when=move || is_active>
                        <span class="text-gray-300">"|"</span>
                        <button on:click=move |_| set_confirm_open.set(true)
                            class="px-2 py-1 border border-red-300 text-red-600 hover:bg-red-50 rounded text-xs font-medium">
                            "Cancel"
                        </button>
                    </Show>
                </div>
            </td>
```

Add the dialog once per row (e.g. after the row, inside the same fragment):

```rust
            <ConfirmDialog
                title="Cancel job"
                message="Cancel this job? It will be retired and stop running. This cannot be undone."
                open=confirm_open
                set_open=set_confirm_open
                on_confirm=on_cancel_confirmed
                confirm_label="Cancel job"
                dismiss_label="Keep job"
                amber=true
            />
```

> Keep the existing inline Status/Versions panel toggling logic; only the Cancel button and its wrapper change. Preserve the existing signal names used to toggle those panels (do not rename them).

- [ ] **Step 2: Compile**

Run: `cargo check -p kronos-dashboard`
Expected: PASS. Resolve any `Clone`/move issues by cloning ids per closure as shown.

- [ ] **Step 3: Commit**

```bash
git add crates/dashboard/src/pages/workspace_detail.rs
git commit -m "feat(dashboard): confirm dialog and safer placement for job cancel"
```

---

## Phase 8 — Full build + smoke

### Task 12: Workspace build, lint, and manual smoke

- [ ] **Step 1: Full check + tests**

Run: `cargo check -p kronos-common -p kronos-api -p kronos-dashboard --tests`
Expected: PASS.
Run: `cargo test -p kronos-common -p kronos-api -p kronos-dashboard`
Expected: PASS.

- [ ] **Step 2: Clippy (no new warnings in changed files)**

Run: `cargo clippy -p kronos-common -p kronos-api -p kronos-dashboard --tests`
Expected: no new warnings in the files this plan touched (pre-existing warnings elsewhere are fine).

- [ ] **Step 3: Rebuild the WASM bundle + smoke test**

Run: `just dashboard-build-dev`
Then restart the server (`just dashboard`) and hard-refresh the browser.
Manually verify:
- Status/Trigger/Endpoint Type are multi-select popovers; selecting multiple ANDs across filters and ORs within a filter; "All" clears.
- Date-range popover: presets set the range; custom month-grid range works; clear works.
- Changing page size / filters / Next / Prev / Cancel does NOT scroll to top.
- Cancel shows the confirm dialog ("Cancel job" / "Keep job"); only on ACTIVE jobs; confirming retires the job and the row updates in place.

- [ ] **Step 4: Push branch + open PR (stacked on fix/jobs-list-pagination-filters)**

```bash
git push -u origin feat/jobs-filters-multiselect-daterange
gh pr create --base fix/jobs-list-pagination-filters --head feat/jobs-filters-multiselect-daterange \
  --title "feat(dashboard): multi-select + date-range jobs filters, cancel confirm, scroll fix" \
  --body-file <(echo "Implements docs/superpowers/specs/2026-06-29-jobs-filters-multiselect-daterange-design.md")
```

---

## Self-Review notes

- **Spec coverage:** A (Tasks 1-9), B (Task 9 step 5), C (Tasks 10-11), D (Tasks 7-9). Smithy (Task 5). ✓
- **Type consistency:** `JobFilters` list fields + `created_after/before` used identically across DB (Tasks 1-3), API (Task 4), and dashboard params (Task 9). `BindValue` defined in Task 1, used in Tasks 2-3. `MultiSelectFilter`/`DateRangeFilter` prop names match between definition (Tasks 6, 8) and call-site (Task 9). `ConfirmDialog` new props (Task 10) match the cancel call-site (Task 11). ✓
- **Known gaps to verify during execution:** exact Leptos 0.7 `window_event_listener` handle API; `cargo check` after each frontend task is the guard.
