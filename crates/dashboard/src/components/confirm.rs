use leptos::prelude::*;

#[component]
pub fn ConfirmDialog(
    #[prop(into)] title: String,
    #[prop(into)] message: String,
    open: ReadSignal<bool>,
    set_open: WriteSignal<bool>,
    on_confirm: Callback<()>,
) -> impl IntoView {
    view! {
        <div
            class="fixed inset-0 z-50 flex items-center justify-center"
            style=move || if open.get() { "" } else { "display:none" }
        >
            <div
                class="absolute inset-0 bg-black/50"
                on:click=move |_| set_open.set(false)
            ></div>
            <div class="relative bg-white rounded-xl shadow-xl max-w-sm w-full mx-4 p-6">
                <h3 class="text-lg font-semibold text-gray-900">{title}</h3>
                <p class="mt-2 text-sm text-gray-600">{message}</p>
                <div class="mt-4 flex justify-end gap-3">
                    <button
                        on:click=move |_| set_open.set(false)
                        class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors"
                    >
                        "Cancel"
                    </button>
                    <button
                        on:click=move |_| {
                            set_open.set(false);
                            on_confirm.run(());
                        }
                        class="px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 text-sm font-medium transition-colors"
                    >
                        "Delete"
                    </button>
                </div>
            </div>
        </div>
    }
}
