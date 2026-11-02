use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

declare_id!("SubSt7K8qVbLmv4eG6xMvZ9rWpY1A5d3F7cNoPQrstU");

#[program]
pub mod substream {
    use super::*;

    /// Initializes a creator profile to register subscription offerings.
    pub fn initialize_creator(
        ctx: Context<InitializeCreator>,
        creator_name: String,
        metadata_uri: String,
    ) -> Result<()> {
        require!(
            creator_name.len() <= 64,
            SubStreamError::CreatorNameTooLong
        );
        require!(
            metadata_uri.len() <= 128,
            SubStreamError::MetadataUriTooLong
        );

        let creator_profile = &mut ctx.accounts.creator_profile;
        creator_profile.creator = ctx.accounts.creator.key();
        creator_profile.creator_name = creator_name;
        creator_profile.metadata_uri = metadata_uri;
        creator_profile.total_subscribers = 0;
        creator_profile.total_revenue = 0;
        creator_profile.tier_count = 0;
        creator_profile.bump = ctx.bumps.creator_profile;

        emit!(CreatorInitializedEvent {
            creator: ctx.accounts.creator.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Creates a dedicated subscription tier with recurring period and cost.
    pub fn create_tier(
        ctx: Context<CreateTier>,
        tier_id: u8,
        name: String,
        cost_lamports: u64,
        cadence_seconds: i64,
    ) -> Result<()> {
        require!(name.len() <= 32, SubStreamError::TierNameTooLong);
        require!(cost_lamports > 0, SubStreamError::InvalidCost);
        require!(cadence_seconds >= 3600, SubStreamError::InvalidCadence); // Min 1 hour

        let creator_profile = &mut ctx.accounts.creator_profile;
        let tier = &mut ctx.accounts.tier;

        tier.creator = ctx.accounts.creator.key();
        tier.tier_id = tier_id;
        tier.name = name.clone();
        tier.cost_lamports = cost_lamports;
        tier.cadence_seconds = cadence_seconds;
        tier.is_active = true;
        tier.bump = ctx.bumps.tier;

        creator_profile.tier_count = creator_profile
            .tier_count
            .checked_add(1)
            .ok_or(SubStreamError::NumericalOverflow)?;

        emit!(TierCreatedEvent {
            creator: ctx.accounts.creator.key(),
            tier_id,
            name,
            cost_lamports,
            cadence_seconds,
        });

        Ok(())
    }

    /// Subscribes a patron to a tier, funding the vault and establishing the expiry timestamp.
    pub fn subscribe(
        ctx: Context<Subscribe>,
        periods: u32,
    ) -> Result<()> {
        require!(periods > 0 && periods <= 12, SubStreamError::InvalidPeriodCount);
        let tier = &ctx.accounts.tier;
        require!(tier.is_active, SubStreamError::TierNotActive);

        let total_cost = tier
            .cost_lamports
            .checked_mul(periods as u64)
            .ok_or(SubStreamError::NumericalOverflow)?;

        let current_time = Clock::get()?.unix_timestamp;
        let additional_time = tier
            .cadence_seconds
            .checked_mul(periods as i64)
            .ok_or(SubStreamError::NumericalOverflow)?;

        // Transfer funds from subscriber to creator vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.subscriber.to_account_info(),
            to: ctx.accounts.vault.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            transfer_instruction,
        );
        transfer(cpi_ctx, total_cost)?;

        let subscription = &mut ctx.accounts.subscription;
        let creator_profile = &mut ctx.accounts.creator_profile;

        if subscription.subscriber == Pubkey::default() {
            // New subscription record
            subscription.subscriber = ctx.accounts.subscriber.key();
            subscription.tier = tier.key();
            subscription.started_at = current_time;
            subscription.expires_at = current_time
                .checked_add(additional_time)
                .ok_or(SubStreamError::NumericalOverflow)?;
            subscription.total_paid = total_cost;
            subscription.bump = ctx.bumps.subscription;

            creator_profile.total_subscribers = creator_profile
                .total_subscribers
                .checked_add(1)
                .ok_or(SubStreamError::NumericalOverflow)?;
        } else {
            // Extension of existing active subscription
            let base_time = if subscription.expires_at > current_time {
                subscription.expires_at
            } else {
                current_time
            };

            subscription.expires_at = base_time
                .checked_add(additional_time)
                .ok_or(SubStreamError::NumericalOverflow)?;
            subscription.total_paid = subscription
                .total_paid
                .checked_add(total_cost)
                .ok_or(SubStreamError::NumericalOverflow)?;
        }

        creator_profile.total_revenue = creator_profile
            .total_revenue
            .checked_add(total_cost)
            .ok_or(SubStreamError::NumericalOverflow)?;

        emit!(SubscriptionCreatedEvent {
            creator: tier.creator,
            tier_id: tier.tier_id,
            subscriber: ctx.accounts.subscriber.key(),
            amount_paid: total_cost,
            expires_at: subscription.expires_at,
        });

        Ok(())
    }

    /// Allows a subscriber to cancel an active subscription and recover PDA storage rent upon expiration.
    pub fn close_expired_subscription(ctx: Context<CloseExpiredSubscription>) -> Result<()> {
        let subscription = &ctx.accounts.subscription;
        let current_time = Clock::get()?.unix_timestamp;

        require!(
            current_time > subscription.expires_at,
            SubStreamError::SubscriptionStillActive
        );

        let creator_profile = &mut ctx.accounts.creator_profile;
        if creator_profile.total_subscribers > 0 {
            creator_profile.total_subscribers = creator_profile
                .total_subscribers
                .saturating_sub(1);
        }

        emit!(SubscriptionClosedEvent {
            subscriber: subscription.subscriber,
            tier: subscription.tier,
            refund_destination: ctx.accounts.subscriber.key(),
        });

        Ok(())
    }

    /// Creator claims accumulated subscription earnings from their dedicated vault.
    pub fn withdraw_earnings(
        ctx: Context<WithdrawEarnings>,
        amount: u64,
    ) -> Result<()> {
        let vault_info = ctx.accounts.vault.to_account_info();
        let creator_info = ctx.accounts.creator.to_account_info();

        let rent_minimum = Rent::get()?.minimum_balance(0);
        let available_balance = vault_info
            .lamports()
            .checked_sub(rent_minimum)
            .ok_or(SubStreamError::InsufficientFunds)?;

        require!(
            amount <= available_balance,
            SubStreamError::WithdrawalExceedsAvailableBalance
        );

        **vault_info.try_borrow_mut_lamports()? = vault_info
            .lamports()
            .checked_sub(amount)
            .ok_or(SubStreamError::NumericalOverflow)?;

        **creator_info.try_borrow_mut_lamports()? = creator_info
            .lamports()
            .checked_add(amount)
            .ok_or(SubStreamError::NumericalOverflow)?;

        emit!(EarningsWithdrawnEvent {
            creator: ctx.accounts.creator.key(),
            amount,
            remaining_balance: vault_info.lamports(),
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(creator_name: String, metadata_uri: String)]
pub struct InitializeCreator<'info> {
    #[account(
        init,
        payer = creator,
        space = 8 + CreatorProfile::LEN,
        seeds = [b"creator-profile", creator.key().as_ref()],
        bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct CreateTier<'info> {
    #[account(
        mut,
        seeds = [b"creator-profile", creator.key().as_ref()],
        bump = creator_profile.bump,
        has_one = creator @ SubStreamError::UnauthorizedCreator
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(
        init,
        payer = creator,
        space = 8 + SubscriptionTier::LEN,
        seeds = [b"subscription-tier", creator.key().as_ref(), &[tier_id]],
        bump
    )]
    pub tier: Account<'info, SubscriptionTier>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Subscribe<'info> {
    #[account(
        mut,
        seeds = [b"creator-profile", tier.creator.as_ref()],
        bump = creator_profile.bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(
        seeds = [b"subscription-tier", tier.creator.as_ref(), &[tier.tier_id]],
        bump = tier.bump
    )]
    pub tier: Account<'info, SubscriptionTier>,
    #[account(
        init_if_needed,
        payer = subscriber,
        space = 8 + SubscriptionRecord::LEN,
        seeds = [b"subscription", tier.key().as_ref(), subscriber.key().as_ref()],
        bump
    )]
    pub subscription: Account<'info, SubscriptionRecord>,
    /// CHECK: Safe PDA acting as protocol escrow vault.
    #[account(
        mut,
        seeds = [b"vault", tier.creator.as_ref()],
        bump
    )]
    pub vault: AccountInfo<'info>,
    #[account(mut)]
    pub subscriber: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseExpiredSubscription<'info> {
    #[account(
        mut,
        seeds = [b"creator-profile", tier.creator.as_ref()],
        bump = creator_profile.bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(
        seeds = [b"subscription-tier", tier.creator.as_ref(), &[tier.tier_id]],
        bump = tier.bump
    )]
    pub tier: Account<'info, SubscriptionTier>,
    #[account(
        mut,
        seeds = [b"subscription", tier.key().as_ref(), subscriber.key().as_ref()],
        bump = subscription.bump,
        has_one = subscriber,
        close = subscriber
    )]
    pub subscription: Account<'info, SubscriptionRecord>,
    #[account(mut)]
    pub subscriber: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawEarnings<'info> {
    #[account(
        seeds = [b"creator-profile", creator.key().as_ref()],
        bump = creator_profile.bump,
        has_one = creator @ SubStreamError::UnauthorizedCreator
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    /// CHECK: PDA escrow vault governed by creator seeds
    #[account(
        mut,
        seeds = [b"vault", creator.key().as_ref()],
        bump
    )]
    pub vault: AccountInfo<'info>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// -----------------------------------------------------------------------------
// ACCOUNT STATES
// -----------------------------------------------------------------------------

#[account]
pub struct CreatorProfile {
    pub creator: Pubkey,          // 32
    pub creator_name: String,     // 4 + 64
    pub metadata_uri: String,     // 4 + 128
    pub total_subscribers: u64,   // 8
    pub total_revenue: u64,       // 8
    pub tier_count: u8,           // 1
    pub bump: u8,                 // 1
}

impl CreatorProfile {
    pub const LEN: usize = 32 + (4 + 64) + (4 + 128) + 8 + 8 + 1 + 1;
}

#[account]
pub struct SubscriptionTier {
    pub creator: Pubkey,          // 32
    pub tier_id: u8,              // 1
    pub name: String,             // 4 + 32
    pub cost_lamports: u64,       // 8
    pub cadence_seconds: i64,     // 8
    pub is_active: bool,          // 1
    pub bump: u8,                 // 1
}

impl SubscriptionTier {
    pub const LEN: usize = 32 + 1 + (4 + 32) + 8 + 8 + 1 + 1;
}

#[account]
pub struct SubscriptionRecord {
    pub subscriber: Pubkey,       // 32
    pub tier: Pubkey,             // 32
    pub started_at: i64,          // 8
    pub expires_at: i64,          // 8
    pub total_paid: u64,          // 8
    pub bump: u8,                 // 1
}

impl SubscriptionRecord {
    pub const LEN: usize = 32 + 32 + 8 + 8 + 8 + 1;
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct CreatorInitializedEvent {
    pub creator: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct TierCreatedEvent {
    pub creator: Pubkey,
    pub tier_id: u8,
    pub name: String,
    pub cost_lamports: u64,
    pub cadence_seconds: i64,
}

#[event]
pub struct SubscriptionCreatedEvent {
    pub creator: Pubkey,
    pub tier_id: u8,
    pub subscriber: Pubkey,
    pub amount_paid: u64,
    pub expires_at: i64,
}

#[event]
pub struct SubscriptionClosedEvent {
    pub subscriber: Pubkey,
    pub tier: Pubkey,
    pub refund_destination: Pubkey,
}

#[event]
pub struct EarningsWithdrawnEvent {
    pub creator: Pubkey,
    pub amount: u64,
    pub remaining_balance: u64,
}

#[error_code]
pub enum SubStreamError {
    #[msg("Creator name exceeds max length of 64 bytes")]
    CreatorNameTooLong,
    #[msg("Metadata URI exceeds max length of 128 bytes")]
    MetadataUriTooLong,
    #[msg("Tier name exceeds max length of 32 bytes")]
    TierNameTooLong,
    #[msg("Tier cost must be greater than zero")]
    InvalidCost,
    #[msg("Cadence must be at least 3600 seconds (1 hour)")]
    InvalidCadence,
    #[msg("Numerical calculation overflow")]
    NumericalOverflow,
    #[msg("Unauthorized signer for this creator profile")]
    UnauthorizedCreator,
    #[msg("Subscription tier is currently marked inactive")]
    TierNotActive,
    #[msg("Subscription period must be between 1 and 12")]
    InvalidPeriodCount,
    #[msg("Cannot close an active subscription before its expiration")]
    SubscriptionStillActive,
    #[msg("Insufficient lamports in vault to withdraw")]
    InsufficientFunds,
    #[msg("Withdrawal amount exceeds available balance above rent reserve")]
    WithdrawalExceedsAvailableBalance,
}