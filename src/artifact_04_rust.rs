use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("AuraStrEAM1111111111111111111111111111111111");

pub const MAX_BPS: u16 = 10_000;
pub const MAX_COLLABORATORS: usize = 3;
pub const DISCRIMINATOR_LENGTH: usize = 8;

#[program]
pub mod aurastream {
    use super::*;

    /// Initializes a globally unique creator registry profile.
    pub fn initialize_creator(
        ctx: Context<InitializeCreator>,
        metadata_uri: String,
    ) -> Result<()> {
        require!(metadata_uri.len() <= 128, AuraError::StringTooLong);

        let creator = &mut ctx.accounts.creator_profile;
        creator.authority = ctx.accounts.authority.key();
        creator.metadata_uri = metadata_uri;
        creator.tier_count = 0;
        creator.total_revenue = 0;
        creator.bump = ctx.bumps.creator_profile;

        emit!(CreatorInitializedEvent {
            authority: creator.authority,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Creates a monetization tier specifying cost per second and collaborator splits.
    pub fn create_tier(
        ctx: Context<CreateTier>,
        tier_id: u8,
        rate_per_second: u64,
        min_duration_seconds: u64,
        collaborator_share_bps: u16,
    ) -> Result<()> {
        require!(rate_per_second > 0, AuraError::InvalidPaymentRate);
        require!(min_duration_seconds >= 60, AuraError::DurationTooShort);
        require!(collaborator_share_bps <= MAX_BPS, AuraError::InvalidBps);

        let tier = &mut ctx.accounts.subscription_tier;
        tier.creator_profile = ctx.accounts.creator_profile.key();
        tier.tier_id = tier_id;
        tier.mint = ctx.accounts.mint.key();
        tier.rate_per_second = rate_per_second;
        tier.min_duration_seconds = min_duration_seconds;
        tier.collaborator = ctx.accounts.collaborator.key();
        tier.collaborator_share_bps = collaborator_share_bps;
        tier.active_subscribers = 0;
        tier.bump = ctx.bumps.subscription_tier;
        tier.vault_bump = ctx.bumps.vault_authority;

        let profile = &mut ctx.accounts.creator_profile;
        profile.tier_count = profile.tier_count.checked_add(1).ok_or(AuraError::MathOverflow)?;

        emit!(TierCreatedEvent {
            creator: profile.key(),
            tier_id,
            rate_per_second,
            collaborator: tier.collaborator,
            collaborator_share_bps,
        });

        Ok(())
    }

    /// Locks user funds into the tier escrow PDA and activates the real-time stream.
    pub fn subscribe(ctx: Context<Subscribe>, duration_seconds: u64) -> Result<()> {
        let tier = &mut ctx.accounts.subscription_tier;
        require!(duration_seconds >= tier.min_duration_seconds, AuraError::DurationTooShort);

        let deposit_amount = tier
            .rate_per_second
            .checked_mul(duration_seconds)
            .ok_or(AuraError::MathOverflow)?;

        // Execute CPI: Transfer SPL tokens from user to vault
        let cpi_accounts = Transfer {
            from: ctx.accounts.subscriber_token_account.to_account_info(),
            to: ctx.accounts.escrow_vault.to_account_info(),
            authority: ctx.accounts.subscriber.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, deposit_amount)?;

        // Configure subscription state
        let clock = Clock::get()?;
        let sub = &mut ctx.accounts.subscription;
        sub.tier = tier.key();
        sub.subscriber = ctx.accounts.subscriber.key();
        sub.start_timestamp = clock.unix_timestamp;
        sub.end_timestamp = clock
            .unix_timestamp
            .checked_add(duration_seconds as i64)
            .ok_or(AuraError::MathOverflow)?;
        sub.last_claimed_timestamp = clock.unix_timestamp;
        sub.total_deposited = deposit_amount;
        sub.claimed_amount = 0;
        sub.bump = ctx.bumps.subscription;

        tier.active_subscribers = tier.active_subscribers.checked_add(1).ok_or(AuraError::MathOverflow)?;

        emit!(SubscriptionStartedEvent {
            subscriber: sub.subscriber,
            tier: sub.tier,
            deposit_amount,
            end_timestamp: sub.end_timestamp,
        });

        Ok(())
    }

    /// Claims linearly accrued tokens from subscriber escrow and splits between creator & collaborator.
    pub fn claim_stream(ctx: Context<ClaimStream>) -> Result<()> {
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
        let sub = &mut ctx.accounts.subscription;
        let tier = &ctx.accounts.subscription_tier;

        require!(current_time > sub.last_claimed_timestamp, AuraError::ZeroAccrual);

        // Calculate bounded time window
        let effective_time = current_time.min(sub.end_timestamp);
        let elapsed = effective_time
            .checked_sub(sub.last_claimed_timestamp)
            .ok_or(AuraError::MathOverflow)? as u64;

        require!(elapsed > 0, AuraError::ZeroAccrual);

        let claimable_amount = elapsed
            .checked_mul(tier.rate_per_second)
            .ok_or(AuraError::MathOverflow)?;

        // Calculate BPS allocations
        let collaborator_share = claimable_amount
            .checked_mul(tier.collaborator_share_bps as u64)
            .ok_or(AuraError::MathOverflow)?
            .checked_div(MAX_BPS as u64)
            .ok_or(AuraError::MathOverflow)?;

        let creator_share = claimable_amount
            .checked_sub(collaborator_share)
            .ok_or(AuraError::MathOverflow)?;

        // Update accounting
        sub.last_claimed_timestamp = effective_time;
        sub.claimed_amount = sub
            .claimed_amount
            .checked_add(claimable_amount)
            .ok_or(AuraError::MathOverflow)?;

        let creator_profile = &mut ctx.accounts.creator_profile;
        creator_profile.total_revenue = creator_profile
            .total_revenue
            .checked_add(claimable_amount)
            .ok_or(AuraError::MathOverflow)?;

        // Transfer funds via Vault PDA Seeds
        let tier_key = tier.key();
        let seeds = &[
            b"vault_authority".as_ref(),
            tier_key.as_ref(),
            &[tier.vault_bump],
        ];
        let signer_seeds = &[&seeds[..]];

        // 1. Transfer Creator Share
        if creator_share > 0 {
            let cpi_accounts = Transfer {
                from: ctx.accounts.escrow_vault.to_account_info(),
                to: ctx.accounts.creator_token_account.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                signer_seeds,
            );
            token::transfer(cpi_ctx, creator_share)?;
        }

        // 2. Transfer Collaborator Share
        if collaborator_share > 0 {
            let cpi_accounts = Transfer {
                from: ctx.accounts.escrow_vault.to_account_info(),
                to: ctx.accounts.collaborator_token_account.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                signer_seeds,
            );
            token::transfer(cpi_ctx, collaborator_share)?;
        }

        emit!(StreamClaimedEvent {
            tier: tier.key(),
            subscriber: sub.subscriber,
            creator_payout: creator_share,
            collaborator_payout: collaborator_share,
            new_last_claimed: sub.last_claimed_timestamp,
        });

        Ok(())
    }

    /// Allows subscriber to unilaterally cancel, refund unstreamed tokens, and reclaim rent.
    pub fn cancel_subscription(ctx: Context<CancelSubscription>) -> Result<()> {
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
        let sub = &ctx.accounts.subscription;
        let tier = &mut ctx.accounts.subscription_tier;

        let effective_time = current_time.min(sub.end_timestamp);
        let elapsed_since_last_claim = effective_time
            .saturating_sub(sub.last_claimed_timestamp) as u64;

        let unearned_stream_debt = elapsed_since_last_claim
            .checked_mul(tier.rate_per_second)
            .ok_or(AuraError::MathOverflow)?;

        let total_consumed = sub
            .claimed_amount
            .checked_add(unearned_stream_debt)
            .ok_or(AuraError::MathOverflow)?;

        let refund_amount = sub
            .total_deposited
            .checked_sub(total_consumed)
            .ok_or(AuraError::InsufficientFundsRemaining)?;

        let tier_key = tier.key();
        let seeds = &[
            b"vault_authority".as_ref(),
            tier_key.as_ref(),
            &[tier.vault_bump],
        ];
        let signer_seeds = &[&seeds[..]];

        // Settle creator unearned debt if any
        if unearned_stream_debt > 0 {
            let collaborator_cut = unearned_stream_debt
                .checked_mul(tier.collaborator_share_bps as u64)
                .ok_or(AuraError::MathOverflow)?
                .checked_div(MAX_BPS as u64)
                .ok_or(AuraError::MathOverflow)?;
            let creator_cut = unearned_stream_debt
                .checked_sub(collaborator_cut)
                .ok_or(AuraError::MathOverflow)?;

            if creator_cut > 0 {
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        Transfer {
                            from: ctx.accounts.escrow_vault.to_account_info(),
                            to: ctx.accounts.creator_token_account.to_account_info(),
                            authority: ctx.accounts.vault_authority.to_account_info(),
                        },
                        signer_seeds,
                    ),
                    creator_cut,
                )?;
            }
            if collaborator_cut > 0 {
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        Transfer {
                            from: ctx.accounts.escrow_vault.to_account_info(),
                            to: ctx.accounts.collaborator_token_account.to_account_info(),
                            authority: ctx.accounts.vault_authority.to_account_info(),
                        },
                        signer_seeds,
                    ),
                    collaborator_cut,
                )?;
            }
        }

        // Refund remaining unstreamed tokens to subscriber
        if refund_amount > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.subscriber_token_account.to_account_info(),
                        authority: ctx.accounts.vault_authority.to_account_info(),
                    },
                    signer_seeds,
                ),
                refund_amount,
            )?;
        }

        tier.active_subscribers = tier.active_subscribers.saturating_sub(1);

        emit!(SubscriptionCancelledEvent {
            subscriber: sub.subscriber,
            tier: tier.key(),
            refunded_amount: refund_amount,
        });

        // Account is automatically closed and rent returned to subscriber via `close = subscriber`
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(metadata_uri: String)]
pub struct InitializeCreator<'info> {
    #[account(
        init,
        payer = authority,
        space = DISCRIMINATOR_LENGTH + CreatorProfile::INIT_SPACE,
        seeds = [b"creator_profile", authority.key().as_ref()],
        bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct CreateTier<'info> {
    #[account(
        mut,
        has_one = authority @ AuraError::Unauthorized
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(
        init,
        payer = authority,
        space = DISCRIMINATOR_LENGTH + SubscriptionTier::INIT_SPACE,
        seeds = [b"subscription_tier", creator_profile.key().as_ref(), &[tier_id]],
        bump
    )]
    pub subscription_tier: Account<'info, SubscriptionTier>,
    /// CHECK: Vault authority PDA serving as transfer signer
    #[account(
        seeds = [b"vault_authority", subscription_tier.key().as_ref()],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,
    #[account(
        init,
        payer = authority,
        token::mint = mint,
        token::authority = vault_authority,
        seeds = [b"escrow_vault", subscription_tier.key().as_ref()],
        bump
    )]
    pub escrow_vault: Account<'info, TokenAccount>,
    pub mint: Account<'info, Mint>,
    /// CHECK: Collaborator recipient account
    pub collaborator: AccountInfo<'info>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Subscribe<'info> {
    #[account(mut)]
    pub subscription_tier: Account<'info, SubscriptionTier>,
    #[account(
        init,
        payer = subscriber,
        space = DISCRIMINATOR_LENGTH + UserSubscription::INIT_SPACE,
        seeds = [b"subscription", subscription_tier.key().as_ref(), subscriber.key().as_ref()],
        bump
    )]
    pub subscription: Account<'info, UserSubscription>,
    #[account(
        mut,
        seeds = [b"escrow_vault", subscription_tier.key().as_ref()],
        bump
    )]
    pub escrow_vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = subscriber_token_account.mint == subscription_tier.mint @ AuraError::InvalidMint,
        constraint = subscriber_token_account.owner == subscriber.key() @ AuraError::Unauthorized
    )]
    pub subscriber_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub subscriber: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimStream<'info> {
    #[account(mut)]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(
        mut,
        has_one = creator_profile @ AuraError::InvalidCreatorProfile,
        has_one = collaborator @ AuraError::InvalidCollaborator
    )]
    pub subscription_tier: Account<'info, SubscriptionTier>,
    #[account(
        mut,
        has_one = tier @ AuraError::InvalidTier
    )]
    pub subscription: Account<'info, UserSubscription>,
    /// CHECK: Validated by PDA derivation seeds
    #[account(
        seeds = [b"vault_authority", subscription_tier.key().as_ref()],
        bump = subscription_tier.vault_bump
    )]
    pub vault_authority: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"escrow_vault", subscription_tier.key().as_ref()],
        bump
    )]
    pub escrow_vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = creator_token_account.mint == subscription_tier.mint @ AuraError::InvalidMint
    )]
    pub creator_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = collaborator_token_account.mint == subscription_tier.mint @ AuraError::InvalidMint
    )]
    pub collaborator_token_account: Account<'info, TokenAccount>,
    /// CHECK: Matches tier collaborator pubkey
    pub collaborator: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelSubscription<'info> {
    #[account(
        mut,
        has_one = collaborator @ AuraError::InvalidCollaborator
    )]
    pub subscription_tier: Account<'info, SubscriptionTier>,
    #[account(
        mut,
        close = subscriber,
        has_one = subscriber @ AuraError::Unauthorized,
        has_one = tier @ AuraError::InvalidTier
    )]
    pub subscription: Account<'info, UserSubscription>,
    /// CHECK: Derived PDA
    #[account(
        seeds = [b"vault_authority", subscription_tier.key().as_ref()],
        bump = subscription_tier.vault_bump
    )]
    pub vault_authority: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"escrow_vault", subscription_tier.key().as_ref()],
        bump
    )]
    pub escrow_vault: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = subscriber_token_account.mint == subscription_tier.mint @ AuraError::InvalidMint
    )]
    pub subscriber_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = creator_token_account.mint == subscription_tier.mint @ AuraError::InvalidMint
    )]
    pub creator_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = collaborator_token_account.mint == subscription_tier.mint @ AuraError::InvalidMint
    )]
    pub collaborator_token_account: Account<'info, TokenAccount>,
    /// CHECK: Checked against tier collaborator
    pub collaborator: AccountInfo<'info>,
    #[account(mut)]
    pub subscriber: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

// -----------------------------------------------------------------------------
// STATE STRUCTS
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct CreatorProfile {
    pub authority: Pubkey,
    #[max_len(128)]
    pub metadata_uri: String,
    pub tier_count: u8,
    pub total_revenue: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct SubscriptionTier {
    pub creator_profile: Pubkey,
    pub mint: Pubkey,
    pub collaborator: Pubkey,
    pub rate_per_second: u64,
    pub min_duration_seconds: u64,
    pub active_subscribers: u32,
    pub collaborator_share_bps: u16,
    pub tier_id: u8,
    pub bump: u8,
    pub vault_bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct UserSubscription {
    pub tier: Pubkey,
    pub subscriber: Pubkey,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub last_claimed_timestamp: i64,
    pub total_deposited: u64,
    pub claimed_amount: u64,
    pub bump: u8,
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct CreatorInitializedEvent {
    pub authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct TierCreatedEvent {
    pub creator: Pubkey,
    pub tier_id: u8,
    pub rate_per_second: u64,
    pub collaborator: Pubkey,
    pub collaborator_share_bps: u16,
}

#[event]
pub struct SubscriptionStartedEvent {
    pub subscriber: Pubkey,
    pub tier: Pubkey,
    pub deposit_amount: u64,
    pub end_timestamp: i64,
}

#[event]
pub struct StreamClaimedEvent {
    pub tier: Pubkey,
    pub subscriber: Pubkey,
    pub creator_payout: u64,
    pub collaborator_payout: u64,
    pub new_last_claimed: i64,
}

#[event]
pub struct SubscriptionCancelledEvent {
    pub subscriber: Pubkey,
    pub tier: Pubkey,
    pub refunded_amount: u64,
}

#[error_code]
pub enum AuraError {
    #[msg("Provided URI string length exceeds maximum allowable allocation (128 bytes)")]
    StringTooLong,
    #[msg("Signer is not authorized to execute this state mutation")]
    Unauthorized,
    #[msg("Streaming rate per second must be strictly greater than zero")]
    InvalidPaymentRate,
    #[msg("Subscription duration is lower than minimum enforced duration (60s)")]
    DurationTooShort,
    #[msg("Collaborator split basis points cannot exceed 10000 (100%)")]
    InvalidBps,
    #[msg("Arithmetic overflow detected during execution")]
    MathOverflow,
    #[msg("Supplied token mint does not match tier payment mint")]
    InvalidMint,
    #[msg("Zero accruable streaming funds available for claim")]
    ZeroAccrual,
    #[msg("Supplied creator profile account does not match expected reference")]
    InvalidCreatorProfile,
    #[msg("Supplied collaborator account does not match tier configuration")]
    InvalidCollaborator,
    #[msg("Supplied subscription tier account reference is invalid")]
    InvalidTier,
    #[msg("Insufficient remaining balance in escrow to satisfy refund request")]
    InsufficientFundsRemaining,
}