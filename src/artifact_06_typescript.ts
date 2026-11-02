import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { AuroraSplit } from "../target/types/aurora_split";
import { Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram } from "@solana/web3.js";
import { expect } from "chai";

describe("AuroraSplit Core Protocol Test Suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.AuroraSplit as Program<AuroraSplit>;

  // Keypairs & Actors
  const admin = Keypair.generate();
  const feeRecipient = Keypair.generate();
  const creator = Keypair.generate();
  const editor = Keypair.generate();
  const producer = Keypair.generate();
  const fan = Keypair.generate();

  let configPda: PublicKey;
  let configBump: number;

  const splitId = new anchor.BN(101);
  let splitPda: PublicKey;

  const streamId = new anchor.BN(5001);
  let streamPda: PublicKey;
  let vaultPda: PublicKey;

  before(async () => {
    // Airdrop SOL to test actors
    for (const actor of [admin, creator, fan]) {
      const sig = await provider.connection.requestAirdrop(actor.publicKey, 5 * LAMPORTS_PER_SOL);
      const latestBlockhash = await provider.connection.getLatestBlockhash();
      await provider.connection.confirmTransaction({
        signature: sig,
        ...latestBlockhash,
      });
    }

    [configPda, configBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("config")],
      program.programId
    );

    [splitPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("split"), creator.publicKey.toBuffer(), splitId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    [streamPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("stream"), streamId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), streamId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );
  });

  it("1. Initializes global protocol configuration", async () => {
    await program.methods
      .initializeProtocol(feeRecipient.publicKey)
      .accounts({
        config: configPda,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    const configAccount = await program.account.protocolConfig.fetch(configPda);
    expect(configAccount.admin.toBase58()).to.equal(admin.publicKey.toBase58());
    expect(configAccount.feeRecipient.toBase58()).to.equal(feeRecipient.publicKey.toBase58());
    expect(configAccount.totalVolume.toNumber()).to.equal(0);
    expect(configAccount.activeStreams.toNumber()).to.equal(0);
  });

  it("2. Registers a Split Group with multi-creator allocations (60% Creator, 30% Editor, 10% Producer)", async () => {
    const recipients = [
      { pubkey: creator.publicKey, bps: new anchor.BN(6000) },
      { pubkey: editor.publicKey, bps: new anchor.BN(3000) },
      { pubkey: producer.publicKey, bps: new anchor.BN(1000) },
    ];

    await program.methods
      .initializeSplitGroup(splitId, recipients)
      .accounts({
        splitGroup: splitPda,
        creator: creator.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creator])
      .rpc();

    const splitAccount = await program.account.splitGroup.fetch(splitPda);
    expect(splitAccount.owner.toBase58()).to.equal(creator.publicKey.toBase58());
    expect(splitAccount.recipients.length).to.equal(3);
    expect(splitAccount.recipients[0].bps.toNumber()).to.equal(6000);
  });

  it("3. Rejects invalid split group allocations that do not equal 10,000 BPS (100%)", async () => {
    const invalidSplitId = new anchor.BN(102);
    const [invalidSplitPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("split"), creator.publicKey.toBuffer(), invalidSplitId.toArrayLike(Buffer, "le", 8)],
      program.programId
    );

    const badRecipients = [
      { pubkey: creator.publicKey, bps: new anchor.BN(5000) },
      { pubkey: editor.publicKey, bps: new anchor.BN(2000) }, // Sums to 7000 (70%)
    ];

    try {
      await program.methods
        .initializeSplitGroup(invalidSplitId, badRecipients)
        .accounts({
          splitGroup: invalidSplitPda,
          creator: creator.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([creator])
        .rpc();
      expect.fail("Should have thrown BpsSumMismatch error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("BpsSumMismatch");
    }
  });

  it("4. Creates and funds a micro-payment stream escrow", async () => {
    const depositAmount = new anchor.BN(1 * LAMPORTS_PER_SOL);
    const ratePerSecond = new anchor.BN(0.001 * LAMPORTS_PER_SOL);
    const durationSeconds = new anchor.BN(1000);
    const metadataUri = "https://arweave.net/stream-creator-challenge-01";

    const initialFanBalance = await provider.connection.getBalance(fan.publicKey);

    await program.methods
      .createStream(streamId, depositAmount, ratePerSecond, durationSeconds, metadataUri)
      .accounts({
        stream: streamPda,
        vault: vaultPda,
        splitGroup: splitPda,
        config: configPda,
        sender: fan.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([fan])
      .rpc();

    const streamAccount = await program.account.streamEscrow.fetch(streamPda);
    expect(streamAccount.depositAmount.toString()).to.equal(depositAmount.toString());
    expect(streamAccount.isActive).to.be.true;

    const vaultBalance = await provider.connection.getBalance(vaultPda);
    expect(vaultBalance).to.equal(depositAmount.toNumber());

    const updatedConfig = await program.account.protocolConfig.fetch(configPda);
    expect(updatedConfig.totalVolume.toString()).to.equal(depositAmount.toString());
    expect(updatedConfig.activeStreams.toNumber()).to.equal(1);
  });

  it("5. Allows time-vested claim and atomic revenue dispersion to all 3 split accounts", async () => {
    // Simulate time passage (2 seconds sleep)
    await new Promise((resolve) => setTimeout(resolve, 2500));

    const editorPreBal = await provider.connection.getBalance(editor.publicKey);
    const producerPreBal = await provider.connection.getBalance(producer.publicKey);

    await program.methods
      .claimAndDisperse()
      .accounts({
        stream: streamPda,
        vault: vaultPda,
        splitGroup: splitPda,
        config: configPda,
        feeRecipient: feeRecipient.publicKey,
        caller: creator.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([
        { pubkey: creator.publicKey, isWritable: true, isSigner: false },
        { pubkey: editor.publicKey, isWritable: true, isSigner: false },
        { pubkey: producer.publicKey, isWritable: true, isSigner: false },
      ])
      .signers([creator])
      .rpc();

    const editorPostBal = await provider.connection.getBalance(editor.publicKey);
    const producerPostBal = await provider.connection.getBalance(producer.publicKey);

    expect(editorPostBal).to.be.greaterThan(editorPreBal);
    expect(producerPostBal).to.be.greaterThan(producerPreBal);

    const streamAccount = await program.account.streamEscrow.fetch(streamPda);
    expect(streamAccount.claimedAmount.toNumber()).to.be.greaterThan(0);
  });

  it("6. Cancels an active stream and returns unvested collateral to the subscriber", async () => {
    const fanPreBal = await provider.connection.getBalance(fan.publicKey);

    await program.methods
      .closeOrCancelStream()
      .accounts({
        stream: streamPda,
        vault: vaultPda,
        config: configPda,
        sender: fan.publicKey,
        caller: fan.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([fan])
      .rpc();

    const fanPostBal = await provider.connection.getBalance(fan.publicKey);
    expect(fanPostBal).to.be.greaterThan(fanPreBal);

    const streamAccount = await program.account.streamEscrow.fetch(streamPda);
    expect(streamAccount.isActive).to.be.false;
  });
});