use leptos::prelude::*;
use leptos_router::components::A;

use crate::app::prefixed;
use crate::api::{self, CreateOrganization, Organization};
use crate::components::loading::{EmptyState, ErrorAlert, LoadingSpinner};
use crate::components::modal::Modal;
use crate::components::status_badge::StatusBadge;

#[component]
pub fn OrganizationsPage() -> impl IntoView {
    let (refresh_counter, set_refresh_counter) = signal(0u32);
    let (modal_open, set_modal_open) = signal(false);

    let orgs = LocalResource::new(move || {
        let _ = refresh_counter.get();
        api::list_organizations()
    });

    view! {
        <div class="space-y-6">
            <div class="flex items-center justify-between">
                <div>
                    <h1 class="text-2xl font-bold">"Organizations"</h1>
                    <p class="text-sm text-gray-500 mt-1">"Manage your organizations and their workspaces"</p>
                </div>
                <button
                    on:click=move |_| set_modal_open.set(true)
                    class="inline-flex items-center gap-2 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"></path>
                    </svg>
                    "New Organization"
                </button>
            </div>

            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || {
                    orgs.get().map(|r| (*r).clone()).map(|result| {
                        match result {
                            Ok(orgs) => {
                                if orgs.is_empty() {
                                    view! {
                                        <EmptyState message="No organizations yet. Create one to get started." />
                                    }.into_any()
                                } else {
                                    view! { <OrgGrid orgs=orgs /> }.into_any()
                                }
                            }
                            Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                        }
                    })
                }}
            </Suspense>

            <Modal
                title="Create Organization"
                open=modal_open
                set_open=set_modal_open
            >
                <CreateOrgForm set_modal_open=set_modal_open set_refresh=set_refresh_counter />
            </Modal>
        </div>
    }
}

#[component]
fn OrgGrid(orgs: Vec<Organization>) -> impl IntoView {
    view! {
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {orgs.into_iter().map(|org| {
                let href = prefixed(&format!("/orgs/{}", org.org_id));
                view! {
                    <A href=href attr:class="block bg-white rounded-xl border border-gray-200 p-5 hover:shadow-md hover:border-blue-300 transition-all">
                        <div class="flex items-start justify-between">
                            <div>
                                <h3 class="font-semibold text-gray-900">{org.name.clone()}</h3>
                                <p class="text-sm text-gray-500 mt-0.5">{org.slug.clone()}</p>
                            </div>
                            <StatusBadge status=org.status.clone() />
                        </div>
                        <div class="mt-4 text-xs text-gray-400">
                            "Created: " {format_date(&org.created_at)}
                        </div>
                    </A>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn CreateOrgForm(
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (slug, set_slug) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let name_val = name.get_untracked();
        let slug_val = slug.get_untracked();

        set_submitting.set(true);
        set_error.set(None);

        leptos::task::spawn_local(async move {
            let body = CreateOrganization {
                name: name_val,
                slug: slug_val,
            };
            match api::create_organization(body).await {
                Ok(_) => {
                    set_modal_open.set(false);
                    set_refresh.update(|c| *c += 1);
                }
                Err(e) => set_error.set(Some(e.to_string())),
            }
            set_submitting.set(false);
        });
    };

    view! {
        <form on:submit=on_submit class="space-y-4">
            <Show when=move || error.get().is_some()>
                <ErrorAlert message=error.get().unwrap_or_default() />
            </Show>

            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Name"</label>
                <input
                    type="text"
                    required=true
                    prop:value=move || name.get()
                    on:input=move |ev| {
                        let val = event_target_value(&ev);
                        let prev_slug_matches = slug.get_untracked().is_empty() || auto_slug(&name.get_untracked()) == slug.get_untracked();
                        set_name.set(val.clone());
                        if prev_slug_matches {
                            set_slug.set(auto_slug(&val));
                        }
                    }
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    placeholder="My Organization"
                />
            </div>

            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Slug"</label>
                <input
                    type="text"
                    required=true
                    prop:value=move || slug.get()
                    on:input=move |ev| set_slug.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    placeholder="my-org"
                />
            </div>

            <div class="flex justify-end gap-3 pt-2">
                <button
                    type="submit"
                    disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors"
                >
                    {move || if submitting.get() { "Creating..." } else { "Create" }}
                </button>
            </div>
        </form>
    }
}

fn auto_slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn format_date(s: &str) -> String {
    if s.len() >= 10 { s[..10].to_string() } else { s.to_string() }
}
