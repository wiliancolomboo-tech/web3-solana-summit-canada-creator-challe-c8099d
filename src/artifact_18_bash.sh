#!/usr/bin/env bash
set -euo pipefail

echo "==> [1/4] Building Anchor Smart Contract for Solana..."
anchor build

echo "==> [2/4] Deploying to Solana Devnet..."
solana config set --url https://api.devnet.solana.com
PROGRAM_KEYPAIR="target/deploy/creator_nexus-keypair.json"
anchor deploy --provider.cluster devnet

echo "==> [3/4] Exporting IDL for Relayer and Frontends..."
anchor idl init --filepath target/idl/creator_nexus.json $(solana address -k $PROGRAM_KEYPAIR) || \
anchor idl upgrade --filepath target/idl/creator_nexus.json $(solana address -k $PROGRAM_KEYPAIR)

echo "==> [4/4] Launching Enterprise Event & Resend Dispatch Daemon..."
docker compose up -d --build
docker compose ps
echo "==> CreatorNexus successfully deployed and online."