#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${KRONOS_BASE_URL:-http://localhost:8080}"
API_KEY="${TE_API_KEY:-dev-api-key}"
ENV_FILE="${1:-.env}"

ORG_NAME="${KRONOS_ORG_NAME:-Dev Org}"
ORG_SLUG="${KRONOS_ORG_SLUG:-dev-org}"
WS_NAME="${KRONOS_WS_NAME:-Dev Workspace}"
WS_SLUG="${KRONOS_WS_SLUG:-dev-ws}"

AUTH="Authorization: Bearer ${API_KEY}"

echo "Creating org '${ORG_NAME}' (slug: ${ORG_SLUG})..."
ORG_RESPONSE=$(curl -sf -X POST "${BASE_URL}/v1/orgs" \
  -H "${AUTH}" \
  -H "Content-Type: application/json" \
  -d "{\"name\": \"${ORG_NAME}\", \"slug\": \"${ORG_SLUG}\"}")

ORG_ID=$(echo "${ORG_RESPONSE}" | jq -r '.data.org_id')
echo "Org created: ${ORG_ID}"

echo "Creating workspace '${WS_NAME}' (slug: ${WS_SLUG})..."
WS_RESPONSE=$(curl -sf -X POST "${BASE_URL}/v1/orgs/${ORG_ID}/workspaces" \
  -H "${AUTH}" \
  -H "Content-Type: application/json" \
  -d "{\"name\": \"${WS_NAME}\", \"slug\": \"${WS_SLUG}\"}")

WS_ID=$(echo "${WS_RESPONSE}" | jq -r '.data.workspace_id')
echo "Workspace created: ${WS_ID}"

# Update .env file
if [ ! -f "${ENV_FILE}" ]; then
  echo "Warning: ${ENV_FILE} not found, creating it"
  touch "${ENV_FILE}"
fi

# Remove old values if present, then append new ones
sed -i '/^#\?\s*KRONOS_ORG_ID=/d' "${ENV_FILE}"
sed -i '/^#\?\s*KRONOS_WORKSPACE_ID=/d' "${ENV_FILE}"

echo "KRONOS_ORG_ID=${ORG_ID}" >> "${ENV_FILE}"
echo "KRONOS_WORKSPACE_ID=${WS_ID}" >> "${ENV_FILE}"

echo "Updated ${ENV_FILE}:"
echo "  KRONOS_ORG_ID=${ORG_ID}"
echo "  KRONOS_WORKSPACE_ID=${WS_ID}"
