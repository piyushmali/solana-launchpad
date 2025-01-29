// programs/solana-launchpad/src/lib.rs
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, Transfer},
};

// Replace this with the program ID you got from the solana address command
declare_id!("AjUxmZYjhXbJq5yDDvxe8Hh2amWnAjLN2Wmf5oET8mZ1");

#[program]
pub mod solana_launchpad {
    use super::*;

    // Initialize the launchpad
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let launchpad = &mut ctx.accounts.launchpad;
        launchpad.admin = *ctx.accounts.admin.key;
        launchpad.total_projects = 0;
        Ok(())
    }

    // Register a new token sale
    pub fn register_token(
        ctx: Context<RegisterToken>,
        soft_cap: u64,
        hard_cap: u64,
        token_mint: Pubkey,
    ) -> Result<()> {
        let token_sale = &mut ctx.accounts.token_sale;
        token_sale.registrant = *ctx.accounts.registrant.key;
        token_sale.soft_cap = soft_cap;
        token_sale.hard_cap = hard_cap;
        token_sale.token_mint = token_mint;
        token_sale.total_raised = 0;
        token_sale.is_active = false;

        ctx.accounts.launchpad.total_projects += 1;
        Ok(())
    }

    // Add a new sale round
    pub fn add_sale_round(
        ctx: Context<AddSaleRound>,
        price_per_token: u64,
        tokens_available: u64,
        min_contribution: u64,
        max_contribution: u64,
        start_time: i64,
        end_time: i64,
    ) -> Result<()> {
        let sale_round = &mut ctx.accounts.sale_round;
        sale_round.price_per_token = price_per_token;
        sale_round.tokens_available = tokens_available;
        sale_round.tokens_sold = 0;
        sale_round.min_contribution = min_contribution;
        sale_round.max_contribution = max_contribution;
        sale_round.start_time = start_time;
        sale_round.end_time = end_time;
        sale_round.is_active = false;

        Ok(())
    }

    // Activate a sale round
    pub fn activate_sale_round(ctx: Context<ActivateSaleRound>) -> Result<()> {
        let sale_round = &mut ctx.accounts.sale_round;
        sale_round.is_active = true;
        Ok(())
    }

    // Purchase tokens
    pub fn purchase_tokens(ctx: Context<PurchaseTokens>, amount: u64) -> Result<()> {
        let sale_round = &mut ctx.accounts.sale_round;
        let token_sale = &mut ctx.accounts.token_sale;

        // Validate contribution
        require!(
            amount >= sale_round.min_contribution,
            LaunchpadError::ContributionTooLow
        );
        require!(
            amount <= sale_round.max_contribution,
            LaunchpadError::ContributionExceeded
        );
        require!(
            token_sale.total_raised + amount <= token_sale.hard_cap,
            LaunchpadError::HardCapReached
        );

        // Calculate tokens
        let tokens = amount
            .checked_mul(10u64.pow(9)) // Assuming 9 decimals
            .unwrap()
            .checked_div(sale_round.price_per_token)
            .unwrap();

        require!(
            tokens <= sale_round.tokens_available,
            LaunchpadError::InsufficientTokens
        );

        // Update state
        sale_round.tokens_available -= tokens;
        sale_round.tokens_sold += tokens;
        token_sale.total_raised += amount;

        // Transfer SOL to vault
        let cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.investor.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(cpi_context, amount)?;

        // Initialize vesting schedule
        let vesting = &mut ctx.accounts.vesting;
        vesting.investor = *ctx.accounts.investor.key;
        vesting.total_allocation = tokens;
        vesting.released = 0;
        vesting.start_time = Clock::get()?.unix_timestamp;
        vesting.duration = 30 * 86400; // 30 days in seconds

        Ok(())
    }

    // Claim vested tokens
    pub fn claim_tokens(ctx: Context<ClaimTokens>) -> Result<()> {
        let vesting = &mut ctx.accounts.vesting;

        let current_time = Clock::get()?.unix_timestamp;
        let elapsed = current_time - vesting.start_time;

        require!(elapsed >= 0, LaunchpadError::VestingNotStarted);

        let vested_amount = if elapsed >= vesting.duration as i64 {
            vesting.total_allocation - vesting.released
        } else {
            vesting.total_allocation * elapsed as u64 / vesting.duration
        };

        require!(vested_amount > 0, LaunchpadError::NothingToClaim);

        // Transfer tokens
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault_token_account.to_account_info(),
                to: ctx.accounts.investor_token_account.to_account_info(),
                authority: ctx.accounts.vault.to_account_info(),
            },
        );

        token::transfer(transfer_ctx, vested_amount)?;

        vesting.released += vested_amount;

        Ok(())
    }
}

// Accounts and Error handling
#[error_code]
pub enum LaunchpadError {
    #[msg("Contribution too low")]
    ContributionTooLow,
    #[msg("Contribution exceeded")]
    ContributionExceeded,
    #[msg("Hard cap reached")]
    HardCapReached,
    #[msg("Insufficient tokens")]
    InsufficientTokens,
    #[msg("Vesting not started")]
    VestingNotStarted,
    #[msg("Nothing to claim")]
    NothingToClaim,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = admin, space = 8 + 32 + 8)]
    pub launchpad: Account<'info, Launchpad>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RegisterToken<'info> {
    #[account(mut)]
    pub launchpad: Account<'info, Launchpad>,
    #[account(init, payer = registrant, space = 8 + 32 + 32 + 8 + 8 + 8 + 1)]
    pub token_sale: Account<'info, TokenSale>,
    #[account(mut)]
    pub registrant: Signer<'info>,
    pub token_mint: Account<'info, Mint>, // Changed from Token to Mint
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AddSaleRound<'info> {
    #[account(mut)]
    pub token_sale: Account<'info, TokenSale>,
    #[account(init, payer = registrant, space = 8 + 8 + 8 + 8 + 8 + 8 + 8 + 1)]
    pub sale_round: Account<'info, SaleRound>,
    #[account(mut)]
    pub registrant: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ActivateSaleRound<'info> {
    #[account(mut)]
    pub sale_round: Account<'info, SaleRound>,
    #[account(mut)]
    pub registrant: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(amount: u64)]
pub struct PurchaseTokens<'info> {
    #[account(mut)]
    pub sale_round: Account<'info, SaleRound>,
    #[account(mut)]
    pub token_sale: Account<'info, TokenSale>,
    #[account(mut)]
    pub investor: Signer<'info>,
    /// CHECK: Safe because this is just a native system account
    #[account(mut)]
    pub vault: UncheckedAccount<'info>,
    pub token_mint: Account<'info, Mint>,
    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = investor
    )]
    pub investor_token_account: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = investor,
        space = 8 + 32 + 8 + 8 + 8 + 8
    )]
    pub vesting: Account<'info, VestingSchedule>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct ClaimTokens<'info> {
    #[account(mut)]
    pub vesting: Account<'info, VestingSchedule>,
    #[account(mut)]
    pub token_sale: Account<'info, TokenSale>,
    #[account(mut)]
    pub investor: Signer<'info>,
    #[account(mut)]
    pub vault: SystemAccount<'info>, // Added vault account
    pub token_mint: Account<'info, Mint>, // Added token_mint account
    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = investor
    )]
    pub investor_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

// Data structures
#[account]
pub struct Launchpad {
    pub admin: Pubkey,
    pub total_projects: u64,
}

#[account]
pub struct TokenSale {
    pub registrant: Pubkey,
    pub token_mint: Pubkey,
    pub soft_cap: u64,
    pub hard_cap: u64,
    pub total_raised: u64,
    pub is_active: bool,
}

#[account]
pub struct SaleRound {
    pub price_per_token: u64,
    pub tokens_available: u64,
    pub tokens_sold: u64,
    pub min_contribution: u64,
    pub max_contribution: u64,
    pub start_time: i64,
    pub end_time: i64,
    pub is_active: bool,
}

#[account]
pub struct VestingSchedule {
    pub investor: Pubkey,
    pub total_allocation: u64,
    pub released: u64,
    pub start_time: i64,
    pub duration: u64,
}
