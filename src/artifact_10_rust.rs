use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::CreatorNexusError;

#[derive(Accounts)]
pub struct CancelSubscription<'info> {
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
        seeds = [SUBSCRIPTION_SEED, tier.key().as_ref(), patron.key().as_ref()],
        bump = subscription.bump,
        constraint = subscription.patron == patron.key() @ CreatorNexusError::Unauthorized
    )]
    pub subscription: Account<'info, SubscriptionAccount>,

    /// CHECK: PDA escrow account
    #[account(
        mut,
        seeds = [ESCROW_SEED, subscription.key().as_ref()],
        bump = subscription.escrow_bump
    )]
    pub escrow_vault: SystemAccount<'info>,

    /// CHECK: Profile payout recipient
    #[account(
        mut,
        address = profile.payout_recipient @ CreatorNexusError::Unauthorized
    )]
    pub payout_recipient: AccountInfo<'info>,

    #[account(mut)]
    pub patron: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[event]
pub struct SubscriptionCancelledEvent {
    pub creator: Pubkey,
    pub patron: Pubkey,
    pub refund_amount: u64,
    pub final_settled: u64,
    pub timestamp: i64,
}

pub fn handle_cancel_subscription(ctx: Context<CancelSubscription>) -> Result<()> {
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let sub = &mut ctx.accounts.subscription;
    let tier = &ctx.accounts.tier;

    // Settle pending dues first
    let elapsed = (now - sub.last_settled_at).max(0) as u64;
    let owed = elapsed.checked_mul(tier.rate_per_second).ok_or(CreatorNexusError::MathOverflow)?;

    let escrow_lamports = ctx.accounts.escrow_vault.lamports();
    let payable_to_creator = owed.min(escrow_lamports);
    let refund_to_patron = escrow_lamports.saturating_sub(payable_to_creator);

    if payable_to_creator > 0 {
        **ctx.accounts.escrow_vault.to_account_info().try_borrow_mut_lamports()? -= payable_to_creator;
        **ctx.accounts.payout_recipient.try_borrow_mut_lamports()? += payable_to_creator;

        let profile = &mut ctx.accounts.profile;
        profile.total_earned = profile.total_earned.checked_add(payable_to_creator).ok_or(CreatorNexusError::MathOverflow)?;
    }

    if refund_to_patron > 0 {
        **ctx.accounts.escrow_vault.to_account_info().try_borrow_mut_lamports()? -= refund_to_patron;
        **ctx.accounts.patron.to_account_info().try_borrow_mut_lamports()? += refund_to_patron;
    }

    sub.is_active = false;
    sub.last_settled_at = now;

    let profile = &mut ctx.accounts.profile;
    profile.total_subscribers = profile.total_subscribers.saturating_sub(1);

    emit!(SubscriptionCancelledEvent {
        creator: profile.creator,
        patron: ctx.accounts.patron.key(),
        refund_amount: refund_to_patron,
        final_settled: payable_to_creator,
        timestamp: now,
    });

    msg!(
        "CreatorNexus: Cancelled. Creator claimed: {}, Patron refunded: {}",
        payable_to_creator,
        refund_to_patron
    );

    Ok(())
}