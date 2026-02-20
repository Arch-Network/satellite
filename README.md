<div align="center">
  <h1>Satellite</h1>

  <p>
    <strong>Smart Contract Framework for Arch Network</strong>
  </p>

  <p>
    <a href="https://github.com/Arch-Network/satellite/actions"><img alt="Build Status" src="https://img.shields.io/badge/build-passing-brightgreen" /></a>
    <a href="https://docs.arch.network"><img alt="Docs" src="https://img.shields.io/badge/docs-arch.network-blueviolet" /></a>
    <a href="https://opensource.org/licenses/Apache-2.0"><img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue" /></a>
  </p>
</div>

## Overview

**Satellite** is Arch Network's smart contract framework, forked from Solana's [Anchor](https://github.com/coral-xyz/anchor) and adapted for building programs on the Arch Bitcoin L2. It maintains ~95% code compatibility with Anchor while adding Bitcoin-native capabilities.

Satellite enables developers to:
- Write Arch programs using familiar Anchor-style Rust macros
- Leverage the same account validation and serialization patterns
- Access Bitcoin integration through Arch's syscalls
- Port existing Solana programs with minimal changes

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SATELLITE FRAMEWORK                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────────────┐    ┌──────────────────┐    ┌──────────────────┐       │
│  │ arch-satellite-  │    │ arch-satellite-  │    │  anchor-client   │       │
│  │      lang        │    │      apl         │    │                  │       │
│  │                  │    │                  │    │                  │       │
│  │ • #[program]     │    │ • Token CPI      │    │ • RPC Client     │       │
│  │ • #[account]     │    │ • ATA CPI        │    │ • Account Fetch  │       │
│  │ • #[derive]      │    │ • Mint/Burn      │    │ • Tx Builder     │       │
│  │ • Context<T>     │    │ • Transfer       │    │ • IDL Support    │       │
│  └────────┬─────────┘    └────────┬─────────┘    └────────┬─────────┘       │
│           │                       │                       │                  │
│           ▼                       ▼                       ▼                  │
│  ┌──────────────────────────────────────────────────────────────────┐       │
│  │                         ARCH RUNTIME                              │       │
│  │                                                                   │       │
│  │   • eBPF/SBF Execution        • Bitcoin Syscalls                 │       │
│  │   • Account Model             • FROST Threshold Signatures       │       │
│  │   • Cross-Program Invocation  • UTXO Validation                  │       │
│  └──────────────────────────────────────────────────────────────────┘       │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Key Differences from Anchor

| Feature | Anchor (Solana) | Satellite (Arch) |
|---------|-----------------|------------------|
| Import | `anchor_lang` | `arch_satellite_lang` |
| SPL Tokens | `anchor_spl` | `arch_satellite_apl` |
| Program ID | Base58 string | 64-char hex |
| Token Interface | `InterfaceAccount<Mint>` | `Account<Mint>` |
| Bitcoin Integration | N/A | Native syscalls |
| Network | Solana | Arch (Bitcoin L2) |

## Packages

| Package | Description | Purpose |
|:--------|:------------|:--------|
| `arch-satellite-lang` | Core framework with macros and types | Writing Arch programs |
| `arch-satellite-apl` | CPI clients for token programs | Token operations |
| `anchor-client` | Rust client for Arch programs | Off-chain interactions |

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
arch-satellite-lang = "0.31"
arch-satellite-apl = "0.31"  # If using tokens
```

### Example: Counter Program

```rust
use arch_satellite_lang::prelude::*;

declare_id!("da075cb2ff5ec6817613de530b692a8735477769da47430cbd8154335c4a8327");

#[program]
mod counter {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, start: u64) -> Result<()> {
        let counter = &mut ctx.accounts.counter;
        counter.authority = *ctx.accounts.authority.key;
        counter.count = start;
        Ok(())
    }

    pub fn increment(ctx: Context<Increment>) -> Result<()> {
        let counter = &mut ctx.accounts.counter;
        counter.count += 1;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = authority, space = 48)]
    pub counter: Account<'info, Counter>,
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Increment<'info> {
    #[account(mut, has_one = authority)]
    pub counter: Account<'info, Counter>,
    pub authority: Signer<'info>,
}

#[account]
pub struct Counter {
    pub authority: Pubkey,
    pub count: u64,
}
```

### Building

```bash
# Build the program
cargo-build-sbf

# Or with cargo
cargo build --release
```

## Migrating from Anchor

### Import Changes

```rust
// BEFORE (Anchor)
use anchor_lang::prelude::*;
use anchor_spl::token::*;

// AFTER (Satellite)
use arch_satellite_lang::prelude::*;
use arch_satellite_apl::token::*;
```

### Program ID Format

```rust
// Anchor (Base58)
declare_id!("BmDHboaj1kBUoinJKKSRqKfMeRKJqQqEbUj1VgzeQe4A");

// Satellite (Hex - 64 chars)
declare_id!("da075cb2ff5ec6817613de530b692a8735477769da47430cbd8154335c4a8327");
```

### Token Interface Types

```rust
// Anchor
pub mint: InterfaceAccount<'info, Mint>,
pub token_program: Interface<'info, TokenInterface>,

// Satellite
pub mint: Account<'info, Mint>,
pub token_program: Program<'info, Token>,
```

### Automated Migration

Use the [anchor-to-satellite](https://github.com/Arch-Network/anchor-to-satellite) CLI tool:

```bash
# Analyze what will change
a2s analyze --path ./my-anchor-program

# Convert to new directory
a2s convert --source ./my-anchor-program --output ./my-arch-satellite-program

# Convert in-place (creates backup)
a2s convert-in-place --path ./my-anchor-program --backup
```

## Bitcoin Integration

Satellite programs can interact with Bitcoin through Arch's syscalls:

```rust
use arch_satellite_lang::prelude::*;

#[program]
mod bitcoin_aware {
    use super::*;

    pub fn verify_btc_tx(ctx: Context<Verify>, txid: [u8; 32]) -> Result<()> {
        // Fetch Bitcoin transaction data
        let btc_tx = arch_get_bitcoin_tx(&txid)?;

        // Validate UTXO ownership
        arch_validate_utxo_ownership(&utxo, &owner)?;

        // Get network's FROST public key
        let network_pubkey = arch_get_network_xonly_pubkey()?;

        Ok(())
    }
}
```

### Available Syscalls

| Syscall | Description | Cost |
|---------|-------------|------|
| `arch_get_bitcoin_tx` | Fetch Bitcoin transaction | 10,000 CU |
| `arch_get_network_xonly_pubkey` | Get network FROST key | 100 CU |
| `arch_set_transaction_to_sign` | Queue tx for signing | 5,000 CU |
| `arch_validate_utxo_ownership` | Validate UTXO owner | 1,000 CU |

## Project Structure

```
satellite/
├── lang/                    # arch-satellite-lang crate
│   ├── src/
│   │   ├── lib.rs          # Core exports
│   │   ├── accounts.rs     # Account types
│   │   ├── context.rs      # Context<T> implementation
│   │   └── error.rs        # Error handling
│   └── attribute/          # Procedural macros
│       ├── program/        # #[program] macro
│       └── account/        # #[account] macro
├── spl/                     # arch-satellite-apl crate
│   └── src/
│       ├── token.rs        # Token program CPI
│       ├── associated_token.rs
│       └── metadata.rs
├── client/                  # anchor-client crate
│   └── src/
│       └── lib.rs          # RPC client
└── docs/                    # Documentation
    ├── SATELLITE_MIGRATION_GUIDE.md
    ├── QUICK_REFERENCE.md
    └── TROUBLESHOOTING.md
```

## Documentation

- [Migration Guide](./docs/SATELLITE_MIGRATION_GUIDE.md) - Complete guide for porting Anchor programs
- [Quick Reference](./docs/QUICK_REFERENCE.md) - Cheat sheet for common conversions
- [Troubleshooting](./docs/TROUBLESHOOTING.md) - Solutions to common issues
- [Arch Network Docs](https://docs.arch.network) - Official documentation

## Related Projects

| Project | Description |
|---------|-------------|
| [Arch Network](https://github.com/Arch-Network/arch-network) | Core validator and runtime |
| [anchor-to-satellite](https://github.com/Arch-Network/anchor-to-satellite) | Automated conversion tool |
| [arch-bridge](https://github.com/Arch-Network/arch-bridge) | Cross-chain bridge |
| [Anchor](https://github.com/coral-xyz/anchor) | Original Solana framework |

## License

Satellite is licensed under [Apache 2.0](./LICENSE).

This project is a fork of [Anchor](https://github.com/coral-xyz/anchor) by Coral. We thank the Anchor team and contributors for their foundational work.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](./CONTRIBUTING.md) for guidelines.
