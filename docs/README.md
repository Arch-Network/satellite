# Satellite Documentation

Satellite is Arch Network's fork of Solana's Anchor framework, adapted for building smart contracts on the Arch Bitcoin L2.

## Migration & Conversion Guides

| Document | Description |
|----------|-------------|
| [Migration Guide](./SATELLITE_MIGRATION_GUIDE.md) | Comprehensive guide for migrating Anchor programs to Satellite |
| [Quick Reference](./QUICK_REFERENCE.md) | Cheat sheet for common conversions and commands |
| [Troubleshooting](./TROUBLESHOOTING.md) | Solutions to common issues and errors |
| [A2S User Guide](../../anchor-to-satellite/docs/A2S_USER_GUIDE.md) | Complete guide for the anchor-to-satellite converter |

## Quick Start

```bash
# 1. Analyze your Anchor program
a2s analyze --path ./my-anchor-program

# 2. Convert to Satellite
a2s convert --source ./my-anchor-program --output ./my-arch-satellite-program

# 3. Update Cargo.toml paths and build
cd my-arch-satellite-program
cargo check
```

## Key Differences from Anchor

| Anchor | Satellite |
|--------|-----------|
| `anchor_lang` | `arch_satellite_lang` |
| `anchor_spl` | `arch_satellite_apl` |
| Base58 program IDs | Hex program IDs (64 chars) |
| `InterfaceAccount` | `Account` |
| `TokenInterface` | `Token` |

## Original Fumadocs

The original Anchor documentation (Fumadocs-based) is located in [`docs/content/docs`](/docs/content/docs).

```bash
npm run dev    # Start development server
# Open http://localhost:3000
```
