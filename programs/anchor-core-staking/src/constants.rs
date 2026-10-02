use anchor_lang::prelude::*;

#[constant]
pub const SEED: &str = "anchor";

// One-time burn-to-earn bonus, expressed as days' worth of rewards
pub const BURN_BONUS_DAYS: i64 = 365;

// ---- Oracle (time-based transfer) ----
pub const OPEN_SECS: i64 = 9 * 3600;              // 09:00 UTC
pub const CLOSE_SECS: i64 = 17 * 3600;            // 17:00 UTC
pub const REWARD_WINDOW_SECS: i64 = 600;          // crank must land within 10 min after a boundary
pub const CRANK_REWARD_LAMPORTS: u64 = 5_000_000; // 0.005 SOL
