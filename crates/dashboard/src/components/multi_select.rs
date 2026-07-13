use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Multi-select dropdown: empty = no filter, "All" clears, closes on outside click.
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
            .and_then(|(el, target)| {
                target
                    .dyn_into::<web_sys::Node>()
                    .ok()
                    .map(|n| el.contains(Some(&n)))
            })
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

    // Build option buttons eagerly; options is &'static so the closures are Fn.
    let option_buttons: Vec<_> = options
        .into_iter()
        .map(|(value, opt_label)| {
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
        })
        .collect();

    view! {
        <div node_ref=node_ref class="relative flex flex-col w-52">
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
            // Popover: kept in DOM, toggled via display style (avoids FnOnce on <Show>).
            <div class="absolute left-0 top-full z-50 mt-1 w-full min-w-[200px] rounded-lg border border-gray-200 bg-white p-1 shadow-lg"
                style=move || if open.get() { "" } else { "display:none" }>
                <button type="button"
                    on:click=move |_| { set_selected.set(Vec::new()); set_open.set(false); }
                    class="flex w-full items-center justify-between rounded px-2 py-1.5 text-sm text-left border-b border-gray-100 mb-1 hover:bg-gray-50"
                    class:font-medium=move || !any_selected()>
                    <span>{format!("All {label}")}</span>
                    <Show when=move || !any_selected()><span class="text-blue-600">"\u{2713}"</span></Show>
                </button>
                {option_buttons}
            </div>
        </div>
    }
}
