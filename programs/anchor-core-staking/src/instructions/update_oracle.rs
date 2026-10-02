use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use mpl_core::accounts::BaseCollectionV1;
use crate::constants::*;
use crate::error::ErrorCode;
use crate::state::*;
use crate::utils::SECONDS_PER_DAY;

#[derive(Accounts)]
pub struct UpdateOracle<'info> {
    /// Anyone can crank
    #[account(mut)]
    pub caller: Signer<'info>,
    pub collection: Account<'info, BaseCollectionV1>,
    #[account(
        mut,
        seeds = [b"oracle", collection.key().as_ref()],
        bump = oracle.bump,
    )]
    pub oracle: Account<'info, OracleState>,
    /// CHECK: reward vault PDA (system account)
    #[account(
        mut,
        seeds = [b"vault", collection.key().as_ref()],
        bump = oracle.vault_bump,
    )]
    pub vault: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<UpdateOracle>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let secs = now.rem_euclid(SECONDS_PER_DAY);
    let is_open = secs >= OPEN_SECS && secs < CLOSE_SECS;
    let desired = if is_open { RESULT_APPROVED } else { RESULT_REJECTED };

    // Nothing to do (also prevents farming rewards with repeated calls)
    require!(ctx.accounts.oracle.transfer != desired, ErrorCode::OracleUpToDate);
    ctx.accounts.oracle.transfer = desired;

    // Reward only if the state flipped shortly after the open/close boundary
    let near_boundary = if is_open {
        secs - OPEN_SECS <= REWARD_WINDOW_SECS
    } else {
        secs >= CLOSE_SECS && secs - CLOSE_SECS <= REWARD_WINDOW_SECS
    };

    if near_boundary {
        let rent_min = Rent::get()?.minimum_balance(0);
        let needed = CRANK_REWARD_LAMPORTS
            .checked_add(rent_min)
            .ok_or(ErrorCode::MathOverflow)?;
        if ctx.accounts.vault.lamports() >= needed {
            let collection_key = ctx.accounts.collection.key();
            let bump = [ctx.accounts.oracle.vault_bump];
            let seeds: &[&[u8]] = &[b"vault", collection_key.as_ref(), &bump];
            transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.system_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.vault.to_account_info(),
                        to: ctx.accounts.caller.to_account_info(),
                    },
                    &[seeds],
                ),
                CRANK_REWARD_LAMPORTS,
            )?;
        }
    }

    Ok(())
}
