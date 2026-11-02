import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { CreatorNexus } from "../target/types/creator_nexus";
import { Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram } from "@solana/web3.js";
import { assert, expect } from "chai";

describe("creator-nexus protocol end-to-end test suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.CreatorNexus as Program<CreatorNexus>;

  const creatorWallet = Keypair.generate();
  const payoutRecipient = Keypair.generate();
  const patronWallet = Keypair.generate();
  const unauthorizedAttacker = Keypair.generate();

  let creatorProfilePda: PublicKey;
  let creatorProfileBump: number;

  let tierPda: PublicKey;
  let tierBump: number;

  let subscriptionPda: PublicKey;
  let subscriptionBump: number;

  let escrowPda: PublicKey;
  let escrowBump: number;

  const RATE_PER_SEC = new anchor.BN(10_000); // 10,000 lamports/sec
  const MIN_DEPOSIT = new anchor.BN(0.5 * LAMPORTS_PER_SOL);
  const INITIAL_PLEDGE = new anchor.BN(1.0 * LAMPORTS_PER_SOL);

  before(async () => {
    // Airdrop balances
    for (const kp of [creatorWallet, patronWallet, unauthorizedAttacker]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 5 * LAMPORTS_PER_SOL);
      const latestBlockhash = await provider.connection.getLatestBlockhash();
      await provider.connection.confirmTransaction({
        signature: sig,
        ...latestBlockhash,
      });
    }

    [creatorProfilePda, creatorProfileBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("creator_profile"), creatorWallet.publicKey.toBuffer()],
      program.programId
    );

    const tierCounterBytes = Buffer.alloc(4);
    tierCounterBytes.writeUInt32LE(0, 0);

    [tierPda, tierBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription_tier"), creatorProfilePda.toBuffer(), tierCounterBytes],
      program.programId
    );

    [subscriptionPda, subscriptionBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription"), tierPda.toBuffer(), patronWallet.publicKey.toBuffer()],
      program.programId
    );

    [escrowPda, escrowBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow_vault"), subscriptionPda.toBuffer()],
      program.programId
    );
  });

  it("1. Initializes a Creator Profile with dedicated payout recipient", async () => {
    await program.methods
      .initializeCreator("montreal_devs")
      .accounts({
        profile: creatorProfilePda,
        payoutRecipient: payoutRecipient.publicKey,
        authority: creatorWallet.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creatorWallet])
      .rpc();

    const profileAccount = await program.account.creatorProfile.fetch(creatorProfilePda);
    assert.equal(profileAccount.handle, "montreal_devs");
    assert.equal(profileAccount.creator.toBase58(), creatorWallet.publicKey.toBase58());
    assert.equal(profileAccount.payoutRecipient.toBase58(), payoutRecipient.publicKey.toBase58());
    assert.equal(profileAccount.tierCounter, 0);
  });

  it("2. Registers a Subscription Tier with dynamic streaming rate", async () => {
    await program.methods
      .initializeTier("VIP Builder Access", RATE_PER_SEC, MIN_DEPOSIT)
      .accounts({
        profile: creatorProfilePda,
        tier: tierPda,
        creator: creatorWallet.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creatorWallet])
      .rpc();

    const tierAccount = await program.account.subscriptionTier.fetch(tierPda);
    assert.equal(tierAccount.title, "VIP Builder Access");
    assert.isTrue(tierAccount.ratePerSecond.eq(RATE_PER_SEC));
    assert.isTrue(tierAccount.isActive);

    const profileAccount = await program.account.creatorProfile.fetch(creatorProfilePda);
    assert.equal(profileAccount.tierCounter, 1);
  });

  it("3. Rejects pledge below minimum deposit requirement", async () => {
    const insufficientDeposit = new anchor.BN(100);
    try {
      await program.methods
        .subscribe(insufficientDeposit)
        .accounts({
          profile: creatorProfilePda,
          tier: tierPda,
          subscription: subscriptionPda,
          escrowVault: escrowPda,
          patron: patronWallet.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([patronWallet])
        .rpc();
      assert.fail("Should have thrown InsufficientBalance error");
    } catch (err: any) {
      expect(err.toString()).to.include("InsufficientBalance");
    }
  });

  it("4. Patron subscribes and locks funds into PDA escrow vault", async () => {
    await program.methods
      .subscribe(INITIAL_PLEDGE)
      .accounts({
        profile: creatorProfilePda,
        tier: tierPda,
        subscription: subscriptionPda,
        escrowVault: escrowPda,
        patron: patronWallet.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([patronWallet])
      .rpc();

    const escrowBalance = await provider.connection.getBalance(escrowPda);
    assert.equal(escrowBalance, INITIAL_PLEDGE.toNumber());

    const subAccount = await program.account.subscriptionAccount.fetch(subscriptionPda);
    assert.isTrue(subAccount.isActive);
    assert.equal(subAccount.patron.toBase58(), patronWallet.publicKey.toBase58());
  });

  it("5. Settles linear stream balance to payout recipient via crank", async () => {
    // Fast-forward wait for linear accrual
    await new Promise((resolve) => setTimeout(resolve, 3000));

    const initialRecipientBalance = await provider.connection.getBalance(payoutRecipient.publicKey);

    await program.methods
      .claimFunds()
      .accounts({
        profile: creatorProfilePda,
        tier: tierPda,
        subscription: subscriptionPda,
        escrowVault: escrowPda,
        payoutRecipient: payoutRecipient.publicKey,
        caller: creatorWallet.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creatorWallet])
      .rpc();

    const postRecipientBalance = await provider.connection.getBalance(payoutRecipient.publicKey);
    assert.isAbove(postRecipientBalance, initialRecipientBalance);
  });

  it("6. Rejects cancellation attempts by unauthorized third-party attacker", async () => {
    try {
      await program.methods
        .cancelSubscription()
        .accounts({
          profile: creatorProfilePda,
          tier: tierPda,
          subscription: subscriptionPda,
          escrowVault: escrowPda,
          payoutRecipient: payoutRecipient.publicKey,
          patron: unauthorizedAttacker.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([unauthorizedAttacker])
        .rpc();
      assert.fail("Attacker must not cancel patron subscription");
    } catch (err: any) {
      expect(err.toString()).to.include("Unauthorized");
    }
  });

  it("7. Patron voluntarily cancels and claims remaining unamortized escrow balance", async () => {
    const patronPreBalance = await provider.connection.getBalance(patronWallet.publicKey);

    await program.methods
      .cancelSubscription()
      .accounts({
        profile: creatorProfilePda,
        tier: tierPda,
        subscription: subscriptionPda,
        escrowVault: escrowPda,
        payoutRecipient: payoutRecipient.publicKey,
        patron: patronWallet.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([patronWallet])
      .rpc();

    const subAccount = await program.account.subscriptionAccount.fetch(subscriptionPda);
    assert.isFalse(subAccount.isActive);

    const patronPostBalance = await provider.connection.getBalance(patronWallet.publicKey);
    assert.isAbove(patronPostBalance, patronPreBalance);
  });
});