use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};

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

/// Combines a calendar date with a UTC time-of-day into an instant.
pub(crate) fn combine(date: NaiveDate, time: NaiveTime) -> DateTime<Utc> {
    Utc.from_utc_datetime(&date.and_time(time))
}

/// Parses an `<input type="time">` value (`HH:MM` or `HH:MM:SS`) into a time.
pub(crate) fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M"))
        .ok()
}

/// Default start-/end-of-day times. Until the user edits the time fields the
/// picker behaves exactly as the date-only version did (00:00:00 / 23:59:59 UTC).
fn default_start_time() -> NaiveTime {
    NaiveTime::from_hms_opt(0, 0, 0).unwrap()
}
fn default_end_time() -> NaiveTime {
    NaiveTime::from_hms_opt(23, 59, 59).unwrap()
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
    // Day currently hovered while picking the end of a range — drives the live
    // preview highlight between the chosen start and the cursor.
    let (hover_day, set_hover_day) = signal(Option::<NaiveDate>::None);
    // UTC time-of-day applied to the start/end dates (defaults preserve the
    // original full-day behaviour).
    let (start_time, set_start_time) = signal(default_start_time());
    let (end_time, set_end_time) = signal(default_end_time());
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

    // Show the time alongside the date only when it differs from the full-day
    // default, so date-only selections stay compact.
    let button_text = move || match (after.get(), before.get()) {
        (Some(a), Some(b)) => {
            if a.time() != default_start_time() || b.time() != default_end_time() {
                format!("{} \u{2013} {}", a.format("%b %d %H:%M"), b.format("%b %d %H:%M"))
            } else {
                format!("{} \u{2013} {}", a.format("%b %d"), b.format("%b %d"))
            }
        }
        (Some(a), None) => format!("From {}", a.format("%b %d")),
        (None, Some(b)) => format!("Until {}", b.format("%b %d")),
        (None, None) => "Created".to_string(),
    };
    let any = move || after.get().is_some() || before.get().is_some();

    let apply_preset = move |p: Preset| {
        let (a, b) = p.range(Utc::now());
        // Presets are whole-day ranges — reset the time fields to match.
        set_start_time.set(default_start_time());
        set_end_time.set(default_end_time());
        set_after.set(Some(a));
        set_before.set(Some(b));
        set_open.set(false);
    };

    // Click a day: first click sets `after` at the start time and clears `before`;
    // second click sets `before` at the end time, swapping if it lands earlier.
    let pick_day = move |d: NaiveDate| {
        match (after.get(), before.get()) {
            // First click, or restarting after a complete range.
            (None, _) | (Some(_), Some(_)) => {
                set_after.set(Some(combine(d, start_time.get())));
                set_before.set(None);
            }
            // Second click: close the range, swapping if it lands before the start.
            (Some(start), None) => {
                if d < start.date_naive() {
                    set_after.set(Some(combine(d, start_time.get())));
                    set_before.set(Some(combine(start.date_naive(), end_time.get())));
                } else {
                    set_before.set(Some(combine(d, end_time.get())));
                }
            }
        }
    };

    // Time-field edits re-attach the new time to the already-picked date.
    let on_start_time = move |ev: leptos::ev::Event| {
        if let Some(t) = parse_time(&event_target_value(&ev)) {
            set_start_time.set(t);
            if let Some(a) = after.get() {
                set_after.set(Some(combine(a.date_naive(), t)));
            }
        }
    };
    let on_end_time = move |ev: leptos::ev::Event| {
        if let Some(t) = parse_time(&event_target_value(&ev)) {
            set_end_time.set(t);
            if let Some(b) = before.get() {
                set_before.set(Some(combine(b.date_naive(), t)));
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
                                set_start_time.set(default_start_time());
                                set_end_time.set(default_end_time());
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
                        on:click=move |_| {
                            set_after.set(None);
                            set_before.set(None);
                            set_start_time.set(default_start_time());
                            set_end_time.set(default_end_time());
                        }
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
                    // No horizontal gap so the range fill on adjacent cells
                    // touches into one continuous band; vertical gap separates weeks.
                    <div class="grid grid-cols-7 gap-y-1"
                        on:mouseleave=move |_| set_hover_day.set(None)>
                        {move || {
                            month_cells(view_month.get())
                                .into_iter()
                                .map(|cell| match cell {
                                    None => view! { <div></div> }.into_any(),
                                    Some(d) => {
                                        // Wrapper paints the continuous range band
                                        // (rounded only at the true ends); the inner
                                        // button is the circular day target with a
                                        // solid marker on each endpoint. One classifier
                                        // drives both the committed range and the live
                                        // hover preview.
                                        let band_class = move || {
                                            let base = "h-9 flex items-center justify-center ";
                                            match day_highlight(d, after.get(), before.get(), hover_day.get()) {
                                                DayHighlight::Start => format!("{base}bg-blue-100 rounded-l-full"),
                                                DayHighlight::End => format!("{base}bg-blue-100 rounded-r-full"),
                                                DayHighlight::Span => format!("{base}bg-blue-100"),
                                                DayHighlight::Only | DayHighlight::None => base.to_string(),
                                            }
                                        };
                                        let inner_class = move || {
                                            let base = "h-9 w-9 flex items-center justify-center text-sm rounded-full transition-colors ";
                                            match day_highlight(d, after.get(), before.get(), hover_day.get()) {
                                                DayHighlight::Start
                                                | DayHighlight::End
                                                | DayHighlight::Only => {
                                                    format!("{base}bg-blue-600 text-white font-semibold")
                                                }
                                                DayHighlight::Span => format!("{base}text-blue-900"),
                                                DayHighlight::None => format!("{base}text-gray-700 hover:bg-gray-100"),
                                            }
                                        };
                                        view! {
                                            <div class=band_class>
                                                <button type="button"
                                                    on:click=move |_| pick_day(d)
                                                    on:mouseenter=move |_| set_hover_day.set(Some(d))
                                                    class=inner_class>
                                                    {d.day().to_string()}
                                                </button>
                                            </div>
                                        }.into_any()
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                    // Time-of-day (UTC) for the start and end of the range.
                    <div class="mt-3 flex items-center justify-center gap-2 text-xs text-gray-600">
                        <input type="time" step="1"
                            prop:value=move || start_time.get().format("%H:%M:%S").to_string()
                            on:change=on_start_time
                            class="rounded border border-gray-300 px-2 py-1 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none" />
                        <span>"\u{2013}"</span>
                        <input type="time" step="1"
                            prop:value=move || end_time.get().format("%H:%M:%S").to_string()
                            on:change=on_end_time
                            class="rounded border border-gray-300 px-2 py-1 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none" />
                        <span class="text-gray-400 font-medium">"UTC"</span>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Where a calendar day sits relative to the selection, unifying the committed
/// range (`after`/`before`) and the live hover preview (`after`/`hover`). Drives
/// the continuous band: `Start`/`End` round the band's ends, `Span` fills
/// between, `Only` is a lone selected day, `None` is outside.
#[derive(Debug, PartialEq)]
pub(crate) enum DayHighlight {
    None,
    Only,
    Start,
    Span,
    End,
}

pub(crate) fn day_highlight(
    d: NaiveDate,
    after: Option<DateTime<Utc>>,
    before: Option<DateTime<Utc>>,
    hover: Option<NaiveDate>,
) -> DayHighlight {
    let start = match after {
        Some(a) => a.date_naive(),
        None => return DayHighlight::None,
    };
    // The end is the committed `before`, or — while picking — the hovered day.
    let end = match before.map(|b| b.date_naive()).or(hover) {
        Some(e) => e,
        // Start chosen, nothing hovered yet: only the start is marked.
        None => return if d == start { DayHighlight::Only } else { DayHighlight::None },
    };
    let (lo, hi) = if end >= start { (start, end) } else { (end, start) };
    if lo == hi {
        if d == lo { DayHighlight::Only } else { DayHighlight::None }
    } else if d == lo {
        DayHighlight::Start
    } else if d == hi {
        DayHighlight::End
    } else if d > lo && d < hi {
        DayHighlight::Span
    } else {
        DayHighlight::None
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

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
    }

    #[test]
    fn combine_attaches_time_to_date_in_utc() {
        let dt = combine(d(10), NaiveTime::from_hms_opt(9, 30, 15).unwrap());
        assert_eq!(dt.to_rfc3339(), "2026-06-10T09:30:15+00:00");
    }

    #[test]
    fn parse_time_accepts_hm_and_hms_rejects_garbage() {
        assert_eq!(parse_time("09:30"), NaiveTime::from_hms_opt(9, 30, 0));
        assert_eq!(parse_time("23:59:59"), NaiveTime::from_hms_opt(23, 59, 59));
        assert_eq!(parse_time("not-a-time"), None);
    }

    #[test]
    fn day_highlight_marks_committed_range_ends_and_span() {
        let a = Some(start_of(d(10)));
        let b = Some(end_of(d(14)));
        assert_eq!(day_highlight(d(10), a, b, None), DayHighlight::Start);
        assert_eq!(day_highlight(d(14), a, b, None), DayHighlight::End);
        assert_eq!(day_highlight(d(12), a, b, None), DayHighlight::Span);
        assert_eq!(day_highlight(d(9), a, b, None), DayHighlight::None);
        assert_eq!(day_highlight(d(15), a, b, None), DayHighlight::None);
    }

    #[test]
    fn day_highlight_previews_hover_during_selection() {
        let a = Some(start_of(d(10)));
        // Start chosen, hovering the 13th: 10=Start, 13=End, 11-12=Span.
        assert_eq!(day_highlight(d(10), a, None, Some(d(13))), DayHighlight::Start);
        assert_eq!(day_highlight(d(13), a, None, Some(d(13))), DayHighlight::End);
        assert_eq!(day_highlight(d(12), a, None, Some(d(13))), DayHighlight::Span);
        assert_eq!(day_highlight(d(14), a, None, Some(d(13))), DayHighlight::None);
    }

    #[test]
    fn day_highlight_preview_handles_hover_before_start() {
        let a = Some(start_of(d(10)));
        // Hovering the 7th (before start 10): band runs 7..10.
        assert_eq!(day_highlight(d(7), a, None, Some(d(7))), DayHighlight::Start);
        assert_eq!(day_highlight(d(10), a, None, Some(d(7))), DayHighlight::End);
        assert_eq!(day_highlight(d(8), a, None, Some(d(7))), DayHighlight::Span);
    }

    #[test]
    fn day_highlight_start_only_when_no_hover_or_single_day() {
        let a = Some(start_of(d(10)));
        assert_eq!(day_highlight(d(10), a, None, None), DayHighlight::Only);
        assert_eq!(day_highlight(d(11), a, None, None), DayHighlight::None);
        // Hovering the start day itself collapses to a single-day selection.
        assert_eq!(day_highlight(d(10), a, None, Some(d(10))), DayHighlight::Only);
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
