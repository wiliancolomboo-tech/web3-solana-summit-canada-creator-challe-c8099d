import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Substream } from "../target/types/substream";
import { PublicKey, Keypair, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { expect } from "chai";

describe("substream test matrix", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Substream as Program<Substream>;

  const creatorKeypair = Keypair.generate();
  const subscriberKeypair = Keypair.generate();

  let creatorProfilePda: PublicKey;
  let tierPda: PublicKey;
  let subscriptionPda: PublicKey;
  let vaultPda: PublicKey;

  const tierId = 1;
  const costLamports = new anchor.BN(0.1 * LAMPORTS_PER_SOL);
  const cadenceSeconds = new anchor.BN(3600); // 1 hour

  before(async () => {
    // Airdrop SOL to test actors
    const airdropCreator = await provider.connection.requestAirdrop(
      creatorKeypair.publicKey,
      2 * LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(airdropCreator);

    const airdropSubscriber = await provider.connection.requestAirdrop(
      subscriberKeypair.publicKey,
      2 * LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(airdropSubscriber);

    // Derive PDAs
    [creatorProfilePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("creator-profile"), creatorKeypair.publicKey.toBuffer()],
      program.programId
    );

    [tierPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription-tier"), creatorKeypair.publicKey.toBuffer(), Buffer.from([tierId])],
      program.programId
    );

    [subscriptionPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription"), tierPda.toBuffer(), subscriberKeypair.publicKey.toBuffer()],
      program.programId
    );

    [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), creatorKeypair.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Successfully registers a creator profile", async () => {
    const tx = await program.methods
      .initializeCreator("Alice Studio", "https://arweave.net/sample-metadata")
      .accounts({
        creatorProfile: creatorProfilePda,
        creator: creatorKeypair.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creatorKeypair])
      .rpc();

    expect(tx).to.be.a("string");

    const profile = await program.account.creatorProfile.fetch(creatorProfilePda);
    expect(profile.creatorName).to.equal("Alice Studio");
    expect(profile.creator.toBase58()).to.equal(creatorKeypair.publicKey.toBase58());
    expect(profile.totalSubscribers.toNumber()).to.equal(0);
  });

  it("2. Successfully configures an automated subscription tier", async () => {
    await program.methods
      .createTier(tierId, "Tier Gold", costLamports, cadenceSeconds)
      .accounts({
        creatorProfile: creatorProfilePda,
        tier: tierPda,
        creator: creatorKeypair.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creatorKeypair])
      .rpc();

    const tier = await program.account.subscriptionTier.fetch(tierPda);
    expect(tier.name).to.equal("Tier Gold");
    expect(tier.costLamports.toNumber()).to.equal(costLamports.toNumber());
    expect(tier.isActive).to.be.true;
  });

  it("3. Executes subscription onboarding and verifies vault transfer", async () => {
    const periods = 2;
    const initialVaultBalance = await provider.connection.getBalance(vaultPda);

    await program.methods
      .subscribe(periods)
      .accounts({
        creatorProfile: creatorProfilePda,
        tier: tierPda,
        subscription: subscriptionPda,
        vault: vaultPda,
        subscriber: subscriberKeypair.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([subscriberKeypair])
      .rpc();

    const expectedCost = costLamports.toNumber() * periods;
    const postVaultBalance = await provider.connection.getBalance(vaultPda);
    expect(postVaultBalance - initialVaultBalance).to.equal(expectedCost);

    const subscription = await program.account.subscriptionRecord.fetch(subscriptionPda);
    expect(subscription.subscriber.toBase58()).to.equal(subscriberKeypair.publicKey.toBase58());
    expect(subscription.totalPaid.toNumber()).to.equal(expectedCost);
    expect(subscription.expiresAt.toNumber()).to.be.greaterThan(subscription.startedAt.toNumber());
  });

  it("4. Rejects premature subscription account closure before expiry", async () => {
    try {
      await program.methods
        .closeExpiredSubscription()
        .accounts({
          creatorProfile: creatorProfilePda,
          tier: tierPda,
          subscription: subscriptionPda,
          subscriber: subscriberKeypair.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([subscriberKeypair])
        .rpc();
      expect.fail("Should have failed due to active subscription");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("SubscriptionStillActive");
    }
  });

  it("5. Allows creator to withdraw accrued earnings safely", async () => {
    const withdrawalAmount = new anchor.BN(0.1 * LAMPORTS_PER_SOL);
    const creatorPreBal = await provider.connection.getBalance(creatorKeypair.publicKey);

    await program.methods
      .withdrawEarnings(withdrawalAmount)
      .accounts({
        creatorProfile: creatorProfilePda,
        vault: vaultPda,
        creator: creatorKeypair.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creatorKeypair])
      .rpc();

    const creatorPostBal = await provider.connection.getBalance(creatorKeypair.publicKey);
    expect(creatorPostBal).to.be.greaterThan(creatorPreBal);
  });
});