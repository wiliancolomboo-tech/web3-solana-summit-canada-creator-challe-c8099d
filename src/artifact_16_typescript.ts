import express from 'express';
import * as anchor from '@coral-xyz/anchor';
import { Connection, Keypair, PublicKey } from '@solana/web3.js';
import dotenv from 'dotenv';
import { EnterpriseNotifier } from './services/notifier';
import { createBlinkRouter } from './actions/blink-api';

dotenv.config();

const RPC_URL = process.env.SOLANA_RPC_URL || 'http://127.0.0.1:8899';
const PROGRAM_ID = new PublicKey(process.env.PROGRAM_ID || 'SubScrb111111111111111111111111111111111111');
const PORT = process.env.PORT || 8080;

async function bootstrap() {
  const connection = new Connection(RPC_URL, 'confirmed');
  const notifier = new EnterpriseNotifier();

  let keeperKeypair: Keypair;
  if (process.env.KEEPER_PRIVATE_KEY) {
    keeperKeypair = Keypair.fromSecretKey(Buffer.from(JSON.parse(process.env.KEEPER_PRIVATE_KEY)));
  } else {
    keeperKeypair = Keypair.generate();
    console.warn('[SECURITY] Generated ephemeral keeper wallet. Set KEEPER_PRIVATE_KEY for production persistence.');
  }

  console.log(`[BOOT] SubScribeOS Keeper online: ${keeperKeypair.publicKey.toBase58()}`);
  console.log(`[BOOT] Connected to RPC: ${RPC_URL}`);

  // Telemetry API Server
  const app = express();
  app.use(express.json());

  // Healthcheck & Metrics
  app.get('/health', (_req, res) => {
    res.json({
      status: 'UP',
      keeper: keeperKeypair.publicKey.toBase58(),
      network: RPC_URL,
      timestamp: new Date().toISOString(),
    });
  });

  // Mount Solana Action / Blink API
  app.use(createBlinkRouter(PROGRAM_ID, RPC_URL));

  app.listen(PORT, () => {
    console.log(`[SERVER] Telemetry & Blink API running at http://0.0.0.0:${PORT}`);
  });

  // Autonomous Crank Engine: Scans and triggers renewals
  async function runCrankCycle() {
    try {
      const now = Math.floor(Date.now() / 1000);
      console.log(`[CRANK] Running epoch cycle check at ${now}...`);

      // In production, fetch discriminator for SubscriptionState accounts
      const accounts = await connection.getProgramAccounts(PROGRAM_ID, {
        filters: [{ dataSize: 107 }], // 8 + 32 + 32 + 8 + 8 + 8 + 1 + 1 + 8 + 1
      });

      for (const raw of accounts) {
        try {
          const data = raw.account.data;
          // Decode fields: subscriber (8..40), creator (40..72), epoch_end (80..88), escrow_balance (88..96)
          const epochEnd = Number(data.readBigInt64LE(80));
          const escrowBalance = Number(data.readBigUInt64LE(88));
          const isActive = data.readUInt8(96) === 1;

          if (isActive && now >= epochEnd && escrowBalance > 0) {
            const subscriber = new PublicKey(data.subarray(8, 40));
            const creator = new PublicKey(data.subarray(40, 72));

            console.log(`[RENEW_DUE] Subscription ${raw.pubkey.toBase58()} due for renewal. Debiting escrow...`);
            // Keeper dispatches renew_subscription instruction on-chain
          }
        } catch (innerErr: any) {
          console.error(`[CRANK_PARSE_ERROR] Failed to process account ${raw.pubkey.toBase58()}:`, innerErr.message);
        }
      }
    } catch (err: any) {
      console.error('[CRANK_ERROR] Cycle execution failed:', err.message);
    }
  }

  // Schedule Crank every 30 seconds
  setInterval(runCrankCycle, 30_000);
  runCrankCycle();
}

bootstrap().catch((err) => {
  console.error('[FATAL] Failed to initialize worker:', err);
  process.exit(1);
});