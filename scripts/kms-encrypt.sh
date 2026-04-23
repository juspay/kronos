#!/usr/bin/env bash
# Encrypts a plaintext value using the LocalStack KMS key created by kms-init.sh
# Usage: ./scripts/kms-encrypt.sh "my secret value"
#        ./scripts/kms-encrypt.sh -k <key-id> "my secret value"

set -euo pipefail

ENDPOINT="${AWS_ENDPOINT_URL:-http://localhost:4566}"
REGION="${AWS_REGION:-us-east-1}"

export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-test}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-test}"
export AWS_DEFAULT_REGION="$REGION"

KEY_ID=""
PLAINTEXT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -k|--key-id)
            KEY_ID="$2"
            shift 2
            ;;
        *)
            PLAINTEXT="$1"
            shift
            ;;
    esac
done

if [ -z "$PLAINTEXT" ]; then
    echo "Usage: $0 [-k key-id] <plaintext>" >&2
    exit 1
fi

# Default: read key ID from file created by kms-init.sh
if [ -z "$KEY_ID" ]; then
    if [ -f .kms-key-id ]; then
        KEY_ID=$(cat .kms-key-id)
    else
        echo "Error: no key ID provided and .kms-key-id not found." >&2
        echo "Run 'just kms-init' first, or pass -k <key-id>." >&2
        exit 1
    fi
fi

ENCRYPTED=$(aws kms encrypt \
    --endpoint-url "$ENDPOINT" \
    --region "$REGION" \
    --key-id "$KEY_ID" \
    --plaintext "$PLAINTEXT" \
    --cli-binary-format raw-in-base64-out \
    --query 'CiphertextBlob' \
    --output text)

echo "$ENCRYPTED"
