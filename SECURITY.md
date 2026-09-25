# Security Policy

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability.

Report it privately through GitHub: on [Arch-Network/satellite](https://github.com/Arch-Network/satellite), open the **Security** tab and choose **Report a vulnerability**. Include the affected crate and version, a description of the impact, and a reproduction if you have one.

We aim to acknowledge reports within three business days and to agree a disclosure date with the reporter once a fix is available. Fixes are released as a new crate version before the advisory is published.

## Supported versions

Only the latest published `arch-satellite-*` release receives security fixes.

## Scope

In scope: every `arch-satellite-*` crate published to crates.io, and the code the macros in `arch-satellite-lang` generate. Code that is compiled into on-chain programs is the priority.

`anchor-client` and `anchor-cli` are unpublished developer tools inherited from Anchor. They submit transactions through Solana client types and RPC, and their Arch/Solana boundary is not integration-tested against an Arch validator. Do not rely on them for production transaction submission. Reports against them are welcome but are handled as tooling issues.

Each release is built against a specific `arch_program` version (see the workspace `Cargo.toml`). Behavior against a validator running a different Arch release is not covered.
