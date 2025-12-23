# Private DEX (Solana · Anchor)

A lightweight, private decentralized exchange built on **Solana**, written in **Rust** using the **Anchor framework**.

Designed for controlled liquidity environments — gated pools, admin-managed operations, and fast constant-product swaps without unnecessary surface area.

This is not a public AMM clone.  
It’s opinionated, minimal, and built for teams that want **control first, flexibility second**.

---

## What this DEX does

- Manages liquidity pools for two SPL tokens
- Mints LP tokens for liquidity providers
- Executes constant-product swaps
- Enforces wallet whitelisting at the pool level
- Gives admins pause + emergency recovery controls

Built to be simple to deploy, easy to reason about, and hard to misuse.

---

## Core Features

### Pool lifecycle
- Initialize pools with:
  - Two token mints
  - Vaults
  - LP mint
- Deterministic account structure via Anchor

### Liquidity
- Add liquidity by depositing token pairs
- Remove liquidity by burning LP tokens
- Vault balances tracked on-chain

### Swaps
- `swap_exact_in` style swaps
- Constant-product pricing
- Uses pool reserves directly (no oracle dependency)

### Whitelist enforcement
- Wallet-level access control per pool
- Add / remove wallets from whitelist
- Swaps and liquidity actions can be gated

### Admin controls
- Pause / unpause pool operations
- Emergency drain for recovery scenarios
- Intended for multisig-backed admin keys

---

## Repository structure

```
programs/
  dex/
    src/lib.rs        # On-chain Anchor program (Rust)

scripts/              # TypeScript interaction scripts
  init_pool.ts
  add_liquidity.ts
  swap.ts
  whitelist.ts

migrations/
  deploy.ts           # Anchor deployment script

tests/
  dex.ts              # Anchor tests

idl/                  # Generated IDL
types/                # Generated TypeScript types

env.example           # Environment variable template
target/               # Build artifacts (gitignored)
```

---

## Prerequisites

- Rust toolchain (see `rust-toolchain.toml`)
- Solana CLI (configured for localnet or devnet)
- Anchor CLI
- Node.js + yarn or npm

---

## Local setup (quick start)

### 1. Environment
```bash
cp env.example .env
```

Fill in:
- `ANCHOR_PROVIDER_URL`
- `ANCHOR_WALLET`
- `DEX_PROGRAM_ID` (after deployment)

---

### 2. Install dependencies
```bash
yarn install
# or
npm install
```

---

### 3. Build the program
```bash
anchor build
```

---

### 4. Start local validator (optional)
```bash
solana-test-validator --reset
```

Ensure your `ANCHOR_PROVIDER_URL` matches (default `http://127.0.0.1:8899`).

---

### 5. Deploy
```bash
anchor deploy
```

Update `DEX_PROGRAM_ID` in `.env` after deployment.

---

### 6. Run scripts
```bash
ts-node scripts/init_pool.ts
ts-node scripts/add_liquidity.ts
ts-node scripts/swap.ts
```

---

### 7. Run tests
```bash
anchor test
```

