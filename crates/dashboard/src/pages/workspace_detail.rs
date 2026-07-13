use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::api::{
    self, Config, CreateConfig, CreateEndpoint, CreatePayloadSpec, CreateSecret, Endpoint,
    Execution, Job, JobListQueryParams, PayloadSpec, UpdateConfig, UpdatePayloadSpec, UpdateSecret,
};
use crate::app::prefixed;
use crate::components::confirm::ConfirmDialog;
use crate::components::copyable_id::CopyableId;
use crate::components::date_range::DateRangeFilter;
use crate::components::loading::{EmptyState, ErrorAlert, LoadingSpinner};
use crate::components::modal::Modal;
use crate::components::multi_select::MultiSelectFilter;
use crate::components::status_badge::StatusBadge;

#[component]
pub fn WorkspaceDetailPage() -> impl IntoView {
    let params = use_params_map();
    let org_id = move || params.read().get("org_id").unwrap_or_default();
    let workspace_id = move || params.read().get("workspace_id").unwrap_or_default();

    let (active_tab, set_active_tab) = signal("jobs".to_string());

    let workspace = LocalResource::new(move || {
        let oid = org_id();
        let wid = workspace_id();
        async move {
            let workspaces = api::list_workspaces(oid).await?;
            workspaces
                .into_iter()
                .find(|w| w.workspace_id == wid)
                .ok_or_else(|| "Workspace not found".to_string())
        }
    });

    view! {
        <div class="space-y-6">
            // Breadcrumb
            <nav class="flex items-center gap-2 text-sm text-gray-500">
                <A href=prefixed("/") attr:class="hover:text-blue-600 transition-colors">"Organizations"</A>
                <ChevronRight />
                <A href={let oid = org_id(); prefixed(&format!("/orgs/{oid}"))} attr:class="hover:text-blue-600 transition-colors">
                    {org_id()}
                </A>
                <ChevronRight />
                <Suspense fallback=move || view! { <span class="animate-pulse bg-gray-200 rounded w-24 h-4 inline-block"></span> }>
                    {move || workspace.get().map(|r| (*r).clone()).map(|result| {
                        match result {
                            Ok(w) => view! { <span class="text-gray-900 font-medium">{w.name.clone()}</span> }.into_any(),
                            Err(_) => view! { <span>"Unknown"</span> }.into_any(),
                        }
                    })}
                </Suspense>
            </nav>

            // Workspace header
            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || workspace.get().map(|r| (*r).clone()).map(|result| {
                    match result {
                        Ok(w) => view! {
                            <div class="bg-white rounded-xl border border-gray-200 p-6">
                                <div class="flex items-center justify-between">
                                    <div>
                                        <h1 class="text-2xl font-bold">{w.name.clone()}</h1>
                                        <div class="flex items-center gap-4 mt-2 text-sm text-gray-500">
                                            <span>"Schema: " <code class="bg-gray-100 px-1.5 py-0.5 rounded text-xs">{w.schema_name.clone()}</code></span>
                                            <span>"Version: " {w.schema_version}</span>
                                        </div>
                                    </div>
                                    <StatusBadge status=w.status.clone() />
                                </div>
                            </div>
                        }.into_any(),
                        Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                    }
                })}
            </Suspense>

            // Tabs
            <div class="border-b border-gray-200">
                <nav class="flex gap-6">
                    <TabButton label="Jobs" tab="jobs" active_tab=active_tab set_active_tab=set_active_tab />
                    <TabButton label="Endpoints" tab="endpoints" active_tab=active_tab set_active_tab=set_active_tab />
                    <TabButton label="Payload Specs" tab="payload_specs" active_tab=active_tab set_active_tab=set_active_tab />
                    <TabButton label="Configs" tab="configs" active_tab=active_tab set_active_tab=set_active_tab />
                    <TabButton label="Secrets" tab="secrets" active_tab=active_tab set_active_tab=set_active_tab />
                </nav>
            </div>

            // Tab content
            {move || {
                let oid = org_id();
                let wid = workspace_id();
                let tab = active_tab.get();
                match tab.as_str() {
                    "jobs" => view! { <JobsTab org_id=oid workspace_id=wid /> }.into_any(),
                    "endpoints" => view! { <EndpointsTab org_id=oid workspace_id=wid /> }.into_any(),
                    "payload_specs" => view! { <PayloadSpecsTab org_id=oid workspace_id=wid /> }.into_any(),
                    "configs" => view! { <ConfigsTab org_id=oid workspace_id=wid /> }.into_any(),
                    "secrets" => view! { <SecretsTab org_id=oid workspace_id=wid /> }.into_any(),
                    _ => view! { <div></div> }.into_any(),
                }
            }}
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
fn TabButton(
    label: &'static str,
    tab: &'static str,
    active_tab: ReadSignal<String>,
    set_active_tab: WriteSignal<String>,
) -> impl IntoView {
    let is_active = move || active_tab.get() == tab;
    view! {
        <button
            on:click=move |_| set_active_tab.set(tab.to_string())
            class=move || {
                if is_active() {
                    "px-1 py-3 text-sm font-medium text-blue-600 border-b-2 border-blue-600 -mb-px"
                } else {
                    "px-1 py-3 text-sm font-medium text-gray-500 hover:text-gray-700 border-b-2 border-transparent -mb-px"
                }
            }
        >
            {label}
        </button>
    }
}

// ════════════════════════════════════════════════════════════
// Payload Specs Tab
// ════════════════════════════════════════════════════════════

#[component]
fn PayloadSpecsTab(org_id: String, workspace_id: String) -> impl IntoView {
    let (refresh, set_refresh) = signal(0u32);
    let (create_open, set_create_open) = signal(false);
    let (edit_open, set_edit_open) = signal(false);
    let (editing_spec, set_editing_spec) = signal(Option::<PayloadSpec>::None);
    let (confirm_open, set_confirm_open) = signal(false);
    let (deleting_name, set_deleting_name) = signal(Option::<String>::None);
    let (delete_error, set_delete_error) = signal(Option::<String>::None);

    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let specs = LocalResource::new(move || {
        let _ = refresh.get();
        let oid = oid.clone();
        let wid = wid.clone();
        api::list_payload_specs(oid, wid)
    });

    let oid_create = org_id.clone();
    let wid_create = workspace_id.clone();
    let oid_edit = org_id.clone();
    let wid_edit = workspace_id.clone();
    let oid_del = org_id.clone();
    let wid_del = workspace_id.clone();

    let on_confirm_delete = Callback::new(move |_: ()| {
        let name = deleting_name.get_untracked();
        if let Some(name) = name {
            let oid = oid_del.clone();
            let wid = wid_del.clone();
            set_delete_error.set(None);
            leptos::task::spawn_local(async move {
                match api::delete_payload_spec(oid, wid, name).await {
                    Ok(_) => set_refresh.update(|c| *c += 1),
                    Err(e) => set_delete_error.set(Some(e.to_string())),
                }
            });
        }
    });

    view! {
        <div class="space-y-4">
            <div class="flex justify-end">
                <button
                    on:click=move |_| set_create_open.set(true)
                    class="inline-flex items-center gap-2 px-3 py-1.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                >
                    <PlusIcon />
                    "New Payload Spec"
                </button>
            </div>

            <Show when=move || delete_error.get().is_some()>
                <ErrorAlert message=delete_error.get().unwrap_or_default() />
            </Show>

            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || specs.get().map(|r| (*r).clone()).map(|result| {
                    match result {
                        Ok(items) => {
                            if items.is_empty() {
                                view! { <EmptyState message="No payload specs yet." /> }.into_any()
                            } else {
                                let items = items.clone();
                                view! {
                                    <div class="bg-white rounded-xl border border-gray-200 overflow-hidden">
                                        <table class="min-w-full divide-y divide-gray-200">
                                            <thead class="bg-gray-50">
                                                <tr>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Name"</th>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Schema"</th>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Updated"</th>
                                                    <th class="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase">"Actions"</th>
                                                </tr>
                                            </thead>
                                            <tbody class="divide-y divide-gray-200">
                                                {items.into_iter().map(|spec| {
                                                    let spec_edit = spec.clone();
                                                    let spec_name_del = spec.name.clone();
                                                    let schema_str = serde_json::to_string(&spec.schema).unwrap_or_default();
                                                    let schema_short = if schema_str.len() > 60 {
                                                        format!("{}...", &schema_str[..60])
                                                    } else {
                                                        schema_str
                                                    };
                                                    view! {
                                                        <tr class="hover:bg-gray-50">
                                                            <td class="px-6 py-4 text-sm font-medium text-gray-900">{spec.name.clone()}</td>
                                                            <td class="px-6 py-4 text-xs font-mono text-gray-500 max-w-xs truncate">{schema_short}</td>
                                                            <td class="px-6 py-4 text-sm text-gray-500">{format_date(&spec.updated_at)}</td>
                                                            <td class="px-6 py-4 text-right">
                                                                <div class="flex items-center justify-end gap-2">
                                                                    <button
                                                                        on:click=move |_| {
                                                                            set_editing_spec.set(Some(spec_edit.clone()));
                                                                            set_edit_open.set(true);
                                                                        }
                                                                        class="text-blue-600 hover:text-blue-800 text-sm font-medium"
                                                                    >"Edit"</button>
                                                                    <button
                                                                        on:click=move |_| {
                                                                            set_deleting_name.set(Some(spec_name_del.clone()));
                                                                            set_confirm_open.set(true);
                                                                        }
                                                                        class="text-red-600 hover:text-red-800 text-sm font-medium"
                                                                    >"Delete"</button>
                                                                </div>
                                                            </td>
                                                        </tr>
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </tbody>
                                        </table>
                                    </div>
                                }.into_any()
                            }
                        }
                        Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                    }
                })}
            </Suspense>

            <Modal title="Create Payload Spec" open=create_open set_open=set_create_open>
                <CreatePayloadSpecForm org_id=oid_create workspace_id=wid_create set_modal_open=set_create_open set_refresh=set_refresh />
            </Modal>

            <Modal title="Edit Payload Spec" open=edit_open set_open=set_edit_open>
                <EditPayloadSpecForm org_id=oid_edit workspace_id=wid_edit editing_spec=editing_spec set_modal_open=set_edit_open set_refresh=set_refresh />
            </Modal>

            <ConfirmDialog
                title="Delete Payload Spec"
                message="Are you sure? This cannot be undone. Endpoints referencing this spec will be affected."
                open=confirm_open
                set_open=set_confirm_open
                on_confirm=on_confirm_delete
            />
        </div>
    }
}

#[component]
fn CreatePayloadSpecForm(
    org_id: String,
    workspace_id: String,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (schema_json, set_schema_json) =
        signal(r#"{"type": "object", "properties": {}}"#.to_string());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let name_val = name.get_untracked();
        let schema_val = schema_json.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let schema = match serde_json::from_str::<serde_json::Value>(&schema_val) {
                Ok(v) => v,
                Err(e) => {
                    set_error.set(Some(format!("Invalid JSON: {e}")));
                    set_submitting.set(false);
                    return;
                }
            };
            let body = CreatePayloadSpec {
                name: name_val,
                schema,
            };
            match api::create_payload_spec(oid, wid, body).await {
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
                    placeholder="my-payload-spec" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Schema (JSON)"</label>
                <textarea prop:value=move || schema_json.get()
                    on:input=move |ev| set_schema_json.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    rows="6"></textarea>
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Creating..." } else { "Create" }}
                </button>
            </div>
        </form>
    }
}

#[component]
fn EditPayloadSpecForm(
    org_id: String,
    workspace_id: String,
    editing_spec: ReadSignal<Option<PayloadSpec>>,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (schema_json, set_schema_json) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    Effect::new(move || {
        if let Some(spec) = editing_spec.get() {
            set_schema_json.set(serde_json::to_string_pretty(&spec.schema).unwrap_or_default());
            set_error.set(None);
        }
    });

    let spec_name = move || {
        editing_spec
            .get()
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let name = spec_name();
        let schema_val = schema_json.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let schema = match serde_json::from_str::<serde_json::Value>(&schema_val) {
                Ok(v) => v,
                Err(e) => {
                    set_error.set(Some(format!("Invalid JSON: {e}")));
                    set_submitting.set(false);
                    return;
                }
            };
            let body = UpdatePayloadSpec { schema };
            match api::update_payload_spec(oid, wid, name, body).await {
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
                <input type="text" disabled=true prop:value=move || spec_name()
                    class="w-full px-3 py-2 border border-gray-200 rounded-lg text-sm bg-gray-50 text-gray-500" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Schema (JSON)"</label>
                <textarea prop:value=move || schema_json.get()
                    on:input=move |ev| set_schema_json.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    rows="6"></textarea>
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="button" on:click=move |_| set_modal_open.set(false)
                    class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors">"Cancel"</button>
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Saving..." } else { "Save Changes" }}
                </button>
            </div>
        </form>
    }
}

// ════════════════════════════════════════════════════════════
// Configs Tab
// ════════════════════════════════════════════════════════════

#[component]
fn ConfigsTab(org_id: String, workspace_id: String) -> impl IntoView {
    let (refresh, set_refresh) = signal(0u32);
    let (create_open, set_create_open) = signal(false);
    let (edit_open, set_edit_open) = signal(false);
    let (editing_config, set_editing_config) = signal(Option::<Config>::None);
    let (confirm_open, set_confirm_open) = signal(false);
    let (deleting_name, set_deleting_name) = signal(Option::<String>::None);
    let (delete_error, set_delete_error) = signal(Option::<String>::None);

    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let configs = LocalResource::new(move || {
        let _ = refresh.get();
        let oid = oid.clone();
        let wid = wid.clone();
        api::list_configs(oid, wid)
    });

    let oid_create = org_id.clone();
    let wid_create = workspace_id.clone();
    let oid_edit = org_id.clone();
    let wid_edit = workspace_id.clone();
    let oid_del = org_id.clone();
    let wid_del = workspace_id.clone();

    let on_confirm_delete = Callback::new(move |_: ()| {
        let name = deleting_name.get_untracked();
        if let Some(name) = name {
            let oid = oid_del.clone();
            let wid = wid_del.clone();
            set_delete_error.set(None);
            leptos::task::spawn_local(async move {
                match api::delete_config(oid, wid, name).await {
                    Ok(_) => set_refresh.update(|c| *c += 1),
                    Err(e) => set_delete_error.set(Some(e.to_string())),
                }
            });
        }
    });

    view! {
        <div class="space-y-4">
            <div class="flex justify-end">
                <button on:click=move |_| set_create_open.set(true)
                    class="inline-flex items-center gap-2 px-3 py-1.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium">
                    <PlusIcon />
                    "New Config"
                </button>
            </div>

            <Show when=move || delete_error.get().is_some()>
                <ErrorAlert message=delete_error.get().unwrap_or_default() />
            </Show>

            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || configs.get().map(|r| (*r).clone()).map(|result| {
                    match result {
                        Ok(items) => {
                            if items.is_empty() {
                                view! { <EmptyState message="No configs yet." /> }.into_any()
                            } else {
                                let items = items.clone();
                                view! {
                                    <div class="bg-white rounded-xl border border-gray-200 overflow-hidden">
                                        <table class="min-w-full divide-y divide-gray-200">
                                            <thead class="bg-gray-50">
                                                <tr>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Name"</th>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Values"</th>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Updated"</th>
                                                    <th class="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase">"Actions"</th>
                                                </tr>
                                            </thead>
                                            <tbody class="divide-y divide-gray-200">
                                                {items.into_iter().map(|cfg| {
                                                    let cfg_edit = cfg.clone();
                                                    let cfg_name_del = cfg.name.clone();
                                                    let values_str = serde_json::to_string(&cfg.values).unwrap_or_default();
                                                    let values_short = if values_str.len() > 60 {
                                                        format!("{}...", &values_str[..60])
                                                    } else {
                                                        values_str
                                                    };
                                                    view! {
                                                        <tr class="hover:bg-gray-50">
                                                            <td class="px-6 py-4 text-sm font-medium text-gray-900">{cfg.name.clone()}</td>
                                                            <td class="px-6 py-4 text-xs font-mono text-gray-500 max-w-xs truncate">{values_short}</td>
                                                            <td class="px-6 py-4 text-sm text-gray-500">{format_date(&cfg.updated_at)}</td>
                                                            <td class="px-6 py-4 text-right">
                                                                <div class="flex items-center justify-end gap-2">
                                                                    <button on:click=move |_| {
                                                                        set_editing_config.set(Some(cfg_edit.clone()));
                                                                        set_edit_open.set(true);
                                                                    } class="text-blue-600 hover:text-blue-800 text-sm font-medium">"Edit"</button>
                                                                    <button on:click=move |_| {
                                                                        set_deleting_name.set(Some(cfg_name_del.clone()));
                                                                        set_confirm_open.set(true);
                                                                    } class="text-red-600 hover:text-red-800 text-sm font-medium">"Delete"</button>
                                                                </div>
                                                            </td>
                                                        </tr>
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </tbody>
                                        </table>
                                    </div>
                                }.into_any()
                            }
                        }
                        Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                    }
                })}
            </Suspense>

            <Modal title="Create Config" open=create_open set_open=set_create_open>
                <CreateConfigForm org_id=oid_create workspace_id=wid_create set_modal_open=set_create_open set_refresh=set_refresh />
            </Modal>

            <Modal title="Edit Config" open=edit_open set_open=set_edit_open>
                <EditConfigForm org_id=oid_edit workspace_id=wid_edit editing_config=editing_config set_modal_open=set_edit_open set_refresh=set_refresh />
            </Modal>

            <ConfirmDialog
                title="Delete Config"
                message="Are you sure? Endpoints referencing this config will be affected."
                open=confirm_open set_open=set_confirm_open on_confirm=on_confirm_delete
            />
        </div>
    }
}

#[component]
fn CreateConfigForm(
    org_id: String,
    workspace_id: String,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (values_json, set_values_json) = signal(r#"{"key": "value"}"#.to_string());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let name_val = name.get_untracked();
        let val = values_json.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let values = match serde_json::from_str::<serde_json::Value>(&val) {
                Ok(v) => v,
                Err(e) => {
                    set_error.set(Some(format!("Invalid JSON: {e}")));
                    set_submitting.set(false);
                    return;
                }
            };
            let body = CreateConfig {
                name: name_val,
                values,
            };
            match api::create_config(oid, wid, body).await {
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
                    placeholder="my-config" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Values (JSON)"</label>
                <textarea prop:value=move || values_json.get()
                    on:input=move |ev| set_values_json.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    rows="6"></textarea>
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Creating..." } else { "Create" }}
                </button>
            </div>
        </form>
    }
}

#[component]
fn EditConfigForm(
    org_id: String,
    workspace_id: String,
    editing_config: ReadSignal<Option<Config>>,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (values_json, set_values_json) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    Effect::new(move || {
        if let Some(cfg) = editing_config.get() {
            set_values_json.set(serde_json::to_string_pretty(&cfg.values).unwrap_or_default());
            set_error.set(None);
        }
    });

    let cfg_name = move || {
        editing_config
            .get()
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let name = cfg_name();
        let val = values_json.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let values = match serde_json::from_str::<serde_json::Value>(&val) {
                Ok(v) => v,
                Err(e) => {
                    set_error.set(Some(format!("Invalid JSON: {e}")));
                    set_submitting.set(false);
                    return;
                }
            };
            let body = UpdateConfig { values };
            match api::update_config(oid, wid, name, body).await {
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
                <input type="text" disabled=true prop:value=move || cfg_name()
                    class="w-full px-3 py-2 border border-gray-200 rounded-lg text-sm bg-gray-50 text-gray-500" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Values (JSON)"</label>
                <textarea prop:value=move || values_json.get()
                    on:input=move |ev| set_values_json.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    rows="6"></textarea>
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="button" on:click=move |_| set_modal_open.set(false)
                    class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors">"Cancel"</button>
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Saving..." } else { "Save Changes" }}
                </button>
            </div>
        </form>
    }
}

// ════════════════════════════════════════════════════════════
// Secrets Tab
// ════════════════════════════════════════════════════════════

#[component]
fn SecretsTab(org_id: String, workspace_id: String) -> impl IntoView {
    let (refresh, set_refresh) = signal(0u32);
    let (create_open, set_create_open) = signal(false);
    let (update_open, set_update_open) = signal(false);
    let (updating_name, set_updating_name) = signal(Option::<String>::None);
    let (confirm_open, set_confirm_open) = signal(false);
    let (deleting_name, set_deleting_name) = signal(Option::<String>::None);
    let (delete_error, set_delete_error) = signal(Option::<String>::None);

    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let secrets = LocalResource::new(move || {
        let _ = refresh.get();
        let oid = oid.clone();
        let wid = wid.clone();
        api::list_secrets(oid, wid)
    });

    let oid_create = org_id.clone();
    let wid_create = workspace_id.clone();
    let oid_update = org_id.clone();
    let wid_update = workspace_id.clone();
    let oid_del = org_id.clone();
    let wid_del = workspace_id.clone();

    let on_confirm_delete = Callback::new(move |_: ()| {
        let name = deleting_name.get_untracked();
        if let Some(name) = name {
            let oid = oid_del.clone();
            let wid = wid_del.clone();
            set_delete_error.set(None);
            leptos::task::spawn_local(async move {
                match api::delete_secret(oid, wid, name).await {
                    Ok(_) => set_refresh.update(|c| *c += 1),
                    Err(e) => set_delete_error.set(Some(e.to_string())),
                }
            });
        }
    });

    view! {
        <div class="space-y-4">
            <div class="flex justify-end">
                <button on:click=move |_| set_create_open.set(true)
                    class="inline-flex items-center gap-2 px-3 py-1.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium">
                    <PlusIcon />
                    "New Secret"
                </button>
            </div>

            <Show when=move || delete_error.get().is_some()>
                <ErrorAlert message=delete_error.get().unwrap_or_default() />
            </Show>

            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || secrets.get().map(|r| (*r).clone()).map(|result| {
                    match result {
                        Ok(items) => {
                            if items.is_empty() {
                                view! { <EmptyState message="No secrets yet." /> }.into_any()
                            } else {
                                let items = items.clone();
                                view! {
                                    <div class="bg-white rounded-xl border border-gray-200 overflow-hidden">
                                        <table class="min-w-full divide-y divide-gray-200">
                                            <thead class="bg-gray-50">
                                                <tr>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Name"</th>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Created"</th>
                                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Updated"</th>
                                                    <th class="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase">"Actions"</th>
                                                </tr>
                                            </thead>
                                            <tbody class="divide-y divide-gray-200">
                                                {items.into_iter().map(|secret| {
                                                    let name_update = secret.name.clone();
                                                    let name_del = secret.name.clone();
                                                    view! {
                                                        <tr class="hover:bg-gray-50">
                                                            <td class="px-6 py-4 text-sm font-medium text-gray-900">{secret.name.clone()}</td>
                                                            <td class="px-6 py-4 text-sm text-gray-500">{format_date(&secret.created_at)}</td>
                                                            <td class="px-6 py-4 text-sm text-gray-500">{format_date(&secret.updated_at)}</td>
                                                            <td class="px-6 py-4 text-right">
                                                                <div class="flex items-center justify-end gap-2">
                                                                    <button on:click=move |_| {
                                                                        set_updating_name.set(Some(name_update.clone()));
                                                                        set_update_open.set(true);
                                                                    } class="text-blue-600 hover:text-blue-800 text-sm font-medium">"Update"</button>
                                                                    <button on:click=move |_| {
                                                                        set_deleting_name.set(Some(name_del.clone()));
                                                                        set_confirm_open.set(true);
                                                                    } class="text-red-600 hover:text-red-800 text-sm font-medium">"Delete"</button>
                                                                </div>
                                                            </td>
                                                        </tr>
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </tbody>
                                        </table>
                                    </div>
                                }.into_any()
                            }
                        }
                        Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                    }
                })}
            </Suspense>

            <Modal title="Create Secret" open=create_open set_open=set_create_open>
                <CreateSecretForm org_id=oid_create workspace_id=wid_create set_modal_open=set_create_open set_refresh=set_refresh />
            </Modal>

            <Modal title="Update Secret" open=update_open set_open=set_update_open>
                <UpdateSecretForm org_id=oid_update workspace_id=wid_update updating_name=updating_name set_modal_open=set_update_open set_refresh=set_refresh />
            </Modal>

            <ConfirmDialog
                title="Delete Secret"
                message="Are you sure? This cannot be undone."
                open=confirm_open set_open=set_confirm_open on_confirm=on_confirm_delete
            />
        </div>
    }
}

#[component]
fn CreateSecretForm(
    org_id: String,
    workspace_id: String,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (value, set_value) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let n = name.get_untracked();
        let v = value.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let body = CreateSecret { name: n, value: v };
            match api::create_secret(oid, wid, body).await {
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
                    placeholder="MY_SECRET_KEY" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Value"</label>
                <input type="password" required=true prop:value=move || value.get()
                    on:input=move |ev| set_value.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    placeholder="secret-value" />
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Creating..." } else { "Create" }}
                </button>
            </div>
        </form>
    }
}

#[component]
fn UpdateSecretForm(
    org_id: String,
    workspace_id: String,
    updating_name: ReadSignal<Option<String>>,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (value, set_value) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    Effect::new(move || {
        if updating_name.get().is_some() {
            set_value.set(String::new());
            set_error.set(None);
        }
    });

    let secret_name = move || updating_name.get().unwrap_or_default();

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let name = secret_name();
        let v = value.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let body = UpdateSecret { value: v };
            match api::update_secret(oid, wid, name, body).await {
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
                <input type="text" disabled=true prop:value=move || secret_name()
                    class="w-full px-3 py-2 border border-gray-200 rounded-lg text-sm bg-gray-50 text-gray-500" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"New Value"</label>
                <input type="password" required=true prop:value=move || value.get()
                    on:input=move |ev| set_value.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    placeholder="new-secret-value" />
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="button" on:click=move |_| set_modal_open.set(false)
                    class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors">"Cancel"</button>
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Updating..." } else { "Update Secret" }}
                </button>
            </div>
        </form>
    }
}

// ════════════════════════════════════════════════════════════
// Jobs Tab (Enhanced)
// ════════════════════════════════════════════════════════════

/// Blank filter string → `None`, else `Some(trimmed)`.
fn filter_opt(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Snapshot of every jobs-list filter; detects filter changes so pagination
/// resets to page 1.
type JobFilterKey = (
    String,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    String,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
);

#[component]
fn JobsTab(org_id: String, workspace_id: String) -> impl IntoView {
    let (refresh, set_refresh) = signal(0u32);
    let (modal_open, set_modal_open) = signal(false);

    // Filters. Vec<String> fields are multi-select; empty means "All"/unset.
    // Changing any filter resets pagination via the Effect below.
    let (job_id_filter, set_job_id_filter) = signal(String::new());
    let (status_filter, set_status_filter) = signal(Vec::<String>::new());
    let (trigger_filter, set_trigger_filter) = signal(Vec::<String>::new());
    let (endpoint_type_filter, set_endpoint_type_filter) = signal(Vec::<String>::new());
    let (endpoint_filter, set_endpoint_filter) = signal(String::new());
    let (created_after, set_created_after) = signal(Option::<chrono::DateTime<chrono::Utc>>::None);
    let (created_before, set_created_before) =
        signal(Option::<chrono::DateTime<chrono::Utc>>::None);
    let (page_size, set_page_size) = signal(50i64);

    // Cursor pagination. The backend cursor is forward-only, so we keep the
    // cursor used for each visited page (`page_cursors[i]`) and an index into
    // it to support a Previous button. Page 1 uses cursor `None`.
    let (page_cursors, set_page_cursors) = signal(vec![Option::<String>::None]);
    let (page_index, set_page_index) = signal(0usize);
    // Scroll position captured when the query changes, restored after the new
    // list renders so applying a filter / paging doesn't jump the page to top.
    let (saved_scroll, set_saved_scroll) = signal(None::<f64>);

    // A snapshot of every active filter. The jobs resource compares the live
    // snapshot against the one the cursor stack was built for (`applied_key`) and
    // falls back to page 1 (cursor `None`) on any mismatch. This makes a filter
    // change immune to the reset effect's timing: even if the resource re-runs
    // with a stale `page_index` before the reset lands, the guard forces cursor
    // `None`, so it can never fetch a mid-list page for freshly changed filters.
    let filter_key = move || -> JobFilterKey {
        (
            job_id_filter.get(),
            status_filter.get(),
            trigger_filter.get(),
            endpoint_type_filter.get(),
            endpoint_filter.get(),
            created_after.get(),
            created_before.get(),
        )
    };
    let (applied_key, set_applied_key) = signal(filter_key());

    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let jobs = LocalResource::new(move || {
        let _ = refresh.get();
        let oid = oid.clone();
        let wid = wid.clone();
        let live = filter_key();
        // Only trust the cursor stack while it still belongs to the live filters;
        // otherwise a filter just changed and we must restart from page 1. Read
        // `applied_key` untracked so bookkeeping writes to it don't refetch.
        let cursor = if live == applied_key.get_untracked() {
            page_cursors.get().get(page_index.get()).cloned().flatten()
        } else {
            None
        };
        let (job_id, status, trigger, endpoint_type, endpoint, created_after, created_before) =
            live;
        let params = JobListQueryParams {
            cursor,
            limit: page_size.get(),
            job_id: filter_opt(job_id),
            status,
            trigger,
            endpoint: filter_opt(endpoint),
            endpoint_type,
            created_after: created_after.map(|d| d.to_rfc3339()),
            created_before: created_before.map(|d| d.to_rfc3339()),
        };
        api::list_jobs(oid, wid, params)
    });

    let oid_render = org_id.clone();
    let wid_render = workspace_id.clone();
    let oid_form = org_id.clone();
    let wid_form = workspace_id.clone();

    let any_filter = move || {
        !job_id_filter.get().is_empty()
            || !status_filter.get().is_empty()
            || !trigger_filter.get().is_empty()
            || !endpoint_type_filter.get().is_empty()
            || !endpoint_filter.get().is_empty()
            || created_after.get().is_some()
            || created_before.get().is_some()
    };

    // Reset pagination whenever any filter changes, and record the snapshot the
    // fresh page-1 cursor stack now belongs to. The `prev` guard skips the
    // initial run. The cursor-stack writes are guarded so a filter change made
    // while already on page 1 doesn't trigger a redundant refetch (the resource
    // already fetches page 1 via the stale-snapshot guard above).
    Effect::new(move |prev: Option<()>| {
        let live = filter_key();
        if prev.is_some() {
            set_applied_key.set(live);
            if page_index.get_untracked() != 0 {
                set_page_index.set(0);
            }
            let cursors = page_cursors.get_untracked();
            if cursors.len() != 1 || cursors[0].is_some() {
                set_page_cursors.set(vec![None]);
            }
        }
    });

    // Capture the current scroll position just before any query change swaps the
    // list content (filters, page navigation, page size).
    Effect::new(move |prev: Option<()>| {
        let _ = (
            job_id_filter.get(),
            status_filter.get(),
            trigger_filter.get(),
            endpoint_type_filter.get(),
            endpoint_filter.get(),
            created_after.get(),
            created_before.get(),
            page_index.get(),
            page_size.get(),
        );
        if prev.is_some() {
            if let Some(w) = web_sys::window() {
                set_saved_scroll.set(w.scroll_y().ok());
            }
        }
    });

    // Once the refetched list has rendered, restore the saved scroll position so
    // the viewport stays put instead of jumping to the top.
    Effect::new(move |_| {
        if jobs.get().is_some() {
            if let Some(y) = saved_scroll.get_untracked() {
                set_saved_scroll.set(None);
                request_animation_frame(move || {
                    if let Some(w) = web_sys::window() {
                        w.scroll_to_with_x_and_y(0.0, y);
                    }
                });
            }
        }
    });

    view! {
        <div class="space-y-4">
            // Filter bar + actions
            <div class="flex items-start justify-between gap-4">
                <div class="space-y-3">
                    // Row 1 — Job ID, Status, Trigger
                    <div class="flex flex-wrap items-center gap-3">
                        // `change` only fires on Enter/blur, so emptying the box
                        // would leave the stale filter applied. Clear on `input`
                        // the moment it goes empty; typing still commits on
                        // Enter/blur so a long id doesn't refetch per keystroke.
                        <input type="search" prop:value=move || job_id_filter.get()
                            on:input=move |ev| {
                                if event_target_value(&ev).is_empty() {
                                    set_job_id_filter.set(String::new());
                                }
                            }
                            on:change=move |ev| set_job_id_filter.set(event_target_value(&ev))
                            class="h-9 w-52 rounded-lg border border-gray-300 px-3 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                            placeholder="Exact job ID" />
                        <MultiSelectFilter label="Status"
                            options=vec![("ACTIVE", "Active"), ("RETIRED", "Retired")]
                            selected=status_filter set_selected=set_status_filter />
                        <MultiSelectFilter label="Trigger"
                            options=vec![("IMMEDIATE", "Immediate"), ("DELAYED", "Delayed"), ("CRON", "CRON")]
                            selected=trigger_filter set_selected=set_trigger_filter />
                    </div>
                    // Row 2 — Endpoint Type, Endpoint, Created
                    <div class="flex flex-wrap items-center gap-3">
                        <MultiSelectFilter label="Endpoint Type"
                            options=vec![("HTTP", "HTTP"), ("KAFKA", "Kafka"), ("REDIS_STREAM", "Redis Stream"), ("INTERNAL", "Internal")]
                            selected=endpoint_type_filter set_selected=set_endpoint_type_filter />
                        // Same as the job-id box: clear takes effect immediately.
                        <input type="search" prop:value=move || endpoint_filter.get()
                            on:input=move |ev| {
                                if event_target_value(&ev).is_empty() {
                                    set_endpoint_filter.set(String::new());
                                }
                            }
                            on:change=move |ev| set_endpoint_filter.set(event_target_value(&ev))
                            class="h-9 w-52 rounded-lg border border-gray-300 px-3 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                            placeholder="Search endpoint\u{2026}" />
                        <DateRangeFilter
                            after=created_after set_after=set_created_after
                            before=created_before set_before=set_created_before />
                        <Show when=any_filter>
                            <button
                                on:click=move |_| {
                                    set_job_id_filter.set(String::new());
                                    set_status_filter.set(Vec::new());
                                    set_trigger_filter.set(Vec::new());
                                    set_endpoint_type_filter.set(Vec::new());
                                    set_endpoint_filter.set(String::new());
                                    set_created_after.set(None);
                                    set_created_before.set(None);
                                }
                                class="px-3 py-1.5 text-sm text-gray-600 hover:text-gray-900 font-medium"
                            >"Clear filters"</button>
                        </Show>
                    </div>
                </div>
                // Primary action — distinct from the filters, pinned top-right.
                <button
                    on:click=move |_| set_modal_open.set(true)
                    class="inline-flex shrink-0 items-center gap-2 px-3 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                >
                    <PlusIcon />
                    "New Job"
                </button>
            </div>

            <Transition fallback=move || view! { <LoadingSpinner /> }>
                {move || {
                    let oid = oid_render.clone();
                    let wid = wid_render.clone();
                    jobs.get().map(|r| (*r).clone()).map(move |result| {
                        match result {
                            Ok(page) => {
                                let next_cursor = page.cursor.clone();
                                if page.data.is_empty() {
                                    let msg = if any_filter() {
                                        "No jobs match the current filters."
                                    } else {
                                        "No jobs in this workspace. Create an endpoint first, then add a job."
                                    };
                                    view! {
                                        <div class="space-y-3">
                                            <EmptyState message=msg />
                                            <JobsPagination
                                                next_cursor=next_cursor
                                                page_size=page_size
                                                set_page_size=set_page_size
                                                set_page_cursors=set_page_cursors
                                                page_index=page_index
                                                set_page_index=set_page_index
                                            />
                                        </div>
                                    }.into_any()
                                } else {
                                    let jobs = page.data.clone();
                                    view! {
                                        <div class="space-y-3">
                                            <JobsTable jobs=jobs org_id=oid.clone() workspace_id=wid.clone() set_refresh=set_refresh />
                                            <JobsPagination
                                                next_cursor=next_cursor
                                                page_size=page_size
                                                set_page_size=set_page_size
                                                set_page_cursors=set_page_cursors
                                                page_index=page_index
                                                set_page_index=set_page_index
                                            />
                                        </div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                        }
                    })
                }}
            </Transition>

            <Modal title="Create Job" open=modal_open set_open=set_modal_open>
                <CreateJobForm org_id=oid_form workspace_id=wid_form set_modal_open=set_modal_open set_refresh=set_refresh />
            </Modal>
        </div>
    }
}

#[component]
fn JobsPagination(
    next_cursor: Option<String>,
    page_size: ReadSignal<i64>,
    set_page_size: WriteSignal<i64>,
    set_page_cursors: WriteSignal<Vec<Option<String>>>,
    page_index: ReadSignal<usize>,
    set_page_index: WriteSignal<usize>,
) -> impl IntoView {
    let has_next = next_cursor.is_some();
    let has_prev = move || page_index.get() > 0;

    let on_next = move |_| {
        if let Some(nc) = next_cursor.clone() {
            let idx = page_index.get_untracked();
            set_page_cursors.update(|cursors| {
                // Drop any forward history, then record the next page's cursor.
                cursors.truncate(idx + 1);
                cursors.push(Some(nc));
            });
            set_page_index.set(idx + 1);
        }
    };

    let on_prev = move |_| {
        let idx = page_index.get_untracked();
        if idx > 0 {
            set_page_index.set(idx - 1);
        }
    };

    view! {
        <div class="flex items-center justify-between text-sm text-gray-600">
            <div class="flex items-center gap-2">
                <span>"Per page:"</span>
                <select prop:value=move || page_size.get().to_string()
                    on:change=move |ev| {
                        if let Ok(size) = event_target_value(&ev).parse::<i64>() {
                            set_page_size.set(size);
                            set_page_cursors.set(vec![None]);
                            set_page_index.set(0);
                        }
                    }
                    class="px-2 py-1 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none">
                    <option value="25">"25"</option>
                    <option value="50">"50"</option>
                    <option value="100">"100"</option>
                </select>
            </div>
            <div class="flex items-center gap-3">
                <span>"Page " {move || page_index.get() + 1}</span>
                <button
                    on:click=on_prev
                    disabled=move || !has_prev()
                    class="px-3 py-1.5 border border-gray-300 rounded-lg font-medium hover:bg-gray-50 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
                >"Previous"</button>
                <button
                    on:click=on_next
                    disabled=move || !has_next
                    class="px-3 py-1.5 border border-gray-300 rounded-lg font-medium hover:bg-gray-50 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
                >"Next"</button>
            </div>
        </div>
    }
}

#[component]
fn CreateJobForm(
    org_id: String,
    workspace_id: String,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (endpoint, set_endpoint) = signal(String::new());
    let (trigger, set_trigger) = signal("IMMEDIATE".to_string());
    let (input_json, set_input_json) = signal(String::new());
    let (idempotency_key, set_idempotency_key) = signal(String::new());
    let (run_at, set_run_at) = signal(String::new());
    let (cron_expr, set_cron_expr) = signal(String::new());
    let (timezone, set_timezone) = signal("UTC".to_string());
    let (starts_at, set_starts_at) = signal(String::new());
    let (ends_at, set_ends_at) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let ep = endpoint.get_untracked();
        let trig = trigger.get_untracked();
        let inp = input_json.get_untracked();
        let ikey = idempotency_key.get_untracked();
        let ra = run_at.get_untracked();
        let cron = cron_expr.get_untracked();
        let tz = timezone.get_untracked();
        let sa = starts_at.get_untracked();
        let ea = ends_at.get_untracked();

        set_submitting.set(true);
        set_error.set(None);

        leptos::task::spawn_local(async move {
            let input = if inp.trim().is_empty() {
                None
            } else {
                match serde_json::from_str::<serde_json::Value>(&inp) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        set_error.set(Some(format!("Invalid JSON input: {e}")));
                        set_submitting.set(false);
                        return;
                    }
                }
            };

            let mut body = serde_json::json!({
                "endpoint": ep,
                "trigger": trig,
                "input": input,
            });

            let obj = body.as_object_mut().unwrap();

            match trig.as_str() {
                "DELAYED" => {
                    if !ikey.is_empty() {
                        obj.insert("idempotency_key".into(), serde_json::Value::String(ikey));
                    }
                    if !ra.is_empty() {
                        obj.insert("run_at".into(), serde_json::Value::String(ra));
                    }
                }
                "CRON" => {
                    if !cron.is_empty() {
                        obj.insert("cron".into(), serde_json::Value::String(cron));
                    }
                    if !tz.is_empty() {
                        obj.insert("timezone".into(), serde_json::Value::String(tz));
                    }
                    if !sa.is_empty() {
                        obj.insert("starts_at".into(), serde_json::Value::String(sa));
                    }
                    if !ea.is_empty() {
                        obj.insert("ends_at".into(), serde_json::Value::String(ea));
                    }
                }
                _ => {
                    if !ikey.is_empty() {
                        obj.insert("idempotency_key".into(), serde_json::Value::String(ikey));
                    }
                }
            }

            match api::create_job(oid, wid, body).await {
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
                <label class="block text-sm font-medium text-gray-700 mb-1">"Endpoint Name"</label>
                <input type="text" required=true prop:value=move || endpoint.get()
                    on:input=move |ev| set_endpoint.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    placeholder="my-endpoint" />
            </div>

            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Trigger Type"</label>
                <select prop:value=move || trigger.get()
                    on:change=move |ev| set_trigger.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none">
                    <option value="IMMEDIATE">"Immediate"</option>
                    <option value="DELAYED">"Delayed"</option>
                    <option value="CRON">"CRON"</option>
                </select>
            </div>

            // DELAYED fields
            <Show when=move || trigger.get() == "DELAYED">
                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">"Idempotency Key (required)"</label>
                    <input type="text" prop:value=move || idempotency_key.get()
                        on:input=move |ev| set_idempotency_key.set(event_target_value(&ev))
                        class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                        placeholder="unique-key-123" />
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">"Run At (ISO 8601)"</label>
                    <input type="datetime-local" prop:value=move || run_at.get()
                        on:input=move |ev| set_run_at.set(event_target_value(&ev))
                        class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none" />
                </div>
            </Show>

            // CRON fields
            <Show when=move || trigger.get() == "CRON">
                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">"Cron Expression (required)"</label>
                    <input type="text" prop:value=move || cron_expr.get()
                        on:input=move |ev| set_cron_expr.set(event_target_value(&ev))
                        class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                        placeholder="0 0/5 * * * *" />
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">"Timezone"</label>
                    <input type="text" prop:value=move || timezone.get()
                        on:input=move |ev| set_timezone.set(event_target_value(&ev))
                        class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                        placeholder="UTC" />
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">"Starts At (optional)"</label>
                    <input type="datetime-local" prop:value=move || starts_at.get()
                        on:input=move |ev| set_starts_at.set(event_target_value(&ev))
                        class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none" />
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">"Ends At (optional)"</label>
                    <input type="datetime-local" prop:value=move || ends_at.get()
                        on:input=move |ev| set_ends_at.set(event_target_value(&ev))
                        class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none" />
                </div>
            </Show>

            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Input (JSON, optional)"</label>
                <textarea prop:value=move || input_json.get()
                    on:input=move |ev| set_input_json.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    rows="3" placeholder="{\"key\": \"value\"}"></textarea>
            </div>

            <div class="flex justify-end gap-3 pt-2">
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Creating..." } else { "Create Job" }}
                </button>
            </div>
        </form>
    }
}

#[component]
fn JobsTable(
    jobs: Vec<Job>,
    org_id: String,
    workspace_id: String,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (selected_job, set_selected_job) = signal(Option::<String>::None);
    let (status_job, set_status_job) = signal(Option::<String>::None);
    let (versions_job, set_versions_job) = signal(Option::<String>::None);
    let (cancel_error, set_cancel_error) = signal(Option::<String>::None);

    // Single table-level cancel confirmation. `cancel_target` holds the job_id
    // pending cancellation; `confirm_open` drives the dialog visibility.
    let (cancel_target, set_cancel_target) = signal(Option::<String>::None);
    let (confirm_open, set_confirm_open) = signal(false);

    let oid_cancel = org_id.clone();
    let wid_cancel = workspace_id.clone();
    let on_cancel_confirmed = Callback::new(move |_: ()| {
        let jid = match cancel_target.get_untracked() {
            Some(j) => j,
            None => return,
        };
        let oid = oid_cancel.clone();
        let wid = wid_cancel.clone();
        set_cancel_error.set(None);
        leptos::task::spawn_local(async move {
            match api::cancel_job(oid, wid, jid).await {
                Ok(_) => set_refresh.update(|r| *r += 1),
                Err(e) => set_cancel_error.set(Some(e.to_string())),
            }
        });
        set_cancel_target.set(None);
    });

    view! {
        <div class="space-y-2">
            <Show when=move || cancel_error.get().is_some()>
                <ErrorAlert message=cancel_error.get().unwrap_or_default() />
            </Show>
            <div class="bg-white rounded-xl border border-gray-200 overflow-hidden">
                <table class="min-w-full divide-y divide-gray-200">
                    <thead class="bg-gray-50">
                        <tr>
                            <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Job ID"</th>
                            <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Endpoint"</th>
                            <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Trigger"</th>
                            <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Status"</th>
                            <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Created"</th>
                            <th class="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase">"Actions"</th>
                        </tr>
                    </thead>
                    <tbody class="divide-y divide-gray-200">
                        {jobs.into_iter().map(|job| {
                            let jid = job.job_id.clone();
                            let jid_click = job.job_id.clone();
                            let jid_show = job.job_id.clone();
                            let jid_status = job.job_id.clone();
                            let jid_status_show = job.job_id.clone();
                            let jid_versions = job.job_id.clone();
                            let jid_versions_show = job.job_id.clone();
                            let jid_cancel = job.job_id.clone();
                            let oid = org_id.clone();
                            let wid = workspace_id.clone();
                            let oid_status = org_id.clone();
                            let wid_status = workspace_id.clone();
                            let oid_versions = org_id.clone();
                            let wid_versions = workspace_id.clone();
                            let is_active = job.status == "ACTIVE";
                            let is_cron = job.trigger == "CRON";
                            let jid_for_status = job.job_id.clone();
                            let jid_for_versions = job.job_id.clone();
                            let jid_for_execs = job.job_id.clone();

                            view! {
                                <tr class="hover:bg-gray-50 cursor-pointer transition-colors"
                                    on:click=move |_| {
                                        let current = selected_job.get_untracked();
                                        if current.as_deref() == Some(&jid_click) {
                                            set_selected_job.set(None);
                                        } else {
                                            set_selected_job.set(Some(jid_click.clone()));
                                        }
                                    }>
                                    <td class="px-6 py-4 text-sm text-gray-900"><CopyableId value=jid /></td>
                                    <td class="px-6 py-4 text-sm text-gray-600">{job.endpoint.clone()}</td>
                                    <td class="px-6 py-4 text-sm"><TriggerBadge trigger=job.trigger.clone() /></td>
                                    <td class="px-6 py-4"><StatusBadge status=job.status.clone() /></td>
                                    <td class="px-6 py-4 text-sm text-gray-500 whitespace-nowrap">{format_datetime(&job.created_at)}</td>
                                    <td class="px-6 py-4 text-right">
                                        <div class="flex items-center justify-end gap-3" on:click=move |ev| ev.stop_propagation()>
                                            // Status + Versions on the left
                                            <button on:click=move |_| {
                                                let current = status_job.get_untracked();
                                                if current.as_deref() == Some(&jid_status) {
                                                    set_status_job.set(None);
                                                } else {
                                                    set_status_job.set(Some(jid_status.clone()));
                                                }
                                            } class="text-blue-600 hover:text-blue-800 text-xs font-medium">"Status"</button>
                                            {if is_cron {
                                                let jid_v = jid_versions.clone();
                                                Some(view! {
                                                    <button on:click=move |_| {
                                                        let current = versions_job.get_untracked();
                                                        if current.as_deref() == Some(&jid_v) {
                                                            set_versions_job.set(None);
                                                        } else {
                                                            set_versions_job.set(Some(jid_v.clone()));
                                                        }
                                                    } class="text-teal-600 hover:text-teal-800 text-xs font-medium">"Versions"</button>
                                                })
                                            } else { None }}
                                            // Divider + Cancel on the right (ACTIVE jobs only)
                                            <Show when=move || is_active>
                                                <span class="text-gray-300">"|"</span>
                                                <button on:click={
                                                    let jid_c = jid_cancel.clone();
                                                    move |_| {
                                                        set_cancel_target.set(Some(jid_c.clone()));
                                                        set_confirm_open.set(true);
                                                    }
                                                }
                                                    class="px-2 py-1 border border-red-300 text-red-600 hover:bg-red-50 rounded text-xs font-medium">
                                                    "Cancel"
                                                </button>
                                            </Show>
                                        </div>
                                    </td>
                                </tr>
                                // Status inline
                                <Show when={
                                    let jid = jid_status_show.clone();
                                    move || status_job.get().as_deref() == Some(&jid)
                                }>
                                    <tr>
                                        <td colspan="6" class="px-6 py-4 bg-blue-50">
                                            <JobStatusPanel org_id=oid_status.clone() workspace_id=wid_status.clone() job_id=jid_for_status.clone() />
                                        </td>
                                    </tr>
                                </Show>
                                // Versions inline
                                <Show when={
                                    let jid = jid_versions_show.clone();
                                    move || versions_job.get().as_deref() == Some(&jid)
                                }>
                                    <tr>
                                        <td colspan="6" class="px-6 py-4 bg-teal-50">
                                            <JobVersionsPanel org_id=oid_versions.clone() workspace_id=wid_versions.clone() job_id=jid_for_versions.clone() />
                                        </td>
                                    </tr>
                                </Show>
                                // Executions inline
                                <Show when={
                                    let job_id = jid_show.clone();
                                    move || selected_job.get().as_deref() == Some(&job_id)
                                }>
                                    <tr>
                                        <td colspan="6" class="px-6 py-4 bg-gray-50">
                                            <JobExecutions org_id=oid.clone() workspace_id=wid.clone() job_id=jid_for_execs.clone() />
                                        </td>
                                    </tr>
                                </Show>
                            }
                        }).collect::<Vec<_>>()}
                    </tbody>
                </table>
            </div>
            // Single table-level cancel confirmation dialog, rendered outside the table.
            <ConfirmDialog
                title="Cancel job"
                message="Cancel this job? It will be retired and stop running. This cannot be undone."
                open=confirm_open
                set_open=set_confirm_open
                on_confirm=on_cancel_confirmed
                confirm_label="Cancel job"
                dismiss_label="Keep job"
                amber=true
            />
        </div>
    }
}

#[component]
fn JobStatusPanel(org_id: String, workspace_id: String, job_id: String) -> impl IntoView {
    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let jid = job_id.clone();
    let status = LocalResource::new(move || {
        let oid = oid.clone();
        let wid = wid.clone();
        let jid = jid.clone();
        api::get_job_status(oid, wid, jid)
    });

    view! {
        <div class="space-y-2">
            <h4 class="text-sm font-medium text-blue-800">"Job Status"</h4>
            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || status.get().map(|r| (*r).clone()).map(|result| {
                    match result {
                        Ok(s) => {
                            let health_color = match s.health.as_str() {
                                "HEALTHY" => "text-green-700 bg-green-100",
                                "DEGRADED" => "text-yellow-700 bg-yellow-100",
                                "FAILING" => "text-red-700 bg-red-100",
                                _ => "text-gray-700 bg-gray-100",
                            };
                            let active_str = s.active_executions.as_ref()
                                .map(|a| format!("pending: {}, running: {}, total: {}",
                                    a.get("pending").and_then(|v| v.as_i64()).unwrap_or(0),
                                    a.get("running").and_then(|v| v.as_i64()).unwrap_or(0),
                                    a.get("total").and_then(|v| v.as_i64()).unwrap_or(0),
                                ))
                                .unwrap_or_else(|| "none".to_string());
                            let last_exec_str = s.last_execution.as_ref()
                                .map(|e| format!("{} - {}",
                                    e.get("execution_id").and_then(|v| v.as_str()).unwrap_or("?"),
                                    e.get("status").and_then(|v| v.as_str()).unwrap_or("?")))
                                .unwrap_or_else(|| "none".to_string());
                            let cron_str = s.cron.as_ref()
                                .map(|c| format!("expr: {}, next: {}",
                                    c.get("expression").and_then(|v| v.as_str()).unwrap_or("?"),
                                    c.get("next_run_at").and_then(|v| v.as_str()).unwrap_or("?")))
                                .unwrap_or_else(|| "N/A".to_string());
                            view! {
                                <div class="grid grid-cols-2 gap-3 text-sm">
                                    <div>
                                        <span class="text-gray-500">"Health: "</span>
                                        <span class={format!("inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {health_color}")}>{s.health.clone()}</span>
                                    </div>
                                    <div><span class="text-gray-500">"Version: "</span><span class="text-gray-900">{s.version}</span></div>
                                    <div><span class="text-gray-500">"Active Executions: "</span><span class="text-gray-900">{active_str}</span></div>
                                    <div><span class="text-gray-500">"Last Execution: "</span><span class="text-gray-900 font-mono text-xs">{last_exec_str}</span></div>
                                    <div class="col-span-2"><span class="text-gray-500">"Cron: "</span><span class="text-gray-900 font-mono text-xs">{cron_str}</span></div>
                                </div>
                            }.into_any()
                        }
                        Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn JobVersionsPanel(org_id: String, workspace_id: String, job_id: String) -> impl IntoView {
    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let jid = job_id.clone();
    let versions = LocalResource::new(move || {
        let oid = oid.clone();
        let wid = wid.clone();
        let jid = jid.clone();
        api::get_job_versions(oid, wid, jid)
    });

    view! {
        <div class="space-y-2">
            <h4 class="text-sm font-medium text-teal-800">"Version History"</h4>
            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || versions.get().map(|r| (*r).clone()).map(|result| {
                    match result {
                        Ok(items) => {
                            if items.is_empty() {
                                view! { <p class="text-sm text-gray-500">"No version history."</p> }.into_any()
                            } else {
                                let items = items.clone();
                                view! {
                                    <div class="space-y-1">
                                        {items.into_iter().map(|v| {
                                            view! {
                                                <div class="flex items-center gap-4 bg-white rounded-lg border border-gray-200 px-4 py-2 text-xs">
                                                    <CopyableId value=v.job_id.clone() />
                                                    <span>"v" {v.version}</span>
                                                    <StatusBadge status=v.status.clone() />
                                                    {v.cron.as_ref().map(|c| view! { <span class="font-mono text-gray-500">{c.clone()}</span> })}
                                                    <span class="text-gray-400">{format_date(&v.created_at)}</span>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }
                        }
                        Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn JobExecutions(org_id: String, workspace_id: String, job_id: String) -> impl IntoView {
    let (refresh, set_refresh) = signal(0u32);

    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let jid = job_id.clone();
    let executions = LocalResource::new(move || {
        let _ = refresh.get();
        let oid = oid.clone();
        let wid = wid.clone();
        let jid = jid.clone();
        api::list_job_executions(oid, wid, jid)
    });

    let oid_r = org_id.clone();
    let wid_r = workspace_id.clone();

    view! {
        <div class="space-y-2">
            <h4 class="text-sm font-medium text-gray-700">"Executions"</h4>
            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || {
                    let oid = oid_r.clone();
                    let wid = wid_r.clone();
                    executions.get().map(|r| (*r).clone()).map(move |result| {
                        match result {
                            Ok(execs) => {
                                if execs.is_empty() {
                                    view! { <p class="text-sm text-gray-500">"No executions yet."</p> }.into_any()
                                } else {
                                    let execs = execs.clone();
                                    view! { <ExecutionsList executions=execs org_id=oid.clone() workspace_id=wid.clone() set_refresh=set_refresh /> }.into_any()
                                }
                            }
                            Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn ExecutionsList(
    executions: Vec<Execution>,
    org_id: String,
    workspace_id: String,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (selected_exec, set_selected_exec) = signal(Option::<String>::None);
    let (cancel_error, set_cancel_error) = signal(Option::<String>::None);

    view! {
        <div class="space-y-2">
            <Show when=move || cancel_error.get().is_some()>
                <ErrorAlert message=cancel_error.get().unwrap_or_default() />
            </Show>
            {executions.into_iter().map(|exec| {
                let eid = exec.execution_id.clone();
                let eid_click = exec.execution_id.clone();
                let eid_show = exec.execution_id.clone();
                let eid_cancel = exec.execution_id.clone();
                let oid = org_id.clone();
                let wid = workspace_id.clone();
                let oid_cancel = org_id.clone();
                let wid_cancel = workspace_id.clone();
                let is_cancellable = exec.status == "PENDING" || exec.status == "QUEUED";
                view! {
                    <div class="bg-white rounded-lg border border-gray-200">
                        <div class="flex items-center justify-between px-4 py-2.5 cursor-pointer hover:bg-gray-50"
                            on:click=move |_| {
                                let current = selected_exec.get_untracked();
                                if current.as_deref() == Some(&eid_click) {
                                    set_selected_exec.set(None);
                                } else {
                                    set_selected_exec.set(Some(eid_click.clone()));
                                }
                            }>
                            <div class="flex items-center gap-4">
                                <CopyableId value=eid.clone() />
                                <StatusBadge status=exec.status.clone() />
                            </div>
                            <div class="flex items-center gap-4 text-xs text-gray-500">
                                <span>"Attempts: " {exec.attempt_count.unwrap_or(0)} "/" {exec.max_attempts.unwrap_or(1)}</span>
                                {exec.duration_ms.map(|d| view! { <span>{d} "ms"</span> })}
                                <span>{format_date(&exec.created_at)}</span>
                                {if is_cancellable {
                                    let oid_c = oid_cancel.clone();
                                    let wid_c = wid_cancel.clone();
                                    let eid_c = eid_cancel.clone();
                                    Some(view! {
                                        <button on:click=move |ev| {
                                            ev.stop_propagation();
                                            let oid = oid_c.clone();
                                            let wid = wid_c.clone();
                                            let eid = eid_c.clone();
                                            set_cancel_error.set(None);
                                            leptos::task::spawn_local(async move {
                                                match api::cancel_execution(oid, wid, eid).await {
                                                    Ok(_) => set_refresh.update(|c| *c += 1),
                                                    Err(e) => set_cancel_error.set(Some(e.to_string())),
                                                }
                                            });
                                        } class="text-orange-600 hover:text-orange-800 text-xs font-medium">"Cancel"</button>
                                    })
                                } else { None }}
                            </div>
                        </div>
                        <Show when={
                            let eid = eid_show.clone();
                            move || selected_exec.get().as_deref() == Some(&eid)
                        }>
                            <ExecutionDetail
                                org_id=oid.clone()
                                workspace_id=wid.clone()
                                execution=exec.clone()
                            />
                        </Show>
                    </div>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn ExecutionDetail(org_id: String, workspace_id: String, execution: Execution) -> impl IntoView {
    let oid_a = org_id.clone();
    let wid_a = workspace_id.clone();
    let eid_a = execution.execution_id.clone();
    let attempts = LocalResource::new(move || {
        let oid = oid_a.clone();
        let wid = wid_a.clone();
        let eid = eid_a.clone();
        api::list_attempts(oid, wid, eid)
    });

    let oid_l = org_id.clone();
    let wid_l = workspace_id.clone();
    let eid_l = execution.execution_id.clone();
    let logs = LocalResource::new(move || {
        let oid = oid_l.clone();
        let wid = wid_l.clone();
        let eid = eid_l.clone();
        api::list_execution_logs(oid, wid, eid)
    });

    let input_str = execution
        .input
        .as_ref()
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .unwrap_or_else(|| "null".to_string());
    let output_str = execution
        .output
        .as_ref()
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .unwrap_or_else(|| "null".to_string());

    view! {
        <div class="border-t border-gray-200 px-4 py-3 space-y-3">
            // Execution detail
            <div class="grid grid-cols-2 gap-2 text-xs">
                <div><span class="text-gray-500">"Worker: "</span><span class="font-mono">{execution.worker_id.unwrap_or_else(|| "-".to_string())}</span></div>
                <div><span class="text-gray-500">"Duration: "</span><span>{execution.duration_ms.map(|d| format!("{d}ms")).unwrap_or_else(|| "-".to_string())}</span></div>
                <div><span class="text-gray-500">"Started: "</span><span>{execution.started_at.unwrap_or_else(|| "-".to_string())}</span></div>
                <div><span class="text-gray-500">"Completed: "</span><span>{execution.completed_at.unwrap_or_else(|| "-".to_string())}</span></div>
            </div>
            <div class="grid grid-cols-2 gap-2 text-xs">
                <div>
                    <span class="text-gray-500 block mb-1">"Input:"</span>
                    <pre class="bg-gray-100 rounded p-2 overflow-auto max-h-32 font-mono text-xs">{input_str}</pre>
                </div>
                <div>
                    <span class="text-gray-500 block mb-1">"Output:"</span>
                    <pre class="bg-gray-100 rounded p-2 overflow-auto max-h-32 font-mono text-xs">{output_str}</pre>
                </div>
            </div>

            // Attempts
            <div>
                <h5 class="text-xs font-medium text-gray-700 mb-1">"Attempts"</h5>
                <Suspense fallback=move || view! { <LoadingSpinner /> }>
                    {move || attempts.get().map(|r| (*r).clone()).map(|result| {
                        match result {
                            Ok(items) => {
                                if items.is_empty() {
                                    view! { <p class="text-xs text-gray-500">"No attempts yet."</p> }.into_any()
                                } else {
                                    let items = items.clone();
                                    view! {
                                        <div class="space-y-1">
                                            {items.into_iter().map(|a| {
                                                view! {
                                                    <div class="flex items-center gap-3 text-xs bg-gray-50 rounded px-3 py-1.5">
                                                        <span class="font-medium">"#" {a.attempt_number}</span>
                                                        <StatusBadge status=a.status.clone() />
                                                        {a.duration_ms.map(|d| view! { <span class="text-gray-500">{d} "ms"</span> })}
                                                        {a.error.as_ref().map(|e| view! { <span class="text-red-600 truncate max-w-xs">{e.clone()}</span> })}
                                                    </div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>

            // Logs
            <div>
                <h5 class="text-xs font-medium text-gray-700 mb-1">"Logs"</h5>
                <Suspense fallback=move || view! { <LoadingSpinner /> }>
                    {move || logs.get().map(|r| (*r).clone()).map(|result| {
                        match result {
                            Ok(items) => {
                                if items.is_empty() {
                                    view! { <p class="text-xs text-gray-500">"No logs."</p> }.into_any()
                                } else {
                                    let items = items.clone();
                                    view! {
                                        <div class="bg-gray-900 rounded p-2 max-h-48 overflow-auto font-mono text-xs">
                                            {items.into_iter().map(|l| {
                                                let level_color = match l.level.as_str() {
                                                    "ERROR" => "text-red-400",
                                                    "WARN" => "text-yellow-400",
                                                    "INFO" => "text-blue-400",
                                                    "DEBUG" => "text-gray-400",
                                                    _ => "text-gray-300",
                                                };
                                                view! {
                                                    <div class="flex gap-2">
                                                        <span class="text-gray-500">{format_date(&l.logged_at)}</span>
                                                        <span class={level_color}>"[" {l.level.clone()} "]"</span>
                                                        <span class="text-gray-200">{l.message.clone()}</span>
                                                    </div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </div>
    }
}

// ════════════════════════════════════════════════════════════
// Endpoints Tab (with confirm dialog for delete)
// ════════════════════════════════════════════════════════════

#[component]
fn EndpointsTab(org_id: String, workspace_id: String) -> impl IntoView {
    let (refresh, set_refresh) = signal(0u32);
    let (create_open, set_create_open) = signal(false);
    let (edit_open, set_edit_open) = signal(false);
    let (editing_ep, set_editing_ep) = signal(Option::<Endpoint>::None);
    let (confirm_open, set_confirm_open) = signal(false);
    let (deleting_name, set_deleting_name) = signal(Option::<String>::None);
    let (delete_error, set_delete_error) = signal(Option::<String>::None);

    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let endpoints = LocalResource::new(move || {
        let _ = refresh.get();
        let oid = oid.clone();
        let wid = wid.clone();
        api::list_endpoints(oid, wid)
    });

    let oid_create = org_id.clone();
    let wid_create = workspace_id.clone();
    let oid_edit = org_id.clone();
    let wid_edit = workspace_id.clone();
    let oid_del = org_id.clone();
    let wid_del = workspace_id.clone();

    let on_confirm_delete = Callback::new(move |_: ()| {
        let name = deleting_name.get_untracked();
        if let Some(name) = name {
            let oid = oid_del.clone();
            let wid = wid_del.clone();
            set_delete_error.set(None);
            leptos::task::spawn_local(async move {
                match api::delete_endpoint(oid, wid, name).await {
                    Ok(_) => set_refresh.update(|c| *c += 1),
                    Err(e) => set_delete_error.set(Some(e.to_string())),
                }
            });
        }
    });

    view! {
        <div class="space-y-4">
            <div class="flex justify-end">
                <button
                    on:click=move |_| set_create_open.set(true)
                    class="inline-flex items-center gap-2 px-3 py-1.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                >
                    <PlusIcon />
                    "New Endpoint"
                </button>
            </div>

            <Show when=move || delete_error.get().is_some()>
                <ErrorAlert message=delete_error.get().unwrap_or_default() />
            </Show>

            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || {
                    endpoints.get().map(|r| (*r).clone()).map(move |result| {
                        match result {
                            Ok(eps) => {
                                if eps.is_empty() {
                                    view! { <EmptyState message="No endpoints yet. Create one to start scheduling jobs." /> }.into_any()
                                } else {
                                    let eps = eps.clone();
                                    view! { <EndpointsTable
                                        endpoints=eps
                                        set_editing_ep=set_editing_ep
                                        set_edit_open=set_edit_open
                                        set_deleting_name=set_deleting_name
                                        set_confirm_open=set_confirm_open
                                    /> }.into_any()
                                }
                            }
                            Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                        }
                    })
                }}
            </Suspense>

            <Modal title="Create Endpoint" open=create_open set_open=set_create_open>
                <CreateEndpointForm org_id=oid_create workspace_id=wid_create set_modal_open=set_create_open set_refresh=set_refresh />
            </Modal>

            <Modal title="Edit Endpoint" open=edit_open set_open=set_edit_open>
                <EditEndpointForm org_id=oid_edit workspace_id=wid_edit editing_ep=editing_ep set_modal_open=set_edit_open set_refresh=set_refresh />
            </Modal>

            <ConfirmDialog
                title="Delete Endpoint"
                message="Are you sure you want to delete this endpoint? Jobs referencing it will be affected."
                open=confirm_open set_open=set_confirm_open on_confirm=on_confirm_delete
            />
        </div>
    }
}

#[component]
fn CreateEndpointForm(
    org_id: String,
    workspace_id: String,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (ep_type, set_ep_type) = signal("HTTP".to_string());
    let (spec_json, set_spec_json) =
        signal(r#"{"url": "http://localhost:9999/webhook", "method": "POST"}"#.to_string());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let name_val = name.get_untracked();
        let ep_type_val = ep_type.get_untracked();
        let spec_val = spec_json.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let spec = match serde_json::from_str::<serde_json::Value>(&spec_val) {
                Ok(v) => v,
                Err(e) => {
                    set_error.set(Some(format!("Invalid JSON spec: {e}")));
                    set_submitting.set(false);
                    return;
                }
            };
            let body = CreateEndpoint {
                name: name_val,
                endpoint_type: ep_type_val,
                spec,
                payload_spec: None,
                config: None,
                retry_policy: None,
            };
            match api::create_endpoint(oid, wid, body).await {
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
                    placeholder="my-webhook" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Type"</label>
                <select prop:value=move || ep_type.get()
                    on:change=move |ev| set_ep_type.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none">
                    <option value="HTTP">"HTTP"</option>
                    <option value="KAFKA">"Kafka"</option>
                    <option value="REDIS_STREAM">"Redis Stream"</option>
                </select>
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Spec (JSON)"</label>
                <textarea prop:value=move || spec_json.get()
                    on:input=move |ev| set_spec_json.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    rows="4"
                    placeholder=r#"{"url": "https://example.com/webhook", "method": "POST"}"#></textarea>
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Creating..." } else { "Create Endpoint" }}
                </button>
            </div>
        </form>
    }
}

#[component]
fn EndpointsTable(
    endpoints: Vec<Endpoint>,
    set_editing_ep: WriteSignal<Option<Endpoint>>,
    set_edit_open: WriteSignal<bool>,
    set_deleting_name: WriteSignal<Option<String>>,
    set_confirm_open: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="bg-white rounded-xl border border-gray-200 overflow-hidden">
            <table class="min-w-full divide-y divide-gray-200">
                <thead class="bg-gray-50">
                    <tr>
                        <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Name"</th>
                        <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Type"</th>
                        <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Spec"</th>
                        <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">"Updated"</th>
                        <th class="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase">"Actions"</th>
                    </tr>
                </thead>
                <tbody class="divide-y divide-gray-200">
                    {endpoints.into_iter().map(|ep| {
                        let ep_edit = ep.clone();
                        let ep_name_del = ep.name.clone();
                        let spec_preview = serde_json::to_string(&ep.spec).unwrap_or_default();
                        let spec_short = if spec_preview.len() > 50 {
                            format!("{}...", &spec_preview[..50])
                        } else {
                            spec_preview
                        };
                        view! {
                            <tr class="hover:bg-gray-50">
                                <td class="px-6 py-4 text-sm font-medium text-gray-900">{ep.name.clone()}</td>
                                <td class="px-6 py-4 text-sm">
                                    <span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-purple-100 text-purple-800">
                                        {ep.endpoint_type.clone()}
                                    </span>
                                </td>
                                <td class="px-6 py-4 text-xs font-mono text-gray-500 max-w-xs truncate">{spec_short}</td>
                                <td class="px-6 py-4 text-sm text-gray-500">{format_date(&ep.updated_at)}</td>
                                <td class="px-6 py-4 text-right">
                                    <div class="flex items-center justify-end gap-2">
                                        <button on:click=move |_| {
                                            set_editing_ep.set(Some(ep_edit.clone()));
                                            set_edit_open.set(true);
                                        } class="text-blue-600 hover:text-blue-800 text-sm font-medium">"Edit"</button>
                                        <button on:click=move |_| {
                                            set_deleting_name.set(Some(ep_name_del.clone()));
                                            set_confirm_open.set(true);
                                        } class="text-red-600 hover:text-red-800 text-sm font-medium">"Delete"</button>
                                    </div>
                                </td>
                            </tr>
                        }
                    }).collect::<Vec<_>>()}
                </tbody>
            </table>
        </div>
    }
}

#[component]
fn EditEndpointForm(
    org_id: String,
    workspace_id: String,
    editing_ep: ReadSignal<Option<Endpoint>>,
    set_modal_open: WriteSignal<bool>,
    set_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let (spec_json, set_spec_json) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (submitting, set_submitting) = signal(false);

    Effect::new(move || {
        if let Some(ep) = editing_ep.get() {
            set_spec_json.set(serde_json::to_string_pretty(&ep.spec).unwrap_or_default());
            set_error.set(None);
        }
    });

    let ep_name = move || {
        editing_ep
            .get()
            .map(|ep| ep.name.clone())
            .unwrap_or_default()
    };

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let oid = org_id.clone();
        let wid = workspace_id.clone();
        let name = ep_name();
        let spec_val = spec_json.get_untracked();
        set_submitting.set(true);
        set_error.set(None);
        leptos::task::spawn_local(async move {
            let spec = match serde_json::from_str::<serde_json::Value>(&spec_val) {
                Ok(v) => v,
                Err(e) => {
                    set_error.set(Some(format!("Invalid JSON: {e}")));
                    set_submitting.set(false);
                    return;
                }
            };
            let body = serde_json::json!({ "spec": spec });
            match api::update_endpoint(oid, wid, name, body).await {
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
                <input type="text" disabled=true prop:value=move || ep_name()
                    class="w-full px-3 py-2 border border-gray-200 rounded-lg text-sm bg-gray-50 text-gray-500" />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">"Spec (JSON)"</label>
                <textarea prop:value=move || spec_json.get()
                    on:input=move |ev| set_spec_json.set(event_target_value(&ev))
                    class="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm font-mono focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none"
                    rows="6"></textarea>
            </div>
            <div class="flex justify-end gap-3 pt-2">
                <button type="button" on:click=move |_| set_modal_open.set(false)
                    class="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 text-sm font-medium transition-colors">"Cancel"</button>
                <button type="submit" disabled=move || submitting.get()
                    class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 text-sm font-medium transition-colors">
                    {move || if submitting.get() { "Saving..." } else { "Save Changes" }}
                </button>
            </div>
        </form>
    }
}

// ════════════════════════════════════════════════════════════
// Shared helpers
// ════════════════════════════════════════════════════════════

#[component]
fn TriggerBadge(trigger: String) -> impl IntoView {
    let (bg, text) = match trigger.as_str() {
        "IMMEDIATE" => ("bg-indigo-100", "text-indigo-800"),
        "DELAYED" => ("bg-amber-100", "text-amber-800"),
        "CRON" => ("bg-teal-100", "text-teal-800"),
        _ => ("bg-gray-100", "text-gray-800"),
    };

    view! {
        <span class={format!("inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {bg} {text}")}>
            {trigger}
        </span>
    }
}

#[component]
fn PlusIcon() -> impl IntoView {
    view! {
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"></path>
        </svg>
    }
}

fn format_date(s: &str) -> String {
    if s.len() >= 10 {
        s[..10].to_string()
    } else {
        s.to_string()
    }
}

/// RFC-3339 timestamp -> `"YYYY-MM-DD HH:MM:SS"` (UTC), e.g. `2026-07-03 12:34:56`.
fn format_datetime(s: &str) -> String {
    if s.len() >= 19 {
        s[..19].replace('T', " ")
    } else {
        s.replace('T', " ")
    }
}
