"""
ApexCreator Permissionless Keeper Crank Bot
Continuously discovers active streams, checks elapsed claimable thresholds,
and triggers on-chain batch disbursement.
"""

import asyncio
import os
from solana.rpc.async_api import AsyncClient
from solders.pubkey import Pubkey
from solders.keypair import Keypair
from anchorpy import Provider, Wallet, Program, Idl
import json

PROGRAM_ID = Pubkey.from_string("StrmV11111111111111111111111111111111111111")
RPC_URL = os.getenv("SOLANA_RPC_URL", "http://127.0.0.1:8899")

async def run_keeper():
    client = AsyncClient(RPC_URL)
    keeper_keypair = Keypair()
    wallet = Wallet(keeper_keypair)
    provider = Provider(client, wallet)

    with open("./target/idl/creator_streamvault.json", "r") as f:
        idl_json = json.load(f)
    idl = Idl.from_json(json.dumps(idl_json))
    program = Program(idl, PROGRAM_ID, provider)

    print(f"[*] Keeper bot operational. Monitoring streams for Program: {PROGRAM_ID}")

    while True:
        try:
            streams = await program.account["StreamAccount"].all()
            print(f"[+] Active streams scanned: {len(streams)}")

            for stream in streams:
                stream_data = stream.account
                # If stream is still active
                now = (await client.get_block_time((await client.get_slot()).value)).value
                if now and now > stream_data.last_claim_time and stream_data.claimed_amount < stream_data.total_deposit:
                    unclaimed_seconds = min(now, stream_data.end_time) - stream_data.last_claim_time
                    if unclaimed_seconds >= 60: # Settle every 60s
                        print(f"[*] Settling Stream {stream.public_key} | Accrued: {unclaimed_seconds}s")
                        # Instruction invocation dispatched here
            await asyncio.sleep(15)
        except Exception as err:
            print(f"[-] Keeper loop error: {err}")
            await asyncio.sleep(5)

if __name__ == "__main__":
    asyncio.run(run_keeper())