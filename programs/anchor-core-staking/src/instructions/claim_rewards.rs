use anchor_lang::prelude::*;
use anchor_spl::{associated_token::AssociatedToken, token_interface::{Mint, TokenAccount, TokenInterface}};
use mpl_core::{
    ID as MPL_CORE_ID,
    accounts::{BaseAssetV1, BaseCollectionV1},
    types::{UpdateAuthority, Attributes, Plugin},
    instructions::UpdatePluginV1CpiBuilder,
};
use crate::Config;
use crate::error::ErrorCode;
use crate::utils::*;

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        seeds = [b"config", collection.key().as_ref()],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        has_one = owner @ ErrorCode::InvalidOwner,
        constraint = asset.update_authority == UpdateAuthority::Collection(collection.key()) @ ErrorCode::InvalidUpdateAuthority,
    )]
    pub asset: Account<'info, BaseAssetV1>,
    #[account(
        mut,
        has_one = update_authority @ ErrorCode::InvalidUpdateAuthority
    )]
    pub collection: Account<'info, BaseCollectionV1>,
    /// CHECK: This account data is not used, we only verify the address
    #[account(
        seeds = [b"update_authority", collection.key().as_ref()],
        bump,
    )]
    pub update_authority: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"rewards_mint", config.key().as_ref()],
        bump = config.rewards_bump,
    )]
    pub rewards_mint: InterfaceAccount<'info, Mint>,
    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = rewards_mint,
        associated_token::authority = owner,
    )]
    pub user_rewards_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    /// CHECK: This is the MPL Core program
    #[account(address = Pubkey::from(MPL_CORE_ID.to_bytes()))]
    pub mpl_core_program: UncheckedAccount<'info>,
}
pub fn handler(ctx: Context<ClaimRewards>) -> Result<()> {
    let attrs = fetch_asset_attributes(&ctx.accounts.asset.to_account_info())
        .ok_or(ErrorCode::AssetNotStaked)?;
    let info = read_stake_info(&attrs)?;
    require!(info.staked, ErrorCode::AssetNotStaked);

    let now = Clock::get()?.unix_timestamp;
    let elapsed = now
        .checked_sub(info.last_claimed_at)
        .ok_or(ErrorCode::InvalidTimestamp)?;
    let days = elapsed / SECONDS_PER_DAY;
    require!(days > 0, ErrorCode::NothingToClaim);

    // Advance last_claimed_at by whole days only, so partial days are not lost
    let new_last_claimed = info
        .last_claimed_at
        .checked_add(days.checked_mul(SECONDS_PER_DAY).ok_or(ErrorCode::MathOverflow)?)
        .ok_or(ErrorCode::MathOverflow)?;

    let attributes_list = build_stake_attributes(info.others, true, info.staked_at, new_last_claimed);

    let collection_key = ctx.accounts.collection.key();
    let bump = [ctx.bumps.update_authority];
    let signer_seeds: &[&[u8]] = &[b"update_authority", collection_key.as_ref(), &bump];

    // Update attributes only: the NFT stays staked and frozen
    UpdatePluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
    .asset(&ctx.accounts.asset.to_account_info())
    .collection(Some(&ctx.accounts.collection.to_account_info()))
    .payer(&ctx.accounts.owner.to_account_info())
    .authority(Some(&ctx.accounts.update_authority.to_account_info()))
    .system_program(&ctx.accounts.system_program.to_account_info())
    .plugin(Plugin::Attributes(Attributes { attribute_list: attributes_list }))
    .invoke_signed(&[signer_seeds])?;

    let amount = rewards_amount(days, ctx.accounts.config.rewards_bps, ctx.accounts.rewards_mint.decimals)?;
    mint_rewards(
        &ctx.accounts.token_program.to_account_info(),
        &ctx.accounts.rewards_mint.to_account_info(),
        &ctx.accounts.user_rewards_ata.to_account_info(),
        &ctx.accounts.config.to_account_info(),
        &collection_key,
        ctx.accounts.config.bump,
        amount,
        ctx.accounts.rewards_mint.decimals,
    )?;

    Ok(())
}
