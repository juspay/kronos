use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

use crate::components::sidebar::Sidebar;
use crate::pages::{
    org_detail::OrgDetailPage,
    organizations::OrganizationsPage,
    workspace_detail::WorkspaceDetailPage,
};

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <div class="flex min-h-screen">
                <Sidebar />
                <main class="flex-1 p-8">
                    <Routes fallback=|| "Page not found.">
                        <Route path=path!("/") view=OrganizationsPage />
                        <Route path=path!("/orgs/:org_id") view=OrgDetailPage />
                        <Route path=path!("/orgs/:org_id/workspaces/:workspace_id") view=WorkspaceDetailPage />
                    </Routes>
                </main>
            </div>
        </Router>
    }
}
