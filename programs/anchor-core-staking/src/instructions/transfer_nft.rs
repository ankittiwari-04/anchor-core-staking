use anchor_lang::prelude::*;
use mpl_core::{
    ID as MPL_CORE_ID,
    accounts::{BaseAssetV1, BaseCollectionV1},
    instructions::TransferV1CpiBuilder,
    types::UpdateAuthority,
};
use crate::error::ErrorCode;
use crate::state::*;

#[derive(Accounts)]
pub struct TransferNft<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        has_one = owner @ ErrorCode::InvalidOwner,
        constraint = asset.update_authority == UpdateAuthority::Collection(collection.key()) @ ErrorCode::InvalidUpdateAuthority,
    )]
    pub asset: Account<'info, BaseAssetV1>,
    #[account(mut)]
    pub collection: Account<'info, BaseCollectionV1>,
    #[account(
        seeds = [b"oracle", collection.key().as_ref()],
        bump = oracle.bump,
    )]
    pub oracle: Account<'info, OracleState>,
    /// CHECK: any account can receive the NFT
    pub new_owner: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: MPL Core program
    #[account(address = MPL_CORE_ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<TransferNft>) -> Result<()> {
    // The Oracle account is passed as a remaining account so mpl-core can validate the Transfer
    let oracle_info = ctx.accounts.oracle.to_account_info();

    TransferV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
    .asset(&ctx.accounts.asset.to_account_info())
    .collection(Some(&ctx.accounts.collection.to_account_info()))
    .payer(&ctx.accounts.owner.to_account_info())
    .authority(Some(&ctx.accounts.owner.to_account_info()))
    .new_owner(&ctx.accounts.new_owner.to_account_info())
    .system_program(Some(&ctx.accounts.system_program.to_account_info()))
    .add_remaining_account(&oracle_info, false, false)
    .invoke()?;

    Ok(())
}
