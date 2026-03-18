use leptos::prelude::*;

#[component]
pub fn Modal(
    #[prop(into)] title: String,
    open: ReadSignal<bool>,
    set_open: WriteSignal<bool>,
    children: Children,
) -> impl IntoView {
    let children = children();
    view! {
        <div
            class="fixed inset-0 z-50 flex items-center justify-center"
            style=move || if open.get() { "" } else { "display:none" }
        >
            <div
                class="absolute inset-0 bg-black/50"
                on:click=move |_| set_open.set(false)
            ></div>
            <div class="relative bg-white rounded-xl shadow-xl max-w-lg w-full mx-4 p-6">
                <div class="flex items-center justify-between mb-4">
                    <h3 class="text-lg font-semibold">{title}</h3>
                    <button
                        on:click=move |_| set_open.set(false)
                        class="text-gray-400 hover:text-gray-600 transition-colors"
                    >
                        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
                        </svg>
                    </button>
                </div>
                {children}
            </div>
        </div>
    }
}
