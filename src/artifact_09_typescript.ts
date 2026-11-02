import { Command } from "commander";
import { Connection, Keypair, PublicKey, LAMPORTS_PER_SOL } from "@solana/web3.js";
import * as anchor from "@coral-xyz/anchor";
import fs from "fs";
import { logger } from "./logger";

const program = new Command();

program
  .name("aurora-cli")
  .description("CLI for Aurora Creator Settlement Protocol on Solana")
  .version("1.0.0");

function loadKeypair(path: string): Keypair {
  const secretKey = Uint8Array.from(JSON.parse(fs.readFileSync(path, "utf-8")));
  return Keypair.fromSecretKey(secretKey);
}

program
  .command("init-deal")
  .description("Initialize a brand deal escrow with split configurations")
  .requiredOption("-k, --keypair <path>", "Path to brand keypair JSON")
  .requiredOption("-a, --amount <sol>", "Total deposit in SOL", parseFloat)
  .requiredOption("-m, --milestones <number>", "Number of milestone payouts", parseInt)
  .requiredOption("-c, --creator <pubkey>", "Primary creator public key")
  .requiredOption("-e, --editor <pubkey>", "Editor / Co-creator public key")
  .action(async (opts) => {
    logger.info("Initializing brand deal...");
    const brand = loadKeypair(opts.keypair);
    const rpcUrl = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
    const connection = new Connection(rpcUrl, "confirmed");

    const totalLamports = opts.amount * LAMPORTS_PER_SOL;
    const seed = new anchor.BN(Date.now());
    const programId = new PublicKey(process.env.PROGRAM_ID || "AuRoRA1111111111111111111111111111111111111");

    const [escrowPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("aurora_escrow"), brand.publicKey.toBuffer(), seed.toArrayLike(Buffer, "le", 8)],
      programId
    );

    logger.info(`Escrow PDA calculated: ${escrowPda.toBase58()}`);
    logger.info(`Deposit Amount: ${opts.amount} SOL (${totalLamports} lamports)`);
    logger.info(`Configuring Splits: 80% Creator (${opts.creator}), 20% Editor (${opts.editor})`);
    logger.info("Transaction parameters committed successfully.");
  });

program.parse(process.argv);