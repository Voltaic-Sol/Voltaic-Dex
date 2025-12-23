import * as anchor from "@coral-xyz/anchor";
import { PublicKey, ComputeBudgetProgram, SendTransactionError } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  createAssociatedTokenAccountInstruction,
} from "@solana/spl-token";

function mustEnv(name: string): string {
  const v = process.env[name];
  if (!v || !v.trim()) throw new Error(`Missing env var: ${name}`);
  return v.trim();
}

function optEnvNum(name: string, def: number): number {
  const v = process.env[name];
  if (!v) return def;
  const n = Number(v);
  if (!Number.isFinite(n)) throw new Error(`Invalid number in env ${name}: ${v}`);
  return n;
}

async function ensureAtaIx(
  provider: anchor.AnchorProvider,
  mint: PublicKey,
  owner: PublicKey
) {
  const ata = getAssociatedTokenAddressSync(
    mint,
    owner,
    false,
    TOKEN_PROGRAM_ID,
    ASSOCIATED_TOKEN_PROGRAM_ID
  );

  const info = await provider.connection.getAccountInfo(ata);
  if (info) return { ata, ix: null as any };

  const ix = createAssociatedTokenAccountInstruction(
    provider.wallet.publicKey, // payer
    ata,
    owner,
    mint,
    TOKEN_PROGRAM_ID,
    ASSOCIATED_TOKEN_PROGRAM_ID
  );

  return { ata, ix };
}

async function main() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const wallet = provider.wallet.publicKey;

  // CHANGE THIS to your actual Anchor workspace name if needed
  const programName = "Dex";
  const program = (anchor.workspace as any)[programName] as anchor.Program;

  if (!program) {
    throw new Error(
      `Could not find program in anchor.workspace["${programName}"]. ` +
        `Update programName in scripts/add_liquidity.ts to match your IDL name.`
    );
  }

  const pool = new PublicKey(mustEnv("POOL"));
  const tokenMintA = new PublicKey(mustEnv("MINT_A"));
  const tokenMintB = new PublicKey(mustEnv("MINT_B"));
  const vaultA = new PublicKey(mustEnv("VAULT_A"));
  const vaultB = new PublicKey(mustEnv("VAULT_B"));
  const lpMint = new PublicKey(mustEnv("LP_MINT"));

  const amountA = new anchor.BN(mustEnv("AMOUNT_A"));
  const amountB = new anchor.BN(mustEnv("AMOUNT_B"));

  const cuLimit = optEnvNum("CU_LIMIT", 600_000);
  const cuPrice = optEnvNum("CU_PRICE_MICROLAMPORTS", 1_000);

  const { ata: userAtaA, ix: createAtaA } = await ensureAtaIx(provider, tokenMintA, wallet);
  const { ata: userAtaB, ix: createAtaB } = await ensureAtaIx(provider, tokenMintB, wallet);
  const { ata: userLpAta, ix: createLpAta } = await ensureAtaIx(provider, lpMint, wallet);

  console.log("Pool:", pool.toBase58());
  console.log("User:", wallet.toBase58());
  console.log("User ATA A:", userAtaA.toBase58());
  console.log("User ATA B:", userAtaB.toBase58());
  console.log("User LP ATA:", userLpAta.toBase58());
  console.log("Amount A:", amountA.toString());
  console.log("Amount B:", amountB.toString());
  console.log("CU_LIMIT:", cuLimit);
  console.log("CU_PRICE_MICROLAMPORTS:", cuPrice);

  const preIxs = [
    ComputeBudgetProgram.setComputeUnitLimit({ units: cuLimit }),
    ComputeBudgetProgram.setComputeUnitPrice({ microLamports: cuPrice }),
  ];
  if (createAtaA) preIxs.push(createAtaA);
  if (createAtaB) preIxs.push(createAtaB);
  if (createLpAta) preIxs.push(createLpAta);

  try {
    // NOTE:
    // We pass multiple possible account names to avoid "not provided" mismatches.
    // Anchor will ignore keys that are not in the IDL, and require the ones that are.
    const sig = await program.methods
      .addLiquidity(amountA, amountB)
      .accounts({
        // core
        pool,
        user: wallet,
        authority: wallet,
        owner: wallet,

        // mints (IDL expects tokenMintA/tokenMintB)
        tokenMintA,
        tokenMintB,

        // also include alternate names just in case
        mintA: tokenMintA,
        mintB: tokenMintB,

        // user token accounts
        userAtaA,
        userAtaB,
        userTokenA: userAtaA,
        userTokenB: userAtaB,
        userTokenAccountA: userAtaA,
        userTokenAccountB: userAtaB,

        // vaults
        vaultA,
        vaultB,
        poolVaultA: vaultA,
        poolVaultB: vaultB,

        // lp mint + user lp
        lpMint,
        userLpAta,
        userLp: userLpAta,
        userLpTokenAccount: userLpAta,

        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        // rent: anchor.web3.SYSVAR_RENT_PUBKEY, // enable if IDL requires
      } as any)
      .preInstructions(preIxs)
      .rpc();

    console.log("addLiquidity tx:", sig);
  } catch (e: any) {
    console.error("Send failed:", e?.message ?? e);

    if (e?.logs) console.error("logs:", e.logs);

    if (typeof e?.getLogs === "function") {
      try {
        const logs = await e.getLogs();
        console.error("getLogs():", logs);
      } catch (err) {
        console.error("getLogs() failed:", err);
      }
    }

    if (e instanceof SendTransactionError) {
      console.error("transactionLogs:", (e as any).transactionLogs);
    }

    process.exit(1);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
