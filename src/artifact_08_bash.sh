# 1. Clone repository & configure environment
git clone https://github.com/summit-canada-bounty/creator-ops-protocol.git
cd creator-ops-protocol

# 2. Build on-chain Anchor program
anchor build

# 3. Synchronize program keys
solana address -k target/deploy/creator_ops-keypair.json
# (Paste output into declare_id!("...") inside programs/creator_ops/src/lib.rs)

# 4. Deploy to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --provider.cluster devnet

# 5. Run test suite against real local-validator / devnet
anchor test