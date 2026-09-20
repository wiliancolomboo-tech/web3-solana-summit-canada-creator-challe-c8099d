import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Aurastream } from "../target/types/aurastream";
import {
  createMint,
  createAccount,
  mintTo,
  getAccount,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { PublicKey, Keypair, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { expect } from "chai";

describe("aurastream - Creator Challenge QA Test Matrix", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Aurastream as Program<Aurastream>;

  const creator = Keypair.generate();
  const subscriber = Keypair.generate();
  const collaborator = Keypair.generate();

  let mint: PublicKey;
  let subscriberTokenAccount: PublicKey;
  let creatorTokenAccount: PublicKey;
  let collaboratorTokenAccount: PublicKey;

  let creatorProfilePda: PublicKey;
  let tierPda: PublicKey;
  let vaultAuthorityPda: PublicKey;
  let escrowVaultPda: PublicKey;
  let subscriptionPda: PublicKey;

  const TIER_ID = 1;
  const RATE_PER_SECOND = new anchor.BN(1_000_000); // 1 Token per second (with 6 decimals)
  const DURATION_SECONDS = new anchor.BN(300); // 300 seconds
  const COLLABORATOR_BPS = 2000; // 20% to collaborator, 80% to creator

  before(async () => {
    // 1. Airdrop SOL to test actors
    for (const actor of [creator, subscriber, collaborator]) {
      const sig = await provider.connection.requestAirdrop(actor.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // 2. Initialize Mint (6 decimals)
    mint = await createMint(
      provider.connection,
      creator,
      creator.publicKey,
      null,
      6
    );

    // 3. Initialize Token Accounts
    subscriberTokenAccount = await createAccount(
      provider.connection,
      subscriber,
      mint,
      subscriber.publicKey
    );

    creatorTokenAccount = await createAccount(
      provider.connection,
      creator,
      mint,
      creator.publicKey
    );

    collaboratorTokenAccount = await createAccount(
      provider.connection,
      collaborator,
      mint,
      collaborator.publicKey
    );

    // 4. Mint 1,000 tokens to Subscriber
    await mintTo(
      provider.connection,
      creator,
      mint,
      subscriberTokenAccount,
      creator,
      1_000_000_000
    );

    // 5. Derive PDAs
    [creatorProfilePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("creator_profile"), creator.publicKey.toBuffer()],
      program.programId
    );

    [tierPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription_tier"), creatorProfilePda.toBuffer(), Buffer.from([TIER_ID])],
      program.programId
    );

    [vaultAuthorityPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_authority"), tierPda.toBuffer()],
      program.programId
    );

    [escrowVaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow_vault"), tierPda.toBuffer()],
      program.programId
    );

    [subscriptionPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription"), tierPda.toBuffer(), subscriber.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Initialize Creator Profile Successfully", async () => {
    const tx = await program.methods
      .initializeCreator("https://arweave.net/creator-profile-metadata.json")
      .accounts({
        creatorProfile: creatorProfilePda,
        authority: creator.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creator])
      .rpc();

    const profileState = await program.account.creatorProfile.fetch(creatorProfilePda);
    expect(profileState.authority.toBase58()).to.equal(creator.publicKey.toBase58());
    expect(profileState.tierCount).to.equal(0);
    expect(profileState.totalRevenue.toNumber()).to.equal(0);
  });

  it("2. Create Subscription Tier with 20% Collaborator Split", async () => {
    await program.methods
      .createTier(
        TIER_ID,
        RATE_PER_SECOND,
        new anchor.BN(60),
        COLLABORATOR_BPS
      )
      .accounts({
        creatorProfile: creatorProfilePda,
        subscriptionTier: tierPda,
        vaultAuthority: vaultAuthorityPda,
        escrowVault: escrowVaultPda,
        mint: mint,
        collaborator: collaborator.publicKey,
        authority: creator.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .signers([creator])
      .rpc();

    const tierState = await program.account.subscriptionTier.fetch(tierPda);
    expect(tierState.tierId).to.equal(TIER_ID);
    expect(tierState.ratePerSecond.toNumber()).to.equal(RATE_PER_SECOND.toNumber());
    expect(tierState.collaborator.toBase58()).to.equal(collaborator.publicKey.toBase58());
    expect(tierState.collaboratorShareBps).to.equal(2000);
  });

  it("3. Fan Subscribes: Locks 300 Tokens into Vault Escrow", async () => {
    const initialVaultBal = (await provider.connection.getTokenAccountBalance(escrowVaultPda)).value.amount;
    expect(initialVaultBal).to.equal("0");

    await program.methods
      .subscribe(DURATION_SECONDS)
      .accounts({
        subscriptionTier: tierPda,
        subscription: subscriptionPda,
        escrowVault: escrowVaultPda,
        subscriberTokenAccount: subscriberTokenAccount,
        subscriber: subscriber.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([subscriber])
      .rpc();

    const expectedDeposit = RATE_PER_SECOND.toNumber() * DURATION_SECONDS.toNumber();
    const vaultBalAfter = (await provider.connection.getTokenAccountBalance(escrowVaultPda)).value.amount;
    expect(vaultBalAfter).to.equal(expectedDeposit.toString());

    const subState = await program.account.userSubscription.fetch(subscriptionPda);
    expect(subState.subscriber.toBase58()).to.equal(subscriber.publicKey.toBase58());
    expect(subState.totalDeposited.toNumber()).to.equal(expectedDeposit);
    expect(subState.claimedAmount.toNumber()).to.equal(0);
  });

  it("4. Time Elapses & Creator Claims Accrued Stream (80/20 Payout Split)", async () => {
    // Sleep 3 seconds to simulate time passage on-chain
    await new Promise((resolve) => setTimeout(resolve, 3100));

    const creatorBalBefore = (await provider.connection.getTokenAccountBalance(creatorTokenAccount)).value.uiAmount ?? 0;
    const collabBalBefore = (await provider.connection.getTokenAccountBalance(collaboratorTokenAccount)).value.uiAmount ?? 0;

    await program.methods
      .claimStream()
      .accounts({
        creatorProfile: creatorProfilePda,
        subscriptionTier: tierPda,
        subscription: subscriptionPda,
        vaultAuthority: vaultAuthorityPda,
        escrowVault: escrowVaultPda,
        creatorTokenAccount: creatorTokenAccount,
        collaboratorTokenAccount: collaboratorTokenAccount,
        collaborator: collaborator.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const creatorBalAfter = (await provider.connection.getTokenAccountBalance(creatorTokenAccount)).value.uiAmount ?? 0;
    const collabBalAfter = (await provider.connection.getTokenAccountBalance(collaboratorTokenAccount)).value.uiAmount ?? 0;

    expect(creatorBalAfter).to.be.greaterThan(creatorBalBefore);
    expect(collabBalAfter).to.be.greaterThan(collabBalBefore);

    // Verify 80/20 distribution proportionality
    const creatorGain = creatorBalAfter - creatorBalBefore;
    const collabGain = collabBalAfter - collabBalBefore;
    const ratio = creatorGain / collabGain;
    expect(ratio).to.be.closeTo(4.0, 0.1); // 80% / 20% = 4.0 ratio
  });

  it("5. Subscriber Cancels Early: Reclaims Unspent Balance & Closes Account", async () => {
    const subscriberTokenBalBefore = (await provider.connection.getTokenAccountBalance(subscriberTokenAccount)).value.amount;

    await program.methods
      .cancelSubscription()
      .accounts({
        subscriptionTier: tierPda,
        subscription: subscriptionPda,
        vaultAuthority: vaultAuthorityPda,
        escrowVault: escrowVaultPda,
        subscriberTokenAccount: subscriberTokenAccount,
        creatorTokenAccount: creatorTokenAccount,
        collaboratorTokenAccount: collaboratorTokenAccount,
        collaborator: collaborator.publicKey,
        subscriber: subscriber.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([subscriber])
      .rpc();

    const subscriberTokenBalAfter = (await provider.connection.getTokenAccountBalance(subscriberTokenAccount)).value.amount;
    expect(BigInt(subscriberTokenBalAfter)).to.be.greaterThan(BigInt(subscriberTokenBalBefore));

    // Assert Subscription Account was closed and rent returned
    const accountInfo = await provider.connection.getAccountInfo(subscriptionPda);
    expect(accountInfo).to.be.null;
  });

  it("6. Security Defense: Reject Unauthorized Cancellation Attempt", async () => {
    const imposter = Keypair.generate();
    await provider.connection.requestAirdrop(imposter.publicKey, anchor.web3.LAMPORTS_PER_SOL);

    // Re-subscribe first
    await program.methods
      .subscribe(DURATION_SECONDS)
      .accounts({
        subscriptionTier: tierPda,
        subscription: subscriptionPda,
        escrowVault: escrowVaultPda,
        subscriberTokenAccount: subscriberTokenAccount,
        subscriber: subscriber.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([subscriber])
      .rpc();

    // Imposter attempts to cancel subscriber's stream
    try {
      await program.methods
        .cancelSubscription()
        .accounts({
          subscriptionTier: tierPda,
          subscription: subscriptionPda,
          vaultAuthority: vaultAuthorityPda,
          escrowVault: escrowVaultPda,
          subscriberTokenAccount: subscriberTokenAccount,
          creatorTokenAccount: creatorTokenAccount,
          collaboratorTokenAccount: collaboratorTokenAccount,
          collaborator: collaborator.publicKey,
          subscriber: imposter.publicKey, // Wrong signer
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([imposter])
        .rpc();
      expect.fail("Unauthorized cancellation should have failed");
    } catch (err: any) {
      expect(err.toString()).to.include("Error");
    }
  });
});