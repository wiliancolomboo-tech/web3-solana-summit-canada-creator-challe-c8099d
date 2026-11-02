import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SolstreamCreatorCore } from "../target/types/solstream_creator_core";
import { Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram } from "@solana/web3.js";
import { expect } from "chai";

describe("solstream_creator_core test suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SolstreamCreatorCore as Program<SolstreamCreatorCore>;

  const creator = Keypair.generate();
  const coHost = Keypair.generate();
  const charity = Keypair.generate();
  const fan = Keypair.generate();
  const cranker = Keypair.generate();

  let creatorVaultPda: PublicKey;
  let creatorVaultBump: number;

  let fanPassPda: PublicKey;
  let fanPassBump: number;

  before(async () => {
    // Airdrop SOL to test actors
    const airdropSigners = [creator, coHost, charity, fan, cranker];
    for (const actor of airdropSigners) {
      const sig = await provider.connection.requestAirdrop(actor.publicKey, 5 * LAMPORTS_PER_SOL);
      const latestBlockhash = await provider.connection.getLatestBlockhash();
      await provider.connection.confirmTransaction({
        signature: sig,
        ...latestBlockhash,
      });
    }

    [creatorVaultPda, creatorVaultBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("creator_vault"), creator.publicKey.toBuffer()],
      program.programId
    );

    [fanPassPda, fanPassBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("creator_pass"), creatorVaultPda.toBuffer(), fan.publicKey.toBuffer()],
      program.programId
    );
  });

  it("Fails to initialize vault if basis points do not total 10,000", async () => {
    const invalidSplits = [
      { recipient: creator.publicKey, bps: 6000 },
      { recipient: coHost.publicKey, bps: 3000 }, // Total = 9000 (Fails)
    ];

    try {
      await program.methods
        .initializeCreatorVault(invalidSplits, new anchor.BN(50_000_000))
        .accounts({
          creatorVault: creatorVaultPda,
          creator: creator.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([creator])
        .rpc();
      expect.fail("Should have thrown InvalidTotalBps error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidTotalBps");
    }
  });

  it("Successfully initializes creator vault with exact 10,000 BPS split", async () => {
    const validSplits = [
      { recipient: creator.publicKey, bps: 7000 }, // 70%
      { recipient: coHost.publicKey, bps: 2000 },  // 20%
      { recipient: charity.publicKey, bps: 1000 }, // 10%
    ];

    const escrowThreshold = new anchor.BN(20_000_000); // 0.02 SOL

    const txSig = await program.methods
      .initializeCreatorVault(validSplits, escrowThreshold)
      .accounts({
        creatorVault: creatorVaultPda,
        creator: creator.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([creator])
      .rpc();

    expect(txSig).to.be.a("string");

    const vaultAccount = await program.account.creatorVault.fetch(creatorVaultPda);
    expect(vaultAccount.creator.toBase58()).to.equal(creator.publicKey.toBase58());
    expect(vaultAccount.splits.length).to.equal(3);
    expect(vaultAccount.splits[0].bps).to.equal(7000);
    expect(vaultAccount.splits[1].bps).to.equal(2000);
    expect(vaultAccount.splits[2].bps).to.equal(1000);
  });

  it("Allows a patron to purchase a Tier 2 Creator Pass with additional tip", async () => {
    const tier = 2; // Silver Pass = 0.05 SOL base
    const tip = new anchor.BN(10_000_000); // 0.01 SOL tip

    const txSig = await program.methods
      .purchaseCreatorPass(tier, tip)
      .accounts({
        creatorVault: creatorVaultPda,
        creatorPass: fanPassPda,
        patron: fan.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([fan])
      .rpc();

    expect(txSig).to.be.a("string");

    const pass = await program.account.creatorPass.fetch(fanPassPda);
    expect(pass.patron.toBase58()).to.equal(fan.publicKey.toBase58());
    expect(pass.tier).to.equal(2);
    expect(pass.lifetimeContributed.toNumber()).to.equal(60_000_000); // 0.05 + 0.01 = 0.06 SOL

    const vault = await program.account.creatorVault.fetch(creatorVaultPda);
    expect(vault.totalPassesMinted.toNumber()).to.equal(1);
    expect(vault.totalRevenueSettled.toNumber()).to.equal(60_000_000);
  });

  it("Executes distribute_escrow and splits balances proportionally according to BPS", async () => {
    const creatorBalBefore = await provider.connection.getBalance(creator.publicKey);
    const coHostBalBefore = await provider.connection.getBalance(coHost.publicKey);
    const charityBalBefore = await provider.connection.getBalance(charity.publicKey);

    // Remaining accounts must be passed in identical order to vault.splits
    const remainingAccounts = [
      { pubkey: creator.publicKey, isWritable: true, isSigner: false },
      { pubkey: coHost.publicKey, isWritable: true, isSigner: false },
      { pubkey: charity.publicKey, isWritable: true, isSigner: false },
    ];

    const txSig = await program.methods
      .distributeEscrow()
      .accounts({
        creatorVault: creatorVaultPda,
        cranker: cranker.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts(remainingAccounts)
      .signers([cranker])
      .rpc();

    expect(txSig).to.be.a("string");

    const creatorBalAfter = await provider.connection.getBalance(creator.publicKey);
    const coHostBalAfter = await provider.connection.getBalance(coHost.publicKey);
    const charityBalAfter = await provider.connection.getBalance(charity.publicKey);

    // Assert that coHost and charity received the calculated cut
    expect(coHostBalAfter).to.be.gt(coHostBalBefore);
    expect(charityBalAfter).to.be.gt(charityBalBefore);
    expect(creatorBalAfter).to.be.gt(creatorBalBefore);
  });

  it("Rejects distribute_escrow when remaining accounts mismatch the vault configuration", async () => {
    const wrongOrderRemainingAccounts = [
      { pubkey: coHost.publicKey, isWritable: true, isSigner: false }, // Swapped order
      { pubkey: creator.publicKey, isWritable: true, isSigner: false },
      { pubkey: charity.publicKey, isWritable: true, isSigner: false },
    ];

    try {
      await program.methods
        .distributeEscrow()
        .accounts({
          creatorVault: creatorVaultPda,
          cranker: cranker.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts(wrongOrderRemainingAccounts)
        .signers([cranker])
        .rpc();
      expect.fail("Should have failed due to account ordering violation");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidSplitAccountOrder");
    }
  });
});