use anchor_lang::prelude::*;
use anchor_lang::solana_program::system_instruction;

declare_id!("Cre8Tor111111111111111111111111111111111111");

pub const MAX_METADATA_LEN: usize = 96;
pub const MAX_CAMPAIGN_NAME_LEN: usize = 48;
pub const BPS_DENOMINATOR: u64 = 10_000;
pub const PROTOCOL_FEE_BPS: u64 = 250; // 2.5% protocol maintenance fee

#[program]
pub mod solcreator_engine {
    use super::*;

    /// Initialize a Sovereign Creator Profile
    pub fn initialize_creator(
        ctx: Context<InitializeCreator>,
        handle: String,
        metadata_uri: String,
    ) -> Result<()> {
        require!(
            handle.as_bytes().len() > 0 && handle.as_bytes().len() <= 32,
            CreatorError::HandleLengthExceeded
        );
        require!(
            metadata_uri.as_bytes().len() <= MAX_METADATA_LEN,
            CreatorError::UriLengthExceeded
        );

        let creator_profile = &mut ctx.accounts.creator_profile;
        creator_profile.authority = ctx.accounts.authority.key();
        creator_profile.payout_wallet = ctx.accounts.payout_wallet.key();
        creator_profile.handle = handle;
        creator_profile.metadata_uri = metadata_uri;
        creator_profile.total_earned = 0;
        creator_profile.campaigns_count = 0;
        creator_profile.subscribers_count = 0;
        creator_profile.bump = ctx.bumps.creator_profile;

        emit!(CreatorInitialized {
            creator: ctx.accounts.authority.key(),
            handle: creator_profile.handle.clone(),
            payout_wallet: creator_profile.payout_wallet,
        });

        Ok(())
    }

    /// Create and Fund a Multi-Stage Milestone Campaign Escrow
    pub fn create_campaign_escrow(
        ctx: Context<CreateCampaignEscrow>,
        campaign_id: u64,
        campaign_name: String,
        total_milestones: u8,
        total_funding_amount: u64,
        deadline_timestamp: i64,
    ) -> Result<()> {
        require!(
            campaign_name.as_bytes().len() > 0 && campaign_name.as_bytes().len() <= MAX_CAMPAIGN_NAME_LEN,
            CreatorError::InvalidCampaignName
        );
        require!(total_milestones > 0 && total_milestones <= 10, CreatorError::InvalidMilestoneCount);
        require!(total_funding_amount > 0, CreatorError::ZeroFundingAmount);
        
        let clock = Clock::get()?;
        require!(deadline_timestamp > clock.unix_timestamp, CreatorError::InvalidDeadline);

        // Transfer funds from sponsor to Escrow PDA
        let deposit_ix = system_instruction::transfer(
            &ctx.accounts.sponsor.key(),
            &ctx.accounts.campaign_escrow.key(),
            total_funding_amount,
        );
        anchor_lang::solana_program::program::invoke(
            &deposit_ix,
            &[
                ctx.accounts.sponsor.to_account_info(),
                ctx.accounts.campaign_escrow.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        let escrow = &mut ctx.accounts.campaign_escrow;
        escrow.campaign_id = campaign_id;
        escrow.creator = ctx.accounts.creator_profile.key();
        escrow.sponsor = ctx.accounts.sponsor.key();
        escrow.campaign_name = campaign_name;
        escrow.total_funding_amount = total_funding_amount;
        escrow.remaining_balance = total_funding_amount;
        escrow.total_milestones = total_milestones;
        escrow.completed_milestones = 0;
        escrow.deadline_timestamp = deadline_timestamp;
        escrow.is_cancelled = false;
        escrow.is_fully_claimed = false;
        escrow.bump = ctx.bumps.campaign_escrow;

        let creator_profile = &mut ctx.accounts.creator_profile;
        creator_profile.campaigns_count = creator_profile.campaigns_count.checked_add(1).ok_or(CreatorError::NumericalOverflow)?;

        emit!(CampaignCreated {
            campaign_id,
            creator: ctx.accounts.creator_profile.key(),
            sponsor: ctx.accounts.sponsor.key(),
            total_funding_amount,
            total_milestones,
            deadline_timestamp,
        });

        Ok(())
    }

    /// Claim Milestone Payout upon Verified Deliverable
    pub fn claim_milestone_payout(
        ctx: Context<ClaimMilestonePayout>,
        campaign_id: u64,
        proof_uri: String,
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.campaign_escrow;
        require!(!escrow.is_cancelled, CreatorError::CampaignCancelled);
        require!(!escrow.is_fully_claimed, CreatorError::CampaignAlreadyCompleted);
        require!(escrow.completed_milestones < escrow.total_milestones, CreatorError::AllMilestonesCompleted);
        require!(proof_uri.as_bytes().len() <= MAX_METADATA_LEN, CreatorError::UriLengthExceeded);

        let payout_per_milestone = escrow.total_funding_amount
            .checked_div(escrow.total_milestones as u64)
            .ok_or(CreatorError::NumericalOverflow)?;

        require!(escrow.remaining_balance >= payout_per_milestone, CreatorError::InsufficientEscrowBalance);

        // Protocol fee deduction
        let protocol_cut = payout_per_milestone
            .checked_mul(PROTOCOL_FEE_BPS)
            .ok_or(CreatorError::NumericalOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(CreatorError::NumericalOverflow)?;
        let creator_payout = payout_per_milestone
            .checked_sub(protocol_cut)
            .ok_or(CreatorError::NumericalOverflow)?;

        // State update prior to value transfer (Checks-Effects-Interactions)
        escrow.completed_milestones = escrow.completed_milestones.checked_add(1).ok_or(CreatorError::NumericalOverflow)?;
        escrow.remaining_balance = escrow.remaining_balance.checked_sub(payout_per_milestone).ok_or(CreatorError::NumericalOverflow)?;
        
        if escrow.completed_milestones == escrow.total_milestones {
            escrow.is_fully_claimed = true;
        }

        let creator_profile = &mut ctx.accounts.creator_profile;
        creator_profile.total_earned = creator_profile.total_earned.checked_add(creator_payout).ok_or(CreatorError::NumericalOverflow)?;

        // Execute transfers directly out of Escrow PDA
        **ctx.accounts.campaign_escrow.to_account_info().try_borrow_mut_lamports()? = ctx
            .accounts
            .campaign_escrow
            .to_account_info()
            .lamports()
            .checked_sub(payout_per_milestone)
            .ok_or(CreatorError::NumericalOverflow)?;

        **ctx.accounts.creator_payout_wallet.try_borrow_mut_lamports()? = ctx
            .accounts
            .creator_payout_wallet
            .lamports()
            .checked_add(creator_payout)
            .ok_or(CreatorError::NumericalOverflow)?;

        **ctx.accounts.protocol_treasury.try_borrow_mut_lamports()? = ctx
            .accounts
            .protocol_treasury
            .lamports()
            .checked_add(protocol_cut)
            .ok_or(CreatorError::NumericalOverflow)?;

        emit!(MilestoneClaimed {
            campaign_id,
            milestone_index: escrow.completed_milestones,
            creator_payout,
            protocol_fee: protocol_cut,
            proof_uri,
        });

        Ok(())
    }

    /// Sponsor Refund in case of Campaign Expiration & Non-Delivery
    pub fn refund_expired_campaign(
        ctx: Context<RefundExpiredCampaign>,
        _campaign_id: u64,
    ) -> Result<()> {
        let clock = Clock::get()?;
        let escrow = &mut ctx.accounts.campaign_escrow;

        require!(!escrow.is_cancelled, CreatorError::CampaignCancelled);
        require!(!escrow.is_fully_claimed, CreatorError::CampaignAlreadyCompleted);
        require!(clock.unix_timestamp > escrow.deadline_timestamp, CreatorError::CampaignNotExpired);
        require!(escrow.remaining_balance > 0, CreatorError::ZeroEscrowBalance);

        let refund_amount = escrow.remaining_balance;
        escrow.remaining_balance = 0;
        escrow.is_cancelled = true;

        **ctx.accounts.campaign_escrow.to_account_info().try_borrow_mut_lamports()? = ctx
            .accounts
            .campaign_escrow
            .to_account_info()
            .lamports()
            .checked_sub(refund_amount)
            .ok_or(CreatorError::NumericalOverflow)?;

        **ctx.accounts.sponsor.try_borrow_mut_lamports()? = ctx
            .accounts
            .sponsor
            .lamports()
            .checked_add(refund_amount)
            .ok_or(CreatorError::NumericalOverflow)?;

        emit!(CampaignRefunded {
            campaign_id: escrow.campaign_id,
            sponsor: ctx.accounts.sponsor.key(),
            refund_amount,
        });

        Ok(())
    }

    /// Fan Direct Support & Tier Subscription
    pub fn subscribe_tier(
        ctx: Context<SubscribeTier>,
        tier_level: u8,
        duration_seconds: i64,
    ) -> Result<()> {
        require!(tier_level >= 1 && tier_level <= 3, CreatorError::InvalidSubscriptionTier);
        require!(duration_seconds >= 86_400, CreatorError::InvalidDuration); // Minimum 1 day

        // Pricing tier table in lamports
        let tier_price = match tier_level {
            1 => 50_000_000,    // 0.05 SOL
            2 => 150_000_000,   // 0.15 SOL
            3 => 500_000_000,   // 0.50 SOL
            _ => return Err(CreatorError::InvalidSubscriptionTier.into()),
        };

        // Transfer funds directly to creator payout wallet
        let split_fee = tier_price
            .checked_mul(PROTOCOL_FEE_BPS)
            .ok_or(CreatorError::NumericalOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(CreatorError::NumericalOverflow)?;
        let creator_cut = tier_price.checked_sub(split_fee).ok_or(CreatorError::NumericalOverflow)?;

        let pay_creator_ix = system_instruction::transfer(
            &ctx.accounts.subscriber.key(),
            &ctx.accounts.creator_payout_wallet.key(),
            creator_cut,
        );
        anchor_lang::solana_program::program::invoke(
            &pay_creator_ix,
            &[
                ctx.accounts.subscriber.to_account_info(),
                ctx.accounts.creator_payout_wallet.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        let pay_fee_ix = system_instruction::transfer(
            &ctx.accounts.subscriber.key(),
            &ctx.accounts.protocol_treasury.key(),
            split_fee,
        );
        anchor_lang::solana_program::program::invoke(
            &pay_fee_ix,
            &[
                ctx.accounts.subscriber.to_account_info(),
                ctx.accounts.protocol_treasury.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;

        let clock = Clock::get()?;
        let subscription = &mut ctx.accounts.subscription_receipt;
        
        // Handle renewal or new subscription
        let starting_timestamp = if subscription.expires_at > clock.unix_timestamp {
            subscription.expires_at
        } else {
            clock.unix_timestamp
        };

        subscription.subscriber = ctx.accounts.subscriber.key();
        subscription.creator = ctx.accounts.creator_profile.key();
        subscription.tier = tier_level;
        subscription.started_at = clock.unix_timestamp;
        subscription.expires_at = starting_timestamp
            .checked_add(duration_seconds)
            .ok_or(CreatorError::NumericalOverflow)?;
        subscription.bump = ctx.bumps.subscription_receipt;

        let creator_profile = &mut ctx.accounts.creator_profile;
        creator_profile.subscribers_count = creator_profile.subscribers_count.checked_add(1).ok_or(CreatorError::NumericalOverflow)?;
        creator_profile.total_earned = creator_profile.total_earned.checked_add(creator_cut).ok_or(CreatorError::NumericalOverflow)?;

        emit!(FanSubscribed {
            subscriber: ctx.accounts.subscriber.key(),
            creator: ctx.accounts.creator_profile.key(),
            tier: tier_level,
            expires_at: subscription.expires_at,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(handle: String)]
pub struct InitializeCreator<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + CreatorProfile::LEN,
        seeds = [b"creator", authority.key().as_ref()],
        bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    /// CHECK: Target wallet receiving milestone and subscription proceeds
    pub payout_wallet: AccountInfo<'info>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(campaign_id: u64)]
pub struct CreateCampaignEscrow<'info> {
    #[account(
        init,
        payer = sponsor,
        space = 8 + CampaignEscrow::LEN,
        seeds = [b"campaign_escrow", creator_profile.key().as_ref(), campaign_id.to_le_bytes().as_ref()],
        bump
    )]
    pub campaign_escrow: Account<'info, CampaignEscrow>,
    #[account(
        mut,
        seeds = [b"creator", creator_profile.authority.as_ref()],
        bump = creator_profile.bump
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(mut)]
    pub sponsor: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(campaign_id: u64)]
pub struct ClaimMilestonePayout<'info> {
    #[account(
        mut,
        seeds = [b"campaign_escrow", creator_profile.key().as_ref(), campaign_id.to_le_bytes().as_ref()],
        bump = campaign_escrow.bump,
        has_one = creator
    )]
    pub campaign_escrow: Account<'info, CampaignEscrow>,
    #[account(
        mut,
        seeds = [b"creator", authority.key().as_ref()],
        bump = creator_profile.bump,
        has_one = authority,
        has_one = payout_wallet @ CreatorError::UnauthorizedPayoutDestination
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    /// CHECK: Creator authority signer
    pub authority: Signer<'info>,
    /// CHECK: Verified match with creator_profile.payout_wallet
    #[account(mut)]
    pub creator_payout_wallet: AccountInfo<'info>,
    /// CHECK: Configured protocol treasury account
    #[account(
        mut,
        seeds = [b"protocol_treasury"],
        bump
    )]
    pub protocol_treasury: AccountInfo<'info>,
    /// CHECK: Creator profile reference
    pub creator: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(campaign_id: u64)]
pub struct RefundExpiredCampaign<'info> {
    #[account(
        mut,
        seeds = [b"campaign_escrow", creator_profile.key().as_ref(), campaign_id.to_le_bytes().as_ref()],
        bump = campaign_escrow.bump,
        has_one = sponsor
    )]
    pub campaign_escrow: Account<'info, CampaignEscrow>,
    pub creator_profile: Account<'info, CreatorProfile>,
    #[account(mut)]
    pub sponsor: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SubscribeTier<'info> {
    #[account(
        init_if_needed,
        payer = subscriber,
        space = 8 + SubscriptionReceipt::LEN,
        seeds = [b"subscription", creator_profile.key().as_ref(), subscriber.key().as_ref()],
        bump
    )]
    pub subscription_receipt: Account<'info, SubscriptionReceipt>,
    #[account(
        mut,
        seeds = [b"creator", creator_profile.authority.as_ref()],
        bump = creator_profile.bump,
        has_one = payout_wallet
    )]
    pub creator_profile: Account<'info, CreatorProfile>,
    /// CHECK: Creator verified payout wallet
    #[account(mut)]
    pub creator_payout_wallet: AccountInfo<'info>,
    /// CHECK: Protocol treasury
    #[account(
        mut,
        seeds = [b"protocol_treasury"],
        bump
    )]
    pub protocol_treasury: AccountInfo<'info>,
    #[account(mut)]
    pub subscriber: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// -----------------------------------------------------------------------------
// DATA STATE STRUCTS
// -----------------------------------------------------------------------------

#[account]
pub struct CreatorProfile {
    pub authority: Pubkey,          // 32
    pub payout_wallet: Pubkey,      // 32
    pub handle: String,             // 4 + 32 = 36
    pub metadata_uri: String,       // 4 + 96 = 100
    pub total_earned: u64,          // 8
    pub campaigns_count: u64,       // 8
    pub subscribers_count: u64,     // 8
    pub bump: u8,                   // 1
}

impl CreatorProfile {
    pub const LEN: usize = 32 + 32 + 36 + 100 + 8 + 8 + 8 + 1;
}

#[account]
pub struct CampaignEscrow {
    pub campaign_id: u64,           // 8
    pub creator: Pubkey,            // 32
    pub sponsor: Pubkey,            // 32
    pub campaign_name: String,      // 4 + 48 = 52
    pub total_funding_amount: u64,  // 8
    pub remaining_balance: u64,     // 8
    pub total_milestones: u8,       // 1
    pub completed_milestones: u8,   // 1
    pub deadline_timestamp: i64,    // 8
    pub is_cancelled: bool,         // 1
    pub is_fully_claimed: bool,     // 1
    pub bump: u8,                   // 1
}

impl CampaignEscrow {
    pub const LEN: usize = 8 + 32 + 32 + 52 + 8 + 8 + 1 + 1 + 8 + 1 + 1 + 1;
}

#[account]
pub struct SubscriptionReceipt {
    pub subscriber: Pubkey,         // 32
    pub creator: Pubkey,            // 32
    pub tier: u8,                   // 1
    pub started_at: i64,            // 8
    pub expires_at: i64,            // 8
    pub bump: u8,                   // 1
}

impl SubscriptionReceipt {
    pub const LEN: usize = 32 + 32 + 1 + 8 + 8 + 1;
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct CreatorInitialized {
    pub creator: Pubkey,
    pub handle: String,
    pub payout_wallet: Pubkey,
}

#[event]
pub struct CampaignCreated {
    pub campaign_id: u64,
    pub creator: Pubkey,
    pub sponsor: Pubkey,
    pub total_funding_amount: u64,
    pub total_milestones: u8,
    pub deadline_timestamp: i64,
}

#[event]
pub struct MilestoneClaimed {
    pub campaign_id: u64,
    pub milestone_index: u8,
    pub creator_payout: u64,
    pub protocol_fee: u64,
    pub proof_uri: String,
}

#[event]
pub struct CampaignRefunded {
    pub campaign_id: u64,
    pub sponsor: Pubkey,
    pub refund_amount: u64,
}

#[event]
pub struct FanSubscribed {
    pub subscriber: Pubkey,
    pub creator: Pubkey,
    pub tier: u8,
    pub expires_at: i64,
}

#[error_code]
pub enum CreatorError {
    #[msg("The creator handle length exceeds maximum 32 bytes.")]
    HandleLengthExceeded,
    #[msg("Metadata URI exceeds maximum 96 bytes.")]
    UriLengthExceeded,
    #[msg("Campaign name must be between 1 and 48 characters.")]
    InvalidCampaignName,
    #[msg("Milestone count must be between 1 and 10.")]
    InvalidMilestoneCount,
    #[msg("Funding amount must be greater than zero.")]
    ZeroFundingAmount,
    #[msg("Deadline timestamp must be in the future.")]
    InvalidDeadline,
    #[msg("The requested campaign has been cancelled.")]
    CampaignCancelled,
    #[msg("Campaign funds have already been claimed entirely.")]
    CampaignAlreadyCompleted,
    #[msg("All milestones have already been claimed.")]
    AllMilestonesCompleted,
    #[msg("Insufficient balance in the escrow PDA.")]
    InsufficientEscrowBalance,
    #[msg("Campaign deadline has not passed yet.")]
    CampaignNotExpired,
    #[msg("Zero escrow balance available to refund.")]
    ZeroEscrowBalance,
    #[msg("Invalid subscription tier level.")]
    InvalidSubscriptionTier,
    #[msg("Minimum subscription duration is 86,400 seconds (1 day).")]
    InvalidDuration,
    #[msg("Numerical overflow detected during math operation.")]
    NumericalOverflow,
    #[msg("Payout destination does not match Creator profile record.")]
    UnauthorizedPayoutDestination,
}