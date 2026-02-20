// Re-export core crates so downstream users can access everything from `satellite-bitcoin`

pub use arch_satellite_collections::*;
pub use arch_satellite_math::*;

// Re-export the transactions crate using the alias defined in Cargo.toml
pub use arch_satellite_bitcoin_transactions::*;

pub mod script;
pub use script::ScriptPubkey;
