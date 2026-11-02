use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::CreatorNexusError;

#[derive(Accounts)]
#[instruction(handle: String)]
pub申struct InitializeCreator<'info> {
    #[account(
        init,
        payer = authority,
        space = CreatorProfile::LEN,
        seeds = [CREATOR_PROFILE_SEED, authority.key().as_ref()],
        bump
    )]
    pub profile: Account<'info, CreatorProfile>,

    /// CHECK: Safe payout receiver account validated by the authority
    pub payout_recipient: AccountInfo<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_creator(
    ctx: Context<InitializeCreator>,
    handle: String,
) -> Result<()> {
    require!(handle.len() <= 32, CreatorNexusError::InvalidMetadataLength);
    require!(!handle.is_empty(), CreatorNexusError::InvalidMetadataLength);

    let profile = &mut ctx.accounts.profile;
    profile.creator = ctx.accounts.authority.key();
    profile.payout_recipient = ctx.accounts.payout_recipient.key();
    profile.total_subscribers = 0;
    profile.total_earned = 0;
    profile.tier_counter = 0;
    profile.bump = ctx.bumps.profile;
    profile.handle = handle;

    msg!("CreatorNexus: Initialized Creator Profile @ {}", profile.handle);
    Ok(())
}