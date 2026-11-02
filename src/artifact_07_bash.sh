# 1. Clone & Configure Environment
git clone https://github.com/aurora-split/solana-summit-challenge.git
cd solana-summit-challenge
cp .env.example .env

# Edit .env with your RESEND_API_KEY, DISCORD_WEBHOOK_URL, and RPC endpoint

# 2. Build Anchor Smart Contracts
anchor build

# 3. Run Deterministic Test Suite
anchor test

# 4. Deploy Program to Solana Devnet
solana program deploy target/deploy/aurora_split.so --url devnet

# 5. Launch Sentinel Relayer via Docker Compose
docker-compose up -d --build