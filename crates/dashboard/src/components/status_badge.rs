use leptos::prelude::*;

#[component]
pub fn StatusBadge(#[prop(into)] status: String) -> impl IntoView {
    let (bg, text) = match status.as_str() {
        "ACTIVE" | "active" | "SUCCESS" => ("bg-green-100 text-green-800", "bg-green-400"),
        "RETIRED" | "CANCELLED" | "FAILED" => ("bg-red-100 text-red-800", "bg-red-400"),
        "PENDING" | "QUEUED" => ("bg-yellow-100 text-yellow-800", "bg-yellow-400"),
        "RUNNING" | "RETRYING" => ("bg-blue-100 text-blue-800", "bg-blue-400"),
        _ => ("bg-gray-100 text-gray-800", "bg-gray-400"),
    };

    let status_clone = status.clone();
    view! {
        <span class={format!("inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium {bg}")}>
            <span class={format!("w-1.5 h-1.5 rounded-full {text}")}></span>
            {status_clone}
        </span>
    }
}
