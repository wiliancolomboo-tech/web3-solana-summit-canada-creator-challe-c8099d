#!/usr/bin/env bash
set -euo pipefail

echo "=========================================================="
echo "    Deploying Aurora Creator Settlement Protocol          "
echo "=========================================================="

NETWORK="${1:-devnet}"
export ANCHOR_PROVIDER_URL="https://api.${NETWORK}.solana.com"
export ANCHOR_WALLET="${HOME}/.config/solana/id.json"

echo "[1/4] Checking Solana configuration..."
solana config set --url "${ANCHOR_PROVIDER_URL}"

echo "[2/4] Compiling Anchor Smart Contracts..."
anchor build

echo "[3/4] Deploying to Solana ${NETWORK}..."
PROGRAM_DEPLOY_LOG=$(anchor deploy --provider.cluster "${NETWORK}")
echo "${PROGRAM_DEPLOY_LOG}"

PROGRAM_ID=$(solana address -k target/deploy/creator_vault-keypair.json)
echo "Contract deployed successfully! Program ID: ${PROGRAM_ID}"

echo "[4/4] Writing configuration to agent environment..."
cat <<EOF > .env
NODE_ENV=production
SOLANA_RPC_URL=${ANCHOR_PROVIDER_URL}
PROGRAM_ID=${PROGRAM_ID}
DISCORD_WEBHOOK_URL=https://api.example.com/webhooks/alerts/placeholder
RESEND_API_KEY=re_enterprise_live_key
ALERT_FALLBACK_EMAIL=settlement@solanasummit.ca
LOG_LEVEL=info
EOF

echo "Deployment complete. Ready for dockerization."