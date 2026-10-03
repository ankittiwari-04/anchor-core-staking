# NFT Staking Core (Anchor + Metaplex Core)

An NFT staking program built on the `anchor-core-staking` template. Staked Metaplex Core assets are frozen and earn SPL reward tokens over time. This version adds claiming without unstaking, burn-to-earn, collection-level stats, and an Oracle plugin that restricts transfers to certain hours of the day.

- Anchor 0.31.1, `mpl-core` 0.11
- Program ID: `6PMpnJgDwFuANLYJsjgqekhY7ortcdE27nzGA8TcRMrS`

## Instructions

| Instruction | What it does |
|---|---|
| `create_collection` | Creates the Core collection (update authority = program PDA) with a `total_staked = 0` Attribute |
| `mint_asset` | Mints an NFT into the collection |
| `initialize` | Creates the config (`rewards_bps`, `freeze_period` in days) and the reward mint |
| `stake` | Freezes the NFT, records `staked`, `staked_at`, `last_claimed_at` Attributes, adds a BurnDelegate, increments `total_staked` |
| `unstake` | After the freeze period: thaws, resets Attributes, decrements `total_staked`, mints unclaimed rewards |
| `claim_rewards` | Mints accrued rewards without unstaking; the NFT stays staked and frozen |
| `burn_staked_nft` | Burns the staked NFT via the BurnDelegate and mints accrued rewards plus a one-time bonus |
| `initialize_oracle` | Creates the Oracle PDA and reward vault, adds the Oracle adapter to the collection |
| `update_oracle` | Permissionless crank: sets Transfer to Approved or Rejected based on the on-chain clock |
| `transfer_nft` | Transfers an NFT through mpl-core with the Oracle passed as a remaining account |

## Task 1: Core plugins

### Claim rewards without unstaking
Rewards are `whole days × rewards_bps × 10^decimals / 10000`. A separate `last_claimed_at` Attribute tracks claims, so `staked_at` is never changed and the freeze-period check in `unstake` still works. `last_claimed_at` advances by whole days only, so partial days are not lost. Claiming twice in a row fails with `NothingToClaim`.

### Burn-to-earn with BurnDelegate
`stake` adds a BurnDelegate plugin whose authority is the update-authority PDA. `burn_staked_nft` thaws the asset, decrements the collection counter, burns the asset with `BurnV1` signed by the PDA, then mints accrued rewards plus a bonus of `BURN_BONUS_DAYS` (365) days of rewards (`constants.rs`). The freeze period is intentionally not enforced, since burning is permanent.

### Collection-level stats
The collection has an Attributes plugin with a `total_staked` key. A shared helper (`utils::update_total_staked`) reads the current value, rewrites the full attribute list, and updates it on stake (+1), unstake (-1) and burn (-1).

## Task 2: Oracle plugin (time-based transfer)

NFTs can be transferred only between 09:00 and 17:00 UTC.

- **Oracle account**: PDA `["oracle", collection]`. Layout after the Anchor discriminator: `[tag, create, transfer, burn, update]`, where `tag = 1` (`OracleValidation::V1`; variant 0 is `Uninitialized`) and each result is `0 = Approved`, `1 = Rejected`, `2 = Pass`. It starts with Transfer = Rejected and everything else = Pass.
- **Adapter**: added to the collection with a lifecycle check on `Transfer` only, with the REJECT capability.
- **`update_oracle`**: anyone can call it. It reads `Clock`, sets Transfer to Approved inside the window and Rejected outside, and fails with `OracleUpToDate` if nothing would change, so repeat calls can't farm rewards.
- **Crank reward**: 0.005 SOL from the vault PDA `["vault", collection]`, paid only when the state flips within 10 minutes after the 09:00 or 17:00 boundary (`REWARD_WINDOW_SECS`) and the vault holds enough lamports. Late cranks still update the state but earn nothing.
- **`transfer_nft`**: calls `TransferV1` with the Oracle account as a remaining account, so mpl-core runs the Oracle check.

## PDAs

| PDA | Seeds |
|---|---|
| `update_authority` | `["update_authority", collection]` |
| `config` | `["config", collection]` |
| `rewards_mint` | `["rewards_mint", config]` |
| `oracle` | `["oracle", collection]` |
| `vault` | `["vault", collection]` |

## Build

The Solana toolchain's default Rust is too old for some recent crates (`edition2024`), so `Cargo.lock` pins `blake3 1.5.5` and `rmp-serde 1.3.0`, and the program is built with a newer platform-tools version:

```bash
yarn install
cargo build-sbf --tools-version v1.52
anchor idl build -o target/idl/anchor_core_staking.json -t target/types/anchor_core_staking.ts
```

(Plain `anchor build` may fail on older Solana toolchains. The `mpl-core` stack-offset warning during the build is expected.)

If you rebuild with a fresh keypair, run `anchor keys sync` and rebuild so `declare_id!`, `Anchor.toml` and the IDL agree.

## Test

Tests use [Surfpool](https://docs.surfpool.run) because they need time travel (`surfnet_timeTravel`).

```bash
# terminal 1
surfpool start        # wait for "Runbook 'deployment' execution completed"

# terminal 2
export ANCHOR_PROVIDER_URL=http://localhost:8899
export ANCHOR_WALLET=~/.config/solana/id.json
yarn run ts-mocha -p ./tsconfig.json -t 1000000 tests/**/*.ts
```

Restart Surfpool after rebuilding so it loads the new program.

`tests/anchor-core-staking.ts` (11 tests): `total_staked` 0 → 2 → 1 → 0, freeze period, claim without unstaking, double-claim rejection, burn bonus, unstake.

`tests/oracle.ts` (7 tests): transfer blocked while Rejected, crank at 09:00 UTC approves and pays the caller, repeat crank rejected, transfer succeeds in-hours, late crank after 17:00 UTC rejects with no reward, transfer blocked again.

## Known limitations

- Re-staking an NFT after unstaking fails, because `stake` adds the FreezeDelegate and BurnDelegate plugins again (carried over from the template).
- The crank reward vault must be funded manually (a plain SOL transfer to the vault PDA).
- The reward formula is linear in whole days; the burn bonus is a flat number of days of rewards.

## Test results

All 18 tests pass (11 for Task 1 in `tests/anchor-core-staking.ts`, 7 for the Oracle in `tests/oracle.ts`).

![All 18 tests passing]<img width="1247" height="761" alt="Screenshot 2026-10-03 041320" src="https://github.com/user-attachments/assets/06d3f3a4-a344-4bb5-9431-4e02b403c1ee" />


Task 1 on its own (claim without unstaking, burn-to-earn, `total_staked` counter):

![Task 1 tests passing]<img width="1900" height="821" alt="Screenshot 2026-10-03 040438" src="https://github.com/user-attachments/assets/c38fcd0d-ff58-4bd4-9f7a-d0e902458798" />

