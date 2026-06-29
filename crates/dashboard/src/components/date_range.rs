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

pub(crate) fn start_of(d: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
}
pub(crate) fn end_of(d: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&d.and_hms_opt(23, 59, 59).unwrap())
}
pub(crate) fn first_of_month(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap()
}

/// Days of `month` laid out in a 7-col grid: leading `None` pad for the weekday
/// offset (Sunday=0), then each day of the month.
pub(crate) fn month_cells(month: NaiveDate) -> Vec<Option<NaiveDate>> {
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
pub(crate) fn add_months(d: NaiveDate, delta: i32) -> NaiveDate {
    let zero = d.year() * 12 + (d.month() as i32 - 1) + delta;
    let year = zero.div_euclid(12);
    let month = zero.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1).unwrap()
}

// ─── View component ──────────────────────────────────────────────────────────

use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Month-grid range picker with preset shortcuts. Writes inclusive [after,
/// before] UTC bounds; empty = no filter. Uses `Utc::now()` (wasmbind-enabled)
/// to seed the view month on open and to apply presets.
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

    // Close on outside click.
    let handle = window_event_listener(leptos::ev::mousedown, move |ev| {
        if !open.get_untracked() {
            return;
        }
        let inside = node_ref
            .get_untracked()
            .zip(ev.target())
            .and_then(|(el, t)| {
                t.dyn_into::<web_sys::Node>()
                    .ok()
                    .map(|n| el.contains(Some(&n)))
            })
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

    // Build preset buttons eagerly to avoid FnOnce move issues inside view!
    let preset_buttons: Vec<_> = Preset::ALL
        .iter()
        .copied()
        .map(|p| {
            view! {
                <button type="button"
                    on:click=move |_| apply_preset(p)
                    class="rounded px-2 py-1.5 text-sm text-left hover:bg-gray-50">
                    {p.label()}
                </button>
            }
        })
        .collect();

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
                            on:click=move |ev| {
                                ev.stop_propagation();
                                set_after.set(None);
                                set_before.set(None);
                            }
                            class="rounded p-0.5 hover:bg-gray-100">"\u{2715}"</span>
                    </Show>
                    <span class="text-gray-400">"\u{25be}"</span>
                </span>
            </button>
            // Popover panel — always in DOM, toggled via display style (avoids
            // FnOnce constraint on <Show> children from moved Vecs).
            <div class="absolute left-0 top-full z-50 mt-1 flex rounded-lg border border-gray-200 bg-white shadow-lg"
                style=move || if open.get() { "" } else { "display:none" }>
                // Presets column
                <div class="flex flex-col gap-0.5 border-r border-gray-100 p-2 w-36">
                    {preset_buttons}
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
                            on:click=move |_| set_view_month.update(|m| *m = add_months(*m, -1))>
                            "\u{2039}"
                        </button>
                        <span class="text-sm font-medium">
                            {move || view_month.get().format("%B %Y").to_string()}
                        </span>
                        <button type="button" class="px-2 rounded hover:bg-gray-100"
                            on:click=move |_| set_view_month.update(|m| *m = add_months(*m, 1))>
                            "\u{203a}"
                        </button>
                    </div>
                    <div class="grid grid-cols-7 gap-0.5 text-center text-[10px] text-gray-400 mb-1">
                        {["Su","Mo","Tu","We","Th","Fr","Sa"]
                            .iter()
                            .map(|d| view! { <span>{*d}</span> })
                            .collect_view()}
                    </div>
                    <div class="grid grid-cols-7 gap-0.5">
                        {move || {
                            month_cells(view_month.get())
                                .into_iter()
                                .map(|cell| match cell {
                                    None => view! { <span></span> }.into_any(),
                                    Some(d) => {
                                        let in_range = move || {
                                            let lo = after.get();
                                            let hi = before.get();
                                            match (lo, hi) {
                                                (Some(a), Some(b)) => {
                                                    start_of(d) >= a && end_of(d) <= b
                                                }
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
                                })
                                .collect_view()
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        // Wed 2026-06-24 10:30:00 UTC
        Utc.from_utc_datetime(
            &NaiveDate::from_ymd_opt(2026, 6, 24)
                .unwrap()
                .and_hms_opt(10, 30, 0)
                .unwrap(),
        )
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

    #[test]
    fn add_months_wraps_year() {
        assert_eq!(
            add_months(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), -1),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
        assert_eq!(
            add_months(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(), 1),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }

    #[test]
    fn month_cells_pads_and_counts() {
        // June 2026: 1st is a Monday -> 1 leading pad; 30 days.
        let cells = month_cells(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
        assert_eq!(cells[0], None);
        assert_eq!(cells[1], NaiveDate::from_ymd_opt(2026, 6, 1));
        assert_eq!(cells.iter().filter(|c| c.is_some()).count(), 30);
    }
}
