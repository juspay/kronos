use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::app::prefixed;
use crate::api::{self, CreateWorkspace, Organization, UpdateOrganization, Workspace};
use crate::components::loading::{EmptyState, ErrorAlert, LoadingSpinner};
use crate::components::modal::Modal;
use crate::components::status_badge::StatusBadge;

#[component]
pub fn OrgDetailPage() -> impl IntoView {
    let params = use_params_map();
    let org_id = move || params.read().get("org_id").unwrap_or_default();

    let (refresh_counter, set_refresh_counter) = signal(0u32);
    let (modal_open, set_modal_open) = signal(false);
    let (edit_open, set_edit_open) = signal(false);

    let org = LocalResource::new(move || {
        let _ = refresh_counter.get();
        let id = org_id();
        api::get_organization(id)
    });

    let workspaces = LocalResource::new(move || {
        let _ = refresh_counter.get();
        let id = org_id();
        api::list_workspaces(id)
    });

    view! {
        <div class="space-y-6">
            // Breadcrumb
            <nav class="flex items-center gap-2 text-sm text-gray-500">
                <A href=prefixed("/") attr:class="hover:text-blue-600 transition-colors">"Organizations"</A>
                <ChevronRight />
                <Suspense fallback=move || view! { <span class="animate-pulse bg-gray-200 rounded w-24 h-4 inline-block"></span> }>
                    {move || org.get().map(|r| (*r).clone()).map(|r| {
                        match r {
                            Ok(o) => view! { <span class="text-gray-900 font-medium">{o.name.clone()}</span> }.into_any(),
                            Err(_) => view! { <span>"Unknown"</span> }.into_any(),
                        }
                    })}
                </Suspense>
            </nav>

            // Org header
            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || org.get().map(|r| (*r).clone()).map(|r| {
                    match r {
                        Ok(o) => {
                            view! { <OrgHeader org=o set_edit_open=set_edit_open /> }.into_any()
                        }
                        Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                    }
                })}
            </Suspense>

            // Workspaces section
            <div class="space-y-4">
                <div class="flex items-center justify-between">
                    <h2 class="text-lg font-semibold">"Workspaces"</h2>
                    <button
                        on:click=move |_| set_modal_open.set(true)
                        class="inline-flex items-center gap-2 px-3 py-1.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                    >
                        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"></path>
                        </svg>
                        "New Workspace"
                    </button>
                </div>

                <Suspense fallback=move || view! { <LoadingSpinner /> }>
                    {move || {
                        let oid = org_id();
                        workspaces.get().map(|r| (*r).clone()).map(move |result| {
                            match result {
                                Ok(wss) => {
                                    if wss.is_empty() {
                                        view! { <EmptyState message="No workspaces yet." /> }.into_any()
                                    } else {
                                        let oid = oid.clone();
                                        view! { <WorkspaceGrid org_id=oid workspaces=wss /> }.into_any()
                                    }
                                }
                                Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                            }
                        })
                    }}
                </Suspense>
            </div>

            <Modal
                title="Create Workspace"
                open=modal_open
                set_open=set_modal_open
            >
                <CreateWorkspaceForm org_id=org_id() set_modal_open=set_modal_open set_refresh=set_refresh_counter />
            </Modal>

            <Modal
                title="Edit Organization"
                open=edit_open
                set_open=set_edit_open
            >
                <EditOrgForm org_id=org_id() set_modal_open=set_edit_open set_refresh=set_refresh_counter />
            </Modal>
        </div>
    }
}

#[component]
fn ChevronRight() -> impl IntoView {
    view! {
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7"></path>
        </svg>
    }
}

#[component]
fn OrgHeader(org: Organization, set_edit_open: WriteSignal<bool>) -> impl IntoView {
    view! {
        <div class="bg-white rounded-xl border border-gray-200 p-6">
            <div class="flex items-center justify-between">
                <div>
                    <h1 class="text-2xl font-bold">{org.name}</h1>
                    <p class="text-sm text-gray-500 mt-1">"Slug: " <code class="bg-gray-100 px-1.5 py-0.5 rounded text-xs">{org.slug}</code></p>
                </div>
                <div class="flex items-center gap-3">
                    <button
                        on:click=move |_| set_edit_open.set(true)
                        class="px-3 py-1.5 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors"
                    >"Edit"</button>
                    <StatusBadge status=org.status />
                </div>
            </div>
        </div>
    }
}

#[component]
fn EditOrgForm(
    org_id: String,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let name_val = name.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let body = UpdateOrganization { name: name_val };
            match api::update_organization(oid, body).await {
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
                <input type="text" required=true prop:value=move || name.get()
                    on:input=move |ev| set_name.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    placeholder="New organization name" />
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="button" on:click=move |_| set_modal_open.set(false)
                    class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors">"Cancel"</button>
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Saving..." } else { "Save" }}
                </button>
            </div>
        </form>
    }
}

#[component]
fn WorkspaceGrid(org_id: String, workspaces: Vec<Workspace>) -> impl IntoView {
    view! {
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {workspaces.into_iter().map(|ws| {
                let href = prefixed(&format!("/orgs/{}/workspaces/{}", org_id, ws.workspace_id));
                view! {
                    <A href=href attr:class="block bg-white rounded-xl border border-gray-200 p-5 hover:shadow-md hover:border-blue-300 transition-all">
                        <div class="flex items-start justify-between">
                            <div>
                                <h3 class="font-semibold text-gray-900">{ws.name.clone()}</h3>
                                <p class="text-sm text-gray-500 mt-0.5">{ws.slug.clone()}</p>
                            </div>
                            <StatusBadge status=ws.status.clone() />
                        </div>
                        <div class="mt-3 flex items-center gap-4 text-xs text-gray-400">
                            <span>"Schema: " <code class="bg-gray-100 px-1 rounded">{ws.schema_name.clone()}</code></span>
                        </div>
                        <div class="mt-2 text-xs text-gray-400">
                            "Created: " {format_date(&ws.created_at)}
                        </div>
                    </A>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn CreateWorkspaceForm(
    org_id: String,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (slug, set_slug) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let org_id = org_id.clone();
        let name_val = name.get_untracked();
        let slug_val = slug.get_untracked();

        set_submitting.set(true);
        set_error.set(None);

        leptos::task::spawn_local(async move {
            let body = CreateWorkspace {
                name: name_val,
                slug: slug_val,
            };
            match api::create_workspace(org_id, body).await {
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
                    placeholder="Production"
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
                    placeholder="production"
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
