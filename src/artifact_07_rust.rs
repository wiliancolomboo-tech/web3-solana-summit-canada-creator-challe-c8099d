use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::CreatorNexusError;

#[derive(Accounts)]
#[instruction(title: String)]
pub struct InitializeTier<'info> {
    #[account(
        mut,
        has_one = creator @ CreatorNexusError::Unauthorized,
        seeds = [CREATOR_PROFILE_SEED, creator.key().as_ref()],
        bump = profile.bump
    )]
    pub profile: Account<'info, CreatorProfile>,

    #[account(
        init,
        payer = creator,
        space = SubscriptionTier::LEN,
        seeds = [
            TIER_SEED,
            profile.key().as_ref(),
            profile.tier_counter.to_le_bytes().as_ref()
        ],
        bump
    )]
    pub tier: Account<'info, SubscriptionTier>,

    #[account(mut)]
    pub creator: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_tier(
    ctx: Context<InitializeTier>,
    title: String,
    rate_per_second: u64,
    min_deposit: u64,
) -> Result<()> {
    require!(title.len() <= 48, CreatorNexusError::InvalidMetadataLength);
    require!(rate_per_second > 0, CreatorNexusError::InvalidStreamRate);
    require!(min_deposit > 0, CreatorNexusError::InvalidStreamRate);

    let profile = &mut ctx.accounts.profile;
    let tier = &mut ctx.accounts.tier;

    tier.creator_profile = profile.key();
    tier.tier_id = profile.tier_counter;
    tier.rate_per_second = rate_per_second;
    tier.min_deposit = min_deposit;
    tier.is_active = true;
    tier.bump = ctx.bumps.tier;
    tier.title = title;

    profile.tier_counter = profile.tier_counter.checked_add(1).ok_or(CreatorNexusError::MathOverflow)?;

    msg!("CreatorNexus: Created Tier ID: {} with rate: {} lamports/s", tier.tier_id, rate_per_second);
    Ok(())
}