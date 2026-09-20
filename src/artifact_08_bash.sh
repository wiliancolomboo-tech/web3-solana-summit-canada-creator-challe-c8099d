# 1. Clone the project and navigate to directory
git clone https://github.com/aurastream/aurastream-protocol.git
cd aurastream-protocol

# 2. Install Node dependencies
yarn install

# 3. Configure Solana localnet environment
solana config set --url localhost

# 4. Generate local keypair if not present
solana-keygen new --no-bip39-passphrase -s -o ~/.config/solana/id.json

# 5. Build the Anchor program to generate binary & IDL
anchor build

# 6. Synchronize program address in lib.rs and Anchor.toml
# Note: Extracts the compiled program address and aligns declarations
anchor keys sync

# 7. Run full integration test suite on isolated localnet
anchor test