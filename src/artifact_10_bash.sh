# 1. Build and verify the Anchor program
anchor build

# 2. Deploy to Solana Devnet (or Mainnet)
solana config set --url https://api.devnet.solana.com
anchor deploy --program-name creator_vault --program-keypair target/deploy/creator_vault-keypair.json

# 3. Configure environment variables for the Relayer Daemon
cp .env.example .env
# Edit .env with your RESEND_API_KEY and DISCORD_WEBHOOK_URL

# 4. Launch the Enterprise Sentinel Node via Docker
docker-compose up -d --build

# 5. Monitor logs and verify connection
docker-compose logs -f sentinel-relayer