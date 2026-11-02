use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

declare_id!("PatrnXb7R97eZ9wQG3HwYkmhGZkGZJmQc7LpWn2VNmS");

pub const MAX_COLLABORATORS: usize = 5;
pub const BPS_DIVISOR: u64 = 10_000;

#[program]
pub mod patron_engine {
    use super::*;

    /// Initializes a creator profile and their monetization tiers.
    pub fn initialize_creator(
        ctx: Context<InitializeCreator>,
        creator_fee_bps: u16,
        collaborator_splits: Vec<CollaboratorSplit>,
    ) -> Result<()> {
        require!(
            collaborator_splits.len() <= MAX_COLLABORATORS,
            PatronError::TooManyCollaborators
        );

        let mut total_split: u64 = creator_fee_bps as u64;
        for split in collaborator_splits.iter() {
            total_split = total_split
                .checked_add(split.bps as u64)
                .ok_or(PatronError::MathOverflow)?;
        }
        require!(total_split == BPS_DIVISOR, PatronError::InvalidSplitTotal);

        let creator_profile = &mut ctx.accounts.creator_profile;
        creator_profile.authority = ctx.accounts.authority.key();
        creator_profile.vault = ctx.accounts.creator_vault.key();
        creator_profile.total_patrons = 0;
        creator_profile.total_volume_lamports = 0;
        creator_profile.collaborator_splits = collaborator_splits;
        creator_profile.bump = ctx.bumps.creator_profile;
        creator_profile.vault_bump = ctx.bumps.creator_vault;

        emit!(CreatorInitializedEvent {
            creator: creator_profile.authority,
            vault: creator_profile.vault,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Creates or updates a subscription tier.
    pub fn create_tier(
        ctx: Context<CreateTier>,
        tier_id: u8,
        cost_lamports: u64,
        duration_seconds: i64,
        tier_metadata_uri: String,
    ) -> Result<()> {
        require!(cost_lamports > 0, PatronError::ZeroCostTier);
        require!(duration_seconds >= 60, PatronError::TierDurationTooShort);
        require!(tier_metadata_uri.len() <= 128, PatronError::UriTooLong);

        let tier = &mut ctx.accounts.tier;
        tier.creator = ctx.accounts.creator_profile.key();
        tier.tier_id = tier_id;
        tier.cost_lamports = cost_lamports;
        tier.duration_seconds = duration_seconds;
        tier.is_active = true;
        tier.metadata_uri = tier_metadata_uri;
        tier.bump = ctx.bumps.tier;

        emit!(TierConfiguredEvent {
            creator: tier.creator,
            tier_id,
            cost_lamports,
            duration_seconds,
        });

        Ok(())
    }

    /// Subscribes a patron to a creator tier for specified duration periods.
    pub fn subscribe(
        ctx: Context<Subscribe>,
        periods: u32,
    ) -> Result<()> {
        require!(periods > 0 && periods <= 52, PatronError::InvalidPeriodCount);
        let tier = &ctx.accounts.tier;
        require!(tier.is_active, PatronError::TierInactive);

        let total_cost = tier
            .cost_lamports
            .checked_mul(periods as u64)
            .ok_or(PatronError::MathOverflow)?;

        // Execute payment transfer directly into the escrow vault
        let transfer_cpi = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            Transfer {
                from: ctx.accounts.patron.to_account_info(),
                to: ctx.accounts.creator_vault.to_account_info(),
            },
        );
        transfer(transfer_cpi, total_cost)?;

        let clock = Clock::get()?;
        let now = clock.unix_timestamp;
        let subscription = &mut ctx.accounts.subscription;

        let total_duration = tier
            .duration_seconds
            .checked_mul(periods as i64)
            .ok_or(PatronError::MathOverflow)?;

        if subscription.active_until > now {
            // Extension of an active subscription
            subscription.active_until = subscription
                .active_until
                .checked_add(total_duration)
                .ok_or(PatronError::MathOverflow)?;
        } else {
            // New subscription or re-activation
            subscription.patron = ctx.accounts.patron.key();
            subscription.creator = ctx.accounts.creator_profile.key();
            subscription.tier_id = tier.tier_id;
            subscription.started_at = now;
            subscription.active_until = now
                .checked_add(total_duration)
                .ok_or(PatronError::MathOverflow)?;
            subscription.bump = ctx.bumps.subscription;

            let creator = &mut ctx.accounts.creator_profile;
            creator.total_patrons = creator
                .total_patrons
                .checked_add(1)
                .ok_or(PatronError::MathOverflow)?;
        }

        let creator = &mut ctx.accounts.creator_profile;
        creator.total_volume_lamports = creator
            .total_volume_lamports
            .checked_add(total_cost)
            .ok_or(PatronError::MathOverflow)?;

        emit!(SubscriptionPurchasedEvent {
            creator: ctx.accounts.creator_profile.key(),
            patron: ctx.accounts.patron.key(),
            tier_id: tier.tier_id,
            lamports_paid: total_cost,
            active_until: subscription.active_until,
            timestamp: now,
        });

        Ok(())
    }

    /// Disburses escrow vault balances to creator and collaborators according to BPS configuration.
    pub fn settle_and_disburse(ctx: Context<SettleAndDisburse>) -> Result<()> {
        let vault_lamports = ctx.accounts.creator_vault.lamports();
        let rent = Rent::get()?;
        let rent_exempt_minimum = rent.minimum_balance(0);

        require!(
            vault_lamports > rent_exempt_minimum,
            PatronError::InsufficientVaultBalance
        );

        let distributable = vault_lamports
            .checked_sub(rent_exempt_minimum)
            .ok_or(PatronError::MathOverflow)?;

        let creator_profile = &ctx.accounts.creator_profile;
        let auth_key = creator_profile.authority;
        let vault_bump = creator_profile.vault_bump;

        let seeds = &[
            b"creator_vault".as_ref(),
            auth_key.as_ref(),
            &[vault_bump],
        ];
        let signer_seeds = &[&seeds[..]];

        // Disburse collaborator splits
        let remaining_accounts = ctx.remaining_accounts;
        let splits = &creator_profile.collaborator_splits;
        require!(
            remaining_accounts.len() == splits.len(),
            PatronError::MismatchedCollaboratorAccounts
        );

        let mut total_distributed = 0u64;

        for (i, split) in splits.iter().enumerate() {
            let target_account = &remaining_accounts[i];
            require_keys_eq!(target_account.key(), split.recipient, PatronError::InvalidRecipient);

            let amount = distributable
                .checked_mul(split.bps as u64)
                .ok_or(PatronError::MathOverflow)?
                .checked_div(BPS_DIVISOR)
                .ok_or(PatronError::MathOverflow)?;

            if amount > 0 {
                **ctx.accounts.creator_vault.try_borrow_mut_lamports()? -= amount;
                **target_account.try_borrow_mut_lamports()? += amount;
                total_distributed = total_distributed
                    .checked_add(amount)
                    .ok_or(PatronError::MathOverflow)?;
            }
        }

        // Remainder belongs to creator authority
        let creator_share = distributable
            .checked_sub(total_distributed)
            .ok_or(PatronError::MathOverflow)?;

        if creator_share > 0 {
            **ctx.accounts.creator_vault.try_borrow_mut_lamports()? -= creator_share;
            **ctx.accounts.authority.try_borrow_mut_lamports()? += creator_share;
        }

        emit!(FundsDisbursedEvent {
            creator: ctx.accounts.authority.key(),
            total_disbursed: distributable,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION STRUCTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
pub struct InitializeCreator<'info> {
    #[account(
        init,
        payer = authority,
        space = CreatorProfile::LEN,
        seeds = [b"creator_profile", authority.key().as_ref()],
        bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,

    /// CHECK: Escrow PDA seed verification only
    #[account(
        mut,
        seeds = [b"creator_vault", authority.key().as_ref()],
        bump
    )]
    pub creator_vault: SystemAccount<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(tier_id: u8)]
pub struct CreateTier<'info> {
    #[account(
        mut,
        has_one = authority @ PatronError::Unauthorized
    )]
    pub creator_profile: Account<'info, CreatorProfile>,

    #[account(
        init_if_needed,
        payer = authority,
        space = Tier::LEN,
        seeds = [b"tier", creator_profile.key().as_ref(), &[tier_id]],
        bump
    )]
    pub tier: Account<'info, Tier>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Subscribe<'info> {
    #[account(mut)]
    pub patron: Signer<'info>,

    #[account(
        mut,
        seeds = [b"creator_profile", creator_profile.authority.as_ref()],
        bump = creator_profile.bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,

    #[account(
        mut,
        seeds = [b"tier", creator_profile.key().as_ref(), &[tier.tier_id]],
        bump = tier.bump
    )]
    pub tier: Account<'info, Tier>,

    #[account(
        init_if_needed,
        payer = patron,
        space = Subscription::LEN,
        seeds = [b"subscription", patron.key().as_ref(), creator_profile.key().as_ref()],
        bump
    )]
    pub subscription: Account<'info, Subscription>,

    /// CHECK: Safe PDA derived from creator authority
    #[account(
        mut,
        seeds = [b"creator_vault", creator_profile.authority.as_ref()],
        bump = creator_profile.vault_bump
    )]
    pub creator_vault: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SettleAndDisburse<'info> {
    #[account(
        mut,
        has_one = authority @ PatronError::Unauthorized
    )]
    pub creator_profile: Account<'info, CreatorProfile>,

    /// CHECK: Protected by PDA seeds and checked against creator authority
    #[account(
        mut,
        seeds = [b"creator_vault", authority.key().as_ref()],
        bump = creator_profile.vault_bump
    )]
    pub creator_vault: SystemAccount<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

// -----------------------------------------------------------------------------
// STATE STRUCTS
// -----------------------------------------------------------------------------

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub struct CollaboratorSplit {
    pub recipient: Pubkey,
    pub bps: u16, // Basis points (1% = 100 bps)
}

#[account]
pub struct CreatorProfile {
    pub authority: Pubkey,
    pub vault: Pubkey,
    pub total_patrons: u64,
    pub total_volume_lamports: u64,
    pub bump: u8,
    pub vault_bump: u8,
    pub collaborator_splits: Vec<CollaboratorSplit>,
}

impl CreatorProfile {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 8 + 1 + 1 + 4 + (MAX_COLLABORATORS * (32 + 2));
}

#[account]
pub struct Tier {
    pub creator: Pubkey,
    pub tier_id: u8,
    pub cost_lamports: u64,
    pub duration_seconds: i64,
    pub is_active: bool,
    pub metadata_uri: String,
    pub bump: u8,
}

impl Tier {
    pub const LEN: usize = 8 + 32 + 1 + 8 + 8 + 1 + (4 + 128) + 1;
}

#[account]
pub struct Subscription {
    pub patron: Pubkey,
    pub creator: Pubkey,
    pub tier_id: u8,
    pub started_at: i64,
    pub active_until: i64,
    pub bump: u8,
}

impl Subscription {
    pub const LEN: usize = 8 + 32 + 32 + 1 + 8 + 8 + 1;
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct CreatorInitializedEvent {
    pub creator: Pubkey,
    pub vault: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct TierConfiguredEvent {
    pub creator: Pubkey,
    pub tier_id: u8,
    pub cost_lamports: u64,
    pub duration_seconds: i64,
}

#[event]
pub struct SubscriptionPurchasedEvent {
    pub creator: Pubkey,
    pub patron: Pubkey,
    pub tier_id: u8,
    pub lamports_paid: u64,
    pub active_until: i64,
    pub timestamp: i64,
}

#[event]
pub struct FundsDisbursedEvent {
    pub creator: Pubkey,
    pub total_disbursed: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum PatronError {
    #[msg("Collaborator list exceeds maximum limit of 5")]
    TooManyCollaborators,
    #[msg("Total Basis Points must strictly equal 10,000 (100%)")]
    InvalidSplitTotal,
    #[msg("Mathematical overflow occurred")]
    MathOverflow,
    #[msg("Tier cost in lamports must be greater than zero")]
    ZeroCostTier,
    #[msg("Tier duration must be at least 60 seconds")]
    TierDurationTooShort,
    #[msg("Metadata URI string exceeds maximum length of 128 characters")]
    UriTooLong,
    #[msg("Caller is unauthorized to mutate this profile")]
    Unauthorized,
    #[msg("Tier is not currently active")]
    TierInactive,
    #[msg("Invalid subscription period amount")]
    InvalidPeriodCount,
    #[msg("Vault has insufficient balance beyond rent-exempt reserve")]
    InsufficientVaultBalance,
    #[msg("Collaborator remaining accounts count does not match registered splits")]
    MismatchedCollaboratorAccounts,
    #[msg("Remaining account pubkey does not match configured collaborator recipient")]
    InvalidRecipient,
}