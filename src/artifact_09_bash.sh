# 1. Build and verify deterministic Anchor artifacts
anchor build

# 2. Configure network target (Solana Devnet or Mainnet-Beta)
solana config set --url https://api.devnet.solana.com

# 3. Deploy the program binary
anchor deploy --program-name solsync

# 4. Initialize Program ID in Anchor.toml and re-sync
anchor keys sync
anchor test --skip-local-validator