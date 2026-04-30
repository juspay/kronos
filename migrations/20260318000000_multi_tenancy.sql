-- Multi-tenancy: organizations and workspaces

CREATE SCHEMA IF NOT EXISTS {{system_schema}};

CREATE TABLE IF NOT EXISTS {{system_schema}}.organizations (
    org_id      TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    name        TEXT        NOT NULL,
    slug        TEXT        NOT NULL UNIQUE,
    status      TEXT        NOT NULL DEFAULT 'ACTIVE',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_organizations PRIMARY KEY (org_id),
    CONSTRAINT chk_org_status CHECK (status IN ('ACTIVE', 'SUSPENDED', 'DELETED'))
);

CREATE TABLE IF NOT EXISTS {{system_schema}}.workspaces (
    workspace_id    TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    org_id          TEXT        NOT NULL,
    name            TEXT        NOT NULL,
    slug            TEXT        NOT NULL,
    schema_name     TEXT        NOT NULL UNIQUE,
    status          TEXT        NOT NULL DEFAULT 'ACTIVE',
    schema_version  BIGINT      NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_workspaces PRIMARY KEY (workspace_id),
    CONSTRAINT fk_workspaces_org FOREIGN KEY (org_id) REFERENCES {{system_schema}}.organizations (org_id),
    CONSTRAINT uq_workspace_slug UNIQUE (org_id, slug),
    CONSTRAINT chk_ws_status CHECK (status IN ('ACTIVE', 'SUSPENDED', 'DELETED'))
);

CREATE INDEX IF NOT EXISTS idx_workspaces_org ON {{system_schema}}.workspaces (org_id);
CREATE INDEX IF NOT EXISTS idx_workspaces_status ON {{system_schema}}.workspaces (status);
