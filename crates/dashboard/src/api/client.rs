use super::models::*;

#[cfg(feature = "hydrate")]
use gloo_net::http::Request;

#[cfg(feature = "hydrate")]
use crate::auth::LoginState;

#[cfg(feature = "hydrate")]
use crate::config::DashboardConfig;

#[cfg(feature = "hydrate")]
use leptos::prelude::*;

#[cfg(feature = "hydrate")]
fn get_config() -> DashboardConfig {
    use wasm_bindgen::JsValue;
    let window = web_sys::window().expect("no global window");
    let config = js_sys::Reflect::get(&window, &JsValue::from_str("__KRONOS_CONFIG__"))
        .unwrap_or(JsValue::UNDEFINED);
    let get = |key: &str| -> String {
        if config.is_undefined() || config.is_null() {
            return String::new();
        }
        js_sys::Reflect::get(&config, &JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default()
    };
    let get_opt = |key: &str| -> Option<String> {
        let v = get(key);
        if v.is_empty() { None } else { Some(v) }
    };
    let get_bool = |key: &str| -> bool {
        matches!(get(key).as_str(), "1" | "true" | "yes")
    };
    DashboardConfig {
        api_base_url: get("apiBaseUrl"),
        api_prefix: get("apiPrefix"),
        dashboard_prefix: get("dashboardPrefix"),
        auth_disabled: get_bool("authDisabled"),
        oidc_issuer: get_opt("oidcIssuer"),
        oidc_client_id: get_opt("oidcClientId"),
        oidc_redirect_url: get_opt("oidcRedirectUrl"),
        oidc_audience: get_opt("oidcAudience"),
    }
}

// ============================================================================
// Auth helpers
// ============================================================================

/// Build the `Authorization` header value for an outbound API call. Returns
/// `None` (i.e. don't send the header) when auth is disabled at the
/// build-time config OR when the user isn't logged in yet (no access token
/// in `LoginState`).
#[cfg(feature = "hydrate")]
fn auth_header(config: &DashboardConfig) -> Option<String> {
    if config.auth_disabled {
        return None;
    }
    let login_state = use_context::<RwSignal<LoginState>>()?;
    login_state
        .with(|s| s.access_token.clone())
        .map(|t| format!("Bearer {t}"))
}

/// Wrap a `gloo_net::http::RequestBuilder` with the `Authorization` header
/// when appropriate. Replaces the prior `.header("Authorization", "Bearer ...")`
/// chain that previously used the static `api_key` config field.
#[cfg(feature = "hydrate")]
fn with_auth(
    req: gloo_net::http::RequestBuilder,
    config: &DashboardConfig,
) -> gloo_net::http::RequestBuilder {
    match auth_header(config) {
        Some(h) => req.header("Authorization", &h),
        None => req,
    }
}

// ============================================================================
// Hydrate (WASM) implementations — actual HTTP calls via gloo-net
// ============================================================================

#[cfg(feature = "hydrate")]
mod inner {
    use super::*;

    pub async fn list_organizations() -> Result<Vec<Organization>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/orgs")), &config)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Vec<Organization>> =
            resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn get_organization(org_id: String) -> Result<Organization, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/orgs/{org_id}")), &config)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Organization> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn create_organization(body: CreateOrganization) -> Result<Organization, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/orgs")), &config)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Organization> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn update_organization(
        org_id: String,
        body: UpdateOrganization,
    ) -> Result<Organization, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::put(&format!("{base}/v1/orgs/{org_id}")), &config)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Organization> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    // -- Workspace API --

    pub async fn list_workspaces(org_id: String) -> Result<Vec<Workspace>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/orgs/{org_id}/workspaces")), &config)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Vec<Workspace>> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn create_workspace(
        org_id: String,
        body: CreateWorkspace,
    ) -> Result<Workspace, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/orgs/{org_id}/workspaces")), &config)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Workspace> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    // -- Jobs API (workspace-scoped) --

    pub async fn list_jobs(org_id: String, workspace_id: String) -> Result<Vec<Job>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/jobs")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: PaginatedResponse<Job> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn get_job(
        org_id: String,
        workspace_id: String,
        job_id: String,
    ) -> Result<Job, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/jobs/{job_id}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Job> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn create_job(
        org_id: String,
        workspace_id: String,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/jobs")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<serde_json::Value> =
            resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn cancel_job(
        org_id: String,
        workspace_id: String,
        job_id: String,
    ) -> Result<serde_json::Value, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/jobs/{job_id}/cancel")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data)
    }

    pub async fn get_job_status(
        org_id: String,
        workspace_id: String,
        job_id: String,
    ) -> Result<JobStatus, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/jobs/{job_id}/status")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<JobStatus> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn get_job_versions(
        org_id: String,
        workspace_id: String,
        job_id: String,
    ) -> Result<Vec<Job>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/jobs/{job_id}/versions")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Vec<Job>> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    // -- Endpoints API (workspace-scoped) --

    pub async fn list_endpoints(
        org_id: String,
        workspace_id: String,
    ) -> Result<Vec<Endpoint>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/endpoints")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: PaginatedResponse<Endpoint> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn create_endpoint(
        org_id: String,
        workspace_id: String,
        body: CreateEndpoint,
    ) -> Result<Endpoint, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/endpoints")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Endpoint> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn update_endpoint(
        org_id: String,
        workspace_id: String,
        name: String,
        body: serde_json::Value,
    ) -> Result<Endpoint, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::put(&format!("{base}/v1/endpoints/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Endpoint> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn delete_endpoint(
        org_id: String,
        workspace_id: String,
        name: String,
    ) -> Result<(), String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::delete(&format!("{base}/v1/endpoints/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }

    // -- Executions API (workspace-scoped) --

    pub async fn list_job_executions(
        org_id: String,
        workspace_id: String,
        job_id: String,
    ) -> Result<Vec<Execution>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/jobs/{job_id}/executions")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: PaginatedResponse<Execution> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn get_execution(
        org_id: String,
        workspace_id: String,
        execution_id: String,
    ) -> Result<Execution, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/executions/{execution_id}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Execution> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn cancel_execution(
        org_id: String,
        workspace_id: String,
        execution_id: String,
    ) -> Result<serde_json::Value, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/executions/{execution_id}/cancel")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data)
    }

    pub async fn list_attempts(
        org_id: String,
        workspace_id: String,
        execution_id: String,
    ) -> Result<Vec<Attempt>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/executions/{execution_id}/attempts")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Vec<Attempt>> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn list_execution_logs(
        org_id: String,
        workspace_id: String,
        execution_id: String,
    ) -> Result<Vec<ExecutionLog>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/executions/{execution_id}/logs")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Vec<ExecutionLog>> =
            resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    // -- Configs API (workspace-scoped) --

    pub async fn list_configs(org_id: String, workspace_id: String) -> Result<Vec<Config>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/configs")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: PaginatedResponse<Config> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn create_config(
        org_id: String,
        workspace_id: String,
        body: CreateConfig,
    ) -> Result<Config, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/configs")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Config> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn update_config(
        org_id: String,
        workspace_id: String,
        name: String,
        body: UpdateConfig,
    ) -> Result<Config, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::put(&format!("{base}/v1/configs/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Config> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn delete_config(
        org_id: String,
        workspace_id: String,
        name: String,
    ) -> Result<(), String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::delete(&format!("{base}/v1/configs/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }

    // -- Payload Specs API (workspace-scoped) --

    pub async fn list_payload_specs(
        org_id: String,
        workspace_id: String,
    ) -> Result<Vec<PayloadSpec>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/payload-specs")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: PaginatedResponse<PayloadSpec> =
            resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn create_payload_spec(
        org_id: String,
        workspace_id: String,
        body: CreatePayloadSpec,
    ) -> Result<PayloadSpec, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/payload-specs")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<PayloadSpec> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn update_payload_spec(
        org_id: String,
        workspace_id: String,
        name: String,
        body: UpdatePayloadSpec,
    ) -> Result<PayloadSpec, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::put(&format!("{base}/v1/payload-specs/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<PayloadSpec> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn delete_payload_spec(
        org_id: String,
        workspace_id: String,
        name: String,
    ) -> Result<(), String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::delete(&format!("{base}/v1/payload-specs/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }

    // -- Secrets API (workspace-scoped) --

    pub async fn list_secrets(
        org_id: String,
        workspace_id: String,
    ) -> Result<Vec<Secret>, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::get(&format!("{base}/v1/secrets")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: PaginatedResponse<Secret> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn create_secret(
        org_id: String,
        workspace_id: String,
        body: CreateSecret,
    ) -> Result<Secret, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::post(&format!("{base}/v1/secrets")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Secret> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn update_secret(
        org_id: String,
        workspace_id: String,
        name: String,
        body: UpdateSecret,
    ) -> Result<Secret, String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::put(&format!("{base}/v1/secrets/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let data: DataResponse<Secret> = resp.json().await.map_err(|e| e.to_string())?;
        Ok(data.data)
    }

    pub async fn delete_secret(
        org_id: String,
        workspace_id: String,
        name: String,
    ) -> Result<(), String> {
        let config = get_config();
        let base = config.api_base();
        let resp = with_auth(Request::delete(&format!("{base}/v1/secrets/{name}")), &config)
            .header("X-Org-Id", &org_id)
            .header("X-Workspace-Id", &workspace_id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.ok() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(())
    }
}

// ============================================================================
// SSR stubs — these exist so the code compiles for the server target.
// They are never called (LocalResource only fetches on the client).
// ============================================================================

#[cfg(feature = "ssr")]
mod inner {
    use super::*;

    pub async fn list_organizations() -> Result<Vec<Organization>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn get_organization(_org_id: String) -> Result<Organization, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn create_organization(_body: CreateOrganization) -> Result<Organization, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn update_organization(
        _org_id: String,
        _body: UpdateOrganization,
    ) -> Result<Organization, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_workspaces(_org_id: String) -> Result<Vec<Workspace>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn create_workspace(
        _org_id: String,
        _body: CreateWorkspace,
    ) -> Result<Workspace, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_jobs(_org_id: String, _workspace_id: String) -> Result<Vec<Job>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn get_job(
        _org_id: String,
        _workspace_id: String,
        _job_id: String,
    ) -> Result<Job, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn create_job(
        _org_id: String,
        _workspace_id: String,
        _body: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn cancel_job(
        _org_id: String,
        _workspace_id: String,
        _job_id: String,
    ) -> Result<serde_json::Value, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn get_job_status(
        _org_id: String,
        _workspace_id: String,
        _job_id: String,
    ) -> Result<JobStatus, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn get_job_versions(
        _org_id: String,
        _workspace_id: String,
        _job_id: String,
    ) -> Result<Vec<Job>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_endpoints(
        _org_id: String,
        _workspace_id: String,
    ) -> Result<Vec<Endpoint>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn create_endpoint(
        _org_id: String,
        _workspace_id: String,
        _body: CreateEndpoint,
    ) -> Result<Endpoint, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn update_endpoint(
        _org_id: String,
        _workspace_id: String,
        _name: String,
        _body: serde_json::Value,
    ) -> Result<Endpoint, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn delete_endpoint(
        _org_id: String,
        _workspace_id: String,
        _name: String,
    ) -> Result<(), String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_job_executions(
        _org_id: String,
        _workspace_id: String,
        _job_id: String,
    ) -> Result<Vec<Execution>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn get_execution(
        _org_id: String,
        _workspace_id: String,
        _execution_id: String,
    ) -> Result<Execution, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn cancel_execution(
        _org_id: String,
        _workspace_id: String,
        _execution_id: String,
    ) -> Result<serde_json::Value, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_attempts(
        _org_id: String,
        _workspace_id: String,
        _execution_id: String,
    ) -> Result<Vec<Attempt>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_execution_logs(
        _org_id: String,
        _workspace_id: String,
        _execution_id: String,
    ) -> Result<Vec<ExecutionLog>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_configs(
        _org_id: String,
        _workspace_id: String,
    ) -> Result<Vec<Config>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn create_config(
        _org_id: String,
        _workspace_id: String,
        _body: CreateConfig,
    ) -> Result<Config, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn update_config(
        _org_id: String,
        _workspace_id: String,
        _name: String,
        _body: UpdateConfig,
    ) -> Result<Config, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn delete_config(
        _org_id: String,
        _workspace_id: String,
        _name: String,
    ) -> Result<(), String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_payload_specs(
        _org_id: String,
        _workspace_id: String,
    ) -> Result<Vec<PayloadSpec>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn create_payload_spec(
        _org_id: String,
        _workspace_id: String,
        _body: CreatePayloadSpec,
    ) -> Result<PayloadSpec, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn update_payload_spec(
        _org_id: String,
        _workspace_id: String,
        _name: String,
        _body: UpdatePayloadSpec,
    ) -> Result<PayloadSpec, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn delete_payload_spec(
        _org_id: String,
        _workspace_id: String,
        _name: String,
    ) -> Result<(), String> {
        Err("SSR: not available".to_string())
    }
    pub async fn list_secrets(
        _org_id: String,
        _workspace_id: String,
    ) -> Result<Vec<Secret>, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn create_secret(
        _org_id: String,
        _workspace_id: String,
        _body: CreateSecret,
    ) -> Result<Secret, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn update_secret(
        _org_id: String,
        _workspace_id: String,
        _name: String,
        _body: UpdateSecret,
    ) -> Result<Secret, String> {
        Err("SSR: not available".to_string())
    }
    pub async fn delete_secret(
        _org_id: String,
        _workspace_id: String,
        _name: String,
    ) -> Result<(), String> {
        Err("SSR: not available".to_string())
    }
}

// Re-export from the active inner module
pub use inner::*;
