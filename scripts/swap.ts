import * as anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  getOrCreateAssociatedTokenAccount,
} from "@solana/spl-token";

(async () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const wallet = provider.wallet as anchor.Wallet;
  const program = anchor.workspace.Dex as anchor.Program;

  const tokenMintA = new PublicKey("76kAyT418V9Dhr6S8u8QghEY51YP5JviovyfwHBJbkZo");
  const tokenMintB = new PublicKey("2Fskpz6AYXcNowXiQN6VT64RL9A6kQV8114ViorxVhz5");
  const pool = new PublicKey("46KJRrAZvQ65jgNVD4Qs3UqwBpFG1DcsSeuaLvvR4Hzb");
  const vaultA = new PublicKey("BKPCU7GNzHJYpXzUhy2jSot2aVrwPxqt9gNnpsJdXNtF");
  const vaultB = new PublicKey("AeCjM4QDuYesBCPE7v8f3p5jr3oA9GEoAc4qD9bZoJsu");

  // ATAs for THE CURRENT WALLET
  const traderAtaA = (
    await getOrCreateAssociatedTokenAccount(
      provider.connection,
      wallet.payer,
      tokenMintA,
      wallet.publicKey
    )
  ).address;

  const traderAtaB = (
    await getOrCreateAssociatedTokenAccount(
      provider.connection,
      wallet.payer,
      tokenMintB,
      wallet.publicKey
    )
  ).address;

  const amountIn = new anchor.BN(10_000_000); // 0.01
  const minOut = new anchor.BN(1);
  const directionAToB = true;

  const tx = await program.methods
    .swap(amountIn, minOut, directionAToB)
    .accounts({
      pool,
      tokenMintA,
      tokenMintB,
      trader: wallet.publicKey,
      vaultA,
      vaultB,
      traderAtaA,
      traderAtaB,
      tokenProgram: TOKEN_PROGRAM_ID,
    } as any)
    .rpc();

  console.log("swap tx:", tx);
})();
