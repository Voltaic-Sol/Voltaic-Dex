use anchor_lang::prelude::*;
use anchor_lang::solana_program::program_option::COption;

use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token as spl_token;
use anchor_spl::token_2022 as spl_token_2022;
use anchor_spl::token_interface::{
    self, Burn, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

declare_id!("FCrSSe5yTaL8svSdnBxFoDexWBv1gbGcJrNCF7gtE3UT");

// =============================
// SEEDS + CONSTANTS
// =============================
pub const POOL_SEED: &[u8] = b"pool";
pub const AUTH_SEED: &[u8] = b"authority";
pub const WL_SEED: &[u8] = b"wl";

// permanently locked to prevent initial rounding attacks
pub const MIN_LP_LOCK: u64 = 1_000;

// =============================
// PROGRAM
// =============================
#[program]
pub mod dex {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        fee_bps: u16,
        whitelist_required: bool,
    ) -> Result<()> {
        require!(fee_bps <= 1000, DexError::InvalidFeeBps); // max 10%

        // Canonical ordering to prevent duplicate pools (A,B) vs (B,A)
        require!(
            ctx.accounts.mint_a.key() < ctx.accounts.mint_b.key(),
            DexError::InvalidMintOrder
        );

        // Only allow SPL Token (Tokenkeg) or Token-2022
        require_supported_token_program(&ctx.accounts.token_program_a.key())?;
        require_supported_token_program(&ctx.accounts.token_program_b.key())?;
        require_supported_token_program(&ctx.accounts.lp_token_program.key())?;

        // Each mint MUST be owned by its token program
        require_keys_eq!(
            *ctx.accounts.mint_a.to_account_info().owner,
            ctx.accounts.token_program_a.key(),
            DexError::WrongTokenProgram
        );
        require_keys_eq!(
            *ctx.accounts.mint_b.to_account_info().owner,
            ctx.accounts.token_program_b.key(),
            DexError::WrongTokenProgram
        );

        // LP mint must be owned by lp_token_program
        require_keys_eq!(
            *ctx.accounts.lp_mint.to_account_info().owner,
            ctx.accounts.lp_token_program.key(),
            DexError::WrongTokenProgram
        );

        // IMPORTANT: lp_mint is init'd in account constraints; reload before reading fields
        ctx.accounts.lp_mint.reload()?;

        // Defense-in-depth: ensure LP mint authority is pool authority
        require!(
            ctx.accounts.lp_mint.mint_authority == COption::Some(ctx.accounts.pool_authority.key()),
            DexError::InvalidLpAuthority
        );
        require!(
            ctx.accounts.lp_mint.freeze_authority == COption::Some(ctx.accounts.pool_authority.key()),
            DexError::InvalidLpAuthority
        );

        let pool = &mut ctx.accounts.pool;
        pool.admin = ctx.accounts.payer.key();
        pool.authority = ctx.accounts.pool_authority.key();

        pool.mint_a = ctx.accounts.mint_a.key();
        pool.mint_b = ctx.accounts.mint_b.key();

        pool.vault_a = ctx.accounts.vault_a.key();
        pool.vault_b = ctx.accounts.vault_b.key();

        pool.token_program_a = ctx.accounts.token_program_a.key();
        pool.token_program_b = ctx.accounts.token_program_b.key();

        pool.lp_mint = ctx.accounts.lp_mint.key();
        pool.lp_token_program = ctx.accounts.lp_token_program.key();

        pool.fee_bps = fee_bps;
        pool.whitelist_required = whitelist_required;

        pool.paused_swaps = false;
        pool.paused_liquidity = false;

        pool.bump_pool = ctx.bumps.pool;
        pool.bump_authority = ctx.bumps.pool_authority;

        Ok(())
    }

    pub fn whitelist_add(ctx: Context<WhitelistAdd>) -> Result<()> {
        only_admin(&ctx.accounts.pool, &ctx.accounts.authority)?;
        let wl = &mut ctx.accounts.whitelist;
        wl.pool = ctx.accounts.pool.key();
        wl.wallet = ctx.accounts.wallet.key();
        wl.allowed = true;
        Ok(())
    }

    pub fn whitelist_remove(_ctx: Context<WhitelistRemove>) -> Result<()> {
        // close = authority handles rent reclaim
        Ok(())
    }

    pub fn set_whitelist_required(ctx: Context<SetWhitelistRequired>, required: bool) -> Result<()> {
        only_admin(&ctx.accounts.pool, &ctx.accounts.authority)?;
        ctx.accounts.pool.whitelist_required = required;
        Ok(())
    }

    pub fn set_paused(
        ctx: Context<SetPaused>,
        paused_swaps: bool,
        paused_liquidity: bool,
    ) -> Result<()> {
        only_admin(&ctx.accounts.pool, &ctx.accounts.authority)?;
        ctx.accounts.pool.paused_swaps = paused_swaps;
        ctx.accounts.pool.paused_liquidity = paused_liquidity;
        Ok(())
    }

    /// Add liquidity and receive LP tokens.
    /// Uses actual vault deltas (Token-2022 fee/hook safe).
    pub fn add_liquidity(
        ctx: Context<AddLiquidity>,
        amount_a: u64,
        amount_b: u64,
        min_lp_out: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.pool.paused_liquidity, DexError::Paused);
        require!(amount_a > 0 && amount_b > 0, DexError::InvalidAmount);

        enforce_pool_programs(
            &ctx.accounts.pool,
            &ctx.accounts.token_program_a,
            &ctx.accounts.token_program_b,
            &ctx.accounts.lp_token_program,
        )?;

        enforce_pool_invariants(
            &ctx.accounts.pool,
            &ctx.accounts.pool_authority,
            &ctx.accounts.mint_a,
            &ctx.accounts.mint_b,
            &ctx.accounts.vault_a,
            &ctx.accounts.vault_b,
            &ctx.accounts.lp_mint,
        )?;

        // Optional whitelist PDA (validated manually if required)
        validate_whitelist_if_required(
            &ctx.accounts.pool,
            &ctx.accounts.user.key(),
            &ctx.accounts.whitelist,
            ctx.program_id,
        )?;

        // Snapshot reserves BEFORE transfers
        let reserve_a_before = ctx.accounts.vault_a.amount as u128;
        let reserve_b_before = ctx.accounts.vault_b.amount as u128;
        let total_lp_before = ctx.accounts.lp_mint.supply as u128;

        // Transfer user -> vault A
        transfer_checked_any(
            &ctx.accounts.token_program_a,
            &ctx.accounts.user_ata_a,
            &ctx.accounts.vault_a,
            &ctx.accounts.user,
            &ctx.accounts.mint_a,
            amount_a,
        )?;

        // Transfer user -> vault B
        transfer_checked_any(
            &ctx.accounts.token_program_b,
            &ctx.accounts.user_ata_b,
            &ctx.accounts.vault_b,
            &ctx.accounts.user,
            &ctx.accounts.mint_b,
            amount_b,
        )?;

        // Reload vaults to see CPI-updated balances
        ctx.accounts.vault_a.reload()?;
        ctx.accounts.vault_b.reload()?;

        let reserve_a_after = ctx.accounts.vault_a.amount as u128;
        let reserve_b_after = ctx.accounts.vault_b.amount as u128;

        require!(reserve_a_after >= reserve_a_before, DexError::InvariantViolation);
        require!(reserve_b_after >= reserve_b_before, DexError::InvariantViolation);

        let actual_a_in_u128 = reserve_a_after - reserve_a_before;
        let actual_b_in_u128 = reserve_b_after - reserve_b_before;

        require!(actual_a_in_u128 > 0 && actual_b_in_u128 > 0, DexError::InvalidAmount);
        require!(actual_a_in_u128 <= u64::MAX as u128, DexError::MathOverflow);
        require!(actual_b_in_u128 <= u64::MAX as u128, DexError::MathOverflow);

        let actual_a_in = actual_a_in_u128 as u64;
        let actual_b_in = actual_b_in_u128 as u64;

        // Compute lp_out using ACTUAL inputs + pre-reserves
        let lp_out: u64 = if total_lp_before == 0 {
            let prod = (actual_a_in as u128)
                .checked_mul(actual_b_in as u128)
                .ok_or(DexError::MathOverflow)?;

            let root = integer_sqrt(prod)?;
            require!(
                root > (MIN_LP_LOCK as u128),
                DexError::InsufficientInitialLiquidity
            );

            let minted_to_user = root
                .checked_sub(MIN_LP_LOCK as u128)
                .ok_or(DexError::MathOverflow)?;

            require!(minted_to_user <= u64::MAX as u128, DexError::MathOverflow);
            minted_to_user as u64
        } else {
            require!(reserve_a_before > 0 && reserve_b_before > 0, DexError::EmptyPool);

            let lp_from_a = (actual_a_in as u128)
                .checked_mul(total_lp_before)
                .ok_or(DexError::MathOverflow)?
                .checked_div(reserve_a_before)
                .ok_or(DexError::MathOverflow)?;

            let lp_from_b = (actual_b_in as u128)
                .checked_mul(total_lp_before)
                .ok_or(DexError::MathOverflow)?
                .checked_div(reserve_b_before)
                .ok_or(DexError::MathOverflow)?;

            let out = lp_from_a.min(lp_from_b);
            require!(out <= u64::MAX as u128, DexError::MathOverflow);
            out as u64
        };

        require!(lp_out >= min_lp_out, DexError::SlippageExceeded);

        // PDA signer seeds
        let pool_key = ctx.accounts.pool.key();
        let signer_seeds: &[&[u8]] = &[
            AUTH_SEED,
            pool_key.as_ref(),
            &[ctx.accounts.pool.bump_authority],
        ];

        // Lock MIN_LP_LOCK on first mint
        if total_lp_before == 0 {
            mint_to_any_signed(
                &ctx.accounts.lp_token_program,
                &ctx.accounts.lp_mint,
                &ctx.accounts.lp_vault,
                &ctx.accounts.pool_authority,
                MIN_LP_LOCK,
                signer_seeds,
            )?;
        }

        // Mint LP to user
        mint_to_any_signed(
            &ctx.accounts.lp_token_program,
            &ctx.accounts.lp_mint,
            &ctx.accounts.user_lp_ata,
            &ctx.accounts.pool_authority,
            lp_out,
            signer_seeds,
        )?;

        Ok(())
    }

    /// Remove liquidity by burning LP and withdrawing proportional reserves.
    pub fn remove_liquidity(
        ctx: Context<RemoveLiquidity>,
        lp_amount: u64,
        min_out_a: u64,
        min_out_b: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.pool.paused_liquidity, DexError::Paused);
        require!(lp_amount > 0, DexError::InvalidAmount);

        enforce_pool_programs(
            &ctx.accounts.pool,
            &ctx.accounts.token_program_a,
            &ctx.accounts.token_program_b,
            &ctx.accounts.lp_token_program,
        )?;

        enforce_pool_invariants(
            &ctx.accounts.pool,
            &ctx.accounts.pool_authority,
            &ctx.accounts.mint_a,
            &ctx.accounts.mint_b,
            &ctx.accounts.vault_a,
            &ctx.accounts.vault_b,
            &ctx.accounts.lp_mint,
        )?;

        validate_whitelist_if_required(
            &ctx.accounts.pool,
            &ctx.accounts.user.key(),
            &ctx.accounts.whitelist,
            ctx.program_id,
        )?;

        let reserve_a = ctx.accounts.vault_a.amount as u128;
        let reserve_b = ctx.accounts.vault_b.amount as u128;
        require!(reserve_a > 0 && reserve_b > 0, DexError::EmptyPool);

        let total_lp = ctx.accounts.lp_mint.supply as u128;
        require!(total_lp > 0, DexError::EmptyPool);

        let out_a_u128 = reserve_a
            .checked_mul(lp_amount as u128)
            .ok_or(DexError::MathOverflow)?
            .checked_div(total_lp)
            .ok_or(DexError::MathOverflow)?;

        let out_b_u128 = reserve_b
            .checked_mul(lp_amount as u128)
            .ok_or(DexError::MathOverflow)?
            .checked_div(total_lp)
            .ok_or(DexError::MathOverflow)?;

        require!(out_a_u128 <= u64::MAX as u128, DexError::MathOverflow);
        require!(out_b_u128 <= u64::MAX as u128, DexError::MathOverflow);

        let out_a = out_a_u128 as u64;
        let out_b = out_b_u128 as u64;

        require!(
            out_a >= min_out_a && out_b >= min_out_b,
            DexError::SlippageExceeded
        );

        // Burn LP from user
        burn_any(
            &ctx.accounts.lp_token_program,
            &ctx.accounts.user_lp_ata,
            &ctx.accounts.lp_mint,
            &ctx.accounts.user,
            lp_amount,
        )?;

        // PDA signer seeds
        let pool_key = ctx.accounts.pool.key();
        let signer_seeds: &[&[u8]] = &[
            AUTH_SEED,
            pool_key.as_ref(),
            &[ctx.accounts.pool.bump_authority],
        ];

        // Transfer vault -> user
        transfer_checked_any_signed(
            &ctx.accounts.token_program_a,
            &ctx.accounts.vault_a,
            &ctx.accounts.user_ata_a,
            &ctx.accounts.pool_authority,
            &ctx.accounts.mint_a,
            out_a,
            signer_seeds,
        )?;

        transfer_checked_any_signed(
            &ctx.accounts.token_program_b,
            &ctx.accounts.vault_b,
            &ctx.accounts.user_ata_b,
            &ctx.accounts.pool_authority,
            &ctx.accounts.mint_b,
            out_b,
            signer_seeds,
        )?;

        Ok(())
    }

    /// Swap exact input; uses actual vault delta for input (Token-2022 fee/hook safe).
    pub fn swap_exact_in(
        ctx: Context<SwapExactIn>,
        amount_in: u64,
        min_out: u64,
        a_to_b: bool,
    ) -> Result<()> {
        require!(!ctx.accounts.pool.paused_swaps, DexError::Paused);
        require!(amount_in > 0, DexError::InvalidAmount);

        enforce_pool_programs(
            &ctx.accounts.pool,
            &ctx.accounts.token_program_a,
            &ctx.accounts.token_program_b,
            &ctx.accounts.lp_token_program,
        )?;

        enforce_pool_invariants(
            &ctx.accounts.pool,
            &ctx.accounts.pool_authority,
            &ctx.accounts.mint_a,
            &ctx.accounts.mint_b,
            &ctx.accounts.vault_a,
            &ctx.accounts.vault_b,
            &ctx.accounts.lp_mint,
        )?;

        validate_whitelist_if_required(
            &ctx.accounts.pool,
            &ctx.accounts.user.key(),
            &ctx.accounts.whitelist,
            ctx.program_id,
        )?;

        // Snapshot reserves BEFORE transfer in
        let reserve_a_before = ctx.accounts.vault_a.amount as u128;
        let reserve_b_before = ctx.accounts.vault_b.amount as u128;
        require!(reserve_a_before > 0 && reserve_b_before > 0, DexError::EmptyPool);

        // Transfer in
        if a_to_b {
            transfer_checked_any(
                &ctx.accounts.token_program_a,
                &ctx.accounts.user_ata_a,
                &ctx.accounts.vault_a,
                &ctx.accounts.user,
                &ctx.accounts.mint_a,
                amount_in,
            )?;
        } else {
            transfer_checked_any(
                &ctx.accounts.token_program_b,
                &ctx.accounts.user_ata_b,
                &ctx.accounts.vault_b,
                &ctx.accounts.user,
                &ctx.accounts.mint_b,
                amount_in,
            )?;
        }

        // Reload vaults to see CPI-updated balances
        ctx.accounts.vault_a.reload()?;
        ctx.accounts.vault_b.reload()?;

        // ACTUAL input received
        let reserve_a_after = ctx.accounts.vault_a.amount as u128;
        let reserve_b_after = ctx.accounts.vault_b.amount as u128;

        let actual_in_u128 = if a_to_b {
            require!(reserve_a_after >= reserve_a_before, DexError::InvariantViolation);
            reserve_a_after - reserve_a_before
        } else {
            require!(reserve_b_after >= reserve_b_before, DexError::InvariantViolation);
            reserve_b_after - reserve_b_before
        };

        require!(actual_in_u128 > 0, DexError::InvalidAmount);
        require!(actual_in_u128 <= u64::MAX as u128, DexError::MathOverflow);
        let actual_in = actual_in_u128 as u64;

        // Apply pool fee to ACTUAL input
        let fee_bps = ctx.accounts.pool.fee_bps as u128;
        let actual_in_after_fee_u128 =
            (actual_in as u128) * (10_000u128 - fee_bps) / 10_000u128;

        // Constant product using pre-transfer reserves
        let (reserve_in, reserve_out) = if a_to_b {
            (reserve_a_before, reserve_b_before)
        } else {
            (reserve_b_before, reserve_a_before)
        };

        let numerator = reserve_out
            .checked_mul(actual_in_after_fee_u128)
            .ok_or(DexError::MathOverflow)?;
        let denominator = reserve_in
            .checked_add(actual_in_after_fee_u128)
            .ok_or(DexError::MathOverflow)?;
        let amount_out_u128 = numerator / denominator;

        require!(amount_out_u128 <= u64::MAX as u128, DexError::MathOverflow);
        let amount_out = amount_out_u128 as u64;

        require!(amount_out > 0, DexError::InvalidAmount);
        require!(amount_out >= min_out, DexError::SlippageExceeded);

        // PDA signer seeds
        let pool_key = ctx.accounts.pool.key();
        let signer_seeds: &[&[u8]] = &[
            AUTH_SEED,
            pool_key.as_ref(),
            &[ctx.accounts.pool.bump_authority],
        ];

        // Transfer out
        if a_to_b {
            transfer_checked_any_signed(
                &ctx.accounts.token_program_b,
                &ctx.accounts.vault_b,
                &ctx.accounts.user_ata_b,
                &ctx.accounts.pool_authority,
                &ctx.accounts.mint_b,
                amount_out,
                signer_seeds,
            )?;
        } else {
            transfer_checked_any_signed(
                &ctx.accounts.token_program_a,
                &ctx.accounts.vault_a,
                &ctx.accounts.user_ata_a,
                &ctx.accounts.pool_authority,
                &ctx.accounts.mint_a,
                amount_out,
                signer_seeds,
            )?;
        }

        Ok(())
    }

    pub fn emergency_drain(
        ctx: Context<EmergencyDrain>,
        amount: u64,
        drain_a: bool,
    ) -> Result<()> {
        only_admin(&ctx.accounts.pool, &ctx.accounts.authority)?;
        require!(amount > 0, DexError::InvalidAmount);

        require_supported_token_program(&ctx.accounts.token_program_a.key())?;
        require_supported_token_program(&ctx.accounts.token_program_b.key())?;

        let pool_key = ctx.accounts.pool.key();
        let signer_seeds: &[&[u8]] = &[
            AUTH_SEED,
            pool_key.as_ref(),
            &[ctx.accounts.pool.bump_authority],
        ];

        if drain_a {
            transfer_checked_any_signed(
                &ctx.accounts.token_program_a,
                &ctx.accounts.vault_a,
                &ctx.accounts.dest_ata_a,
                &ctx.accounts.pool_authority,
                &ctx.accounts.mint_a,
                amount,
                signer_seeds,
            )?;
        } else {
            transfer_checked_any_signed(
                &ctx.accounts.token_program_b,
                &ctx.accounts.vault_b,
                &ctx.accounts.dest_ata_b,
                &ctx.accounts.pool_authority,
                &ctx.accounts.mint_b,
                amount,
                signer_seeds,
            )?;
        }

        Ok(())
    }
}

// =============================
// ACCOUNTS
// =============================

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Pool state PDA
    #[account(
        init,
        payer = payer,
        space = 8 + Pool::SIZE,
        seeds = [POOL_SEED, mint_a.key().as_ref(), mint_b.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA authority; signer via seeds
    #[account(
        seeds = [AUTH_SEED, pool.key().as_ref()],
        bump
    )]
    pub pool_authority: UncheckedAccount<'info>,

    // Hybrid mints
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    // Vault ATAs owned by pool_authority
    #[account(
        init,
        payer = payer,
        associated_token::mint = mint_a,
        associated_token::authority = pool_authority,
        associated_token::token_program = token_program_a
    )]
    pub vault_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint_b,
        associated_token::authority = pool_authority,
        associated_token::token_program = token_program_b
    )]
    pub vault_b: Box<InterfaceAccount<'info, TokenAccount>>,

    /// LP mint (Token-2022 recommended, but Tokenkeg is allowed)
    #[account(
        init,
        payer = payer,
        mint::decimals = 9,
        mint::authority = pool_authority,
        mint::freeze_authority = pool_authority,
        mint::token_program = lp_token_program
    )]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    /// LP vault ATA owned by pool_authority to lock MIN_LP_LOCK
    #[account(
        init,
        payer = payer,
        associated_token::mint = lp_mint,
        associated_token::authority = pool_authority,
        associated_token::token_program = lp_token_program
    )]
    pub lp_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    // Token programs
    pub token_program_a: Interface<'info, TokenInterface>,
    pub token_program_b: Interface<'info, TokenInterface>,
    pub lp_token_program: Interface<'info, TokenInterface>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    // Include rent to avoid rare runtime access violations during init on some setups
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct WhitelistAdd<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub pool: Account<'info, Pool>,

    /// CHECK: wallet to allow
    pub wallet: UncheckedAccount<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + WhitelistEntry::SIZE,
        seeds = [WL_SEED, pool.key().as_ref(), wallet.key().as_ref()],
        bump
    )]
    pub whitelist: Account<'info, WhitelistEntry>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WhitelistRemove<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub pool: Account<'info, Pool>,

    /// CHECK: wallet being removed
    pub wallet: UncheckedAccount<'info>,

    #[account(
        mut,
        close = authority,
        seeds = [WL_SEED, pool.key().as_ref(), wallet.key().as_ref()],
        bump
    )]
    pub whitelist: Account<'info, WhitelistEntry>,
}

#[derive(Accounts)]
pub struct SetWhitelistRequired<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub pool: Account<'info, Pool>,
}

#[derive(Accounts)]
pub struct SetPaused<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub pool: Account<'info, Pool>,
}

#[derive(Accounts)]
pub struct AddLiquidity<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        has_one = mint_a @ DexError::InvalidMint,
        has_one = mint_b @ DexError::InvalidMint,
        has_one = vault_a @ DexError::InvalidVault,
        has_one = vault_b @ DexError::InvalidVault,
        has_one = lp_mint @ DexError::InvalidLpMint,
        constraint = pool.authority == pool_authority.key() @ DexError::InvalidAuthority
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA authority signer
    pub pool_authority: UncheckedAccount<'info>,

    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub vault_a: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub vault_b: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut)]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub lp_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = user,
        associated_token::token_program = token_program_a
    )]
    pub user_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_b,
        associated_token::authority = user,
        associated_token::token_program = token_program_b
    )]
    pub user_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Must exist (create ATA in TS)
    #[account(
        mut,
        associated_token::mint = lp_mint,
        associated_token::authority = user,
        associated_token::token_program = lp_token_program
    )]
    pub user_lp_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: optional whitelist PDA; validated in handler if required
    pub whitelist: Option<UncheckedAccount<'info>>,

    pub token_program_a: Interface<'info, TokenInterface>,
    pub token_program_b: Interface<'info, TokenInterface>,
    pub lp_token_program: Interface<'info, TokenInterface>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RemoveLiquidity<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        has_one = mint_a @ DexError::InvalidMint,
        has_one = mint_b @ DexError::InvalidMint,
        has_one = vault_a @ DexError::InvalidVault,
        has_one = vault_b @ DexError::InvalidVault,
        has_one = lp_mint @ DexError::InvalidLpMint,
        constraint = pool.authority == pool_authority.key() @ DexError::InvalidAuthority
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA authority signer
    pub pool_authority: UncheckedAccount<'info>,

    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub vault_a: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub vault_b: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut)]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = lp_mint,
        associated_token::authority = user,
        associated_token::token_program = lp_token_program
    )]
    pub user_lp_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = user,
        associated_token::token_program = token_program_a
    )]
    pub user_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_b,
        associated_token::authority = user,
        associated_token::token_program = token_program_b
    )]
    pub user_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: optional whitelist PDA; validated in handler if required
    pub whitelist: Option<UncheckedAccount<'info>>,

    pub token_program_a: Interface<'info, TokenInterface>,
    pub token_program_b: Interface<'info, TokenInterface>,
    pub lp_token_program: Interface<'info, TokenInterface>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SwapExactIn<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        has_one = mint_a @ DexError::InvalidMint,
        has_one = mint_b @ DexError::InvalidMint,
        has_one = vault_a @ DexError::InvalidVault,
        has_one = vault_b @ DexError::InvalidVault,
        has_one = lp_mint @ DexError::InvalidLpMint,
        constraint = pool.authority == pool_authority.key() @ DexError::InvalidAuthority
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA authority signer
    pub pool_authority: UncheckedAccount<'info>,

    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub vault_a: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub vault_b: Box<InterfaceAccount<'info, TokenAccount>>,

    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = user,
        associated_token::token_program = token_program_a
    )]
    pub user_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_b,
        associated_token::authority = user,
        associated_token::token_program = token_program_b
    )]
    pub user_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: optional whitelist PDA; validated in handler if required
    pub whitelist: Option<UncheckedAccount<'info>>,

    pub token_program_a: Interface<'info, TokenInterface>,
    pub token_program_b: Interface<'info, TokenInterface>,
    pub lp_token_program: Interface<'info, TokenInterface>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct EmergencyDrain<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = mint_a @ DexError::InvalidMint,
        has_one = mint_b @ DexError::InvalidMint,
        has_one = vault_a @ DexError::InvalidVault,
        has_one = vault_b @ DexError::InvalidVault,
        constraint = pool.authority == pool_authority.key() @ DexError::InvalidAuthority
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: PDA authority signer
    pub pool_authority: UncheckedAccount<'info>,

    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub vault_a: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub vault_b: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut)]
    pub dest_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub dest_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program_a: Interface<'info, TokenInterface>,
    pub token_program_b: Interface<'info, TokenInterface>,
}

// =============================
// STATE
// =============================
#[account]
pub struct Pool {
    pub admin: Pubkey,
    pub authority: Pubkey,

    pub mint_a: Pubkey,
    pub mint_b: Pubkey,

    pub vault_a: Pubkey,
    pub vault_b: Pubkey,

    pub token_program_a: Pubkey,
    pub token_program_b: Pubkey,

    pub lp_mint: Pubkey,
    pub lp_token_program: Pubkey,

    pub fee_bps: u16,

    pub whitelist_required: bool,
    pub paused_swaps: bool,
    pub paused_liquidity: bool,

    pub bump_pool: u8,
    pub bump_authority: u8,
}

impl Pool {
    pub const SIZE: usize =
        32 + // admin
        32 + // authority
        32 + 32 + // mint_a, mint_b
        32 + 32 + // vault_a, vault_b
        32 + 32 + // token_program_a, token_program_b
        32 + 32 + // lp_mint, lp_token_program
        2 + // fee_bps
        1 + 1 + 1 + // whitelist_required, paused_swaps, paused_liquidity
        1 + 1; // bumps
}

#[account]
pub struct WhitelistEntry {
    pub pool: Pubkey,
    pub wallet: Pubkey,
    pub allowed: bool,
}

impl WhitelistEntry {
    pub const SIZE: usize = 32 + 32 + 1;
}

// =============================
// HELPERS
// =============================

fn is_supported_token_program(program_id: &Pubkey) -> bool {
    *program_id == spl_token::ID || *program_id == spl_token_2022::ID
}

fn require_supported_token_program(program_id: &Pubkey) -> Result<()> {
    require!(is_supported_token_program(program_id), DexError::WrongTokenProgram);
    Ok(())
}

fn only_admin(pool: &Account<Pool>, authority: &Signer) -> Result<()> {
    require_keys_eq!(pool.admin, authority.key(), DexError::Unauthorized);
    Ok(())
}

fn enforce_pool_programs(
    pool: &Account<Pool>,
    token_program_a: &Interface<TokenInterface>,
    token_program_b: &Interface<TokenInterface>,
    lp_token_program: &Interface<TokenInterface>,
) -> Result<()> {
    require_supported_token_program(&token_program_a.key())?;
    require_supported_token_program(&token_program_b.key())?;
    require_supported_token_program(&lp_token_program.key())?;

    require_keys_eq!(pool.token_program_a, token_program_a.key(), DexError::WrongTokenProgram);
    require_keys_eq!(pool.token_program_b, token_program_b.key(), DexError::WrongTokenProgram);
    require_keys_eq!(pool.lp_token_program, lp_token_program.key(), DexError::WrongTokenProgram);
    Ok(())
}

/// Prevent account substitution / wrong vault owner / wrong mint, etc.
fn enforce_pool_invariants<'info>(
    pool: &Account<'info, Pool>,
    pool_authority: &UncheckedAccount<'info>,
    mint_a: &InterfaceAccount<'info, Mint>,
    mint_b: &InterfaceAccount<'info, Mint>,
    vault_a: &InterfaceAccount<'info, TokenAccount>,
    vault_b: &InterfaceAccount<'info, TokenAccount>,
    lp_mint: &InterfaceAccount<'info, Mint>,
) -> Result<()> {
    require_keys_eq!(pool.mint_a, mint_a.key(), DexError::InvalidMint);
    require_keys_eq!(pool.mint_b, mint_b.key(), DexError::InvalidMint);

    require_keys_eq!(pool.vault_a, vault_a.key(), DexError::InvalidVault);
    require_keys_eq!(pool.vault_b, vault_b.key(), DexError::InvalidVault);

    require_keys_eq!(vault_a.mint, mint_a.key(), DexError::InvalidVault);
    require_keys_eq!(vault_b.mint, mint_b.key(), DexError::InvalidVault);

    require_keys_eq!(vault_a.owner, pool_authority.key(), DexError::InvalidVault);
    require_keys_eq!(vault_b.owner, pool_authority.key(), DexError::InvalidVault);

    require_keys_eq!(pool.lp_mint, lp_mint.key(), DexError::InvalidLpMint);

    Ok(())
}

fn validate_whitelist_if_required<'info>(
    pool: &Account<'info, Pool>,
    user: &Pubkey,
    wl_opt: &Option<UncheckedAccount<'info>>,
    program_id: &Pubkey,
) -> Result<()> {
    if !pool.whitelist_required {
        return Ok(());
    }

    let wl_acc = wl_opt.as_ref().ok_or(DexError::NotWhitelisted)?;

    let (expected, _bump) = Pubkey::find_program_address(
        &[WL_SEED, pool.key().as_ref(), user.as_ref()],
        program_id,
    );

    require_keys_eq!(wl_acc.key(), expected, DexError::NotWhitelisted);

    // ✅ FIX: deserialize directly from account data (no Account<..> wrapper, no lifetime issues)
    let wl_info = wl_acc.to_account_info();
    let data = wl_info.try_borrow_data()?;
    let mut data_slice: &[u8] = &data;

    // This checks discriminator too (since WhitelistEntry is #[account])
    let wl = WhitelistEntry::try_deserialize(&mut data_slice)?;

    require_keys_eq!(wl.pool, pool.key(), DexError::NotWhitelisted);
    require_keys_eq!(wl.wallet, *user, DexError::NotWhitelisted);
    require!(wl.allowed, DexError::NotWhitelisted);

    Ok(())
}


fn integer_sqrt(x: u128) -> Result<u128> {
    if x == 0 {
        return Ok(0);
    }
    let mut z = x;
    let mut y = (z + 1) / 2;
    while y < z {
        z = y;
        y = (x / y + y) / 2;
    }
    Ok(z)
}

fn transfer_checked_any<'info>(
    token_program: &Interface<'info, TokenInterface>,
    from: &InterfaceAccount<'info, TokenAccount>,
    to: &InterfaceAccount<'info, TokenAccount>,
    authority: &Signer<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    amount: u64,
) -> Result<()> {
    let cpi_accounts = TransferChecked {
        from: from.to_account_info(),
        mint: mint.to_account_info(),
        to: to.to_account_info(),
        authority: authority.to_account_info(),
    };
    let cpi_ctx = CpiContext::new(token_program.to_account_info(), cpi_accounts);
    token_interface::transfer_checked(cpi_ctx, amount, mint.decimals)
}

fn transfer_checked_any_signed<'info>(
    token_program: &Interface<'info, TokenInterface>,
    from: &InterfaceAccount<'info, TokenAccount>,
    to: &InterfaceAccount<'info, TokenAccount>,
    authority_pda: &UncheckedAccount<'info>,
    mint: &InterfaceAccount<'info, Mint>,
    amount: u64,
    signer_seeds: &[&[u8]],
) -> Result<()> {
    let cpi_accounts = TransferChecked {
        from: from.to_account_info(),
        mint: mint.to_account_info(),
        to: to.to_account_info(),
        authority: authority_pda.to_account_info(),
    };

    let signer_seeds_arr: [&[&[u8]]; 1] = [signer_seeds];
    let cpi_ctx = CpiContext::new_with_signer(
        token_program.to_account_info(),
        cpi_accounts,
        &signer_seeds_arr,
    );

    token_interface::transfer_checked(cpi_ctx, amount, mint.decimals)
}

fn mint_to_any_signed<'info>(
    token_program: &Interface<'info, TokenInterface>,
    mint: &InterfaceAccount<'info, Mint>,
    to: &InterfaceAccount<'info, TokenAccount>,
    authority_pda: &UncheckedAccount<'info>,
    amount: u64,
    signer_seeds: &[&[u8]],
) -> Result<()> {
    let cpi_accounts = MintTo {
        mint: mint.to_account_info(),
        to: to.to_account_info(),
        authority: authority_pda.to_account_info(),
    };
    let signer_seeds_arr: [&[&[u8]]; 1] = [signer_seeds];
    let cpi_ctx = CpiContext::new_with_signer(
        token_program.to_account_info(),
        cpi_accounts,
        &signer_seeds_arr,
    );
    token_interface::mint_to(cpi_ctx, amount)
}

fn burn_any<'info>(
    token_program: &Interface<'info, TokenInterface>,
    from: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    authority: &Signer<'info>,
    amount: u64,
) -> Result<()> {
    let cpi_accounts = Burn {
        mint: mint.to_account_info(),
        from: from.to_account_info(),
        authority: authority.to_account_info(),
    };
    let cpi_ctx = CpiContext::new(token_program.to_account_info(), cpi_accounts);
    token_interface::burn(cpi_ctx, amount)
}

// =============================
// ERRORS
// =============================
#[error_code]
pub enum DexError {
    #[msg("Invalid fee bps")]
    InvalidFeeBps,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Slippage exceeded")]
    SlippageExceeded,
    #[msg("Pool is empty")]
    EmptyPool,
    #[msg("Wrong token program")]
    WrongTokenProgram,
    #[msg("Invalid mint")]
    InvalidMint,
    #[msg("Invalid vault")]
    InvalidVault,
    #[msg("Invalid authority")]
    InvalidAuthority,
    #[msg("Invalid LP mint")]
    InvalidLpMint,
    #[msg("Invalid LP mint authority")]
    InvalidLpAuthority,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Not whitelisted")]
    NotWhitelisted,
    #[msg("Paused")]
    Paused,
    #[msg("Initial liquidity too small")]
    InsufficientInitialLiquidity,
    #[msg("Invalid mint order (mint_a must be < mint_b)")]
    InvalidMintOrder,
    #[msg("Invariant violation")]
    InvariantViolation,
}
