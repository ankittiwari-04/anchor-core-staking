import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { AnchorCoreStaking } from "../target/types/anchor_core_staking";
import { SystemProgram, PublicKey, SYSVAR_CLOCK_PUBKEY, Keypair } from "@solana/web3.js";
import { MPL_CORE_PROGRAM_ID } from "@metaplex-foundation/mpl-core";
import assert from "assert";

const DAY = 86400;
const OPEN = 9 * 3600;
const CLOSE = 17 * 3600;

describe("oracle: time-based transfer", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.anchorCoreStaking as Program<AnchorCoreStaking>;

  const collectionKp = Keypair.generate();
  const nft1 = Keypair.generate();
  const nft2 = Keypair.generate();
  const cranker = Keypair.generate();
  const receiver = Keypair.generate();

  const [updateAuthority] = PublicKey.findProgramAddressSync(
    [Buffer.from("update_authority"), collectionKp.publicKey.toBuffer()], program.programId);
  const [oracle] = PublicKey.findProgramAddressSync(
    [Buffer.from("oracle"), collectionKp.publicKey.toBuffer()], program.programId);
  const [vault] = PublicKey.findProgramAddressSync(
    [Buffer.from("vault"), collectionKp.publicKey.toBuffer()], program.programId);

  async function chainNow(): Promise<number> {
    const info = await provider.connection.getAccountInfo(SYSVAR_CLOCK_PUBKEY);
    return Number(info!.data.readBigInt64LE(32));
  }

  async function travelTo(unixSeconds: number) {
    const res = await fetch(provider.connection.rpcEndpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0", id: 1, method: "surfnet_timeTravel",
        params: [{ absoluteTimestamp: unixSeconds * 1000 }],
      }),
    });
    const result = (await res.json()) as { error?: any };
    if (result.error) throw new Error(`Time travel failed: ${JSON.stringify(result.error)}`);
    await new Promise((r) => setTimeout(r, 1000));
  }

  // next occurrence of a given second-of-day (UTC) strictly after "now"
  function nextAt(now: number, secOfDay: number): number {
    let t = Math.floor(now / DAY) * DAY + secOfDay;
    if (t <= now) t += DAY;
    return t;
  }

  async function oracleTransferState(): Promise<number> {
    const info = await provider.connection.getAccountInfo(oracle);
    return info!.data[8 + 2]; // disc(8) + tag + create + transfer
  }

  async function assetOwner(asset: PublicKey): Promise<string> {
    const info = await provider.connection.getAccountInfo(asset);
    return new PublicKey(info!.data.subarray(1, 33)).toBase58();
  }

  const mintNft = (kp: Keypair, name: string) =>
    program.methods.mintAsset(name, "https://example.com/nft").accountsPartial({
      user: provider.wallet.publicKey,
      asset: kp.publicKey,
      collection: collectionKp.publicKey,
      updateAuthority,
      systemProgram: SystemProgram.programId,
      mplCoreProgram: MPL_CORE_PROGRAM_ID,
    }).signers([kp]).rpc();

  const transferNft = (asset: PublicKey) =>
    program.methods.transferNft().accountsPartial({
      owner: provider.wallet.publicKey,
      asset,
      collection: collectionKp.publicKey,
      oracle,
      newOwner: receiver.publicKey,
      systemProgram: SystemProgram.programId,
      mplCoreProgram: MPL_CORE_PROGRAM_ID,
    }).rpc();

  const crank = () =>
    program.methods.updateOracle().accountsPartial({
      caller: cranker.publicKey,
      collection: collectionKp.publicKey,
      oracle,
      vault,
      systemProgram: SystemProgram.programId,
    }).signers([cranker]).rpc();

  it("Setup: collection, NFTs, oracle, funded vault", async () => {
    await program.methods.createCollection("Oracle Collection", "https://example.com/c")
      .accountsPartial({
        payer: provider.wallet.publicKey,
        collection: collectionKp.publicKey,
        updateAuthority,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      }).signers([collectionKp]).rpc();

    await mintNft(nft1, "NFT 1");
    await mintNft(nft2, "NFT 2");

    await program.methods.initializeOracle().accountsPartial({
      admin: provider.wallet.publicKey,
      collection: collectionKp.publicKey,
      updateAuthority,
      oracle,
      vault,
      systemProgram: SystemProgram.programId,
      mplCoreProgram: MPL_CORE_PROGRAM_ID,
    }).rpc();

    // fund the reward vault and the cranker (for fees)
    const fundTx = new anchor.web3.Transaction().add(
      SystemProgram.transfer({ fromPubkey: provider.wallet.publicKey, toPubkey: vault, lamports: 50_000_000 }),
      SystemProgram.transfer({ fromPubkey: provider.wallet.publicKey, toPubkey: cranker.publicKey, lamports: 10_000_000 }),
    );
    await provider.sendAndConfirm(fundTx);
    assert.strictEqual(await oracleTransferState(), 1); // Rejected
  });

  it("Transfer is blocked before the oracle is opened", async () => {
    await assert.rejects(() => transferNft(nft1.publicKey));
  });

  it("Crank right after 09:00 UTC -> Approved and caller is rewarded", async () => {
    await travelTo(nextAt(await chainNow(), OPEN + 30));
    const before = await provider.connection.getBalance(cranker.publicKey);
    await crank();
    const after = await provider.connection.getBalance(cranker.publicKey);
    assert.strictEqual(await oracleTransferState(), 0); // Approved
    assert.ok(after > before, "cranker should be net positive after reward");
    console.log("Reward (lamports, net of fee):", after - before);
  });

  it("Cranking again does nothing (no reward farming)", async () => {
    await assert.rejects(() => crank());
  });

  it("Transfer succeeds during open hours", async () => {
    await transferNft(nft1.publicKey);
    assert.strictEqual(await assetOwner(nft1.publicKey), receiver.publicKey.toBase58());
  });

  it("Late crank after 17:00 UTC (outside window) -> Rejected, no reward", async () => {
    await travelTo(nextAt(await chainNow(), CLOSE + 30 * 60));
    const before = await provider.connection.getBalance(cranker.publicKey);
    await crank();
    const after = await provider.connection.getBalance(cranker.publicKey);
    assert.strictEqual(await oracleTransferState(), 1); // Rejected
    assert.ok(after <= before, "no reward when cranked late");
  });

  it("Transfer is blocked outside open hours", async () => {
    await assert.rejects(() => transferNft(nft2.publicKey));
    assert.strictEqual(await assetOwner(nft2.publicKey), provider.wallet.publicKey.toBase58());
  });
});
