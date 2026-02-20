# Satellite & A2S Troubleshooting Guide

Solutions to common issues when migrating from Anchor to Satellite.

## Table of Contents

1. [Build Errors](#build-errors)
2. [A2S Converter Issues](#a2s-converter-issues)
3. [Runtime Errors](#runtime-errors)
4. [Token Program Issues](#token-program-issues)
5. [Bitcoin Integration Issues](#bitcoin-integration-issues)
6. [IDE & Tooling Issues](#ide--tooling-issues)

---

## Build Errors

### Error: "program id must be 64 hex chars"

**Symptom:**
```
error: program id must be 64 hex chars (optionally prefixed with 0x)
 --> src/lib.rs:3:1
  |
3 | declare_id!("BmDHboaj1kBUoinJKKSRqKfMeRKJqQqEbUj1VgzeQe4A");
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

**Cause:** Satellite uses hex format for program IDs, not base58.

**Solution:**

Option 1: Use the a2s converter (automatic):
```bash
a2s convert --source ./my-program --output ./my-program-converted
```

Option 2: Manual conversion:
```python
# Python script to convert
import base58
pubkey = "BmDHboaj1kBUoinJKKSRqKfMeRKJqQqEbUj1VgzeQe4A"
hex_id = base58.b58decode(pubkey).hex()
print(f'declare_id!("{hex_id}");')
```

Option 3: Generate a new ID:
```bash
# Generate random 32 bytes as hex
openssl rand -hex 32
# Output: 9fe85121d1f19362930d881e105e84b82df0cc379f9984cbb7edd40720e4fe33
```

---

### Error: "cannot find value `ID` in this scope"

**Symptom:**
```
error[E0425]: cannot find value `ID` in this scope
 --> src/lib.rs:5:1
  |
5 | #[program]
  | ^^^^^^^^^^ not found in this scope
```

**Cause:** The `declare_id!` macro failed to parse (usually wrong format).

**Solution:** Ensure your program ID is exactly 64 hex characters:

```rust
// Wrong (base58)
declare_id!("BmDHboaj1kBUoinJKKSRqKfMeRKJqQqEbUj1VgzeQe4A");

// Wrong (too short)
declare_id!("9fe85121d1f19362930d");

// Correct (64 hex chars)
declare_id!("9fe85121d1f19362930d881e105e84b82df0cc379f9984cbb7edd40720e4fe33");
```

---

### Error: "init_if_needed requires feature"

**Symptom:**
```
error: init_if_needed requires that arch-satellite-lang be imported with the
init-if-needed cargo feature enabled.
```

**Cause:** Using `init_if_needed` constraint without enabling the feature.

**Solution:** Add the feature to Cargo.toml:

```toml
[dependencies]
arch-satellite-lang = {
    path = "../satellite/lang",
    features = ["derive", "init-if-needed"]  # Add this
}
```

**Warning:** Read the security implications of `init_if_needed` in the Anchor docs before using it.

---

### Error: "unresolved import `anchor_lang`"

**Symptom:**
```
error[E0432]: unresolved import `anchor_lang`
 --> src/lib.rs:1:5
  |
1 | use anchor_lang::prelude::*;
  |     ^^^^^^^^^^ use of undeclared crate
```

**Cause:** Import not converted from Anchor to Satellite.

**Solution:** Replace imports:

```rust
// Before
use anchor_lang::prelude::*;
use anchor_spl::token::Token;

// After
use arch_satellite_lang::prelude::*;
use arch_satellite_apl::token::Token;
```

Or run the converter:
```bash
a2s convert --source ./my-program --output ./converted
```

---

### Error: "mismatched types `InterfaceAccount` vs `Account`"

**Symptom:**
```
error[E0308]: mismatched types
  --> src/lib.rs:15:5
   |
15 |     pub mint: InterfaceAccount<'info, Mint>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected struct `Account`
```

**Cause:** Token interface types not converted.

**Solution:** Replace all token interface types:

| Before | After |
|--------|-------|
| `InterfaceAccount<'info, Mint>` | `Account<'info, Mint>` |
| `InterfaceAccount<'info, TokenAccount>` | `Account<'info, TokenAccount>` |
| `Interface<'info, TokenInterface>` | `Program<'info, Token>` |

---

### Error: "linking with `cc` failed" / "undefined symbol `_sol_log_pubkey`"

**Symptom:**
```
error: linking with `cc` failed: exit status: 1
  ...
  Undefined symbols for architecture arm64:
    "_sol_log_pubkey", referenced from:
```

**Cause:** Trying to build for native target instead of BPF.

**Solution:** This is expected when running `cargo build`. Satellite programs must be built for the BPF target:

```bash
# Wrong - builds for native
cargo build

# Correct - builds for BPF
cargo-build-sbf

# For just checking compilation (no linking)
cargo check  # This works!
```

---

### Error: "the trait bound `X: Bumps` is not satisfied"

**Symptom:**
```
error[E0277]: the trait bound `MyAccounts<'_>: Bumps` is not satisfied
```

**Cause:** Usually happens with `init_if_needed` or complex account structures.

**Solution:**

1. Enable the `init-if-needed` feature if using that constraint
2. Ensure all `#[derive(Accounts)]` structs are properly defined
3. Check for circular dependencies between modules

---

### Error: "unexpected `cfg` condition value: `satellite-debug`"

**Symptom:**
```
warning: unexpected `cfg` condition value: `satellite-debug`
```

**Cause:** This is a warning, not an error. It's from internal Satellite macros.

**Solution:** This warning is harmless and can be ignored. To suppress it, add to Cargo.toml:

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(feature, values("satellite-debug"))'] }
```

---

## A2S Converter Issues

### Error: "No such file or directory"

**Symptom:**
```
Error: No such file or directory (os error 2)
```

**Cause:** Invalid path provided.

**Solution:** Check the path exists and is a directory:

```bash
# Verify path
ls -la ./my-anchor-program

# Use absolute paths if needed
a2s analyze --path /full/path/to/my-program
```

---

### Issue: Conversion runs but no changes made

**Symptom:** A2S reports 0 files modified.

**Cause:**
- Program may already be converted
- Path might point to wrong directory
- No `.rs` files found

**Solution:**

```bash
# Check for Rust files
find ./my-program -name "*.rs" | head

# Verify it's an Anchor program
grep -r "anchor_lang" ./my-program

# Check Cargo.toml
cat ./my-program/Cargo.toml | grep anchor
```

---

### Issue: declare_id! not converted

**Symptom:** Program ID still in base58 after conversion.

**Cause:** This was a bug in earlier a2s versions.

**Solution:** Update to latest a2s version and re-run:

```bash
cd anchor-to-satellite
git pull
cargo build --release
a2s convert --source ./my-program --output ./converted
```

Or manually convert using Python:
```python
import base58
base58.b58decode("YOUR_BASE58_ID").hex()
```

---

### Issue: Cargo.toml paths are wrong

**Symptom:**
```toml
arch-satellite-lang = { path = "../satellite/lang" }  # Path doesn't exist
```

**Cause:** A2S uses relative paths that may not match your setup.

**Solution:** Edit Cargo.toml after conversion:

```toml
# Option 1: Absolute path
arch-satellite-lang = { path = "/home/user/satellite/lang", features = ["derive"] }

# Option 2: Correct relative path
arch-satellite-lang = { path = "../../satellite/lang", features = ["derive"] }

# Option 3: Git dependency (when published)
arch-satellite-lang = { git = "https://github.com/org/satellite", features = ["derive"] }
```

---

## Runtime Errors

### Error: "Account not initialized"

**Symptom:** Transaction fails with account initialization error.

**Cause:** Account constraints differ between Solana and Arch.

**Solution:** Verify your `init` constraints:

```rust
#[account(
    init,
    payer = payer,
    space = 8 + MyAccount::INIT_SPACE,  // Ensure correct space
    seeds = [b"seed"],
    bump
)]
pub my_account: Account<'info, MyAccount>,
```

---

### Error: "Instruction data too large"

**Symptom:** Transaction rejected for size.

**Cause:** Arch has different transaction size limits.

**Solution:**
- Reduce instruction data size
- Split into multiple transactions
- Use account data instead of instruction data for large payloads

---

### Error: "Compute budget exceeded"

**Symptom:**
```
Error: Compute budget exceeded
```

**Cause:** Instruction used more than 200,000 compute units.

**Solution:**
- Optimize loops and computations
- Split complex operations
- Avoid unnecessary account reads
- Use efficient data structures

**Arch Compute Limits:**
- 200,000 CU per instruction
- Bitcoin syscalls cost 10,000 CU each (max 19 calls)

---

## Token Program Issues

### Error: "token_program constraint not found"

**Symptom:** Constraint error related to token program.

**Cause:** Satellite removes the need for explicit token_program constraints.

**Solution:** Remove these constraints:

```rust
// Before (Anchor)
#[account(
    init,
    associated_token::mint = mint,
    associated_token::authority = owner,
    associated_token::token_program = token_program,  // Remove
)]

// After (Satellite)
#[account(
    init,
    associated_token::mint = mint,
    associated_token::authority = owner,
    // No token_program constraint needed
)]
```

---

### Error: "TokenInterface not found"

**Symptom:**
```
error[E0412]: cannot find type `TokenInterface` in this scope
```

**Cause:** Token Extensions (Token-2022) not supported in Satellite.

**Solution:** Use standard token types:

```rust
// Before
use anchor_spl::token_interface::TokenInterface;
pub token_program: Interface<'info, TokenInterface>,

// After
use arch_satellite_apl::token::Token;
pub token_program: Program<'info, Token>,
```

---

### Error: "Token account mismatch"

**Symptom:** Token operations fail with account mismatch.

**Cause:** Using wrong token program or account types.

**Solution:** Ensure consistent usage:

```rust
use arch_satellite_apl::token::{Token, TokenAccount, Mint};
use arch_satellite_apl::associated_token::AssociatedToken;

#[derive(Accounts)]
pub struct MyAccounts<'info> {
    pub mint: Account<'info, Mint>,
    pub token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}
```

---

## Bitcoin Integration Issues

### Error: "Bitcoin syscall failed"

**Symptom:** Bitcoin-related syscall returns error.

**Cause:** Various - invalid txid, network issues, or exceeded call limits.

**Solution:**

1. Verify txid format (32 bytes)
2. Check you haven't exceeded 19 Bitcoin syscalls
3. Ensure the transaction exists and is confirmed

```rust
// Verify txid before calling
assert_eq!(txid.len(), 32, "Invalid txid length");

// Check call count
// Each arch_get_bitcoin_tx costs 10,000 CU
// With 200,000 CU limit, max 19 calls per instruction
```

---

### Error: "UTXO not found"

**Symptom:** UTXO validation fails.

**Cause:** UTXO doesn't exist, already spent, or not yet confirmed.

**Solution:**

```rust
// Verify UTXO exists and is confirmed
let utxo = arch_get_bitcoin_tx(txid)?;

// Check confirmation depth based on network
// Mainnet: 6 confirmations
// Testnet: 3 confirmations
// Regtest: 1 confirmation
```

---

## IDE & Tooling Issues

### Issue: rust-analyzer shows errors

**Symptom:** IDE shows errors but `cargo check` passes.

**Cause:** rust-analyzer may not handle all macro expansions correctly.

**Solution:**

1. Restart rust-analyzer
2. Run `cargo check` to verify actual status
3. Add to `.vscode/settings.json`:
```json
{
    "rust-analyzer.cargo.features": ["derive"]
}
```

---

### Issue: Slow compilation

**Symptom:** Build takes very long.

**Cause:** Macro expansion and multiple dependencies.

**Solution:**

1. Use `cargo check` for quick verification
2. Use incremental compilation:
```toml
[profile.dev]
incremental = true
```
3. Use `cargo-build-sbf` only when deploying

---

### Issue: cargo-build-sbf not found

**Symptom:**
```
error: no such command: `build-sbf`
```

**Cause:** Solana CLI tools not installed.

**Solution:**

```bash
# Install Solana CLI
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"

# Add to PATH
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"

# Verify
cargo-build-sbf --version
```

---

## Quick Fixes Checklist

When something doesn't work, try these in order:

- [ ] Run `cargo check` to see actual errors
- [ ] Verify program ID is 64 hex characters
- [ ] Check all imports are `arch_satellite_*` not `anchor_*`
- [ ] Remove `InterfaceAccount` → use `Account`
- [ ] Remove `TokenInterface` → use `Token`
- [ ] Remove `token_program = x` constraints
- [ ] Add `init-if-needed` feature if using that constraint
- [ ] Update Cargo.toml paths
- [ ] Run `cargo clean && cargo check`
- [ ] Check for TODO(a2s) comments and fix them

---

## Getting More Help

### Debug Output

For detailed debugging:
```bash
RUST_BACKTRACE=1 cargo check 2>&1 | head -100
```

### Check Versions

```bash
rustc --version
cargo --version
cargo-build-sbf --version
```

### Minimal Reproduction

Create a minimal test case:
```rust
use arch_satellite_lang::prelude::*;

declare_id!("0000000000000000000000000000000000000000000000000000000000000001");

#[program]
pub mod test {
    use super::*;
    pub fn test(_ctx: Context<Test>) -> Result<()> { Ok(()) }
}

#[derive(Accounts)]
pub struct Test {}
```

If this compiles, the issue is in your specific code. If it doesn't, there's an environment issue.

---

*Last updated: January 2026*
