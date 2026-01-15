# Satellite Quick Reference

A cheat sheet for migrating Anchor programs to Satellite.

---

## Import Replacements

```rust
// BEFORE (Anchor)                    // AFTER (Satellite)
use anchor_lang::prelude::*;          use satellite_lang::prelude::*;
use anchor_lang::system_program;      use satellite_lang::system_program;
use anchor_spl::token::*;             use satellite_apl::token::*;
use anchor_spl::associated_token::*;  use satellite_apl::associated_token::*;
use solana_program::*;                use arch_program::*;
```

---

## Type Replacements

| Anchor | Satellite |
|--------|-----------|
| `InterfaceAccount<'info, Mint>` | `Account<'info, Mint>` |
| `InterfaceAccount<'info, TokenAccount>` | `Account<'info, TokenAccount>` |
| `Interface<'info, TokenInterface>` | `Program<'info, Token>` |
| `InterfaceAccount` | `Account` |
| `InterfaceMint` | `Mint` |
| `InterfaceTokenAccount` | `TokenAccount` |
| `TokenInterface` | `Token` |

---

## Program ID Format

```rust
// Anchor (base58)
declare_id!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

// Satellite (64 hex chars)
declare_id!("06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a9");
```

**Convert base58 to hex:**
```python
import base58
base58.b58decode("YOUR_BASE58_ID").hex()
```

---

## Cargo.toml

```toml
# Anchor
[dependencies]
anchor-lang = "0.31.0"
anchor-spl = { version = "0.31.0", features = ["token"] }

# Satellite
[dependencies]
satellite-lang = { path = "../satellite/lang", features = ["derive"] }
satellite-apl = { path = "../satellite/spl", features = ["token", "associated_token"] }

# Optional features
# "init-if-needed" - for init_if_needed constraint
# "idl-build" - for IDL generation
```

---

## Constraint Changes

```rust
// REMOVE these constraints (not needed in Satellite):
associated_token::token_program = token_program
mint::token_program = token_program
```

---

## Native Transfers

```rust
// DON'T use direct manipulation:
**from.try_borrow_mut_lamports()? -= amount;
**to.try_borrow_mut_lamports()? += amount;

// DO use CPI pattern:
use satellite_lang::system_program::{transfer, Transfer};

let cpi_ctx = CpiContext::new(
    ctx.accounts.system_program.to_account_info(),
    Transfer {
        from: ctx.accounts.from.to_account_info(),
        to: ctx.accounts.to.to_account_info(),
    },
);
transfer(cpi_ctx, amount)?;
```

---

## Sysvar Access

```rust
// Anchor (account-based)
pub rent: Sysvar<'info, Rent>,
let rent_lamports = ctx.accounts.rent.minimum_balance(size);

// Satellite (syscall-based)
let rent_lamports = rent::minimum_rent(size)?;
let clock = Clock::get()?;
```

---

## A2S Commands

```bash
# Analyze (see what changes are needed)
a2s analyze --path ./my-anchor-program

# Convert (create new directory)
a2s convert --source ./my-anchor --output ./my-satellite

# Convert with diff output
a2s convert --source ./my-anchor --output ./my-satellite --diff

# Convert in-place (creates backup)
a2s convert-in-place --path ./my-anchor --backup true

# Dry run (preview only)
a2s convert --source ./my-anchor --output ./my-satellite --dry-run
```

---

## Build Commands

```bash
# Check compilation (quick)
cargo check

# Build for deployment
cargo-build-sbf

# Clean and rebuild
cargo clean && cargo check
```

---

## Feature Flags

| Feature | Purpose |
|---------|---------|
| `derive` | Enable macros (required) |
| `init-if-needed` | Allow init_if_needed constraint |
| `idl-build` | Generate IDL |
| `event-cpi` | Emit events via CPI |

---

## Bitcoin Syscalls

```rust
// Fetch Bitcoin transaction (10,000 CU each, max 19)
let btc_tx = arch_get_bitcoin_tx(txid)?;

// Get network FROST public key
let network_pubkey = arch_get_network_xonly_pubkey()?;

// Request network signing
arch_set_transaction_to_sign(bitcoin_tx)?;

// Validate UTXO ownership
let valid = arch_validate_utxo_ownership(utxo, owner)?;
```

---

## Common Errors → Solutions

| Error | Solution |
|-------|----------|
| "program id must be 64 hex chars" | Convert base58 to hex |
| "cannot find value `ID`" | Check declare_id! format |
| "init_if_needed requires feature" | Add `"init-if-needed"` feature |
| "unresolved import `anchor_lang`" | Change to `satellite_lang` |
| "InterfaceAccount" type error | Use `Account` instead |
| "linking failed" / "undefined symbol" | Use `cargo check` not `cargo build` |

---

## Minimal Working Program

```rust
use satellite_lang::prelude::*;

declare_id!("0000000000000000000000000000000000000000000000000000000000000001");

#[program]
pub mod my_program {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        ctx.accounts.data.value = 0;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + 8
    )]
    pub data: Account<'info, MyData>,

    pub system_program: Program<'info, System>,
}

#[account]
pub struct MyData {
    pub value: u64,
}
```

---

## Post-Conversion Checklist

- [ ] Update Cargo.toml paths
- [ ] Convert program ID to hex (64 chars)
- [ ] Replace all `anchor_*` imports
- [ ] Replace `InterfaceAccount` → `Account`
- [ ] Replace `TokenInterface` → `Token`
- [ ] Remove `token_program` constraints
- [ ] Add `init-if-needed` feature if needed
- [ ] Search for `TODO(a2s)` and fix manually
- [ ] Run `cargo check`
- [ ] Run `cargo-build-sbf`

---

## Docs Location

- **Migration Guide:** `satellite/docs/SATELLITE_MIGRATION_GUIDE.md`
- **A2S User Guide:** `anchor-to-satellite/docs/A2S_USER_GUIDE.md`
- **Troubleshooting:** `satellite/docs/TROUBLESHOOTING.md`
- **This Cheat Sheet:** `satellite/docs/QUICK_REFERENCE.md`

---

*Last updated: January 2026*
