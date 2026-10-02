use anchor_lang::prelude::*;
use mpl_core::{
    ID as MPL_CORE_ID,
    accounts::{BaseAssetV1, BaseCollectionV1},
    instructions::{AddPluginV1CpiBuilder, UpdatePluginV1CpiBuilder},
    types::{UpdateAuthority, Attributes, Plugin, PluginAuthority, FreezeDelegate, BurnDelegate},
};
use crate::state::Config;
use crate::error::ErrorCode;
use crate::utils::*;

#[derive(Accounts)]
pub struct Stake<'info> {
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
        has_one = update_authority @ ErrorCode::InvalidUpdateAuthority,
    )]
    pub collection: Account<'info, BaseCollectionV1>,
    /// CHECK: This account is not initialized and is being used for signing purposes only, we verify that derives from the correct seeds
    #[account(
        seeds = [b"update_authority", collection.key().as_ref()],
        bump,
    )]
    pub update_authority: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: This is the ID of the MPL Core Program
    #[account(address = MPL_CORE_ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
}
pub fn handler(ctx: Context<Stake>) -> Result<()> {
    let attributes_fetched = fetch_asset_attributes(&ctx.accounts.asset.to_account_info());
    let now = Clock::get()?.unix_timestamp;

    // Keep non-staking attributes, and make sure the asset is not already staked
    let mut others = Vec::new();
    if let Some(attrs) = &attributes_fetched {
        let info = read_stake_info(attrs)?;
        require!(!info.staked, ErrorCode::AlreadyStaked);
        others = info.others;
    }
    let attributes_list = build_stake_attributes(others, true, now, now);

    let collection_key = ctx.accounts.collection.key();
    let bump = [ctx.bumps.update_authority];
    let signer_seeds: &[&[u8]] = &[b"update_authority", collection_key.as_ref(), &bump];

    // Attributes plugin is authority-managed (update authority PDA signs)
    if attributes_fetched.is_none() {
        AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
        .asset(&ctx.accounts.asset.to_account_info())
        .collection(Some(&ctx.accounts.collection.to_account_info()))
        .payer(&ctx.accounts.owner.to_account_info())
        .authority(Some(&ctx.accounts.update_authority.to_account_info()))
        .system_program(&ctx.accounts.system_program.to_account_info())
        .plugin(Plugin::Attributes(Attributes { attribute_list: attributes_list }))
        .init_authority(PluginAuthority::UpdateAuthority)
        .invoke_signed(&[signer_seeds])?;
    } else {
        UpdatePluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
        .asset(&ctx.accounts.asset.to_account_info())
        .collection(Some(&ctx.accounts.collection.to_account_info()))
        .payer(&ctx.accounts.owner.to_account_info())
        .authority(Some(&ctx.accounts.update_authority.to_account_info()))
        .system_program(&ctx.accounts.system_program.to_account_info())
        .plugin(Plugin::Attributes(Attributes { attribute_list: attributes_list }))
        .invoke_signed(&[signer_seeds])?;
    }

    // BurnDelegate (owner-managed, owner signs) with the PDA as delegate authority
    AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
    .asset(&ctx.accounts.asset.to_account_info())
    .collection(Some(&ctx.accounts.collection.to_account_info()))
    .payer(&ctx.accounts.owner.to_account_info())
    .authority(Some(&ctx.accounts.owner.to_account_info()))
    .system_program(&ctx.accounts.system_program.to_account_info())
    .plugin(Plugin::BurnDelegate(BurnDelegate {}))
    .init_authority(PluginAuthority::UpdateAuthority)
    .invoke()?;

    // Freeze the asset (owner-managed, owner signs)
    AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
    .asset(&ctx.accounts.asset.to_account_info())
    .collection(Some(&ctx.accounts.collection.to_account_info()))
    .payer(&ctx.accounts.owner.to_account_info())
    .authority(Some(&ctx.accounts.owner.to_account_info()))
    .system_program(&ctx.accounts.system_program.to_account_info())
    .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: true }))
    .init_authority(PluginAuthority::UpdateAuthority)
    .invoke()?;

    // Collection stats: total_staked += 1
    update_total_staked(
        &ctx.accounts.collection.to_account_info(),
        &ctx.accounts.owner.to_account_info(),
        &ctx.accounts.update_authority.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.mpl_core_program.to_account_info(),
        signer_seeds,
        true,
    )?;

    Ok(())
}
