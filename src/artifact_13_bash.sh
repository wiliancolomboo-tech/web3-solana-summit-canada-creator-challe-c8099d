#!/usr/bin/env bash
set -euo pipefail

echo ">>> [1/4] Building Solana Anchor Smart Contract..."
anchor build

echo ">>> [2/4] Deploying to Solana Devnet..."
solana config set --url https://api.devnet.solana.com
PROGRAM_ID=$(solana address -k target/deploy/creator_engine-keypair.json)
echo "Deploying Creator Engine Program ID: ${PROGRAM_ID}"
anchor deploy --provider.cluster devnet

echo ">>> [3/4] Initializing Environment Credentials..."
cp .env.example .env
sed -i "s|PROGRAM_ID=.*|PROGRAM_ID=${PROGRAM_ID}|g" .env

echo ">>> [4/4] Building Client & Running Comprehensive QA Suite..."
npm install
npm test

echo "✅ Turnkey Deployment and QA Validation Complete!"