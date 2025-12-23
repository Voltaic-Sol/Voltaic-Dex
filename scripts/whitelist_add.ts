import * as anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";

function mustEnv(name: string): string {
  const v = process.env[name];
  if (!v) throw new Error(`Missing env var: ${name}`);
  return v;
}

(async () => {
  // Anchor env:
  // ANCHOR_PROVIDER_URL=https://api.testnet.solana.com
  // ANCHOR_WALLET=~/.config/solana/id.json
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  // Uses workspace program name "dex" (matches Anchor.toml/programs/dex)
  const program = anchor.workspace.Dex as anchor.Program;

  const POOL = new PublicKey(mustEnv("POOL"));
  const BOT_WALLET = new PublicKey(mustEnv("BOT_WALLET"));

  const [whitelistPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("wl"), POOL.toBuffer(), BOT_WALLET.toBuffer()],
    program.programId
  );

  console.log("Program:", program.programId.toBase58());
  console.log("Pool:", POOL.toBase58());
  console.log("Bot:", BOT_WALLET.toBase58());
  console.log("Whitelist PDA:", whitelistPda.toBase58());

  const tx = await program.methods
    .whitelistAdd()
    .accounts({
      authority: provider.wallet.publicKey, // ADMIN signer
      pool: POOL,
      wallet: BOT_WALLET,
      whitelist: whitelistPda,
      systemProgram: SystemProgram.programId,
    } as any)
    .rpc({ commitment: "confirmed" });

  console.log("whitelist_add tx:", tx);
})();
