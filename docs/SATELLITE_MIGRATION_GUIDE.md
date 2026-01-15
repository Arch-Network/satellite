# Satellite Migration Guide

A comprehensive guide for migrating Solana Anchor programs to Arch Network using the Satellite framework.

## Table of Contents

1. [Overview](#overview)
2. [Key Differences from Anchor](#key-differences-from-anchor)
3. [Quick Start](#quick-start)
4. [Detailed Migration Steps](#detailed-migration-steps)
5. [Bitcoin Integration](#bitcoin-integration)
6. [Common Patterns](#common-patterns)
7. [Feature Flags](#feature-flags)
8. [Examples](#examples)

---

## Overview

**Satellite** is Arch Network's fork of Solana's Anchor framework, adapted for building smart contracts on the Arch Bitcoin L2. It maintains ~95% code compatibility with Anchor while adding Bitcoin-native capabilities.

### What Stays the Same

- `#[program]` macro for defining program entry points
- `#[derive(Accounts)]` for account validation
- `#[account]` for defining account structures
- `Context<T>`, `Account<T>`, `Signer<T>`, `Program<T>` types
- Error handling with `Result<()>` and `#[error_code]`
- Events with `#[event]` and `emit!`
- PDA derivation and validation
- CPI (Cross-Program Invocation) patterns

### What's Different

| Anchor | Satellite | Reason |
|--------|-----------|--------|
| `anchor_lang` | `satellite_lang` | Namespace change |
| `anchor_spl` | `satellite_apl` | Namespace change |
| Base58 program IDs | Hex program IDs | Arch uses hex format |
| `InterfaceAccount` | `Account` | Token Extensions removed |
| `TokenInterface` | `Token` | Simplified token handling |
| Sysvars as accounts | Syscall access | Different runtime model |
| N/A | Bitcoin syscalls | New: Bitcoin integration |

---

## Key Differences from Anchor

### 1. Import Changes

```rust
// Anchor
use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount, Mint};
use anchor_spl::associated_token::AssociatedToken;

// Satellite
use satellite_lang::prelude::*;
use satellite_apl::token::{Token, TokenAccount, Mint};
use satellite_apl::associated_token::AssociatedToken;
```

### 2. Program ID Format

Satellite uses **64-character hex strings** instead of base58:

```rust
// Anchor (base58)
declare_id!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

// Satellite (hex - 64 characters)
declare_id!("06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a9");
```

**Converting Base58 to Hex:**
```bash
# Use the a2s tool (automatic)
a2s convert --source ./my-anchor-program --output ./my-satellite-program

# Or manually in Python
import base58
pubkey_bytes = base58.b58decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
print(pubkey_bytes.hex())  # 64-char hex string
```

### 3. Token Interface Simplification

Satellite removes Token-2022 extensions. Use standard token types:

```rust
// Anchor (with Token Extensions)
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

#[derive(Accounts)]
pub struct MyAccounts<'info> {
    pub mint: InterfaceAccount<'info, Mint>,
    pub token_account: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

// Satellite (simplified)
use satellite_apl::token::{Mint, TokenAccount, Token};

#[derive(Accounts)]
pub struct MyAccounts<'info> {
    pub mint: Account<'info, Mint>,
    pub token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}
```

### 4. Sysvar Access

Access sysvars via syscalls, not as account arguments:

```rust
// Anchor
#[derive(Accounts)]
pub struct MyAccounts<'info> {
    pub rent: Sysvar<'info, Rent>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn my_instruction(ctx: Context<MyAccounts>) -> Result<()> {
    let rent = &ctx.accounts.rent;
    let clock = &ctx.accounts.clock;
    // ...
}

// Satellite
use satellite_lang::prelude::*;

#[derive(Accounts)]
pub struct MyAccounts<'info> {
    // No sysvar accounts needed
}

pub fn my_instruction(ctx: Context<MyAccounts>) -> Result<()> {
    // Access via syscalls
    let min_rent = rent::minimum_rent(data_len)?;
    let clock = Clock::get()?;
    let current_slot = clock.slot;
    // ...
}
```

### 5. Constraint Changes

Remove `token_program` constraints (Arch handles this automatically):

```rust
// Anchor
#[account(
    init,
    payer = payer,
    associated_token::mint = mint,
    associated_token::authority = owner,
    associated_token::token_program = token_program,  // Remove this
)]
pub token_account: Account<'info, TokenAccount>,

// Satellite
#[account(
    init,
    payer = payer,
    associated_token::mint = mint,
    associated_token::authority = owner,
    // No token_program constraint needed
)]
pub token_account: Account<'info, TokenAccount>,
```

---

## Quick Start

### Option 1: Automatic Conversion (Recommended)

```bash
# Install the a2s converter
cd anchor-to-satellite
cargo build --release

# Analyze your program first
./target/release/a2s analyze --path /path/to/your/anchor-program

# Convert to a new directory
./target/release/a2s convert \
    --source /path/to/your/anchor-program \
    --output /path/to/satellite-program

# Update Cargo.toml paths
# Edit the satellite-lang and satellite-apl paths to match your setup
```

### Option 2: Manual Migration

1. **Update Cargo.toml:**
```toml
[dependencies]
satellite-lang = { path = "../satellite/lang", features = ["derive"] }
satellite-apl = { path = "../satellite/spl", features = ["token", "associated_token"] }
```

2. **Update imports in your Rust files:**
```rust
// Replace all anchor_lang with satellite_lang
// Replace all anchor_spl with satellite_apl
```

3. **Convert program ID to hex format**

4. **Simplify token interfaces**

5. **Build:**
```bash
cargo check  # Verify compilation
cargo-build-sbf  # Build for deployment
```

---

## Detailed Migration Steps

### Step 1: Cargo.toml

```toml
# Before (Anchor)
[dependencies]
anchor-lang = "0.31.0"
anchor-spl = { version = "0.31.0", features = ["token"] }

[features]
idl-build = ["anchor-lang/idl-build", "anchor-spl/idl-build"]

# After (Satellite)
[dependencies]
satellite-lang = { path = "../satellite/lang", features = ["derive"] }
satellite-apl = { path = "../satellite/spl", features = ["token", "associated_token"] }

[features]
idl-build = ["satellite-lang/idl-build", "satellite-apl/idl-build"]
```

### Step 2: Source Files

Run search-and-replace across your codebase:

| Find | Replace |
|------|---------|
| `use anchor_lang::prelude::*` | `use satellite_lang::prelude::*` |
| `use anchor_lang::` | `use satellite_lang::` |
| `use anchor_spl::` | `use satellite_apl::` |
| `anchor_lang::solana_program::` | `satellite_lang::arch_program::` |
| `InterfaceAccount<'info, Mint>` | `Account<'info, Mint>` |
| `InterfaceAccount<'info, TokenAccount>` | `Account<'info, TokenAccount>` |
| `Interface<'info, TokenInterface>` | `Program<'info, Token>` |
| `InterfaceAccount` | `Account` |
| `TokenInterface` | `Token` |

### Step 3: Native Transfers

If your program uses direct lamport manipulation, convert to CPI:

```rust
// Before (direct manipulation - NOT supported)
**ctx.accounts.from.try_borrow_mut_lamports()? -= amount;
**ctx.accounts.to.try_borrow_mut_lamports()? += amount;

// After (CPI pattern - supported)
use satellite_lang::system_program::{transfer, Transfer};

let cpi_context = CpiContext::new(
    ctx.accounts.system_program.to_account_info(),
    Transfer {
        from: ctx.accounts.from.to_account_info(),
        to: ctx.accounts.to.to_account_info(),
    },
);
transfer(cpi_context, amount)?;
```

### Step 4: Verify Build

```bash
# Check for compilation errors
cargo check

# If using init_if_needed, enable the feature:
# satellite-lang = { ..., features = ["derive", "init-if-needed"] }

# Build for deployment
cargo-build-sbf
```

---

## Bitcoin Integration

Satellite programs can interact with Bitcoin through special syscalls:

### Available Syscalls

```rust
use satellite_lang::arch_program::bitcoin;

// Fetch a Bitcoin transaction by txid
let btc_tx = arch_get_bitcoin_tx(txid)?;  // Costs 10,000 CU

// Get the network's FROST aggregate public key
let network_pubkey = arch_get_network_xonly_pubkey()?;

// Request the network to sign a Bitcoin transaction
arch_set_transaction_to_sign(bitcoin_tx)?;

// Validate UTXO ownership
let is_valid = arch_validate_utxo_ownership(utxo, owner)?;
```

### Bitcoin Transaction Builder

```rust
use satellite_bitcoin::TransactionBuilder;

// Create a transaction builder with compile-time bounds
let mut builder = TransactionBuilder::<8, 4>::new();  // max 8 accounts, 4 inputs

// Add inputs and outputs
builder.add_tx_input(utxo_info)?;
builder.add_state_transition(account_info)?;

// Finalize and get the transaction
let btc_tx = builder.finalize()?;

// Request network signing
arch_set_transaction_to_sign(btc_tx)?;
```

### Rune Support

```rust
use satellite_bitcoin::RuneAmount;

// Get runes from a UTXO output
let runes = get_runes_from_output(txid, vout)?;

// Work with rune amounts
let rune_amount = RuneAmount {
    id: rune_id,
    amount: 1000,
};
```

---

## Common Patterns

### Account Initialization

```rust
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + MyAccount::INIT_SPACE,
        seeds = [b"my-seed", payer.key().as_ref()],
        bump
    )]
    pub my_account: Account<'info, MyAccount>,

    pub system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct MyAccount {
    pub authority: Pubkey,
    pub counter: u64,
}
```

### Token Transfers

```rust
use satellite_apl::token::{transfer_checked, TransferChecked};

pub fn transfer_tokens(ctx: Context<TransferTokens>, amount: u64) -> Result<()> {
    let cpi_accounts = TransferChecked {
        from: ctx.accounts.source.to_account_info(),
        to: ctx.accounts.destination.to_account_info(),
        mint: ctx.accounts.mint.to_account_info(),
        authority: ctx.accounts.authority.to_account_info(),
    };

    let cpi_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        cpi_accounts,
    );

    transfer_checked(cpi_ctx, amount, ctx.accounts.mint.decimals)
}
```

### PDA Signing

```rust
pub fn transfer_from_pda(ctx: Context<PdaTransfer>, amount: u64) -> Result<()> {
    let seeds = &[
        b"vault",
        ctx.accounts.authority.key().as_ref(),
        &[ctx.bumps.vault],
    ];
    let signer_seeds = &[&seeds[..]];

    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        Transfer { ... },
        signer_seeds,
    );

    transfer(cpi_ctx, amount)
}
```

---

## Feature Flags

### satellite-lang Features

| Feature | Description | Default |
|---------|-------------|---------|
| `derive` | Enable derive macros | Required |
| `init-if-needed` | Allow `init_if_needed` constraint | Off |
| `idl-build` | Generate IDL during build | Off |
| `event-cpi` | Emit events via CPI | Off |

### satellite-apl Features

| Feature | Description | Default |
|---------|-------------|---------|
| `token` | SPL Token program CPI | Off |
| `associated_token` | Associated Token Account CPI | Off |
| `metadata` | Token Metadata CPI | Off |
| `stake` | Stake program CPI | Off |

### Example Cargo.toml

```toml
[dependencies]
satellite-lang = {
    path = "../satellite/lang",
    features = ["derive", "init-if-needed"]
}
satellite-apl = {
    path = "../satellite/spl",
    features = ["token", "associated_token"]
}

[features]
default = []
idl-build = ["satellite-lang/idl-build", "satellite-apl/idl-build"]
```

---

## Examples

### Minimal Program

```rust
use satellite_lang::prelude::*;

declare_id!("0000000000000000000000000000000000000000000000000000000000000001");

#[program]
pub mod hello_arch {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Hello, Arch Network!");
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
```

### Counter Program

```rust
use satellite_lang::prelude::*;

declare_id!("your64charhexstringhere0000000000000000000000000000000000000000");

#[program]
pub mod counter {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        ctx.accounts.counter.count = 0;
        ctx.accounts.counter.authority = ctx.accounts.authority.key();
        Ok(())
    }

    pub fn increment(ctx: Context<Increment>) -> Result<()> {
        ctx.accounts.counter.count = ctx.accounts.counter.count.checked_add(1)
            .ok_or(ErrorCode::Overflow)?;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + Counter::INIT_SPACE
    )]
    pub counter: Account<'info, Counter>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Increment<'info> {
    #[account(
        mut,
        has_one = authority
    )]
    pub counter: Account<'info, Counter>,
    pub authority: Signer<'info>,
}

#[account]
#[derive(InitSpace)]
pub struct Counter {
    pub authority: Pubkey,
    pub count: u64,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Counter overflow")]
    Overflow,
}
```

---

## Next Steps

1. **Read the A2S User Guide** - Learn to use the automatic converter
2. **Check the Troubleshooting Guide** - Common issues and solutions
3. **Explore Bitcoin Integration** - Build Bitcoin-native applications
4. **Join the Community** - Get help from other developers

---

*Last updated: January 2026*
