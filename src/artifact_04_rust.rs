use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

declare_id!("So1SyncCreatorCha11engePart2Winner11111111");

pub const MAX_RECIPIENTS: usize = 5;
pub const BASIS_POINTS_DIVISOR: u64 = 10_000;
pub const MAX_CHANNEL_NAME_LEN: usize = 32;

#[program]
pub mod solsync {
    use super::*;

    /// Initializes a verified creator channel with multi-contributor royalty split rules.
    pub fn initialize_channel(
        ctx: Context<InitializeChannel>,
        name: String,
        recipients: Vec<Pubkey>,
        basis_points: Vec<u16>,
    ) -> Result<()> {
        require!(
            name.as_bytes().len() > 0 && name.as_bytes().len() <= MAX_CHANNEL_NAME_LEN,
            CreatorError::InvalidNameLength
        );
        require!(
            recipients.len() > 0 && recipients.len() <= MAX_RECIPIENTS,
            CreatorError::InvalidRecipientsLength
        );
        require!(
            recipients.len() == basis_points.len(),
            CreatorError::MismatchedSplitsLength
        );

        let mut total_bps: u64 = 0;
        for &bps in basis_points.iter() {
            require!(bps > 0, CreatorError::ZeroBasisPoints);
            total_bps = total_bps
                .checked_add(bps as u64)
                .ok_or(CreatorError::MathOverflow)?;
        }
        require!(
            total_bps == BASIS_POINTS_DIVISOR,
            CreatorError::InvalidTotalBasisPoints
        );

        let channel = &mut ctx.accounts.channel;
        channel.creator = ctx.accounts.creator.key();
        channel.name = name.clone();
        channel.total_subscribers = 0;
        channel.total_revenue_lamports = 0;
        channel.bump = ctx.bumps.channel;
        channel.vault_bump = ctx.bumps.vault;

        let splitter = &mut ctx.accounts.splitter;
        splitter.channel = channel.key();
        splitter.recipient_count = recipients.len() as u8;
        for i in 0..recipients.len() {
            splitter.recipients[i] = recipients[i];
            splitter.basis_points[i] = basis_points[i];
        }
        splitter.bump = ctx.bumps.splitter;

        emit!(ChannelCreatedEvent {
            channel: channel.key(),
            creator: channel.creator,
            name,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Registers a tier (e.g., Monthly Backer Tier) with duration in seconds and price in lamports.
    pub fn create_tier(
        ctx: Context<CreateTier>,
        tier_id: u8,
        name: String,
        price_lamports: u64,
        validity_seconds: i64,
    ) -> Result<()> {
        require!(validity_seconds > 0, CreatorError::InvalidTierDuration);
        require!(price_lamports > 0, CreatorError::InvalidTierPrice);
        require!(
            name.as_bytes().len() > 0 && name.as_bytes().len() <= MAX_CHANNEL_NAME_LEN,
            CreatorError::InvalidNameLength
        );

        let tier = &mut ctx.accounts.tier;
        tier.channel = ctx.accounts.channel.key();
        tier.tier_id = tier_id;
        tier.name = name.clone();
        tier.price_lamports = price_lamports;
        tier.validity_seconds = validity_seconds;
        tier.bump = ctx.bumps.tier;

        emit!(TierCreatedEvent {
            channel: ctx.accounts.channel.key(),
            tier_id,
            name,
            price_lamports,
            validity_seconds,
        });

        Ok(())
    }

    /// Subscribes a fan to a tier, routes payouts via splitter instantly, and emits audit event.
    pub fn subscribe(
        ctx: Context<Subscribe>,
        external_user_ref: String,
    ) -> Result<()> {
        let clock = Clock::get()?;
        let now = clock.unix_timestamp;
        let tier = &ctx.accounts.tier;
        let splitter = &ctx.accounts.splitter;
        let channel = &mut ctx.accounts.channel;
        let subscription = &mut ctx.accounts.subscription;

        let amount_due = tier.price_lamports;

        // Route payments immediately across configured beneficiaries
        let recipient_count = splitter.recipient_count as usize;
        let mut distributed_sum: u64 = 0;

        for i in 0..recipient_count {
            let beneficiary = ctx.remaining_accounts[i].key();
            require_keys_eq!(
                beneficiary,
                splitter.recipients[i],
                CreatorError::BeneficiaryAccountMismatch
            );

            let bps = splitter.basis_points[i] as u64;
            let share = (amount_due as u128)
                .checked_mul(bps as u128)
                .ok_or(CreatorError::MathOverflow)?
                .checked_div(BASIS_POINTS_DIVISOR as u128)
                .ok_or(CreatorError::MathOverflow)? as u64;

            if share > 0 {
                let cpi_context = CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.subscriber.to_account_info(),
                        to: ctx.remaining_accounts[i].to_account_info(),
                    },
                );
                transfer(cpi_context, share)?;
                distributed_sum = distributed_sum
                    .checked_add(share)
                    .ok_or(CreatorError::MathOverflow)?;
            }
        }

        // Send residual dust lamports due to rounding to the primary creator
        let dust = amount_due
            .checked_sub(distributed_sum)
            .ok_or(CreatorError::MathOverflow)?;
        if dust > 0 {
            let cpi_context = CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.subscriber.to_account_info(),
                    to: ctx.accounts.creator.to_account_info(),
                },
            );
            transfer(cpi_context, dust)?;
        }

        // Initialize or extend expiration
        let new_expiry = if subscription.expires_at > now {
            subscription
                .expires_at
                .checked_add(tier.validity_seconds)
                .ok_or(CreatorError::MathOverflow)?
        } else {
            now.checked_add(tier.validity_seconds)
                .ok_or(CreatorError::MathOverflow)?
        };

        let is_new_sub = subscription.expires_at == 0;
        if is_new_sub {
            channel.total_subscribers = channel
                .total_subscribers
                .checked_add(1)
                .ok_or(CreatorError::MathOverflow)?;
        }

        channel.total_revenue_lamports = channel
            .total_revenue_lamports
            .checked_add(amount_due)
            .ok_or(CreatorError::MathOverflow)?;

        subscription.channel = channel.key();
        subscription.subscriber = ctx.accounts.subscriber.key();
        subscription.tier_id = tier.tier_id;
        subscription.expires_at = new_expiry;
        subscription.bump = ctx.bumps.subscription;

        emit!(SubscriptionPurchasedEvent {
            channel: channel.key(),
            subscriber: ctx.accounts.subscriber.key(),
            tier_id: tier.tier_id,
            paid_lamports: amount_due,
            new_expires_at: new_expiry,
            external_user_ref,
            timestamp: now,
        });

        Ok(())
    }

    /// Direct micro-tipping instruction to support creator with arbitrary lamport donation.
    pub fn send_tip(
        ctx: Context<SendTip>,
        amount_lamports: u64,
        memo: String,
    ) -> Result<()> {
        require!(amount_lamports > 0, CreatorError::InvalidTipAmount);
        require!(memo.as_bytes().len() <= 128, CreatorError::MemoTooLong);

        let channel = &mut ctx.accounts.channel;
        let splitter = &ctx.accounts.splitter;
        let recipient_count = splitter.recipient_count as usize;

        let mut distributed_sum: u64 = 0;
        for i in 0..recipient_count {
            let beneficiary = ctx.remaining_accounts[i].key();
            require_keys_eq!(
                beneficiary,
                splitter.recipients[i],
                CreatorError::BeneficiaryAccountMismatch
            );

            let bps = splitter.basis_points[i] as u64;
            let share = (amount_lamports as u128)
                .checked_mul(bps as u128)
                .ok_or(CreatorError::MathOverflow)?
                .checked_div(BASIS_POINTS_DIVISOR as u128)
                .ok_or(CreatorError::MathOverflow)? as u64;

            if share > 0 {
                let cpi_context = CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.fan.to_account_info(),
                        to: ctx.remaining_accounts[i].to_account_info(),
                    },
                );
                transfer(cpi_context, share)?;
                distributed_sum = distributed_sum
                    .checked_add(share)
                    .ok_or(CreatorError::MathOverflow)?;
            }
        }

        let dust = amount_lamports
            .checked_sub(distributed_sum)
            .ok_or(CreatorError::MathOverflow)?;
        if dust > 0 {
            let cpi_context = CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.fan.to_account_info(),
                    to: ctx.accounts.creator.to_account_info(),
                },
            );
            transfer(cpi_context, dust)?;
        }

        channel.total_revenue_lamports = channel
            .total_revenue_lamports
            .checked_add(amount_lamports)
            .ok_or(CreatorError::MathOverflow)?;

        emit!(TipReceivedEvent {
            channel: channel.key(),
            donor: ctx.accounts.fan.key(),
            amount_lamports,
            memo,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(name: String)]
pub self struct InitializeChannel<'info> {
    #[account(
        init,
        payer = creator,
        space = 8 + ChannelAccount::INIT_SPACE,
        seeds = [b"channel", creator.key().as_ref()],
        bump
    )]
    pub channel: Account<'info, ChannelAccount>,

    #[account(
        init,
        payer = creator,
        space = 8 + SplitterAccount::INIT_SPACE,
        seeds = [b"splitter", channel.key().as_ref()],
        bump
    )]
    pub splitter: Account<'info, SplitterAccount>,

    /// CHECK: PDA used as program escrow vault.
    #[account(
        seeds = [b"vault", channel.key().as_ref()],
        bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct CreateTier<'info> {
    #[account(
        has_one = creator @ CreatorError::Unauthorized
    )]
    pub channel: Account<'info, ChannelAccount>,

    #[account(
        init,
        payer = creator,
        space = 8 + TierAccount::INIT_SPACE,
        seeds = [b"tier", channel.key().as_ref(), &[tier_id]],
        bump
    )]
    pub tier: Account<'info, TierAccount>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Subscribe<'info> {
    #[account(mut)]
    pub channel: Account<'info, ChannelAccount>,

    #[account(
        seeds = [b"tier", channel.key().as_ref(), &[tier.tier_id]],
        bump = tier.bump,
        has_one = channel @ CreatorError::TierChannelMismatch
    )]
    pub tier: Account<'info, TierAccount>,

    #[account(
        seeds = [b"splitter", channel.key().as_ref()],
        bump = splitter.bump,
        has_one = channel @ CreatorError::SplitterChannelMismatch
    )]
    pub splitter: Account<'info, SplitterAccount>,

    #[account(
        init_if_needed,
        payer = subscriber,
        space = 8 + SubscriptionAccount::INIT_SPACE,
        seeds = [b"subscription", subscriber.key().as_ref(), channel.key().as_ref()],
        bump
    )]
    pub subscription: Account<'info, SubscriptionAccount>,

    /// CHECK: Primary creator key for dust settlements and validation.
    #[account(mut, address = channel.creator)]
    pub creator: AccountInfo<'info>,

    #[account(mut)]
    pub subscriber: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SendTip<'info> {
    #[account(mut)]
    pub channel: Account<'info, ChannelAccount>,

    #[account(
        seeds = [b"splitter", channel.key().as_ref()],
        bump = splitter.bump,
        has_one = channel @ CreatorError::SplitterChannelMismatch
    )]
    pub splitter: Account<'info, SplitterAccount>,

    /// CHECK: Channel owner.
    #[account(mut, address = channel.creator)]
    pub creator: AccountInfo<'info>,

    #[account(mut)]
    pub fan: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// -----------------------------------------------------------------------------
// STATE SCHEMAS
// -----------------------------------------------------------------------------

#[account]
pub struct ChannelAccount {
    pub creator: Pubkey,
    pub name: String,
    pub total_subscribers: u64,
    pub total_revenue_lamports: u64,
    pub bump: u8,
    pub vault_bump: u8,
}

impl ChannelAccount {
    pub const INIT_SPACE: usize = 32 + (4 + MAX_CHANNEL_NAME_LEN) + 8 + 8 + 1 + 1;
}

#[account]
pub struct SplitterAccount {
    pub channel: Pubkey,
    pub recipient_count: u8,
    pub recipients: [Pubkey; MAX_RECIPIENTS],
    pub basis_points: [u16; MAX_RECIPIENTS],
    pub bump: u8,
}

impl SplitterAccount {
    pub const INIT_SPACE: usize = 32 + 1 + (32 * MAX_RECIPIENTS) + (2 * MAX_RECIPIENTS) + 1;
}

#[account]
pub struct TierAccount {
    pub channel: Pubkey,
    pub tier_id: u8,
    pub name: String,
    pub price_lamports: u64,
    pub validity_seconds: i64,
    pub bump: u8,
}

impl TierAccount {
    pub const INIT_SPACE: usize = 32 + 1 + (4 + MAX_CHANNEL_NAME_LEN) + 8 + 8 + 1;
}

#[account]
pub struct SubscriptionAccount {
    pub channel: Pubkey,
    pub subscriber: Pubkey,
    pub tier_id: u8,
    pub expires_at: i64,
    pub bump: u8,
}

impl SubscriptionAccount {
    pub const INIT_SPACE: usize = 32 + 32 + 1 + 8 + 1;
}

// -----------------------------------------------------------------------------
// EVENTS
// -----------------------------------------------------------------------------

#[event]
pub struct ChannelCreatedEvent {
    pub channel: Pubkey,
    pub creator: Pubkey,
    pub name: String,
    pub timestamp: i64,
}

#[event]
pub struct TierCreatedEvent {
    pub channel: Pubkey,
    pub tier_id: u8,
    pub name: String,
    pub price_lamports: u64,
    pub validity_seconds: i64,
}

#[event]
pub struct SubscriptionPurchasedEvent {
    pub channel: Pubkey,
    pub subscriber: Pubkey,
    pub tier_id: u8,
    pub paid_lamports: u64,
    pub new_expires_at: i64,
    pub external_user_ref: String,
    pub timestamp: i64,
}

#[event]
pub struct TipReceivedEvent {
    pub channel: Pubkey,
    pub donor: Pubkey,
    pub amount_lamports: u64,
    pub memo: String,
    pub timestamp: i64,
}

// -----------------------------------------------------------------------------
// ERROR CODES
// -----------------------------------------------------------------------------

#[error_code]
pub enum CreatorError {
    #[msg("Channel name exceeds permissible length limit")]
    InvalidNameLength,
    #[msg("Recipients list cannot be empty or exceed maximum supported count")]
    InvalidRecipientsLength,
    #[msg("Recipients and basis points arrays length mismatch")]
    MismatchedSplitsLength,
    #[msg("Basis points must be strictly greater than 0")]
    ZeroBasisPoints,
    #[msg("Cumulative basis points must equal exactly 10,000")]
    InvalidTotalBasisPoints,
    #[msg("Arithmetic overflow detected in financial calculation")]
    MathOverflow,
    #[msg("Tier validity duration must be positive")]
    InvalidTierDuration,
    #[msg("Tier price must be strictly greater than zero")]
    InvalidTierPrice,
    #[msg("Beneficiary account does not match registered splitter account")]
    BeneficiaryAccountMismatch,
    #[msg("Unauthorized access attempt for creator-restricted action")]
    Unauthorized,
    #[msg("Provided tier does not belong to the targeted channel")]
    TierChannelMismatch,
    #[msg("Provided splitter does not belong to the targeted channel")]
    SplitterChannelMismatch,
    #[msg("Tip amount must be strictly greater than 0 lamports")]
    InvalidTipAmount,
    #[msg("Memo string exceeds 128 bytes")]
    MemoTooLong,
}