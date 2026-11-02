import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Solsync } from "../target/types/solsync";
import { PublicKey, Keypair, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { assert, expect } from "chai";

describe("SolSync Creator Protocol - Comprehensive QA Suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Solsync as Program<Solsync>;

  const creator = Keypair.generate();
  const collaborator1 = Keypair.generate();
  const collaborator2 = Keypair.generate();
  const subscriber = Keypair.generate();
  const unauthorizedUser = Keypair.generate();

  let channelPda: PublicKey;
  let splitterPda: PublicKey;
  let vaultPda: PublicKey;
  let tierPda: PublicKey;
  let subscriptionPda: PublicKey;

  const tierId = 1;
  const tierPrice = new anchor.BN(0.5 * LAMPORTS_PER_SOL); // 0.5 SOL
  const validitySeconds = new anchor.BN(86400 * 30); // 30 days

  before(async () => {
    // Airdrop SOL to test actors
    const airdropSigners = [creator, collaborator1, collaborator2, subscriber, unauthorizedUser];
    for (const actor of airdropSigners) {
      const sig = await provider.connection.requestAirdrop(actor.publicKey, 5 * LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig, "confirmed");
    }

    [channelPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("channel"), creator.publicKey.toBuffer()],
      program.programId
    );

    [splitterPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("splitter"), channelPda.toBuffer()],
      program.programId
    );

    [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), channelPda.toBuffer()],
      program.programId
    );

    [tierPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("tier"), channelPda.toBuffer(), Buffer.from([tierId])],
      program.programId
    );

    [subscriptionPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription"), subscriber.publicKey.toBuffer(), channelPda.toBuffer()],
      program.programId
    );
  });

  it("1. Successfully registers Creator Channel with valid 10,000 Basis Points split", async () => {
    const recipients = [collaborator1.publicKey, collaborator2.publicKey];
    const basisPoints = [6000, 4000]; // 60% + 40% = 100%

    await program.methods
      .initializeChannel("Canada Creator Studio", recipients, basisPoints)
      .accounts({
        channel: channelPda,
        splitter: splitterPda,
        vault: vaultPda,
        creator: creator.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creator])
      .rpc();

    const channelAccount = await program.account.channelAccount.fetch(channelPda);
    assert.equal(channelAccount.name, "Canada Creator Studio");
    assert.equal(channelAccount.creator.toBase58(), creator.publicKey.toBase58());
    assert.equal(channelAccount.totalSubscribers.toNumber(), 0);

    const splitterAccount = await program.account.splitterAccount.fetch(splitterPda);
    assert.equal(splitterAccount.recipientCount, 2);
    assert.equal(splitterAccount.recipients[0].toBase58(), collaborator1.publicKey.toBase58());
    assert.equal(splitterAccount.basisPoints[0], 6000);
  });

  it("2. Fails to initialize channel when total basis points do not equal 10,000", async () => {
    const rogueCreator = Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(rogueCreator.publicKey, LAMPORTS_PER_SOL)
    );

    const [rogueChannel] = PublicKey.findProgramAddressSync(
      [Buffer.from("channel"), rogueCreator.publicKey.toBuffer()],
      program.programId
    );
    const [rogueSplitter] = PublicKey.findProgramAddressSync(
      [Buffer.from("splitter"), rogueChannel.toBuffer()],
      program.programId
    );
    const [rogueVault] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), rogueChannel.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .initializeChannel(
          "Invalid Split Channel",
          [collaborator1.publicKey, collaborator2.publicKey],
          [5000, 4000] // Sums to 9,000 bps -> Must Fail
        )
        .accounts({
          channel: rogueChannel,
          splitter: rogueSplitter,
          vault: rogueVault,
          creator: rogueCreator.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([rogueCreator])
        .rpc();
      assert.fail("Transaction should have thrown InvalidTotalBasisPoints error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidTotalBasisPoints");
    }
  });

  it("3. Configures Tier 1 Pass under creator authorization", async () => {
    await program.methods
      .createTier(tierId, "Monthly VIP Supporter", tierPrice, validitySeconds)
      .accounts({
        channel: channelPda,
        tier: tierPda,
        creator: creator.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creator])
      .rpc();

    const tierAccount = await program.account.tierAccount.fetch(tierPda);
    assert.equal(tierAccount.name, "Monthly VIP Supporter");
    assert.equal(tierAccount.priceLamports.toString(), tierPrice.toString());
  });

  it("4. Prevents unauthorized actor from creating a tier on another channel", async () => {
    const unauthorizedTierId = 2;
    const [fakeTierPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("tier"), channelPda.toBuffer(), Buffer.from([unauthorizedTierId])],
      program.programId
    );

    try {
      await program.methods
        .createTier(unauthorizedTierId, "Hijacked Tier", tierPrice, validitySeconds)
        .accounts({
          channel: channelPda,
          tier: fakeTierPda,
          creator: unauthorizedUser.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([unauthorizedUser])
        .rpc();
      assert.fail("Should have failed with unauthorized error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("Unauthorized");
    }
  });

  it("5. Executes subscription purchase and instantly routes 60/40 splits to collaborators", async () => {
    const col1BalBefore = await provider.connection.getBalance(collaborator1.publicKey);
    const col2BalBefore = await provider.connection.getBalance(collaborator2.publicKey);

    await program.methods
      .subscribe("fan_user_id_48291")
      .accounts({
        channel: channelPda,
        tier: tierPda,
        splitter: splitterPda,
        subscription: subscriptionPda,
        creator: creator.publicKey,
        subscriber: subscriber.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([
        { pubkey: collaborator1.publicKey, isWritable: true, isSigner: false },
        { pubkey: collaborator2.publicKey, isWritable: true, isSigner: false },
      ])
      .signers([subscriber])
      .rpc();

    const col1BalAfter = await provider.connection.getBalance(collaborator1.publicKey);
    const col2BalAfter = await provider.connection.getBalance(collaborator2.publicKey);

    // 60% of 0.5 SOL = 0.3 SOL
    const expectedCol1Share = 0.3 * LAMPORTS_PER_SOL;
    // 40% of 0.5 SOL = 0.2 SOL
    const expectedCol2Share = 0.2 * LAMPORTS_PER_SOL;

    assert.equal(col1BalAfter - col1BalBefore, expectedCol1Share);
    assert.equal(col2BalAfter - col2BalBefore, expectedCol2Share);

    const subRecord = await program.account.subscriptionAccount.fetch(subscriptionPda);
    assert.isAbove(subRecord.expiresAt.toNumber(), 0);
  });

  it("6. Direct micro-tip automatically pays out split without retention", async () => {
    const tipAmount = new anchor.BN(0.1 * LAMPORTS_PER_SOL);
    const col1BalBefore = await provider.connection.getBalance(collaborator1.publicKey);

    await program.methods
      .sendTip(tipAmount, "Great summit demo!")
      .accounts({
        channel: channelPda,
        splitter: splitterPda,
        creator: creator.publicKey,
        fan: subscriber.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([
        { pubkey: collaborator1.publicKey, isWritable: true, isSigner: false },
        { pubkey: collaborator2.publicKey, isWritable: true, isSigner: false },
      ])
      .signers([subscriber])
      .rpc();

    const col1BalAfter = await provider.connection.getBalance(collaborator1.publicKey);
    assert.equal(col1BalAfter - col1BalBefore, 0.06 * LAMPORTS_PER_SOL); // 60%
  });
});