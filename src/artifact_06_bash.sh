# 1. Install dependencies
yarn install

# 2. Build Anchor smart contract
anchor build

# 3. Synchronize program ID
anchor keys sync

# 4. Execute test suite against local validator
anchor test

# 5. Deploy to Devnet
solana config set --url https://api.devnet.solana.com
anchor deploy --provider.cluster devnet