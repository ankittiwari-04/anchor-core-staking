use anchor_lang::prelude::*;

// ExternalValidationResult values as stored on-chain by mpl-core
pub const RESULT_APPROVED: u8 = 0;
pub const RESULT_REJECTED: u8 = 1;
pub const RESULT_PASS: u8 = 2;

/// Layout after the 8-byte Anchor discriminator matches mpl-core's
/// OracleValidation::V1 { create, transfer, burn, update } (borsh):
/// [tag=1 (V1), create, transfer, burn, update]
#[account]
pub struct OracleState {
    pub tag: u8,
    pub create: u8,
    pub transfer: u8,
    pub burn: u8,
    pub update: u8,
    pub bump: u8,
    pub vault_bump: u8,
}

impl OracleState {
    pub const SPACE: usize = 8 + 7;
}
