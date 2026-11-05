import { Connection, PublicKey } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet } from "@coral-xyz/anchor";
import { Resend } from "resend";
import axios from "axios";
import * as dotenv from "dotenv";

dotenv.config();

// Environment Validation
const SOLANA_RPC_URL = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
const SOLANA_WSS_URL = process.env.SOLANA_WSS_URL || "wss://api.devnet.solana.com";
const PROGRAM_ID = new PublicKey(
  process.env.PROGRAM_ID || "PatrnXb7R97eZ9wQG3HwYkmhGZkGZJmQc7LpWn2VNmS"
);
const RESEND_API_KEY = process.env.RESEND_API_KEY || "re_test_000000000000000000000000";
const DISCORD_WEBHOOK_URL = process.env.DISCORD_WEBHOOK_URL || "";

const resend = new Resend(RESEND_API_KEY);

interface SubscriptionEvent {
  creator: string;
  patron: string;
  tierId: number;
  lamportsPaid: number;
  activeUntil: number;
  timestamp: number;
}

// In-Memory Idempotency Cache
const processedSignatures = new Set<string>();

/**
 * Dispatches an enterprise transactional email via Resend
 */
async function sendPatronWelcomeEmail(
  patronPubkey: string,
  tierId: number,
  expiryTimestamp: number
): Promise<void> {
  const expiryDate = new Date(expiryTimestamp * 1000).toUTCString();
  try {
    const response = await resend.emails.send({
      from: "Creator VIP <patron@notifications.solanasummit.dev>",
      to: [`${patronPubkey.slice(0, 8)}@mailinator.com`], // Dynamic sandbox mapping
      subject: `🎉 Access Granted: Tier ${tierId} Patron Pass Activated!`,
      html: `
        <div style="font-family: Arial, sans-serif; background-color: #0b0e14; color: #ffffff; padding: 24px; border-radius: 8px;">
          <h1 style="color: #14F195;">Welcome to Elite Patron Status!</h1>
          <p>Your on-chain subscription on Solana has been confirmed.</p>
          <div style="background-color: #1a1f2e; padding: 16px; border-radius: 6px; margin: 16px 0;">
            <p><strong>Wallet:</strong> <code>${patronPubkey}</code></p>
            <p><strong>Tier:</strong> #${tierId}</p>
            <p><strong>Access Active Until:</strong> ${expiryDate}</p>
          </div>
          <p>Click below to claim your creator perk bundle:</p>
          <a href="https://summit.creators.solana.com/perks?wallet=${patronPubkey}" 
             style="background: linear-gradient(90deg, #9945FF, #14F195); color: #000; padding: 12px 24px; text-decoration: none; border-radius: 4px; font-weight: bold; display: inline-block;">
            Unlock Private Creator Vault
          </a>
        </div>
      `,
    });
    console.log(`[Resend] Successfully delivered notification for patron: ${patronPubkey}. Email ID: ${response.data?.id}`);
  } catch (error) {
    console.error(`[Resend Error] Failed to send email to ${patronPubkey}:`, error);
  }
}

/**
 * Triggers Discord VIP Webhook Alert
 */
async function dispatchDiscordWebhook(event: SubscriptionEvent): Promise<void> {
  if (!DISCORD_WEBHOOK_URL) return;

  try {
    await axios.post(DISCORD_WEBHOOK_URL, {
      embeds: [
        {
          title: "🔥 New Patron Subscription Activated!",
          color: 0x14f195,
          fields: [
            { name: "Patron Public Key", value: `\`${event.patron}\``, inline: false },
            { name: "Creator Vault", value: `\`${event.creator}\``, inline: false },
            { name: "Tier Subscribed", value: `Tier #${event.tierId}`, inline: true },
            {
              name: "Value Settled",
              value: `${(event.lamportsPaid / 1e9).toFixed(3)} SOL`,
              inline: true,
            },
            {
              name: "Active Until",
              value: new Date(event.activeUntil * 1000).toLocaleString(),
              inline: false,
            },
          ],
          footer: {
            text: "Solana Summit Canada Creator Challenge | Powered by PatronEngine",
          },
          timestamp: new Date().toISOString(),
        },
      ],
    });
    console.log(`[Discord] Webhook posted for patron: ${event.patron}`);
  } catch (error) {
    console.error("[Discord Webhook Error]:", error);
  }
}

/**
 * Main WebSocket Listener
 */
async function runDaemon(): Promise<void> {
  console.log("⚡ Starting Solana Patron Engine Integration Daemon...");
  console.log(`📡 Connected to RPC: ${SOLANA_RPC_URL}`);
  console.log(`🎯 Watching Program ID: ${PROGRAM_ID.toBase58()}`);

  const connection = new Connection(SOLANA_RPC_URL, {
    wsEndpoint: SOLANA_WSS_URL,
    commitment: "confirmed",
  });

  // Subscribe to program transaction logs
  connection.onLogs(
    PROGRAM_ID,
    async ({ logs, err, signature }) => {
      if (err) return;
      if (processedSignatures.has(signature)) return;

      processedSignatures.add(signature);
      if (processedSignatures.size > 10_000) {
        const first = processedSignatures.values().next().value;
        if (first) processedSignatures.delete(first);
      }

      console.log(`🔍 [TX Detected] Signature: ${signature}`);

      for (const log of logs) {
        if (log.includes("SubscriptionPurchasedEvent")) {
          console.log("🎯 Subscription Event Detected in Program Log!");
          
          // In production, the event is decoded via Anchor EventParser
          // Simulated parsed payload representing the validated on-chain event:
          const eventPayload: SubscriptionEvent = {
            creator: "CrEaToR11111111111111111111111111111111111",
            patron: "PaTrOn99999999999999999999999999999999999",
            tierId: 1,
            lamportsPaid: 500_000_000, // 0.5 SOL
            activeUntil: Math.floor(Date.now() / 1000) + 86400 * 30,
            timestamp: Math.floor(Date.now() / 1000),
          };

          await Promise.all([
            sendPatronWelcomeEmail(eventPayload.patron, eventPayload.tierId, eventPayload.activeUntil),
            dispatchDiscordWebhook(eventPayload),
          ]);
        }
      }
    },
    "confirmed"
  );
}

runDaemon().catch((err) => {
  console.error("Fatal Daemon Failure:", err);
  process.exit(1);
});