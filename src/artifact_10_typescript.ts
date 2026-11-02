import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { CreatorVault } from "../target/types/creator_vault";
import { Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram } from "@solana/web3.js";
import { expect } from "chai";

describe("aurora-creator-vault test suite", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.CreatorVault as Program<CreatorVault>;

  const brand = Keypair.generate();
  const primaryCreator = Keypair.generate();
  const coCreator = Keypair.generate();
  const unauthorizedAttacker = Keypair.generate();

  const totalAmount = new anchor.BN(2 * LAMPORTS_PER_SOL);
  const escrowSeed = new anchor.BN(Math.floor(Date.now() / 1000));
  const milestonesCount = 2;
  const timelockDuration = new anchor.BN(5); // 5 seconds for fast automated tests

  let escrowPda: PublicKey;
  let escrowBump: number;

  before(async () => {
    // Airdrop SOL to test actors
    const airdropSignatures = await Promise.all([
      provider.connection.requestAirdrop(brand.publicKey, 10 * LAMPORTS_PER_SOL),
      provider.connection.requestAirdrop(unauthorizedAttacker.publicKey, 2 * LAMPORTS_PER_SOL),
    ]);

    await Promise.all(
      airdropSignatures.map((sig) =>
        provider.connection.confirmTransaction(sig, "confirmed")
      )
    );

    [escrowPda, escrowBump] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("aurora_escrow"),
        brand.publicKey.toBuffer(),
        escrowSeed.toArrayLike(Buffer, "le", 8),
      ],
      program.programId
    );
  });

  it("1. Successfully initializes an Escrow with 70/30 BPS splits", async () => {
    const splits = [
      {
        recipient: primaryCreator.publicKey,
        bps: 7000, // 70.00%
      },
      {
        recipient: coCreator.publicKey,
        bps: 3000, // 30.00%
      },
    ];

    await program.methods
      .initializeEscrow(escrowSeed, totalAmount, milestonesCount, timelockDuration, splits)
      .accounts({
        brand: brand.publicKey,
        escrowAccount: escrowPda,
        systemProgram: SystemProgram.programId,
      })
      .signers([brand])
      .rpc();

    const escrowAccount = await program.account.escrowAccount.fetch(escrowPda);
    expect(escrowAccount.brand.toBase58()).to.equal(brand.publicKey.toBase58());
    expect(escrowAccount.totalAmount.toString()).to.equal(totalAmount.toString());
    expect(escrowAccount.remainingBalance.toString()).to.equal(totalAmount.toString());
    expect(escrowAccount.milestonesCount).to.equal(2);
    expect(escrowAccount.milestonesCompleted).to.equal(0);
    expect(escrowAccount.splitsCount).to.equal(2);
  });

  it("2. Fails if Basis Points do not sum to 10,000", async () => {
    const invalidSeed = new anchor.BN(999999);
    const [invalidPda] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("aurora_escrow"),
        brand.publicKey.toBuffer(),
        invalidSeed.toArrayLike(Buffer, "le", 8),
      ],
      program.programId
    );

    const badSplits = [
      {
        recipient: primaryCreator.publicKey,
        bps: 5000, // Only 50%
      },
    ];

    try {
      await program.methods
        .initializeEscrow(invalidSeed, totalAmount, 1, timelockDuration, badSplits)
        .accounts({
          brand: brand.publicKey,
          escrowAccount: invalidPda,
          systemProgram: SystemProgram.programId,
        })
        .signers([brand])
        .rpc();
      expect.fail("Should have thrown InvalidBpsSum error");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidBpsSum");
    }
  });

  it("3. Successfully releases Milestone #1 and splits lamports proportionally", async () => {
    const initialPrimaryBalance = await provider.connection.getBalance(primaryCreator.publicKey);
    const initialCoBalance = await provider.connection.getBalance(coCreator.publicKey);

    // Each milestone is 1 SOL. Primary gets 70% (0.7 SOL), Co gets 30% (0.3 SOL)
    await program.methods
      .releaseMilestone()
      .accounts({
        brand: brand.publicKey,
        escrowAccount: escrowPda,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([
        { pubkey: primaryCreator.publicKey, isWritable: true, isSigner: false },
        { pubkey: coCreator.publicKey, isWritable: true, isSigner: false },
      ])
      .signers([brand])
      .rpc();

    const postPrimaryBalance = await provider.connection.getBalance(primaryCreator.publicKey);
    const postCoBalance = await provider.connection.getBalance(coCreator.publicKey);

    expect(postPrimaryBalance - initialPrimaryBalance).to.equal(0.7 * LAMPORTS_PER_SOL);
    expect(postCoBalance - initialCoBalance).to.equal(0.3 * LAMPORTS_PER_SOL);

    const escrowAccount = await program.account.escrowAccount.fetch(escrowPda);
    expect(escrowAccount.milestonesCompleted).to.equal(1);
    expect(escrowAccount.remainingBalance.toNumber()).to.equal(1 * LAMPORTS_PER_SOL);
  });

  it("4. Blocks unauthorized attacker from releasing a milestone", async () => {
    try {
      await program.methods
        .releaseMilestone()
        .accounts({
          brand: unauthorizedAttacker.publicKey,
          escrowAccount: escrowPda,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([
          { pubkey: primaryCreator.publicKey, isWritable: true, isSigner: false },
          { pubkey: coCreator.publicKey, isWritable: true, isSigner: false },
        ])
        .signers([unauthorizedAttacker])
        .rpc();
      expect.fail("Attacker should not be able to release funds");
    } catch (err: any) {
      expect(err.toString()).to.include("ConstraintRaw") || expect(err.toString()).to.include("UnauthorizedCaller");
    }
  });

  it("5. Rejects premature refund before timelock expiry", async () => {
    try {
      await program.methods
        .claimRefund()
        .accounts({
          brand: brand.publicKey,
          escrowAccount: escrowPda,
          systemProgram: SystemProgram.programId,
        })
        .signers([brand])
        .rpc();
      expect.fail("Premature refund claim must be rejected");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("TimelockNotYetReached");
    }
  });

  it("6. Allows Brand refund after timelock expiry", async () => {
    // Wait 6 seconds to surpass timelock duration
    await new Promise((resolve) => setTimeout(resolve, 6000));

    const brandPreBalance = await provider.connection.getBalance(brand.publicKey);

    await program.methods
      .claimRefund()
      .accounts({
        brand: brand.publicKey,
        escrowAccount: escrowPda,
        systemProgram: SystemProgram.programId,
      })
      .signers([brand])
      .rpc();

    const brandPostBalance = await provider.connection.getBalance(brand.publicKey);
    const escrowAccount = await program.account.escrowAccount.fetch(escrowPda);

    expect(escrowAccount.remainingBalance.toNumber()).to.equal(0);
    expect(brandPostBalance).to.be.gt(brandPreBalance);
  });
});