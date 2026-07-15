use leptos::portal::Portal;
use leptos::prelude::*;

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
    // Portal to <body> so the overlay's `fixed` positioning is relative to the
    // viewport, not to a containing block established by an ancestor of the
    // caller (which otherwise leaves part of the screen uncovered by the backdrop).
    view! {
        <Portal>
            <div
                class="fixed inset-0 z-50 flex items-center justify-center"
                style=move || if open.get() { "" } else { "display:none" }
            >
                <div
                    class="absolute inset-0 bg-black/50"
                    on:click=move |_| set_open.set(false)
                ></div>
                <div class="relative bg-white rounded-xl shadow-xl max-w-sm w-full mx-4 p-6">
                    <h3 class="text-lg font-semibold text-gray-900">{title.clone()}</h3>
                    <p class="mt-2 text-sm text-gray-600">{message.clone()}</p>
                    <div class="mt-4 flex justify-end gap-3">
                        <button
                            on:click=move |_| set_open.set(false)
                            class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors"
                        >
                            {dismiss_label.clone()}
                        </button>
                        <button
                            on:click=move |_| {
                                set_open.set(false);
                                on_confirm.run(());
                            }
                            class=confirm_class
                        >
                            {confirm_label.clone()}
                        </button>
                    </div>
                </div>
            </div>
        </Portal>
    }
}
