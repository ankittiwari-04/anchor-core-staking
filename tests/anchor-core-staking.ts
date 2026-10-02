import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { AnchorCoreStaking } from "../target/types/anchor_core_staking";
import { SystemProgram, PublicKey } from "@solana/web3.js";
import { MPL_CORE_PROGRAM_ID } from "@metaplex-foundation/mpl-core";
import { ASSOCIATED_TOKEN_PROGRAM_ID, getAssociatedTokenAddressSync, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import assert from "assert";

const MILLISECONDS_PER_DAY = 86400000;
const REWARDS_BPS = 10000; // 1 token per day (6 decimals)
const FREEZE_PERIOD_IN_DAYS = 7;
const TIME_TRAVEL_IN_DAYS = 8;
const BURN_BONUS_DAYS = 365;
const ONE_TOKEN = 1_000_000n;

describe("anchor-core-staking", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.anchorCoreStaking as Program<AnchorCoreStaking>;

  const collectionKeypair = anchor.web3.Keypair.generate();
  const nftClaim = anchor.web3.Keypair.generate(); // claimed, then unstaked
  const nftBurn = anchor.web3.Keypair.generate();  // burned

  const updateAuthority = PublicKey.findProgramAddressSync(
    [Buffer.from("update_authority"), collectionKeypair.publicKey.toBuffer()],
    program.programId
  )[0];
  const config = PublicKey.findProgramAddressSync(
    [Buffer.from("config"), collectionKeypair.publicKey.toBuffer()],
    program.programId
  )[0];
  const rewardsMint = PublicKey.findProgramAddressSync(
    [Buffer.from("rewards_mint"), config.toBuffer()],
    program.programId
  )[0];
  const userRewardsAta = getAssociatedTokenAddressSync(
    rewardsMint, provider.wallet.publicKey, false, TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID
  );

  // ---------- helpers ----------
  async function advanceTime(params: { absoluteTimestamp: number }): Promise<void> {
    const res = await fetch(provider.connection.rpcEndpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "surfnet_timeTravel", params: [params] }),
    });
    const result = (await res.json()) as { error?: any };
    if (result.error) throw new Error(`Time travel failed: ${JSON.stringify(result.error)}`);
    await new Promise((r) => setTimeout(r, 1000));
  }

  // Reads a borsh-encoded (String key, String value) attribute from raw account data
  async function readAttr(account: PublicKey, key: string): Promise<string | null> {
    const info = await provider.connection.getAccountInfo(account);
    if (!info) return null;
    const k = Buffer.from(key);
    const len = Buffer.alloc(4);
    len.writeUInt32LE(k.length);
    const needle = Buffer.concat([len, k]);
    const idx = info.data.indexOf(needle);
    if (idx < 0) return null;
    let off = idx + needle.length;
    const vlen = info.data.readUInt32LE(off);
    off += 4;
    return info.data.subarray(off, off + vlen).toString();
  }

  async function balance(): Promise<bigint> {
    try {
      return BigInt((await provider.connection.getTokenAccountBalance(userRewardsAta)).value.amount);
    } catch {
      return 0n;
    }
  }

  const rewardAccounts = (asset: PublicKey) => ({
    owner: provider.wallet.publicKey,
    updateAuthority,
    config,
    rewardsMint,
    userRewardsAta,
    asset,
    collection: collectionKeypair.publicKey,
    mplCoreProgram: MPL_CORE_PROGRAM_ID,
    systemProgram: SystemProgram.programId,
    tokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
  });

  const stakeNft = (asset: PublicKey) =>
    program.methods.stake().accountsPartial({
      owner: provider.wallet.publicKey,
      updateAuthority,
      config,
      asset,
      collection: collectionKeypair.publicKey,
      systemProgram: SystemProgram.programId,
      mplCoreProgram: MPL_CORE_PROGRAM_ID,
    }).rpc();

  const mintNft = (kp: anchor.web3.Keypair, name: string) =>
    program.methods.mintAsset(name, "https://example.com/nft").accountsPartial({
      user: provider.wallet.publicKey,
      asset: kp.publicKey,
      collection: collectionKeypair.publicKey,
      updateAuthority,
      systemProgram: SystemProgram.programId,
      mplCoreProgram: MPL_CORE_PROGRAM_ID,
    }).signers([kp]).rpc();

  // ---------- setup ----------
  it("Create a collection (total_staked starts at 0)", async () => {
    await program.methods.createCollection("Test Collection", "https://example.com/collection")
      .accountsPartial({
        payer: provider.wallet.publicKey,
        collection: collectionKeypair.publicKey,
        updateAuthority,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      })
      .signers([collectionKeypair])
      .rpc();
    assert.strictEqual(await readAttr(collectionKeypair.publicKey, "total_staked"), "0");
  });

  it("Mint two NFTs", async () => {
    await mintNft(nftClaim, "NFT Claim");
    await mintNft(nftBurn, "NFT Burn");
  });

  it("Initialize Config", async () => {
    await program.methods.initialize(REWARDS_BPS, FREEZE_PERIOD_IN_DAYS)
      .accountsPartial({
        admin: provider.wallet.publicKey,
        collection: collectionKeypair.publicKey,
        updateAuthority,
        config,
        rewardsMint,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();
  });

  // ---------- Task 1.3: total_staked ----------
  it("Stake both NFTs -> total_staked = 2", async () => {
    await stakeNft(nftClaim.publicKey);
    assert.strictEqual(await readAttr(collectionKeypair.publicKey, "total_staked"), "1");
    await stakeNft(nftBurn.publicKey);
    assert.strictEqual(await readAttr(collectionKeypair.publicKey, "total_staked"), "2");
    assert.strictEqual(await readAttr(nftClaim.publicKey, "staked"), "true");
  });

  it("Unstake before the freeze period fails", async () => {
    try {
      await program.methods.unstake().accountsPartial(rewardAccounts(nftClaim.publicKey)).rpc();
      assert.fail("unstake should have failed");
    } catch (err) {
      if (err instanceof anchor.AnchorError) {
        assert.strictEqual(err.error.errorCode.code, "FreezePeriodNotElapsed");
      } else {
        throw err;
      }
    }
  });

  it("Claim before a full day has passed fails", async () => {
    try {
      await program.methods.claimRewards().accountsPartial(rewardAccounts(nftClaim.publicKey)).rpc();
      assert.fail("claim should have failed");
    } catch (err) {
      if (err instanceof anchor.AnchorError) {
        assert.strictEqual(err.error.errorCode.code, "NothingToClaim");
      } else {
        throw err;
      }
    }
  });

  it("Time travel to the future", async () => {
    await advanceTime({ absoluteTimestamp: Date.now() + TIME_TRAVEL_IN_DAYS * MILLISECONDS_PER_DAY });
  });

  // ---------- Task 1.1: claim_rewards ----------
  it("Claim rewards without unstaking", async () => {
    const before = await balance();
    await program.methods.claimRewards().accountsPartial(rewardAccounts(nftClaim.publicKey)).rpc();
    const after = await balance();
    console.log("Claimed (tokens):", Number(after - before) / 1e6);
    assert.ok(after - before >= BigInt(TIME_TRAVEL_IN_DAYS) * ONE_TOKEN, "expected ~8 tokens");
    // Still staked and still counted
    assert.strictEqual(await readAttr(nftClaim.publicKey, "staked"), "true");
    assert.strictEqual(await readAttr(collectionKeypair.publicKey, "total_staked"), "2");
  });

  it("Claiming again right away fails (no double claim)", async () => {
    try {
      await program.methods.claimRewards().accountsPartial(rewardAccounts(nftClaim.publicKey)).rpc();
      assert.fail("second claim should have failed");
    } catch (err) {
      if (err instanceof anchor.AnchorError) {
        assert.strictEqual(err.error.errorCode.code, "NothingToClaim");
      } else {
        throw err;
      }
    }
  });

  // ---------- Task 1.2: burn_staked_nft ----------
  it("Burn a staked NFT for the bonus", async () => {
    const before = await balance();
    await program.methods.burnStakedNft().accountsPartial(rewardAccounts(nftBurn.publicKey)).rpc();
    const after = await balance();
    console.log("Burn reward (tokens):", Number(after - before) / 1e6);
    assert.ok(
      after - before >= BigInt(TIME_TRAVEL_IN_DAYS + BURN_BONUS_DAYS) * ONE_TOKEN,
      "expected accrued rewards + burn bonus"
    );
    // The asset is gone (Core leaves at most a 1-byte stub)
    const info = await provider.connection.getAccountInfo(nftBurn.publicKey);
    assert.ok(info === null || info.data.length <= 1, "asset should be burned");
    // Counter decremented
    assert.strictEqual(await readAttr(collectionKeypair.publicKey, "total_staked"), "1");
  });

  it("Unstake the remaining NFT -> total_staked = 0", async () => {
    await program.methods.unstake().accountsPartial(rewardAccounts(nftClaim.publicKey)).rpc();
    assert.strictEqual(await readAttr(nftClaim.publicKey, "staked"), "false");
    assert.strictEqual(await readAttr(collectionKeypair.publicKey, "total_staked"), "0");
  });
});
