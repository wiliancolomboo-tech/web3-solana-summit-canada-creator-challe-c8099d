# Terminal 1: Run local test validator
solana-test-validator --reset

# Terminal 2: Deploy to localnet
anchor deploy --provider.cluster localnet

# Verify program account
solana program show <PROGRAM_ID> --url localhost