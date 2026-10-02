use anchor_lang::prelude::*;

#[constant]
pub const SEED: &str = "anchor";

// One-time burn-to-earn bonus, expressed as days' worth of rewards
pub const BURN_BONUS_DAYS: i64 = 365;
