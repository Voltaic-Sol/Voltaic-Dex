# Voltaic DEX - Complete Usage Guide

**Program ID:** `DLbvQf65M4yaB5bJPMLnPQnhHAGUrtrRw4iLKhGVp4PJ`
**Wallet:** `5uv7g3txfiYYkjA74xz3BicJpTcoAkV7xfLgHfxnEDYg`
**Network:** Testnet (`https://api.testnet.solana.com`)

---

## Step 1: Create Tokens

Create two SPL tokens for your DEX pool:

```bash
npx ts-node scripts/create_tokens.ts
```

**Output:** Save `MINT_A` and `MINT_B` addresses.

---

## Step 2: Initialize Pool

Set environment variables and run the init script:

```bash
# Set your token mints
export MINT_A=<your_mint_a_address>
export MINT_B=<your_mint_b_address>

# Optional: set fee (default 30 bps = 0.3%)
export FEE_BPS=30

# Run init script
npx ts-node scripts/init_pool.ts
```

**Output:** Save `POOL`, `VAULT_A`, `VAULT_B`, `LP_MINT` addresses.

---

## Step 3: Add Liquidity

Add liquidity to your pool:

```bash
# Use addresses from Step 2
export POOL=<pool_address>
export MINT_A=<mint_a>
export MINT_B=<mint_b>
export VAULT_A=<vault_a>
export VAULT_B=<vault_b>
export LP_MINT=<lp_mint>

# Amounts (in base units, 9 decimals)
export AMOUNT_A=1000000000000  # 1000 tokens
export AMOUNT_B=1000000000000  # 1000 tokens

# Run add liquidity
npx ts-node scripts/add_liquidity.ts
```

---

## Step 4: Swap Tokens

Swap Token A for Token B (or vice versa):

```bash
# Use same addresses as Step 3
export POOL=<pool_address>
export MINT_A=<mint_a>
export MINT_B=<mint_b>
export VAULT_A=<vault_a>
export VAULT_B=<vault_b>

# Swap parameters
export AMOUNT_IN=1000000000    # 1 token
export MIN_OUT=1               # minimum output (slippage protection)
export A_TO_B=true             # true = A->B, false = B->A

# Run swap
npx ts-node scripts/swap.ts
```

---

## View on Explorer

After each transaction, view it on Solana Explorer:

```
https://explorer.solana.com/address/<ADDRESS>?cluster=testnet
```

Replace `<ADDRESS>` with:
- Pool address
- Transaction signature
- Token mint address

---

## Quick Reference

| Script | Purpose |
|--------|---------|
| `create_tokens.ts` | Create SPL tokens |
| `init_pool.ts` | Initialize DEX pool |
| `add_liquidity.ts` | Add liquidity & get LP tokens |
| `swap.ts` | Swap tokens |
| `whitelist_add.ts` | Add wallet to whitelist |
| `whitelist_remove.ts` | Remove wallet from whitelist |

---

## Troubleshooting

**Insufficient SOL:** Request more from faucet at https://faucet.solana.com/

**Account not found:** Ensure you're using correct addresses from previous steps

**Transaction failed:** Check compute units and priority fees in scripts
