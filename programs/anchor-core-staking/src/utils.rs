use anchor_lang::prelude::*;
use anchor_spl::token_interface::{mint_to_checked, MintToChecked};
use mpl_core::{
    accounts::{BaseAssetV1, BaseCollectionV1},
    fetch_plugin,
    instructions::UpdateCollectionPluginV1CpiBuilder,
    types::{Attribute, Attributes, Plugin, PluginType},
};
use crate::error::ErrorCode;

pub const SECONDS_PER_DAY: i64 = 86400;
pub const TOTAL_STAKED_KEY: &str = "total_staked";

pub struct StakeInfo {
    pub staked: bool,
    pub staked_at: i64,
    pub last_claimed_at: i64,
    pub others: Vec<Attribute>,
}

pub fn fetch_asset_attributes(asset: &AccountInfo) -> Option<Attributes> {
    fetch_plugin::<BaseAssetV1, Attributes>(asset, PluginType::Attributes)
        .ok()
        .map(|(_, attrs, _)| attrs)
}

pub fn read_stake_info(attrs: &Attributes) -> Result<StakeInfo> {
    let mut info = StakeInfo {
        staked: false,
        staked_at: 0,
        last_claimed_at: 0,
        others: Vec::new(),
    };
    for a in &attrs.attribute_list {
        match a.key.as_str() {
            "staked" => info.staked = a.value == "true",
            "staked_at" => {
                info.staked_at = a.value.parse::<i64>().map_err(|_| ErrorCode::InvalidTimestamp)?
            }
            "last_claimed_at" => {
                info.last_claimed_at = a.value.parse::<i64>().map_err(|_| ErrorCode::InvalidTimestamp)?
            }
            _ => info.others.push(a.clone()),
        }
    }
    // Assets staked before last_claimed_at existed fall back to staked_at
    if info.last_claimed_at == 0 {
        info.last_claimed_at = info.staked_at;
    }
    Ok(info)
}

pub fn build_stake_attributes(
    mut others: Vec<Attribute>,
    staked: bool,
    staked_at: i64,
    last_claimed_at: i64,
) -> Vec<Attribute> {
    others.push(Attribute { key: "staked".to_string(), value: staked.to_string() });
    others.push(Attribute { key: "staked_at".to_string(), value: staked_at.to_string() });
    others.push(Attribute { key: "last_claimed_at".to_string(), value: last_claimed_at.to_string() });
    others
}

pub fn rewards_amount(days: i64, rewards_bps: u16, decimals: u8) -> Result<u64> {
    let days = u64::try_from(days).map_err(|_| ErrorCode::InvalidTimestamp)?;
    let amount = days
        .checked_mul(rewards_bps as u64)
        .ok_or(ErrorCode::MathOverflow)?
        .checked_mul(10u64.pow(decimals as u32))
        .ok_or(ErrorCode::MathOverflow)?
        .checked_div(10000u64)
        .ok_or(ErrorCode::MathOverflow)?;
    Ok(amount)
}

pub fn mint_rewards<'info>(
    token_program: &AccountInfo<'info>,
    rewards_mint: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    config: &AccountInfo<'info>,
    collection_key: &Pubkey,
    config_bump: u8,
    amount: u64,
    decimals: u8,
) -> Result<()> {
    let bump = [config_bump];
    let seeds: &[&[u8]] = &[b"config", collection_key.as_ref(), &bump];
    mint_to_checked(
        CpiContext::new_with_signer(
            token_program.clone(),
            MintToChecked {
                mint: rewards_mint.clone(),
                to: to.clone(),
                authority: config.clone(),
            },
            &[seeds],
        ),
        amount,
        decimals,
    )
}

/// Increment or decrement the "total_staked" Attribute on the Collection
pub fn update_total_staked<'info>(
    collection: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    update_authority: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    mpl_core_program: &AccountInfo<'info>,
    signer_seeds: &[&[u8]],
    increment: bool,
) -> Result<()> {
    let (_, attrs, _) = fetch_plugin::<BaseCollectionV1, Attributes>(collection, PluginType::Attributes)
        .map_err(|_| error!(ErrorCode::CollectionAttributesMissing))?;

    let mut list: Vec<Attribute> = Vec::with_capacity(attrs.attribute_list.len() + 1);
    let mut total: u64 = 0;
    for a in &attrs.attribute_list {
        if a.key == TOTAL_STAKED_KEY {
            total = a.value.parse::<u64>().map_err(|_| ErrorCode::InvalidCounter)?;
        } else {
            list.push(a.clone());
        }
    }

    total = if increment {
        total.checked_add(1).ok_or(ErrorCode::MathOverflow)?
    } else {
        total.checked_sub(1).ok_or(ErrorCode::InvalidCounter)?
    };
    list.push(Attribute { key: TOTAL_STAKED_KEY.to_string(), value: total.to_string() });

    UpdateCollectionPluginV1CpiBuilder::new(mpl_core_program)
        .collection(collection)
        .payer(payer)
        .authority(Some(update_authority))
        .system_program(system_program)
        .plugin(Plugin::Attributes(Attributes { attribute_list: list }))
        .invoke_signed(&[signer_seeds])?;

    Ok(())
}
