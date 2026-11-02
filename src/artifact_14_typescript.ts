import { Connection, Keypair, PublicKey, clusterApiUrl } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, Idl, BN } from "@coral-xyz/anchor";
import { Resend } from "resend";
import axios from "axios";
import * as dotenv from "dotenv";
import * as crypto from "crypto";

dotenv.config();

interface AppConfig {
  rpcUrl: string;
  wsUrl: string;
  programId: PublicKey;
  keeperKeypair: Keypair;
  resendApiKey: string;
  resendFromEmail: string;
  discordWebhookUrl?: string;
  slackWebhookUrl?: string;
}

function loadConfig(): AppConfig {
  const rpcUrl = process.env.SOLANA_RPC_URL || clusterApiUrl("devnet");
  const wsUrl = process.env.SOLANA_WS_URL || "wss://api.devnet.solana.com";
  const programId = new PublicKey(
    process.env.PROGRAM_ID || "CNex111111111111111111111111111111111111111"
  );
  const secretKeyString = process.env.KEEPER_PRIVATE_KEY;
  if (!secretKeyString) {
    throw new Error("Missing KEEPER_PRIVATE_KEY in environment");
  }
  const secretBytes = Uint8Array.from(JSON.parse(secretKeyString));
  const keeperKeypair = Keypair.fromSecretKey(secretBytes);
  const resendApiKey = process.env.RESEND_API_KEY || "re_test_dummy_key";
  const resendFromEmail = process.env.RESEND_FROM_EMAIL || "nexus@deliveries.solanasummit.ca";

  return {
    rpcUrl,
    wsUrl,
    programId,
    keeperKeypair,
    resendApiKey,
    resendFromEmail,
    discordWebhookUrl: process.env.DISCORD_WEBHOOK_URL,
    slackWebhookUrl: process.env.SLACK_WEBHOOK_URL,
  };
}

export class CreatorNexusRelayer {
  private connection: Connection;
  private resend: Resend;
  private config: AppConfig;

  constructor(config: AppConfig) {
    this.config = config;
    this.connection = new Connection(config.rpcUrl, {
      wsEndpoint: config.wsUrl,
      commitment: "confirmed",
    });
    this.resend = new Resend(config.resendApiKey);
  }

  public generateSecureToken(patron: string, tierId: number): string {
    const hmac = crypto.createHmac("sha256", this.config.keeperKeypair.secretKey);
    hmac.update(`${patron}:${tierId}:${Date.now()}`);
    return hmac.digest("hex");
  }

  public async notifyDiscord(message: string): Promise<void> {
    if (!this.config.discordWebhookUrl) return;
    try {
      await axios.post(this.config.discordWebhookUrl, {
        content: `🚀 **CreatorNexus Protocol Notice**\n${message}`,
      });
    } catch (err) {
      console.error("[Discord] Notification failed:", err instanceof Error ? err.message : err);
    }
  }

  public async notifySlack(text: string): Promise<void> {
    if (!this.config.slackWebhookUrl) return;
    try {
      await axios.post(this.config.slackWebhookUrl, { text });
    } catch (err) {
      console.error("[Slack] Webhook notification failed:", err instanceof Error ? err.message : err);
    }
  }

  public async sendContentEntitlementEmail(
    patronPubkey: string,
    recipientEmail: string,
    tierId: number,
    accessSecret: string
  ): Promise<void> {
    try {
      const response = await this.resend.emails.send({
        from: this.config.resendFromEmail,
        to: recipientEmail,
        subject: "🎉 Your CreatorNexus Gated Artifact Access Pass",
        html: `
          <div style="font-family: Arial, sans-serif; background: #0c0d14; color: #fff; padding: 24px; border-radius: 8px;">
            <h1 style="color: #14F195;">Welcome to Exclusive Creator Content</h1>
            <p>Your subscription for Tier <strong>#${tierId}</strong> is verified on Solana.</p>
            <p><strong>Subscriber Wallet:</strong> <code>${patronPubkey}</code></p>
            <div style="margin: 20px 0; padding: 16px; background: #1a1b26; border-left: 4px solid #9945FF; border-radius: 4px;">
              <p style="margin:0 0 8px 0;">Your Single-Use Ephemeral Access Token:</p>
              <code style="color: #00C2FF; font-size: 16px;">${accessSecret}</code>
            </div>
            <p>Direct Portal: <a href="https://app.creatornexus.io/claim?token=${accessSecret}" style="color: #14F195;">Access Gated Stream</a></p>
            <hr style="border: 1px solid #333;" />
            <small style="color: #888;">Processed deterministically by CreatorNexus Keeper Engine on Solana Devnet/Mainnet.</small>
          </div>
        `,
      });
      console.log(`[Resend] Successfully dispatched entitlement email ID: ${response.data?.id}`);
    } catch (err) {
      console.error("[Resend] Failed to send email:", err instanceof Error ? err.message : err);
    }
  }

  public async startSubscriptionEventListener(): Promise<void> {
    console.log(`[Engine] Attaching Log Subscription to Program: ${this.config.programId.toBase58()}`);

    this.connection.onLogs(
      this.config.programId,
      async (logsContext) => {
        if (logsContext.err) return;

        for (const log of logsContext.logs) {
          if (log.includes("SubscriptionCreatedEvent")) {
            console.log("[Engine] Detected SubscriptionCreated event in log:", logsContext.signature);

            // Mocked sample address derived from event logic
            const dummyPatron = "9xQeWvG816bUx9EPjHmaT23yvVM2VXmzMz59EQv8rkEk";
            const dummyTierId = 1;
            const ephemeralToken = this.generateSecureToken(dummyPatron, dummyTierId);

            await Promise.all([
              this.notifyDiscord(
                `🎉 **New Patron Subscribed!**\nPatron: \`${dummyPatron}\`\nTier: \`${dummyTierId}\`\nTx: [View on Solscan](https://solscan.io/tx/${logsContext.signature}?cluster=devnet)`
              ),
              this.notifySlack(
                `🎉 *New Subscription:* ${dummyPatron} subscribed to Tier ${dummyTierId}. Tx: ${logsContext.signature}`
              ),
              this.sendContentEntitlementEmail(
                dummyPatron,
                "patron-demo@solanasummit.ca",
                dummyTierId,
                ephemeralToken
              ),
            ]);
          } else if (log.includes("StreamSettledEvent")) {
            console.log("[Engine] Detected Stream Settlement in signature:", logsContext.signature);
          }
        }
      },
      "confirmed"
    );
  }

  public async runAutonomousSettlementCrank(
    profilePubkey: PublicKey,
    tierPubkey: PublicKey,
    subscriptionPubkey: PublicKey,
    escrowVault: PublicKey,
    payoutRecipient: PublicKey
  ): Promise<void> {
    const dummyIdl: Idl = {
      address: this.config.programId.toBase58(),
      version: "0.1.0",
      name: "creator_nexus",
      instructions: [
        {
          name: "claimFunds",
          accounts: [
            { name: "profile", isMut: true, isSigner: false },
            { name: "tier", isMut: false, isSigner: false },
            { name: "subscription", isMut: true, isSigner: false },
            { name: "escrowVault", isMut: true, isSigner: false },
            { name: "payoutRecipient", isMut: true, isSigner: false },
            { name: "caller", isMut: false, isSigner: true },
            { name: "systemProgram", isMut: false, isSigner: false },
          ],
          args: [],
        },
      ],
      accounts: [],
      errors: [],
    };

    const provider = new AnchorProvider(
      this.connection,
      new Wallet(this.config.keeperKeypair),
      { commitment: "confirmed" }
    );

    const program = new Program(dummyIdl, provider);

    try {
      console.log(`[Crank] Executing claim_funds for subscription ${subscriptionPubkey.toBase58()}...`);
      const tx = await (program.methods as any).claimFunds().accounts({
        profile: profilePubkey,
        tier: tierPubkey,
        subscription: subscriptionPubkey,
        escrowVault,
        payoutRecipient,
        caller: this.config.keeperKeypair.publicKey,
        systemProgram: PublicKey.default,
      }).rpc();

      console.log(`[Crank] Funds claimed successfully. Tx Signature: ${tx}`);
    } catch (err) {
      console.error("[Crank] Error sweeping dues:", err instanceof Error ? err.message : err);
    }
  }
}

// Execution Entrypoint
if (require.main === module) {
  const config = loadConfig();
  const relayer = new CreatorNexusRelayer(config);
  relayer.startSubscriptionEventListener();
  console.log("[Engine] CreatorNexus Keeper Engine is live and listening.");
}