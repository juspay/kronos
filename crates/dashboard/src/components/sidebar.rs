use leptos::prelude::*;
use leptos_router::components::A;

use crate::app::prefixed;

#[component]
pub fn Sidebar() -> impl IntoView {
    let home = prefixed("/");
    view! {
        <aside class="w-64 bg-gray-900 text-white min-h-screen flex flex-col">
            <div class="p-6 border-b border-gray-700">
                <h1 class="text-xl font-bold tracking-tight">"Kronos"</h1>
                <p class="text-xs text-gray-400 mt-1">"Job Scheduling Engine"</p>
            </div>

            <nav class="flex-1 p-4 space-y-1">
                <SidebarSection title="Management">
                    <SidebarLink href=home label="Organizations" icon="M19 21V5a2 2 0 00-2-2H7a2 2 0 00-2 2v16m14 0h2m-2 0h-5m-9 0H3m2 0h5M9 7h1m-1 4h1m4-4h1m-1 4h1m-5 10v-5a1 1 0 011-1h2a1 1 0 011 1v5m-4 0h4" />
                </SidebarSection>
            </nav>

            <div class="p-4 border-t border-gray-700 text-xs text-gray-500">
                "v0.1.0"
            </div>
        </aside>
    }
}

#[component]
fn SidebarSection(title: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="mb-6">
            <h2 class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-2 px-3">
                {title}
            </h2>
            {children()}
        </div>
    }
}

#[component]
fn SidebarLink(href: String, label: &'static str, icon: &'static str) -> impl IntoView {
    view! {
        <A href=href attr:class="flex items-center gap-3 px-3 py-2 rounded-lg text-sm text-gray-300 hover:bg-gray-800 hover:text-white transition-colors">
            <svg class="w-5 h-5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d={icon}></path>
            </svg>
            <span>{label}</span>
        </A>
    }
}
