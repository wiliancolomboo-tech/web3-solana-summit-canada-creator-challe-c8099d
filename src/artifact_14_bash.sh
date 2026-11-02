# Build Anchor binary
anchor build

# Sync Program ID into lib.rs and Anchor.toml
anchor keys sync

# Deploy to Devnet with authority keypair
solana config set --url https://api.devnet.solana.com
anchor deploy --program-name solstream_creator_core --provider.cluster devnet