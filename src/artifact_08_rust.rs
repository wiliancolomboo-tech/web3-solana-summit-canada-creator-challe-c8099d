use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use crate::state::*;
use crate::errors::CreatorNexusError;

#[derive(Accounts)]
pub struct Subscribe<'info> {
    #[account(
        mut,
        seeds = [CREATOR_PROFILE_SEED, profile.creator.as_ref()],
        bump = profile.bump
    )]
    pub profile: Account<'info, CreatorProfile>,

    #[account(
        seeds = [
            TIER_SEED,
            profile.key().as_ref(),
            tier.tier_id.to_le_bytes().as_ref()
        ],
        bump = tier.bump,
        constraint = tier.is_active @ CreatorNexusError::TierDisabled
    )]
    pub tier: Account<'info, SubscriptionTier>,

    #[account(
        init,
        payer = patron,
        space = SubscriptionAccount::LEN,
        seeds = [SUBSCRIPTION_SEED, tier.key().as_ref(), patron.key().as_ref()],
        bump
    )]
    pub subscription: Account<'info, SubscriptionAccount>,

    /// CHECK: PDA escrow account that stores lamport collateral
    #[account(
        mut,
        seeds = [ESCROW_SEED, subscription.key().as_ref()],
        bump
    )]
    pub escrow_vault: SystemAccount<'info>,

    #[account(mut)]
    pub patron: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[event]
pub struct SubscriptionCreatedEvent {
    pub creator: Pubkey,
    pub patron: Pubkey,
    pub tier_id: u32,
    pub deposit_amount: u64,
    pub start_timestamp: i64,
}

pub fn handle_subscribe(ctx: Context<Subscribe>, initial_deposit: u64) -> Result<()> {
    let tier = &ctx.accounts.tier;
    require!(initial_deposit >= tier.min_deposit, CreatorNexusError::InsufficientBalance);

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;

    let sub = &mut ctx.accounts.subscription;
    sub.tier = tier.key();
    sub.patron = ctx.accounts.patron.key();
    sub.start_timestamp = now;
    sub.last_settled_at = now;
    sub.total_streamed = 0;
    sub.escrow_bump = ctx.bumps.escrow_vault;
    sub.bump = ctx.bumps.subscription;
    sub.is_active = true;

    let profile = &mut ctx.accounts.profile;
    profile.total_subscribers = profile.total_subscribers.checked_add(1).ok_or(CreatorNexusError::MathOverflow)?;

    // Transfer SOL from patron to Escrow PDA
    transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            Transfer {
                from: ctx.accounts.patron.to_account_info(),
                to: ctx.accounts.escrow_vault.to_account_info(),
            },
        ),
        initial_deposit,
    )?;

    emit!(SubscriptionCreatedEvent {
        creator: profile.creator,
        patron: sub.patron,
        tier_id: tier.tier_id,
        deposit_amount: initial_deposit,
        start_timestamp: now,
    });

    msg!("CreatorNexus: Patron {} subscribed to tier {}", sub.patron, tier.tier_id);
    Ok(())
}