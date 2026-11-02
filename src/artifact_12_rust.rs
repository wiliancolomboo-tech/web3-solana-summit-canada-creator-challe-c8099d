// src/lib.rs
use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("CNex111111111111111111111111111111111111111");

#[program]
pub mod creator_nexus {
    use super::*;

    pub fn initialize_creator(
        ctx: Context<InitializeCreator>,
        handle: String,
    ) -> Result<()> {
        instructions::initialize_creator::handle_initialize_creator(ctx, handle)
    }

    pub fn initialize_tier(
        ctx: Context<InitializeTier>,
        title: String,
        rate_per_second: u64,
        min_deposit: u64,
    ) -> Result<()> {
        instructions::initialize_tier::handle_initialize_tier(ctx, title, rate_per_second, min_deposit)
    }

    pub fn subscribe(ctx: Context<Subscribe>, initial_deposit: u64) -> Result<()> {
        instructions::subscribe::handle_subscribe(ctx, initial_deposit)
    }

    pub fn claim_funds(ctx: Context<ClaimFunds>) -> Result<()> {
        instructions::claim_funds::handle_claim_funds(ctx)
    }

    pub fn cancel_subscription(ctx: Context<CancelSubscription>) -> Result<()> {
        instructions::cancel_subscription::handle_cancel_subscription(ctx)
    }
}