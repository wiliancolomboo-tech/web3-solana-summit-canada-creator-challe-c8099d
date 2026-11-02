import { SolSyncEnterpriseAgent } from "./agent";
import * as dotenv from "dotenv";

dotenv.config();

const config = {
  rpcUrl: process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com",
  wsUrl: process.env.SOLANA_WS_URL || "wss://api.devnet.solana.com",
  programId: process.env.PROGRAM_ID || "So1SyncCreatorCha11engePart2Winner11111111",
  resendApiKey: process.env.RESEND_API_KEY || "",
  discordWebhookUrl: process.env.DISCORD_WEBHOOK_URL || "",
  senderEmail: process.env.SENDER_EMAIL || "onboarding@resend.dev",
};

const agent = new SolSyncEnterpriseAgent(config);
agent.start().catch((err) => {
  console.error("Fatal Daemon Failure:", err);
  process.exit(1);
});