// scripts/create_tokens.ts
import * as anchor from "@coral-xyz/anchor";
import {
  createMint,
  mintTo,
  getOrCreateAssociatedTokenAccount,
  TOKEN_PROGRAM_ID,
  NATIVE_MINT,
  createSyncNativeInstruction,
  getAssociatedTokenAddressSync,
  createAssociatedTokenAccountInstruction,
} from "@solana/spl-token";
import { PublicKey, SystemProgram, Transaction } from "@solana/web3.js";

async function main() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const wallet = provider.wallet as anchor.Wallet;
  const connection = provider.connection;

  console.log("Creating tokens for wallet:", wallet.publicKey.toBase58());
  console.log("RPC:", (connection as any)._rpcEndpoint);

  // Create Token A (your custom token)
  console.log("\n=== Creating Token A (Custom Token) ===");
  const mintA = await createMint(
    connection,
    wallet.payer,
    wallet.publicKey, // mint authority
    wallet.publicKey, // freeze authority
    9 // decimals
  );
  console.log("Token A Mint:", mintA.toBase58());

  // Token B = SOL (Wrapped SOL / Native Mint)
  console.log("\n=== Token B = SOL (Native Mint) ===");
  const mintB = NATIVE_MINT;
  console.log("Token B Mint (WSOL):", mintB.toBase58());

  // Create ATAs and mint tokens
  console.log("\n=== Setting up token accounts ===");

  // Create ATA for Token A
  const ataA = await getOrCreateAssociatedTokenAccount(
    connection,
    wallet.payer,
    mintA,
    wallet.publicKey
  );
  console.log("ATA A:", ataA.address.toBase58());

  // Mint 1 million Token A
  await mintTo(
    connection,
    wallet.payer,
    mintA,
    ataA.address,
    wallet.publicKey,
    1_000_000_000_000_000 // 1M tokens with 9 decimals
  );
  console.log("✓ Minted 1,000,000 Token A");

  // For WSOL (Token B), we need to wrap SOL
  console.log("\n=== Wrapping SOL for Token B ===");
  const wsolAta = getAssociatedTokenAddressSync(
    NATIVE_MINT,
    wallet.publicKey,
    false,
    TOKEN_PROGRAM_ID
  );

  const wsolAccountInfo = await connection.getAccountInfo(wsolAta);

  const tx = new Transaction();

  // Create WSOL ATA if it doesn't exist
  if (!wsolAccountInfo) {
    tx.add(
      createAssociatedTokenAccountInstruction(
        wallet.publicKey,
        wsolAta,
        wallet.publicKey,
        NATIVE_MINT,
        TOKEN_PROGRAM_ID
      )
    );
  }

  // Transfer SOL to the WSOL account (wrap 10 SOL)
  const wrapAmount = 10_000_000_000; // 10 SOL
  tx.add(
    SystemProgram.transfer({
      fromPubkey: wallet.publicKey,
      toPubkey: wsolAta,
      lamports: wrapAmount,
    })
  );

  // Sync native to update the token balance
  tx.add(createSyncNativeInstruction(wsolAta, TOKEN_PROGRAM_ID));

  const sig = await provider.sendAndConfirm(tx);
  console.log("✓ Wrapped 10 SOL, tx:", sig);
  console.log("WSOL ATA:", wsolAta.toBase58());

  console.log("\n=== Summary ===");
  console.log("MINT_A=" + mintA.toBase58());
  console.log("MINT_B=" + mintB.toBase58() + " (Native SOL)");
  console.log("\nNOTE: MINT_B is the native SOL mint (WSOL)");
  console.log("Your pool will be: Token A <-> SOL");
  console.log("\nSave these addresses for the next steps!");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
