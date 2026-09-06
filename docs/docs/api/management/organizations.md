---
id: organizations
title: Organizations
title_meta: Organizations API
---

# Organizations

Organizations are the top-level tenant entity in Invokr's multi-tenant hierarchy. Every workspace belongs to an organization. Organizations live in the `public` PostgreSQL schema and are shared across all tenants.

```
Organization (public schema)
  └── Workspace (tenant schema: org_workspace)
        ├── Payload Specs
        ├── Configs
        ├── Secrets
        ├── Endpoints
        ├── Jobs
        └── Executions
```

Organization and workspace management endpoints do **not** require `X-Org-Id` or `X-Workspace-Id` headers. All other endpoints (payload specs, configs, secrets, endpoints, jobs, executions) require both headers.

## Authentication

All organization endpoints require a bearer token:

```
Authorization: Bearer <api_key>
```

The default API key for development is `dev-api-key`. Set `INVOKR_API_KEY` in production.

## Fields

| Field | Type | Description |
|-------|------|-------------|
| `org_id` | string (UUID) | Unique organization identifier |
| `name` | string | Display name of the organization |
| `slug` | string | URL-friendly identifier (1-25 chars, lowercase letters, digits, interior hyphens) |
| `status` | string | Organization status (e.g. `active`) |
| `created_at` | string (ISO 8601) | Creation timestamp |
| `updated_at` | string (ISO 8601) | Last update timestamp (included in `GET` and `PUT` responses) |

### Slug validation

Slugs must be 1-25 characters of lowercase letters, digits, and interior hyphens (no leading or trailing hyphens). Examples:

- `my-company` ✓
- `acme-corp-123` ✓
- `My-Company` ✗ (uppercase)
- `-my-company` ✗ (leading hyphen)
- `my-company-` ✗ (trailing hyphen)

## Endpoints

### Create organization

```
POST /v1/orgs
```

Creates a new organization. Returns `409 Conflict` if an organization with the same slug already exists.

**Request body:**

```json
{
  "name": "My Company",
  "slug": "my-company"
}
```

**Example:**

```bash
curl -X POST http://localhost:8080/v1/orgs \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "My Company",
    "slug": "my-company"
  }'
```

**Response (`201 Created`):**

```json
{
  "data": {
    "org_id": "org_3f8a2b1c-...",
    "name": "My Company",
    "slug": "my-company",
    "status": "active",
    "created_at": "2026-06-20T10:00:00Z"
  }
}
```

**Error responses:**

| Status | Error | Description |
|--------|-------|-------------|
| `400` | `InvalidRequest` | Invalid slug format |
| `409` | `Conflict` | Organization with slug already exists |

---

### List organizations

```
GET /v1/orgs
```

Returns all organizations. This endpoint does not currently support pagination.

**Example:**

```bash
curl http://localhost:8080/v1/orgs \
  -H "Authorization: Bearer dev-api-key"
```

**Response (`200 OK`):**

```json
{
  "data": [
    {
      "org_id": "org_3f8a2b1c-...",
      "name": "My Company",
      "slug": "my-company",
      "status": "active",
      "created_at": "2026-06-20T10:00:00Z"
    },
    {
      "org_id": "org_7c4d9e2a-...",
      "name": "Acme Corp",
      "slug": "acme-corp",
      "status": "active",
      "created_at": "2026-06-19T14:30:00Z"
    }
  ]
}
```

:::note
The list response does not include `updated_at` — only the single-object `GET` and `PUT` responses include it.
:::

---

### Get organization

```
GET /v1/orgs/{org_id}
```

Returns a single organization by ID. The `org_id` path parameter can be the organization's UUID or its slug.

**Example:**

```bash
curl http://localhost:8080/v1/orgs/org_3f8a2b1c-... \
  -H "Authorization: Bearer dev-api-key"
```

**Response (`200 OK`):**

```json
{
  "data": {
    "org_id": "org_3f8a2b1c-...",
    "name": "My Company",
    "slug": "my-company",
    "status": "active",
    "created_at": "2026-06-20T10:00:00Z",
    "updated_at": "2026-06-20T10:00:00Z"
  }
}
```

**Error responses:**

| Status | Error | Description |
|--------|-------|-------------|
| `404` | `OrgNotFound` | Organization not found |

---

### Update organization

```
PUT /v1/orgs/{org_id}
```

Updates an organization's name. Only the `name` field can be updated — the `slug` is immutable after creation.

**Request body:**

```json
{
  "name": "My Company (Renamed)"
}
```

**Example:**

```bash
curl -X PUT http://localhost:8080/v1/orgs/org_3f8a2b1c-... \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "My Company (Renamed)"
  }'
```

**Response (`200 OK`):**

```json
{
  "data": {
    "org_id": "org_3f8a2b1c-...",
    "name": "My Company (Renamed)",
    "slug": "my-company",
    "status": "active",
    "created_at": "2026-06-20T10:00:00Z",
    "updated_at": "2026-06-20T11:30:00Z"
  }
}
```

**Error responses:**

| Status | Error | Description |
|--------|-------|-------------|
| `400` | `InvalidRequest` | `name` field is missing |
| `404` | `OrgNotFound` | Organization not found |

:::info
The `name` field is required in the update request body (it is not optional in the API handler, despite the `UpdateOrganization` struct using `Option<String>`). If `name` is not provided, the API returns a `400 InvalidRequest` error.
:::

## After creating an organization

Once you have an organization, the next step is to create a workspace within it:

```bash
curl -X POST http://localhost:8080/v1/orgs/org_3f8a2b1c-.../workspaces \
  -H "Authorization: Bearer dev-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Production",
    "slug": "production"
  }'
```

See [Workspaces](./workspaces) for the full workspace API.

## See also

- [Workspaces](./workspaces) — create and manage workspaces within organizations
- [Configs](./configs) — static configuration variables
- [Secrets](./secrets) — encrypted secret management
- [Multi-tenancy](../../core-concepts/multi-tenancy) — how schema-per-tenant isolation works
