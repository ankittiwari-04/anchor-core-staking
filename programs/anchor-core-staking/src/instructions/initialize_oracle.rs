use anchor_lang::prelude::*;
use mpl_core::{
    ID as MPL_CORE_ID,
    accounts::BaseCollectionV1,
    instructions::AddCollectionExternalPluginAdapterV1CpiBuilder,
    types::{
        ExternalCheckResult, ExternalPluginAdapterInitInfo, HookableLifecycleEvent,
        OracleInitInfo, PluginAuthority, ValidationResultsOffset,
    },
};
use crate::error::ErrorCode;
use crate::state::*;

#[derive(Accounts)]
pub struct InitializeOracle<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        mut,
        has_one = update_authority @ ErrorCode::InvalidUpdateAuthority,
    )]
    pub collection: Account<'info, BaseCollectionV1>,
    /// CHECK: PDA used only as signer, address verified by seeds
    #[account(
        seeds = [b"update_authority", collection.key().as_ref()],
        bump,
    )]
    pub update_authority: UncheckedAccount<'info>,
    #[account(
        init,
        payer = admin,
        space = OracleState::SPACE,
        seeds = [b"oracle", collection.key().as_ref()],
        bump,
    )]
    pub oracle: Account<'info, OracleState>,
    /// CHECK: reward vault PDA (system account); only its bump is stored
    #[account(
        seeds = [b"vault", collection.key().as_ref()],
        bump,
    )]
    pub vault: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: MPL Core program
    #[account(address = MPL_CORE_ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<InitializeOracle>) -> Result<()> {
    ctx.accounts.oracle.set_inner(OracleState {
        tag: 1,                     // OracleValidation::V1 (variant 0 is Uninitialized)
        create: RESULT_PASS,
        transfer: RESULT_REJECTED,  // closed until the first crank says otherwise
        burn: RESULT_PASS,
        update: RESULT_PASS,
        bump: ctx.bumps.oracle,
        vault_bump: ctx.bumps.vault,
    });

    let collection_key = ctx.accounts.collection.key();
    let bump = [ctx.bumps.update_authority];
    let signer_seeds: &[&[u8]] = &[b"update_authority", collection_key.as_ref(), &bump];

    // Oracle adapter: only the Transfer lifecycle, with REJECT capability (flag = 4)
    AddCollectionExternalPluginAdapterV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
    .collection(&ctx.accounts.collection.to_account_info())
    .payer(&ctx.accounts.admin.to_account_info())
    .authority(Some(&ctx.accounts.update_authority.to_account_info()))
    .system_program(&ctx.accounts.system_program.to_account_info())
    .init_info(ExternalPluginAdapterInitInfo::Oracle(OracleInitInfo {
        base_address: ctx.accounts.oracle.key().to_bytes().into(),
        init_plugin_authority: Some(PluginAuthority::UpdateAuthority),
        lifecycle_checks: vec![(
            HookableLifecycleEvent::Transfer,
            ExternalCheckResult { flags: 4 },
        )],
        base_address_config: None,
        results_offset: Some(ValidationResultsOffset::Anchor),
    }))
    .invoke_signed(&[signer_seeds])?;

    Ok(())
}
