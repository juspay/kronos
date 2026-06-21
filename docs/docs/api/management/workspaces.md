---
id: workspaces
title: Workspaces
title_meta: Workspaces API
---

# Workspaces

Workspaces are the second level of Kronos's multi-tenant hierarchy. Each workspace belongs to an organization and gets its own isolated PostgreSQL schema with dedicated tables for jobs, executions, configs, secrets, endpoints, and payload specs.

```
Organization
  └── Workspace → PostgreSQL schema (org_{org_id}_{slug})
        ├── payload_specs
        ├── configs
        ├── secrets
        ├── endpoints
        ├── jobs
        ├── executions
        ├── attempts
        └── execution_logs
```

## Schema isolation

When a workspace is created, Kronos:

1. Creates a new PostgreSQL schema named `org_{org_id}_{slug}` (e.g. `org_3f8a..._production`)
2. Runs the `workspace_v1.sql` migration to create all tenant-scoped tables in that schema
3. Installs a pg_cron entry for the reaper — a dogfooded CRON sweep that retires expired CRON jobs and unschedules their pg_cron entries

The reaper's cron expression is read from `TE_REAPER_CRON_EXPRESSION` at workspace creation time and baked into the workspace's pg_cron entry. Changing the env var only affects workspaces created afterwards.

:::info
All tenant-scoped operations (payload specs, configs, secrets, endpoints, jobs, executions) require both `X-Org-Id` and `X-Workspace-Id` headers. The API uses these to resolve the correct PostgreSQL schema for the request.
:::

## Authentication and headers

Workspace endpoints require:

```
Authorization: Bearer <api_key>
```

The `org_id` in the URL path can be the organization's UUID **or** its slug. The handler resolves the org by either identifier and uses the canonical `org_id` for schema naming.

## Fields

| Field | Type | Description |
|-------|------|-------------|
| `workspace_id` | string (UUID) | Unique workspace identifier |
| `org_id` | string (UUID) | Parent organization ID |
| `name` | string | Display name of the workspace |
| `slug` | string | URL-friendly identifier (1-25 chars, lowercase letters, digits, interior hyphens) |
| `schema_name` | string | PostgreSQL schema name (e.g. `org_3f8a..._production`) |
| `status` | string | Workspace status (e.g. `active`) |
| `schema_version` | integer | Schema migration version (starts at 1) |
| `created_at` | string (ISO 8601) | Creation timestamp |
| `updated_at` | string (ISO 8601) | Last update timestamp (included in `GET` response) |

### Slug validation

Slugs follow the same rules as organization slugs: 1-25 characters of lowercase letters, digits, and interior hyphens (no leading or trailing hyphens).

## Endpoints

### Create workspace

```
POST /v1/orgs/{org_id}/workspaces
```

Creates a new workspace within an organization. This provisions the PostgreSQL schema and installs the reaper pg_cron entry. Returns `409 Conflict` if a workspace with the same slug already exists in the org.

**Request body:**

```json
{
  "name": "Production",
  "slug": "production"
}
```

**Example:**

```bash
curl -X POST http://localhost:8080/v1/orgs/org_3f8a2b1c-.../workspaces \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Production",
    "slug": "production"
  }'
```

**Response (`201 Created`):**

```json
{
  "data": {
    "workspace_id": "ws_5e2c8d9f-...",
    "org_id": "org_3f8a2b1c-...",
    "name": "Production",
    "slug": "production",
    "schema_name": "org_3f8a2b1c_production",
    "status": "active",
    "schema_version": 1,
    "created_at": "2026-06-20T10:05:00Z"
  }
}
```

**Error responses:**

| Status | Error | Description |
|--------|-------|-------------|
| `400` | `InvalidRequest` | Invalid slug format |
| `404` | `OrgNotFound` | Organization not found |
| `409` | `Conflict` | Workspace with slug already exists in this org |

:::warning
Workspace creation involves schema provisioning and pg_cron entry installation. If the database is under heavy load, this operation may take longer than typical API calls.
:::

---

### List workspaces

```
GET /v1/orgs/{org_id}/workspaces
```

Returns all workspaces within an organization. This endpoint does not currently support pagination.

**Example:**

```bash
curl http://localhost:8080/v1/orgs/org_3f8a2b1c-.../workspaces \
  -H "Authorization: Bearer dev-api-key"
```

**Response (`200 OK`):**

```json
{
  "data": [
    {
      "workspace_id": "ws_5e2c8d9f-...",
      "org_id": "org_3f8a2b1c-...",
      "name": "Production",
      "slug": "production",
      "schema_name": "org_3f8a2b1c_production",
      "status": "active",
      "schema_version": 1,
      "created_at": "2026-06-20T10:05:00Z"
    },
    {
      "workspace_id": "ws_8a1f3b7e-...",
      "org_id": "org_3f8a2b1c-...",
      "name": "Staging",
      "slug": "staging",
      "schema_name": "org_3f8a2b1c_staging",
      "status": "active",
      "schema_version": 1,
      "created_at": "2026-06-20T10:10:00Z"
    }
  ]
}
```

**Error responses:**

| Status | Error | Description |
|--------|-------|-------------|
| `404` | `OrgNotFound` | Organization not found |

:::note
The list response does not include `updated_at` — only the single-object `GET` response includes it.
:::

---

### Get workspace

```
GET /v1/orgs/{org_id}/workspaces/{workspace_id}
```

Returns a single workspace by ID within an organization.

**Example:**

```bash
curl http://localhost:8080/v1/orgs/org_3f8a2b1c-.../workspaces/ws_5e2c8d9f-... \
  -H "Authorization: Bearer dev-api-key"
```

**Response (`200 OK`):**

```json
{
  "data": {
    "workspace_id": "ws_5e2c8d9f-...",
    "org_id": "org_3f8a2b1c-...",
    "name": "Production",
    "slug": "production",
    "schema_name": "org_3f8a2b1c_production",
    "status": "active",
    "schema_version": 1,
    "created_at": "2026-06-20T10:05:00Z",
    "updated_at": "2026-06-20T10:05:00Z"
  }
}
```

**Error responses:**

| Status | Error | Description |
|--------|-------|-------------|
| `404` | `WorkspaceNotFound` | Workspace not found in the specified org |
| `404` | `OrgNotFound` | Organization not found |

## After creating a workspace

Once you have an organization and workspace, you need to include the `X-Org-Id` and `X-Workspace-Id` headers for all tenant-scoped operations:

```bash
# Create a config
curl -X POST http://localhost:8080/v1/configs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: org_3f8a2b1c-..." \
  -H "X-Workspace-Id: ws_5e2c8d9f-..." \
  -H "Content-Type: application/json" \
  -d '{
    "name": "email-service",
    "values": {
      "api_base_url": "https://api.myapp.com"
    }
  }'
```

## See also

- [Organizations](./organizations) — create and manage organizations
- [Configs](./configs-api) — static configuration variables
- [Secrets](./secrets-api) — encrypted secret management
- [Multi-tenancy](../../core-concepts/multi-tenancy) — how schema-per-tenant isolation works
