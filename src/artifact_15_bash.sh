cd orchestrator
cp .env.example .env
# Fill in RESEND_API_KEY and DISCORD_WEBHOOK_URL
docker compose up -d --build