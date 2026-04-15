-- Migrate secrets from encrypted DB storage to external KMS references.
-- This migration must be applied to each workspace schema.
-- Before running: ensure all existing secrets have been re-created in your KMS
-- and their references are known.

ALTER TABLE secrets DROP COLUMN IF EXISTS encrypted_value;
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS provider TEXT NOT NULL DEFAULT 'aws';
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS reference TEXT NOT NULL DEFAULT '';
ALTER TABLE secrets ALTER COLUMN provider DROP DEFAULT;
ALTER TABLE secrets ALTER COLUMN reference DROP DEFAULT;
