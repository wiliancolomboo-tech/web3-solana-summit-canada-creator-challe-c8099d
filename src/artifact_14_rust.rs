use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked, CloseAccount};
use crate::state::{AccessTicket, StreamEscrow};
use crate::errors::CreatorShieldError;

#[derive(Accounts)]
pub struct CloseStream<'info> {
    #[account(mut)]
    pub patron: Signer<'info>,

    /// CHECK: Creator account referenced for payout
    #[account(mut)]
    pub creator: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"stream", creator.key().as_ref(), patron.key().as_ref()],
        bump = stream_escrow.escrow_bump,
        has_one = patron,
        has_one = creator,
        has_one = mint,
        close = patron
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
        associated_token::authority = patron,
    )]
    pub patron_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = creator,
    )]
    pub creator_token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handler(ctx: Context<CloseStream>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;

    let stream = &ctx.accounts.stream_escrow;
    let claimable_creator = stream.calculate_unlocked_balance(now)?;
    let total_remaining = stream
        .total_deposited
        .checked_sub(stream.claimed_amount)
        .ok_or(CreatorShieldError::MathOverflow)?;

    let refund_patron = total_remaining
        .checked_sub(claimable_creator)
        .ok_or(CreatorShieldError::MathOverflow)?;

    let stream_key = ctx.accounts.stream_escrow.key();
    let vault_signer_seeds: &[&[&[u8]]] = &[&[
        b"vault",
        stream_key.as_ref(),
        &[ctx.accounts.stream_escrow.vault_bump],
    ]];

    // 1. Pay Creator any accrued uncollected drip
    if claimable_creator > 0 {
        let transfer_to_creator = CpiContext::new_with_signer(
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
            transfer_to_creator,
            claimable_creator,
            ctx.accounts.token_mint.decimals,
        )?;
    }

    // 2. Refund remainder to Patron
    if refund_patron > 0 {
        let transfer_to_patron = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault.to_account_info(),
                mint: ctx.accounts.token_mint.to_account_info(),
                to: ctx.accounts.patron_token_account.to_account_info(),
                authority: ctx.accounts.vault.to_account_info(),
            },
            vault_signer_seeds,
        );
        token_interface::transfer_checked(
            transfer_to_patron,
            refund_patron,
            ctx.accounts.token_mint.decimals,
        )?;
    }

    // 3. Close the token vault account completely
    let close_cpi = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        CloseAccount {
            account: ctx.accounts.vault.to_account_info(),
            destination: ctx.accounts.patron.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        },
        vault_signer_seeds,
    );
    token_interface::close_account(close_cpi)?;

    Ok(())
}