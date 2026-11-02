# 1. Clone repository and install dependencies
git clone https://github.com/veristream/veristream-protocol.git
cd veristream-protocol
npm install

# 2. Build Anchor Smart Contracts
anchor build

# 3. Synchronize program keys
anchor keys sync

# 4. Run automated test suite
anchor test

# 5. Deploy to Solana Devnet
solana config set --url https://api.devnet.solana.com
solana airdrop 2
anchor deploy --provider.cluster devnet