use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::CreatorNexusError;

#[derive(Accounts)]
pub struct ClaimFunds<'info> {
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
        bump = tier.bump
    )]
    pub tier: Account<'info, SubscriptionTier>,

    #[account(
        mut,
        seeds = [SUBSCRIPTION_SEED, tier.key().as_ref(), subscription.patron.as_ref()],
        bump = subscription.bump,
        constraint = subscription.is_active @ CreatorNexusError::SubscriptionInactive
    )]
    pub subscription: Account<'info, SubscriptionAccount>,

    /// CHECK: PDA escrow holding subscriber lamports
    #[account(
        mut,
        seeds = [ESCROW_SEED, subscription.key().as_ref()],
        bump = subscription.escrow_bump
    )]
    pub escrow_vault: SystemAccount<'info>,

    /// CHECK: Direct payment recipient designated on creator profile
    #[account(
        mut,
        address = profile.payout_recipient @ CreatorNexusError::Unauthorized
    )]
    pub payout_recipient: AccountInfo<'info>,

    /// Autonomous keeper or creator triggering claim
    pub caller: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[event]
pub struct StreamSettledEvent {
    pub creator: Pubkey,
    pub patron: Pubkey,
    pub settled_lamports: u64,
    pub timestamp: i64,
}

pub fn handle_claim_funds(ctx: Context<ClaimFunds>) -> Result<()> {
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let sub = &mut ctx.accounts.subscription;
    let tier = &ctx.accounts.tier;

    let elapsed = (now - sub.last_settled_at).max(0) as u64;
    let owed = elapsed.checked_mul(tier.rate_per_second).ok_or(CreatorNexusError::MathOverflow)?;

    if owed == 0 {
        return Ok(());
    }

    let escrow_lamports = ctx.accounts.escrow_vault.lamports();
    let payable = owed.min(escrow_lamports);

    if payable > 0 {
        **ctx.accounts.escrow_vault.to_account_info().try_borrow_mut_lamports()? -= payable;
        **ctx.accounts.payout_recipient.try_borrow_mut_lamports()? += payable;

        sub.last_settled_at = now;
        sub.total_streamed = sub.total_streamed.checked_add(payable).ok_or(CreatorNexusError::MathOverflow)?;

        let profile = &mut ctx.accounts.profile;
        profile.total_earned = profile.total_earned.checked_add(payable).ok_or(CreatorNexusError::MathOverflow)?;

        emit!(StreamSettledEvent {
            creator: profile.creator,
            patron: sub.patron,
            settled_lamports: payable,
            timestamp: now,
        });

        msg!("CreatorNexus: Settled {} lamports to creator", payable);
    }

    // Auto-disable if escrow runs dry
    if ctx.accounts.escrow_vault.lamports() == 0 {
        sub.is_active = false;
        msg!("CreatorNexus: Escrow exhausted, subscription marked inactive");
    }

    Ok(())
}