use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};

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

/// Default start-/end-of-day times (00:00:00 / 23:59:59 UTC).
fn default_start_time() -> NaiveTime {
    NaiveTime::from_hms_opt(0, 0, 0).unwrap()
}
fn default_end_time() -> NaiveTime {
    NaiveTime::from_hms_opt(23, 59, 59).unwrap()
}

/// Days of `month` in a 7-col grid: leading `None` pad for the weekday offset (Sunday=0).
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

/// Month-grid range picker with presets; writes inclusive [after, before] UTC
/// bounds (empty = no filter).
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
    // Hovered day while picking a range end; drives the live preview highlight.
    let (hover_day, set_hover_day) = signal(Option::<NaiveDate>::None);
    // UTC time-of-day for the start/end dates (defaults = full day).
    let (start_time, set_start_time) = signal(default_start_time());
    let (end_time, set_end_time) = signal(default_end_time());
    let today = Utc::now().date_naive();
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

    // Trigger is content-sized: "Created" when empty, else the selected date + time.
    let button_text = move || match (after.get(), before.get()) {
        (Some(a), Some(b)) => {
            format!("{} \u{2013} {}", a.format("%b %d %H:%M:%S"), b.format("%b %d %H:%M:%S"))
        }
        (Some(a), None) => format!("From {}", a.format("%b %d %H:%M:%S")),
        (None, Some(b)) => format!("Until {}", b.format("%b %d %H:%M:%S")),
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

    // First click sets `after` (clears `before`); second sets `before`, swapping if earlier.
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

    // Re-attach a new time-of-day to an already-picked date; handed to the time dropdowns.
    let apply_start = Callback::new(move |t: NaiveTime| {
        set_start_time.set(t);
        if let Some(a) = after.get() {
            set_after.set(Some(combine(a.date_naive(), t)));
        }
    });
    let apply_end = Callback::new(move |t: NaiveTime| {
        set_end_time.set(t);
        if let Some(b) = before.get() {
            set_before.set(Some(combine(b.date_naive(), t)));
        }
    });

    // Preset buttons built eagerly (avoids FnOnce in view!); highlight on match.
    let preset_buttons: Vec<_> = Preset::ALL
        .iter()
        .copied()
        .map(|p| {
            let cls = move || {
                let (a, b) = p.range(Utc::now());
                let active = after.get() == Some(a) && before.get() == Some(b);
                let base = "rounded-md px-3 py-1.5 text-sm text-left transition-colors ";
                if active {
                    format!("{base}bg-blue-50 text-blue-700 font-semibold")
                } else {
                    format!("{base}text-gray-600 hover:bg-gray-100 hover:text-gray-900")
                }
            };
            view! {
                <button type="button" on:click=move |_| apply_preset(p) class=cls>
                    {p.label()}
                </button>
            }
        })
        .collect();

    view! {
        <div node_ref=node_ref class="relative">
            <button type="button"
                on:click=move |_| set_open.update(|o| *o = !*o)
                class="flex h-9 items-center justify-between gap-2 rounded-lg border border-gray-300 bg-white px-3 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none">
                <span class="whitespace-nowrap" class:text-gray-400=move || !any()>{button_text}</span>
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
            // Popover: kept in DOM, toggled via display style (avoids FnOnce on <Show>).
            <div class="absolute left-0 top-full z-50 mt-2 flex flex-col rounded-xl border border-gray-200 bg-white shadow-xl ring-1 ring-gray-900/5 overflow-hidden"
                style=move || if open.get() { "" } else { "display:none" }>
                <div class="flex">
                // Presets column
                <div class="flex flex-col gap-0.5 border-r border-gray-100 bg-gray-50/50 p-3 w-40">
                    <div class="px-3 pb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400">"Quick ranges"</div>
                    {preset_buttons}
                    <button type="button"
                        on:click=move |_| {
                            set_after.set(None);
                            set_before.set(None);
                            set_start_time.set(default_start_time());
                            set_end_time.set(default_end_time());
                        }
                        class="mt-1.5 rounded-md px-3 py-1.5 text-sm text-left text-gray-500 hover:bg-gray-100 hover:text-gray-900 transition-colors border-t border-gray-100">
                        "Clear"
                    </button>
                </div>
                // Calendar column
                <div class="p-4 w-80">
                    <div class="flex items-center justify-between mb-3">
                        <button type="button"
                            class="inline-flex h-7 w-7 items-center justify-center rounded-md text-gray-500 hover:bg-gray-100 hover:text-gray-900 transition-colors"
                            on:click=move |_| set_view_month.update(|m| *m = add_months(*m, -1))>
                            "\u{2039}"
                        </button>
                        <span class="text-sm font-semibold text-gray-900">
                            {move || view_month.get().format("%B %Y").to_string()}
                        </span>
                        <button type="button"
                            class="inline-flex h-7 w-7 items-center justify-center rounded-md text-gray-500 hover:bg-gray-100 hover:text-gray-900 transition-colors"
                            on:click=move |_| set_view_month.update(|m| *m = add_months(*m, 1))>
                            "\u{203a}"
                        </button>
                    </div>
                    <div class="grid grid-cols-7 gap-0.5 text-center text-[11px] font-medium text-gray-400 mb-1">
                        {["Su","Mo","Tu","We","Th","Fr","Sa"]
                            .iter()
                            .map(|d| view! { <span>{*d}</span> })
                            .collect_view()}
                    </div>
                    // No horizontal gap so the range fill forms one continuous band.
                    <div class="grid grid-cols-7 gap-y-1"
                        on:mouseleave=move |_| set_hover_day.set(None)>
                        {move || {
                            month_cells(view_month.get())
                                .into_iter()
                                .map(|cell| match cell {
                                    None => view! { <div></div> }.into_any(),
                                    Some(d) => {
                                        // Wrapper paints the range band; inner button
                                        // is the circular day target.
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
                                                // Today gets a subtle ring when not selected.
                                                DayHighlight::None if d == today => format!(
                                                    "{base}text-blue-700 font-semibold ring-1 ring-inset ring-blue-300 hover:bg-gray-100"
                                                ),
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
                    </div>
                </div>
                // FROM / TO band: label, date, and HH:MM:SS dropdowns per bound.
                <div class="border-t border-gray-100 bg-gray-50/60 px-4 py-3 space-y-2.5">
                    <div class="flex items-center gap-3">
                        <span class="w-10 shrink-0 text-[11px] font-semibold uppercase tracking-wide text-gray-400">"From"</span>
                        <span class="flex-1 min-w-0 truncate text-sm font-semibold text-gray-900">
                            {move || after.get()
                                .map(|a| a.format("%b %-d, %Y").to_string())
                                .unwrap_or_else(|| "Not set".to_string())}
                        </span>
                        <TimeSelect time=start_time on_change=apply_start />
                    </div>
                    <div class="flex items-center gap-3">
                        <span class="w-10 shrink-0 text-[11px] font-semibold uppercase tracking-wide text-gray-400">"To"</span>
                        <span class="flex-1 min-w-0 truncate text-sm font-semibold text-gray-900">
                            {move || before.get()
                                .map(|b| b.format("%b %-d, %Y").to_string())
                                .unwrap_or_else(|| "Not set".to_string())}
                        </span>
                        <TimeSelect time=end_time on_change=apply_end />
                    </div>
                    <div class="text-right text-[10px] text-gray-400">"All times in UTC"</div>
                </div>
            </div>
        </div>
    }
}

/// `0..end` as zero-padded `<option>`s. Selection is set via each option's
/// `selected` (a `<select>` `prop:value` doesn't apply before options render).
fn time_options(end: u32, current: impl Fn() -> u32 + Copy + Send + 'static) -> Vec<impl IntoView> {
    (0..end)
        .map(move |n| {
            view! {
                <option value=n.to_string() selected=move || current() == n>
                    {format!("{n:02}")}
                </option>
            }
        })
        .collect()
}

/// Styled HH:MM:SS dropdowns bound to a `NaiveTime`; emits the new time on change.
#[component]
fn TimeSelect(
    time: ReadSignal<NaiveTime>,
    #[prop(into)] on_change: Callback<NaiveTime>,
) -> impl IntoView {
    let sel = "rounded-md border border-gray-300 bg-white py-1 pl-1.5 pr-0.5 text-sm \
               text-gray-900 focus:ring-2 focus:ring-blue-500 focus:border-blue-500 \
               outline-none cursor-pointer";
    let on_h = move |ev: leptos::ev::Event| {
        if let Ok(h) = event_target_value(&ev).parse::<u32>() {
            let t = time.get_untracked();
            if let Some(nt) = NaiveTime::from_hms_opt(h, t.minute(), t.second()) {
                on_change.run(nt);
            }
        }
    };
    let on_m = move |ev: leptos::ev::Event| {
        if let Ok(m) = event_target_value(&ev).parse::<u32>() {
            let t = time.get_untracked();
            if let Some(nt) = NaiveTime::from_hms_opt(t.hour(), m, t.second()) {
                on_change.run(nt);
            }
        }
    };
    let on_s = move |ev: leptos::ev::Event| {
        if let Ok(s) = event_target_value(&ev).parse::<u32>() {
            let t = time.get_untracked();
            if let Some(nt) = NaiveTime::from_hms_opt(t.hour(), t.minute(), s) {
                on_change.run(nt);
            }
        }
    };
    view! {
        <div class="flex shrink-0 items-center gap-1">
            <select on:change=on_h class=sel>
                {time_options(24, move || time.get().hour())}
            </select>
            <span class="text-xs text-gray-400">":"</span>
            <select on:change=on_m class=sel>
                {time_options(60, move || time.get().minute())}
            </select>
            <span class="text-xs text-gray-400">":"</span>
            <select on:change=on_s class=sel>
                {time_options(60, move || time.get().second())}
            </select>
        </div>
    }
}

/// A day's position in the selection band: `Start`/`End` cap the ends, `Span`
/// fills between, `Only` is a lone day, `None` is outside.
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
