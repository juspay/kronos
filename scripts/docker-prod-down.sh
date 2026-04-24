#!/usr/bin/env bash
# Tears down the prod-like Docker environment and cleans up generated files.

set -euo pipefail

echo "==> Stopping all services and removing volumes..."
docker compose -f docker-compose.prod.yml down -v

rm -f .env.prod.kms
echo "==> Cleanup complete."
