cd relayer
npm install
npm run build

# Populate environment
cat <<EOF > .env
SOLANA_RPC_URL=https://api.devnet.solana.com
SOLANA_WS_URL=wss://api.devnet.solana.com
PROGRAM_ID=StrMPas555555555555555555555555555555555555
RESEND_API_KEY=re_test_000000000000000000000000
DISCORD_WEBHOOK_URL=https://api.example.com/webhooks/alerts/placeholder
PORT=8080
EOF

npm run start