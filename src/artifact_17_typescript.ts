import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SubscribeOs } from "../target/types/subscribe_os";
import { PublicKey, Keypair, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { assert } from "chai";

describe("SubScribeOS Enterprise Test Suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SubscribeOs as Program<SubscribeOs>;

  const creatorAuthority = Keypair.generate();
  const creatorPayout = Keypair.generate();
  const subscriber = Keypair.generate();
  const unauthorizedUser = Keypair.generate();

  const TIER_RATE = new anchor.BN(0.1 * LAMPORTS_PER_SOL); // 0.1 SOL per epoch
  const EPOCH_DURATION = new anchor.BN(60); // 60 seconds for determinism

  let creatorProfilePda: PublicKey;
  let subscriptionStatePda: PublicKey;
  let protocolTreasuryPda: PublicKey;

  before(async () => {
    // Airdrop balances
    for (const kp of [creatorAuthority, subscriber, unauthorizedUser]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 5 * LAMPORTS_PER_SOL);
      const latestBlockhash = await provider.connection.getLatestBlockhash();
      await provider.connection.confirmTransaction({
        signature: sig,
        blockhash: latestBlockhash.blockhash,
        lastValidBlockHeight: latestBlockhash.lastValidBlockHeight,
      });
    }

    [creatorProfilePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("creator_profile"), creatorAuthority.publicKey.toBuffer()],
      program.programId
    );

    [protocolTreasuryPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("protocol_treasury")],
      program.programId
    );

    [subscriptionStatePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription_state"), creatorProfilePda.toBuffer(), subscriber.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Successfully initializes a Creator Profile with tier pricing", async () => {
    await program.methods
      .initializeCreator("CanadaTechStream", TIER_RATE, EPOCH_DURATION)
      .accounts({
        creatorProfile: creatorProfilePda,
        authority: creatorAuthority.publicKey,
        payoutWallet: creatorPayout.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creatorAuthority])
      .rpc();

    const profile = await program.account.creatorProfile.fetch(creatorProfilePda);
    assert.equal(profile.name, "CanadaTechStream");
    assert.isTrue(profile.authority.equals(creatorAuthority.publicKey));
    assert.isTrue(profile.payoutWallet.equals(creatorPayout.publicKey));
    assert.equal(profile.tierRateLamports.toString(), TIER_RATE.toString());
    assert.equal(profile.totalSubscribers.toNumber(), 0);
  });

  it("2. Rejects tier initialization with 0 rate lamports", async () => {
    const dummyCreator = Keypair.generate();
    const sig = await provider.connection.requestAirdrop(dummyCreator.publicKey, 1 * LAMPORTS_PER_SOL);
    const latestBlockhash = await provider.connection.getLatestBlockhash();
    await provider.connection.confirmTransaction({
      signature: sig,
      blockhash: latestBlockhash.blockhash,
      lastValidBlockHeight: latestBlockhash.lastValidBlockHeight,
    });

    const [dummyPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("creator_profile"), dummyCreator.publicKey.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .initializeCreator("ZeroTier", new anchor.BN(0), EPOCH_DURATION)
        .accounts({
          creatorProfile: dummyPda,
          authority: dummyCreator.publicKey,
          payoutWallet: dummyCreator.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([dummyCreator])
        .rpc();
      assert.fail("Should have thrown InvalidTierConfig error");
    } catch (err: any) {
      assert.include(err.toString(), "InvalidTierConfig");
    }
  });

  it("3. Fan subscribes and deposits prepaid escrow buffer", async () => {
    const initialPayoutBalance = await provider.connection.getBalance(creatorPayout.publicKey);
    const escrowDeposit = new anchor.BN(0.2 * LAMPORTS_PER_SOL); // 2 epochs in reserve

    await program.methods
      .subscribe(escrowDeposit)
      .accounts({
        subscriptionState: subscriptionStatePda,
        creatorProfile: creatorProfilePda,
        protocolTreasury: protocolTreasuryPda,
        payoutWallet: creatorPayout.publicKey,
        subscriber: subscriber.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([subscriber])
      .rpc();

    const sub = await program.account.subscriptionState.fetch(subscriptionStatePda);
    assert.isTrue(sub.isActive);
    assert.isTrue(sub.autoRenew);
    assert.equal(sub.escrowBalance.toString(), escrowDeposit.toString());
    assert.isTrue(sub.subscriber.equals(subscriber.publicKey));

    const finalPayoutBalance = await provider.connection.getBalance(creatorPayout.publicKey);
    // Creator received initial payment minus protocol fee (0.5%)
    const expectedShare = 0.1 * LAMPORTS_PER_SOL * (1 - 0.005);
    assert.approximately(finalPayoutBalance - initialPayoutBalance, expectedShare, 1000);

    const profile = await program.account.creatorProfile.fetch(creatorProfilePda);
    assert.equal(profile.totalSubscribers.toNumber(), 1);
  });

  it("4. Prevents early renewal before epoch completion", async () => {
    try {
      await program.methods
        .renewSubscription()
        .accounts({
          subscriptionState: subscriptionStatePda,
          creatorProfile: creatorProfilePda,
          payoutWallet: creatorPayout.publicKey,
          protocolTreasury: protocolTreasuryPda,
          creator: creatorProfilePda,
          keeper: subscriber.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([subscriber])
        .rpc();
      assert.fail("Should have blocked premature renewal");
    } catch (err: any) {
      assert.include(err.toString(), "SubscriptionNotDue");
    }
  });

  it("5. Subscriber cancels and receives 100% of unused escrow refund", async () => {
    const balanceBefore = await provider.connection.getBalance(subscriber.publicKey);

    await program.methods
      .cancelSubscription()
      .accounts({
        subscriptionState: subscriptionStatePda,
        subscriber: subscriber.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([subscriber])
      .rpc();

    const sub = await program.account.subscriptionState.fetch(subscriptionStatePda);
    assert.isFalse(sub.isActive);
    assert.isFalse(sub.autoRenew);
    assert.equal(sub.escrowBalance.toNumber(), 0);

    const balanceAfter = await provider.connection.getBalance(subscriber.publicKey);
    // Received ~0.2 SOL refund minus transaction gas
    assert.isTrue(balanceAfter > balanceBefore);
  });

  it("6. Unauthorized caller cannot withdraw protocol treasury", async () => {
    try {
      await program.methods
        .withdrawTreasury(new anchor.BN(100))
        .accounts({
          protocolTreasury: protocolTreasuryPda,
          admin: unauthorizedUser.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([unauthorizedUser])
        .rpc();
      assert.fail("Should have failed on has_one admin check");
    } catch (err: any) {
      assert.include(err.toString(), "ConstraintHasOne");
    }
  });
});