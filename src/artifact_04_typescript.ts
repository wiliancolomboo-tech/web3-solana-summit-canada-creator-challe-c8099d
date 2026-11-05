import { Connection, Keypair, PublicKey, clusterApiUrl } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, Idl, BN } from "@coral-xyz/anchor";
import axios from "axios";
import { Resend } from "resend";
import dotenv from "dotenv";

dotenv.config();

// Configuration Interfaces
interface Config {
  rpcUrl: string;
  programId: PublicKey;
  resendApiKey: string;
  resendFromEmail: string;
  creatorWebhookUrl: string;
  operatorPrivateKey: string;
}

const config: Config = {
  rpcUrl: process.env.SOLANA_RPC_URL || clusterApiUrl("devnet"),
  programId: new PublicKey(
    process.env.SUBSTREAM_PROGRAM_ID || "SubSt7K8qVbLmv4eG6xMvZ9rWpY1A5d3F7cNoPQrstU"
  ),
  resendApiKey: process.env.RESEND_API_KEY || "re_test_000000000000000000000000",
  resendFromEmail: process.env.RESEND_FROM_EMAIL || "notifications@substream.io",
  creatorWebhookUrl: process.env.CREATOR_WEBHOOK_URL || "https://httpbin.org/post",
  operatorPrivateKey: process.env.OPERATOR_PRIVATE_KEY || "",
};

// Initialize Clients
const connection = new Connection(config.rpcUrl, "confirmed");
const resend = new Resend(config.resendApiKey);

/**
 * Enterprise Service Class orchestrating off-chain event handling and content gating.
 */
export class SubStreamEnterpriseAgent {
  private connection: Connection;
  private resend: Resend;
  private webhookUrl: string;

  constructor(connection: Connection, resend: Resend, webhookUrl: string) {
    this.connection = connection;
    this.resend = resend;
    this.webhookUrl = webhookUrl;
  }

  /**
   * Evaluates if a given wallet holds an active subscription for a specific tier.
   */
  public async verifyPatronGate(
    tierPda: PublicKey,
    subscriber: PublicKey
  ): Promise<{ authorized: boolean; expiresAt: number; remainingSeconds: number }> {
    const [subPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription"), tierPda.toBuffer(), subscriber.toBuffer()],
      config.programId
    );

    const accountInfo = await this.connection.getAccountInfo(subPda);
    if (!accountInfo) {
      return { authorized: false, expiresAt: 0, remainingSeconds: 0 };
    }

    // Offset decoding: 8 (discriminator) + 32 (subscriber) + 32 (tier) + 8 (started_at)
    // expires_at is located at byte index 80..88 as an i64 little-endian
    const expiresAtBuffer = accountInfo.data.subarray(80, 88);
    const expiresAt = Number(expiresAtBuffer.readBigInt64LE(0));
    const now = Math.floor(Date.now() / 1000);

    const authorized = expiresAt > now;
    const remainingSeconds = Math.max(0, expiresAt - now);

    return { authorized, expiresAt, remainingSeconds };
  }

  /**
   * Dispatches transactional email receipt via Resend.
   */
  public async sendSubscriptionReceipt(
    recipientEmail: string,
    subscriberWallet: string,
    amountSol: number,
    tierName: string,
    expiresAtTimestamp: number
  ): Promise<string> {
    try {
      const expirationDate = new Date(expiresAtTimestamp * 1000).toUTCString();
      const response = await this.resend.emails.send({
        from: config.resendFromEmail,
        to: recipientEmail,
        subject: `[SubStream] Subscription Confirmed: ${tierName}`,
        html: `
          <div style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 600px; margin: auto; padding: 24px; border: 1px solid #e2e8f0; border-radius: 8px;">
            <h2 style="color: #6366f1;">Welcome to ${tierName}!</h2>
            <p>Your on-chain subscription on Solana has been verified and registered.</p>
            <table style="width: 100%; border-collapse: collapse; margin: 20px 0;">
              <tr>
                <td style="padding: 8px 0; color: #64748b;">Subscriber Wallet:</td>
                <td style="padding: 8px 0; font-weight: bold; font-family: monospace;">${subscriberWallet}</td>
              </tr>
              <tr>
                <td style="padding: 8px 0; color: #64748b;">Amount Paid:</td>
                <td style="padding: 8px 0; font-weight: bold;">${amountSol} SOL</td>
              </tr>
              <tr>
                <td style="padding: 8px 0; color: #64748b;">Access Valid Until:</td>
                <td style="padding: 8px 0; font-weight: bold; color: #10b981;">${expirationDate}</td>
              </tr>
            </table>
            <p style="font-size: 12px; color: #94a3b8;">Cryptographically secured by Solana SubStream smart contracts.</p>
          </div>
        `,
      });
      return response.data?.id || "QUEUED";
    } catch (err: any) {
      console.error("[Enterprise Receipt Delivery Error]", err.message);
      throw err;
    }
  }

  /**
   * Notifies the creator's enterprise channel via incoming webhook.
   */
  public async notifyCreatorChannel(
    creatorPubkey: string,
    subscriberPubkey: string,
    tierId: number,
    amountLamports: number
  ): Promise<void> {
    const payload = {
      service: "SubStream Daemon",
      eventType: "NEW_SUBSCRIPTION_SETTLED",
      timestamp: new Date().toISOString(),
      creator: creatorPubkey,
      subscriber: subscriberPubkey,
      tierId,
      revenueLamports: amountLamports,
      revenueSol: amountLamports / 1e9,
    };

    try {
      await axios.post(this.webhookUrl, payload, {
        headers: { "Content-Type": "application/json" },
        timeout: 5000,
      });
    } catch (err: any) {
      console.error("[Enterprise Webhook Push Error]", err.message);
    }
  }

  /**
   * Starts the continuous WebSocket listener daemon for on-chain events.
   */
  public async startSubscriptionListener(): Promise<void> {
    console.log(`[SubStream Daemon] Monitoring program: ${config.programId.toBase58()}`);
    
    this.connection.onLogs(
      config.programId,
      async (logsContext) => {
        if (logsContext.err) return;
        
        for (const log of logsContext.logs) {
          if (log.includes("SubscriptionCreatedEvent")) {
            console.log(`[Event Detected] Signature: ${logsContext.signature}`);
            // In production, parse Anchor encoded event logs or fetch transaction metadata
            await this.notifyCreatorChannel(
              "CreatorPubkeyStream",
              "SubscriberPubkeyExample",
              1,
              100_000_000
            );
          }
        }
      },
      "confirmed"
    );
  }
}

// CLI runner if invoked directly
if (require.main === module) {
  const agent = new SubStreamEnterpriseAgent(
    connection,
    resend,
    config.creatorWebhookUrl
  );
  agent.startSubscriptionListener().catch((err) => {
    console.error("[Fatal Daemon Exception]", err);
    process.exit(1);
  });
}