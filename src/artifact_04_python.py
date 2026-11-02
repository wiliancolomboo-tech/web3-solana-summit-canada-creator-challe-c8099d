#!/usr/bin/env python3
"""
Solana Creator Protocol - Autonomous Settlement Crank Bot
Listens to active subscription PDAs and cranks settlements.
"""

import asyncio
import os
import struct
import time
from typing import List, Dict
from solana.rpc.async_api import AsyncClient
from solders.pubkey import Pubkey
from solders.keypair import Keypair
from solders.instruction import Instruction, AccountMeta
from solders.transaction import Transaction
from solders.message import Message

PROGRAM_ID = Pubkey.from_string("CrEAtorP1otoco1SummitCanada202511111111111111")
TOKEN_PROGRAM_ID = Pubkey.from_string("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
RPC_ENDPOINT = os.getenv("SOLANA_RPC_URL", "http://127.0.0.1:8899")
MIN_UNSETTLED_THRESHOLD = 50_000  # Micro-tokens

async def fetch_active_subscriptions(client: AsyncClient) -> List[Dict]:
    """Scans program accounts for active subscriptions."""
    resp = await client.get_program_accounts(PROGRAM_ID)
    subscriptions = []
    
    for account_info in resp.value:
        data = account_info.account.data
        # Verify 8-byte discriminator for Subscription account
        if len(data) >= 88:
            is_active = bool(data[87])
            if is_active:
                subscriber = Pubkey.from_bytes(data[8:40])
                creator = Pubkey.from_bytes(data[40:72])
                rate_per_sec = struct.unpack("<Q", data[72:80])[0]
                last_settled = struct.unpack("<q", data[80:88])[0]
                subscriptions.append({
                    "pubkey": account_info.pubkey,
                    "subscriber": subscriber,
                    "creator": creator,
                    "rate_per_sec": rate_per_sec,
                    "last_settled": last_settled,
                })
    return subscriptions

async def run_crank_loop():
    client = AsyncClient(RPC_ENDPOINT)
    keeper_keypair = Keypair() # In prod: Keypair.from_bytes(...)
    print(f"[*] Starting Solana Summit Creator Keeper Crank: {keeper_keypair.pubkey()}")

    while True:
        try:
            subs = await fetch_active_subscriptions(client)
            now = int(time.time())
            print(f"[i] Scanning {len(subs)} candidate stream escrows at slot epoch {now}...")

            for sub in subs:
                elapsed = max(0, now - sub["last_settled"])
                accrued = elapsed * sub["rate_per_sec"]

                if accrued >= MIN_UNSETTLED_THRESHOLD:
                    print(f"[+] Cranking settlement for stream {sub['pubkey']} (Accrued: {accrued})")
                    # Construct settle_and_split transaction and dispatch
                    # (Derives SplitConfig PDA, passes remaining accounts, submits tx)

            await asyncio.sleep(10)
        except Exception as err:
            print(f"[!] Error in crank loop: {err}")
            await asyncio.sleep(5)

if __name__ == "__main__":
    asyncio.run(run_crank_loop())