-- DB-global objects only. Tenant tables are created per-workspace by
-- workspace_v1.sql; control tables by 20260318000000_multi_tenancy.sql.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS region_heartbeats (
    region        TEXT          NOT NULL,
    component     TEXT          NOT NULL,
    last_beat_at  TIMESTAMPTZ   NOT NULL DEFAULT now(),
    status        TEXT          NOT NULL DEFAULT 'ALIVE',
    metadata      JSONB,
    CONSTRAINT pk_region_heartbeats PRIMARY KEY (region, component)
);

CREATE TABLE IF NOT EXISTS region_status (
    region        TEXT          NOT NULL,
    alive         BOOL          NOT NULL DEFAULT true,
    failed_at     TIMESTAMPTZ,
    adopted_by    TEXT,
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_region_status PRIMARY KEY (region)
);

INSERT INTO region_status (region, alive, updated_at) VALUES ('default', true, now())
ON CONFLICT (region) DO NOTHING;
