use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};
use crate::state::{CreatorProfile, StreamEscrow};
use crate::errors::CreatorShieldError;

#[derive(Accounts)]
pub struct ClaimFunds<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,

    #[account(
        mut,
        seeds = [b"creator_profile", creator.key().as_ref()],
        bump = creator_profile.bump,
        has_one = creator,
        has_one = token_mint,
    )]
    pub creator_profile: Account<'info, CreatorProfile>,

    #[account(
        mut,
        seeds = [b"stream", creator.key().as_ref(), stream_escrow.patron.as_ref()],
        bump = stream_escrow.escrow_bump,
        has_one = creator,
        has_one = mint,
    )]
    pub stream_escrow: Account<'info, StreamEscrow>,

    pub token_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        seeds = [b"vault", stream_escrow.key().as_ref()],
        bump = stream_escrow.vault_bump,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = creator,
    )]
    pub creator_token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handler(ctx: Context<ClaimFunds>) -> Result<()> {
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;

    let claimable = ctx.accounts.stream_escrow.calculate_unlocked_balance(now)?;
    require!(claimable > 0, CreatorShieldError::ZeroClaimableFunds);

    let stream_key = ctx.accounts.stream_escrow.key();
    let vault_signer_seeds: &[&[&[u8]]] = &[&[
        b"vault",
        stream_key.as_ref(),
        &[ctx.accounts.stream_escrow.vault_bump],
    ]];

    let cpi_transfer = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        TransferChecked {
            from: ctx.accounts.vault.to_account_info(),
            mint: ctx.accounts.token_mint.to_account_info(),
            to: ctx.accounts.creator_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        },
        vault_signer_seeds,
    );

    token_interface::transfer_checked(
        cpi_transfer,
        claimable,
        ctx.accounts.token_mint.decimals,
    )?;

    // Mutate state post-transfer
    let stream = &mut ctx.accounts.stream_escrow;
    stream.claimed_amount = stream
        .claimed_amount
        .checked_add(claimable)
        .ok_or(CreatorShieldError::MathOverflow)?;
    stream.last_claimed_at = now;

    let profile = &mut ctx.accounts.creator_profile;
    profile.lifetime_earned = profile.lifetime_earned.saturating_add(claimable);

    Ok(())
}