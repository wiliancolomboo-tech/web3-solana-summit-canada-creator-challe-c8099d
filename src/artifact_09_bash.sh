# Set network to Devnet
solana config set --url https://api.devnet.solana.com

# Fund deployer account
solana airdrop 2

# Build with devnet profile
anchor build

# Deploy program
anchor deploy --provider.cluster devnet

# Verify deployment
solana program show <PROGRAM_ID>