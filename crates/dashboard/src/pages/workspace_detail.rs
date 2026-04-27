use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::app::prefixed;
use crate::api::{
    self, Config, CreateConfig, CreateEndpoint, CreatePayloadSpec, CreateSecret, Endpoint,
    Execution, Job, PayloadSpec, UpdateConfig, UpdatePayloadSpec,
    UpdateSecret,
};
use crate::components::confirm::ConfirmDialog;
use crate::components::loading::{EmptyState, ErrorAlert, LoadingSpinner};
use crate::components::modal::Modal;
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
    let (schema_json, set_schema_json) = signal(r#"{"type": "object", "properties": {}}"#.to_string());
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
            let body = CreatePayloadSpec { name: name_val, schema };
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

    let spec_name = move || editing_spec.get().map(|s| s.name.clone()).unwrap_or_default();

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
            let body = CreateConfig { name: name_val, values };
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

    let cfg_name = move || editing_config.get().map(|c| c.name.clone()).unwrap_or_default();

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

#[component]
fn JobsTab(org_id: String, workspace_id: String) -> impl IntoView {
    let (refresh, set_refresh) = signal(0u32);
    let (modal_open, set_modal_open) = signal(false);

    let oid = org_id.clone();
    let wid = workspace_id.clone();
    let jobs = LocalResource::new(move || {
        let _ = refresh.get();
        let oid = oid.clone();
        let wid = wid.clone();
        api::list_jobs(oid, wid)
    });

    let oid_render = org_id.clone();
    let wid_render = workspace_id.clone();
    let oid_form = org_id.clone();
    let wid_form = workspace_id.clone();

    view! {
        <div class="space-y-4">
            <div class="flex justify-end">
                <button
                    on:click=move |_| set_modal_open.set(true)
                    class="inline-flex items-center gap-2 px-3 py-1.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium"
                >
                    <PlusIcon />
                    "New Job"
                </button>
            </div>

            <Suspense fallback=move || view! { <LoadingSpinner /> }>
                {move || {
                    let oid = oid_render.clone();
                    let wid = wid_render.clone();
                    jobs.get().map(|r| (*r).clone()).map(move |result| {
                        match result {
                            Ok(jobs) => {
                                if jobs.is_empty() {
                                    view! { <EmptyState message="No jobs in this workspace. Create an endpoint first, then add a job." /> }.into_any()
                                } else {
                                    let jobs = jobs.clone();
                                    view! { <JobsTable jobs=jobs org_id=oid.clone() workspace_id=wid.clone() set_refresh=set_refresh /> }.into_any()
                                }
                            }
                            Err(e) => view! { <ErrorAlert message=e.to_string() /> }.into_any(),
                        }
                    })
                }}
            </Suspense>

            <Modal title="Create Job" open=modal_open set_open=set_modal_open>
                <CreateJobForm org_id=oid_form workspace_id=wid_form set_modal_open=set_modal_open set_refresh=set_refresh />
            </Modal>
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
fn JobsTable(jobs: Vec<Job>, org_id: String, workspace_id: String, set_refresh: WriteSignal<u32>) -> impl IntoView {
    let (selected_job, set_selected_job) = signal(Option::<String>::None);
    let (status_job, set_status_job) = signal(Option::<String>::None);
    let (versions_job, set_versions_job) = signal(Option::<String>::None);
    let (cancel_error, set_cancel_error) = signal(Option::<String>::None);

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
                            let oid_cancel = org_id.clone();
                            let wid_cancel = workspace_id.clone();
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
                                    <td class="px-6 py-4 text-sm font-mono text-gray-900">{truncate_id(&jid)}</td>
                                    <td class="px-6 py-4 text-sm text-gray-600">{job.endpoint.clone()}</td>
                                    <td class="px-6 py-4 text-sm"><TriggerBadge trigger=job.trigger.clone() /></td>
                                    <td class="px-6 py-4"><StatusBadge status=job.status.clone() /></td>
                                    <td class="px-6 py-4 text-sm text-gray-500">{format_date(&job.created_at)}</td>
                                    <td class="px-6 py-4 text-right">
                                        <div class="flex items-center justify-end gap-2" on:click=move |ev| ev.stop_propagation()>
                                            {if is_active {
                                                let oid_c = oid_cancel.clone();
                                                let wid_c = wid_cancel.clone();
                                                let jid_c = jid_cancel.clone();
                                                Some(view! {
                                                    <button on:click=move |_| {
                                                        let oid = oid_c.clone();
                                                        let wid = wid_c.clone();
                                                        let jid = jid_c.clone();
                                                        set_cancel_error.set(None);
                                                        leptos::task::spawn_local(async move {
                                                            match api::cancel_job(oid, wid, jid).await {
                                                                Ok(_) => set_refresh.update(|c| *c += 1),
                                                                Err(e) => set_cancel_error.set(Some(e.to_string())),
                                                            }
                                                        });
                                                    } class="text-orange-600 hover:text-orange-800 text-xs font-medium">"Cancel"</button>
                                                })
                                            } else { None }}
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
                                                    <span class="font-mono text-gray-600">{truncate_id(&v.job_id)}</span>
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
fn ExecutionsList(executions: Vec<Execution>, org_id: String, workspace_id: String, set_refresh: WriteSignal<u32>) -> impl IntoView {
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
                                <span class="text-xs font-mono text-gray-600">{truncate_id(&eid)}</span>
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

    let input_str = execution.input.as_ref()
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .unwrap_or_else(|| "null".to_string());
    let output_str = execution.output.as_ref()
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
    let (spec_json, set_spec_json) = signal(r#"{"url": "http://localhost:9999/webhook", "method": "POST"}"#.to_string());
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

    let ep_name = move || editing_ep.get().map(|ep| ep.name.clone()).unwrap_or_default();

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

fn truncate_id(id: &str) -> String {
    if id.len() > 8 {
        format!("{}...", &id[..8])
    } else {
        id.to_string()
    }
}

fn format_date(s: &str) -> String {
    if s.len() >= 10 { s[..10].to_string() } else { s.to_string() }
}
