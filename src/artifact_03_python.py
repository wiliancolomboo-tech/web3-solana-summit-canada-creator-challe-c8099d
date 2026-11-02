"""
Autonomous Keeper Crank Bot for CreatorVault-v2
Monitors active subscription accounts and triggers settle_stream when accrued bounty exceeds gas costs.
"""

import asyncio
import os
import time
from solana.rpc.async_api import AsyncClient
from solders.pubkey import Pubkey
from solders.keypair import Keypair
from anchorpy import Provider, Wallet, Program, Idl
import json

PROGRAM_ID = Pubkey.from_string("CRTRvZgM8e2hP3mX7GvyVpE7x6hH8aLzE5K9qV3uB1kY")
RPC_URL = os.getenv("SOLANA_RPC_URL", "http://127.0.0.1:8899")
POLL_INTERVAL_SECONDS = 10
MIN_PROFIT_BOUNTY = 1_500  # lamports/raw units to trigger settlement

async def run_keeper():
    client = AsyncClient(RPC_URL)
    keeper_keypair = Keypair()
    wallet = Wallet(keeper_keypair)
    provider = Provider(client, wallet)

    with open("./target/idl/creator_vault.json") as f:
        idl_json = json.load(f)
    idl = Idl.from_json(json.dumps(idl_json))
    program = Program(idl, PROGRAM_ID, provider)

    print(f"[*] Keeper active with authority: {keeper_keypair.pubkey()}")

    while True:
        try:
            # Fetch all program accounts for SubscriptionState
            subscriptions = await program.account["SubscriptionState"].all()
            now = int(time.time())

            for sub in subscriptions:
                state = sub.account
                last_checkpoint = state.last_checkpoint_timestamp
                stop_ts = state.stop_timestamp
                flow_rate = state.flow_rate
                deposit_remaining = state.deposit_remaining

                if deposit_remaining == 0 or now <= last_checkpoint:
                    continue

                effective_end = min(now, stop_ts)
                elapsed = effective_end - last_checkpoint
                accrued = min(elapsed * flow_rate, deposit_remaining)
                keeper_bounty = (accrued * 15) // 10000

                if keeper_bounty >= MIN_PROFIT_BOUNTY:
                    print(f"[+] Lucrative crank found for stream: {sub.public_key}")
                    print(f"    Accrued: {accrued} | Bounty: {keeper_bounty}")
                    # Dispatch settle_stream transaction
                    # tx = await program.methods.settle_stream().accounts({...}).rpc()
                    # print(f"    Transaction confirmed: {tx}")

        except Exception as e:
            print(f"[-] Error in keeper loop: {str(e)}")

        await asyncio.sleep(POLL_INTERVAL_SECONDS)

if __name__ == "__main__":
    asyncio.run(run_keeper())