use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked, CloseAccount};
use crate::state::{AccessTicket, StreamEscrow};
use crate::errors::CreatorShieldError;

#[derive(Accounts)]
pub struct LiquidateExpiredStream<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,

    /// CHECK: Stream Patron receives remaining rent lamports or residuals
    #[account(mut)]
    pub patron: AccountInfo<'info>,

    /// CHECK: Creator account
    #[account(mut)]
    pub creator: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"stream", creator.key().as_ref(), patron.key().as_ref()],
        bump = stream_escrow.escrow_bump,
        has_one = patron,
        has_one = creator,
        has_one = mint,
        close = keeper // Keeper receives stream PDA rent as incentive bounty!
    )]
    pub stream_escrow: Account<'info, StreamEscrow>,

    #[account(
        mut,
        seeds = [b"access_ticket", stream_escrow.key().as_ref()],
        bump = access_ticket.bump,
        close = patron
    )]
    pub access_ticket: Account<'info, AccessTicket>,

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

pub fn handler(ctx: Context<LiquidateExpiredStream>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let stream = &ctx.accounts.stream_escrow;

    // Guard: Verify stream is depleted
    require!(stream.is_depleted(now)?, CreatorShieldError::StreamStillActive);

    let vault_balance = ctx.accounts.vault.amount;
    let stream_key = stream.key();
    let vault_signer_seeds: &[&[&[u8]]] = &[&[
        b"vault",
        stream_key.as_ref(),
        &[stream.vault_bump],
    ]];

    // Sweep all remaining tokens in the vault directly to the creator
    if vault_balance > 0 {
        let transfer_cpi = CpiContext::new_with_signer(
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
            transfer_cpi,
            vault_balance,
            ctx.accounts.token_mint.decimals,
        )?;
    }

    // Close token vault; send rent lamports to keeper
    let close_cpi = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        CloseAccount {
            account: ctx.accounts.vault.to_account_info(),
            destination: ctx.accounts.keeper.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        },
        vault_signer_seeds,
    );
    token_interface::close_account(close_cpi)?;

    Ok(())
}