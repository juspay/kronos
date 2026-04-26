use gloo_net::http::Request;
use serde::{de::DeserializeOwned, Serialize};

use super::models::*;

const API_KEY: &str = "dev-api-key";

fn base_url() -> String {
    // If TE_API_BASE_URL is set at compile time, use it directly.
    // e.g. TE_API_BASE_URL=http://localhost:8080/kronos
    if let Some(url) = option_env!("TE_API_BASE_URL") {
        return url.trim_end_matches('/').to_string();
    }

    let location = web_sys::window().unwrap().location();
    let host = location.host().unwrap_or_default();
    if host.contains("3000") {
        String::new()
    } else {
        format!("http://{host}")
    }
}

async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let resp = Request::get(url)
        .header("Authorization", &format!("Bearer {API_KEY}"))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

async fn post_json<T: DeserializeOwned, B: Serialize>(
    url: &str,
    body: &B,
    workspace_headers: Option<(&str, &str)>,
) -> Result<T, String> {
    let mut req = Request::post(url)
        .header("Authorization", &format!("Bearer {API_KEY}"))
        .header("Content-Type", "application/json");

    if let Some((org_id, ws_id)) = workspace_headers {
        req = req
            .header("X-Org-Id", org_id)
            .header("X-Workspace-Id", ws_id);
    }

    let resp = req
        .json(body)
        .map_err(|e| format!("Serialize error: {e}"))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

async fn post_no_body(
    url: &str,
    workspace_headers: Option<(&str, &str)>,
) -> Result<serde_json::Value, String> {
    let mut req = Request::post(url)
        .header("Authorization", &format!("Bearer {API_KEY}"))
        .header("Content-Type", "application/json");

    if let Some((org_id, ws_id)) = workspace_headers {
        req = req
            .header("X-Org-Id", org_id)
            .header("X-Workspace-Id", ws_id);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

async fn put_json<T: DeserializeOwned, B: Serialize>(
    url: &str,
    body: &B,
    workspace_headers: Option<(&str, &str)>,
) -> Result<T, String> {
    let mut req = Request::put(url)
        .header("Authorization", &format!("Bearer {API_KEY}"))
        .header("Content-Type", "application/json");

    if let Some((org_id, ws_id)) = workspace_headers {
        req = req
            .header("X-Org-Id", org_id)
            .header("X-Workspace-Id", ws_id);
    }

    let resp = req
        .json(body)
        .map_err(|e| format!("Serialize error: {e}"))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

async fn delete_request(
    url: &str,
    workspace_headers: Option<(&str, &str)>,
) -> Result<(), String> {
    let mut req = Request::delete(url)
        .header("Authorization", &format!("Bearer {API_KEY}"));

    if let Some((org_id, ws_id)) = workspace_headers {
        req = req
            .header("X-Org-Id", org_id)
            .header("X-Workspace-Id", ws_id);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    Ok(())
}

async fn get_json_ws<T: DeserializeOwned>(
    url: &str,
    org_id: &str,
    workspace_id: &str,
) -> Result<T, String> {
    let resp = Request::get(url)
        .header("Authorization", &format!("Bearer {API_KEY}"))
        .header("X-Org-Id", org_id)
        .header("X-Workspace-Id", workspace_id)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

// -- Organization API --

pub async fn list_organizations() -> Result<Vec<Organization>, String> {
    let base = base_url();
    let resp: DataResponse<Vec<Organization>> = get_json(&format!("{base}/v1/orgs")).await?;
    Ok(resp.data)
}

pub async fn get_organization(org_id: &str) -> Result<Organization, String> {
    let base = base_url();
    let resp: DataResponse<Organization> = get_json(&format!("{base}/v1/orgs/{org_id}")).await?;
    Ok(resp.data)
}

pub async fn create_organization(body: &CreateOrganization) -> Result<Organization, String> {
    let base = base_url();
    let resp: DataResponse<Organization> =
        post_json(&format!("{base}/v1/orgs"), body, None).await?;
    Ok(resp.data)
}

pub async fn update_organization(
    org_id: &str,
    body: &UpdateOrganization,
) -> Result<Organization, String> {
    let base = base_url();
    let resp: DataResponse<Organization> =
        put_json(&format!("{base}/v1/orgs/{org_id}"), body, None).await?;
    Ok(resp.data)
}

// -- Workspace API --

pub async fn list_workspaces(org_id: &str) -> Result<Vec<Workspace>, String> {
    let base = base_url();
    let resp: DataResponse<Vec<Workspace>> =
        get_json(&format!("{base}/v1/orgs/{org_id}/workspaces")).await?;
    Ok(resp.data)
}

pub async fn create_workspace(
    org_id: &str,
    body: &CreateWorkspace,
) -> Result<Workspace, String> {
    let base = base_url();
    let resp: DataResponse<Workspace> =
        post_json(&format!("{base}/v1/orgs/{org_id}/workspaces"), body, None).await?;
    Ok(resp.data)
}

// -- Jobs API (workspace-scoped) --

pub async fn list_jobs(org_id: &str, workspace_id: &str) -> Result<Vec<Job>, String> {
    let base = base_url();
    let resp: PaginatedResponse<Job> =
        get_json_ws(&format!("{base}/v1/jobs"), org_id, workspace_id).await?;
    Ok(resp.data)
}

pub async fn get_job(org_id: &str, workspace_id: &str, job_id: &str) -> Result<Job, String> {
    let base = base_url();
    let resp: DataResponse<Job> =
        get_json_ws(&format!("{base}/v1/jobs/{job_id}"), org_id, workspace_id).await?;
    Ok(resp.data)
}

pub async fn create_job(
    org_id: &str,
    workspace_id: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let base = base_url();
    let resp: DataResponse<serde_json::Value> = post_json(
        &format!("{base}/v1/jobs"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn cancel_job(
    org_id: &str,
    workspace_id: &str,
    job_id: &str,
) -> Result<serde_json::Value, String> {
    let base = base_url();
    post_no_body(
        &format!("{base}/v1/jobs/{job_id}/cancel"),
        Some((org_id, workspace_id)),
    )
    .await
}

pub async fn get_job_status(
    org_id: &str,
    workspace_id: &str,
    job_id: &str,
) -> Result<JobStatus, String> {
    let base = base_url();
    let resp: DataResponse<JobStatus> = get_json_ws(
        &format!("{base}/v1/jobs/{job_id}/status"),
        org_id,
        workspace_id,
    )
    .await?;
    Ok(resp.data)
}

pub async fn get_job_versions(
    org_id: &str,
    workspace_id: &str,
    job_id: &str,
) -> Result<Vec<Job>, String> {
    let base = base_url();
    let resp: DataResponse<Vec<Job>> = get_json_ws(
        &format!("{base}/v1/jobs/{job_id}/versions"),
        org_id,
        workspace_id,
    )
    .await?;
    Ok(resp.data)
}

// -- Endpoints API (workspace-scoped) --

pub async fn list_endpoints(org_id: &str, workspace_id: &str) -> Result<Vec<Endpoint>, String> {
    let base = base_url();
    let resp: PaginatedResponse<Endpoint> =
        get_json_ws(&format!("{base}/v1/endpoints"), org_id, workspace_id).await?;
    Ok(resp.data)
}

pub async fn create_endpoint(
    org_id: &str,
    workspace_id: &str,
    body: &CreateEndpoint,
) -> Result<Endpoint, String> {
    let base = base_url();
    let resp: DataResponse<Endpoint> = post_json(
        &format!("{base}/v1/endpoints"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn update_endpoint(
    org_id: &str,
    workspace_id: &str,
    name: &str,
    body: &serde_json::Value,
) -> Result<Endpoint, String> {
    let base = base_url();
    let resp: DataResponse<Endpoint> = put_json(
        &format!("{base}/v1/endpoints/{name}"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn delete_endpoint(
    org_id: &str,
    workspace_id: &str,
    name: &str,
) -> Result<(), String> {
    let base = base_url();
    delete_request(
        &format!("{base}/v1/endpoints/{name}"),
        Some((org_id, workspace_id)),
    )
    .await
}

// -- Executions API (workspace-scoped) --

pub async fn list_job_executions(
    org_id: &str,
    workspace_id: &str,
    job_id: &str,
) -> Result<Vec<Execution>, String> {
    let base = base_url();
    let resp: PaginatedResponse<Execution> = get_json_ws(
        &format!("{base}/v1/jobs/{job_id}/executions"),
        org_id,
        workspace_id,
    )
    .await?;
    Ok(resp.data)
}

pub async fn get_execution(
    org_id: &str,
    workspace_id: &str,
    execution_id: &str,
) -> Result<Execution, String> {
    let base = base_url();
    let resp: DataResponse<Execution> = get_json_ws(
        &format!("{base}/v1/executions/{execution_id}"),
        org_id,
        workspace_id,
    )
    .await?;
    Ok(resp.data)
}

pub async fn cancel_execution(
    org_id: &str,
    workspace_id: &str,
    execution_id: &str,
) -> Result<serde_json::Value, String> {
    let base = base_url();
    post_no_body(
        &format!("{base}/v1/executions/{execution_id}/cancel"),
        Some((org_id, workspace_id)),
    )
    .await
}

pub async fn list_attempts(
    org_id: &str,
    workspace_id: &str,
    execution_id: &str,
) -> Result<Vec<Attempt>, String> {
    let base = base_url();
    let resp: DataResponse<Vec<Attempt>> = get_json_ws(
        &format!("{base}/v1/executions/{execution_id}/attempts"),
        org_id,
        workspace_id,
    )
    .await?;
    Ok(resp.data)
}

pub async fn list_execution_logs(
    org_id: &str,
    workspace_id: &str,
    execution_id: &str,
) -> Result<Vec<ExecutionLog>, String> {
    let base = base_url();
    let resp: DataResponse<Vec<ExecutionLog>> = get_json_ws(
        &format!("{base}/v1/executions/{execution_id}/logs"),
        org_id,
        workspace_id,
    )
    .await?;
    Ok(resp.data)
}

// -- Configs API (workspace-scoped) --

pub async fn list_configs(org_id: &str, workspace_id: &str) -> Result<Vec<Config>, String> {
    let base = base_url();
    let resp: PaginatedResponse<Config> =
        get_json_ws(&format!("{base}/v1/configs"), org_id, workspace_id).await?;
    Ok(resp.data)
}

pub async fn create_config(
    org_id: &str,
    workspace_id: &str,
    body: &CreateConfig,
) -> Result<Config, String> {
    let base = base_url();
    let resp: DataResponse<Config> = post_json(
        &format!("{base}/v1/configs"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn update_config(
    org_id: &str,
    workspace_id: &str,
    name: &str,
    body: &UpdateConfig,
) -> Result<Config, String> {
    let base = base_url();
    let resp: DataResponse<Config> = put_json(
        &format!("{base}/v1/configs/{name}"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn delete_config(
    org_id: &str,
    workspace_id: &str,
    name: &str,
) -> Result<(), String> {
    let base = base_url();
    delete_request(
        &format!("{base}/v1/configs/{name}"),
        Some((org_id, workspace_id)),
    )
    .await
}

// -- Payload Specs API (workspace-scoped) --

pub async fn list_payload_specs(
    org_id: &str,
    workspace_id: &str,
) -> Result<Vec<PayloadSpec>, String> {
    let base = base_url();
    let resp: PaginatedResponse<PayloadSpec> =
        get_json_ws(&format!("{base}/v1/payload-specs"), org_id, workspace_id).await?;
    Ok(resp.data)
}

pub async fn create_payload_spec(
    org_id: &str,
    workspace_id: &str,
    body: &CreatePayloadSpec,
) -> Result<PayloadSpec, String> {
    let base = base_url();
    let resp: DataResponse<PayloadSpec> = post_json(
        &format!("{base}/v1/payload-specs"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn update_payload_spec(
    org_id: &str,
    workspace_id: &str,
    name: &str,
    body: &UpdatePayloadSpec,
) -> Result<PayloadSpec, String> {
    let base = base_url();
    let resp: DataResponse<PayloadSpec> = put_json(
        &format!("{base}/v1/payload-specs/{name}"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn delete_payload_spec(
    org_id: &str,
    workspace_id: &str,
    name: &str,
) -> Result<(), String> {
    let base = base_url();
    delete_request(
        &format!("{base}/v1/payload-specs/{name}"),
        Some((org_id, workspace_id)),
    )
    .await
}

// -- Secrets API (workspace-scoped) --

pub async fn list_secrets(org_id: &str, workspace_id: &str) -> Result<Vec<Secret>, String> {
    let base = base_url();
    let resp: PaginatedResponse<Secret> =
        get_json_ws(&format!("{base}/v1/secrets"), org_id, workspace_id).await?;
    Ok(resp.data)
}

pub async fn create_secret(
    org_id: &str,
    workspace_id: &str,
    body: &CreateSecret,
) -> Result<Secret, String> {
    let base = base_url();
    let resp: DataResponse<Secret> = post_json(
        &format!("{base}/v1/secrets"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn update_secret(
    org_id: &str,
    workspace_id: &str,
    name: &str,
    body: &UpdateSecret,
) -> Result<Secret, String> {
    let base = base_url();
    let resp: DataResponse<Secret> = put_json(
        &format!("{base}/v1/secrets/{name}"),
        body,
        Some((org_id, workspace_id)),
    )
    .await?;
    Ok(resp.data)
}

pub async fn delete_secret(
    org_id: &str,
    workspace_id: &str,
    name: &str,
) -> Result<(), String> {
    let base = base_url();
    delete_request(
        &format!("{base}/v1/secrets/{name}"),
        Some((org_id, workspace_id)),
    )
    .await
}
