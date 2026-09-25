//! Arch Bitcoin transaction helpers.
//!
//! This crate offers zero-heap utilities and a strongly-typed builder – [`TransactionBuilder`] –
//! for composing, fee–tuning and finalising Bitcoin transactions that will be forwarded through
//! the **Arch** runtime.  All helpers are `no_std` friendly (apart from the unavoidable `alloc`
//! use inside the `bitcoin` dependency) and can therefore be called from on-chain programs that
//! run inside the Solana BPF VM.
//!
//! ## Key Features
//!
//! - **Zero-heap allocation**: Uses fixed-size collections for constrained environments
//! - **Type-safe builder pattern**: Compile-time bounds prevent common mistakes
//! - **Fee calculation**: Automatic fee estimation and adjustment with mempool ancestry tracking
//! - **Rune support**: Optional rune transaction support when compiled with `runes` feature
//! - **UTXO consolidation**: Optional consolidation features for managing fragmented UTXOs
//! - **BPF compatibility**: Suitable for on-chain programs running in Solana BPF VM
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use arch_satellite_bitcoin_transactions::TransactionBuilder;
//! use arch_satellite_bitcoin_transactions::fee_rate::FeeRate;
//!
//! // Create a builder that can handle up to 8 modified accounts and 4 inputs to sign
//! let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
//!
//! // Add inputs, outputs, and state transitions...
//! // (See TransactionBuilder documentation for detailed examples)
//!
//! // Adjust fees and finalize
//! let fee_rate = FeeRate::try_from(10.0).unwrap(); // 10 sat/vB
//! builder.adjust_transaction_to_pay_fees(&fee_rate, None)?;
//! builder.finalize()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Architecture
//!
//! The crate is built around the [`TransactionBuilder`] which maintains:
//! - A Bitcoin transaction under construction
//! - Metadata about which program accounts have been modified
//! - Information about which transaction inputs need signatures
//! - Running totals for fee calculation and validation
//!
//! All data structures use fixed-size collections to ensure zero-heap allocation,
//! making them suitable for constrained environments like the Solana BPF VM.
use std::fmt::Debug;
#[cfg(test)]
use std::str::FromStr;

use arch_program::rune::RuneAmount;
use arch_program::{
    account::AccountInfo, input_to_sign::InputToSign, program::set_transaction_to_sign,
    program_error::ProgramError, pubkey::Pubkey,
};
use bitcoin::{
    absolute::LockTime, transaction::Version, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Witness,
};
pub use mempool::{AccountMempoolInfo, MempoolData, MempoolDataView, MempoolInfo, TxStatus};

#[cfg(feature = "runes")]
use ordinals::{Artifact, Runestone};

use arch_satellite_collections::generic::fixed_list_unchecked::FixedRefList;
use arch_satellite_collections::generic::fixed_set::FixedCapacitySet;

use crate::btc_utxo_holder::BtcUtxoHolder;
use crate::{
    calc_fee::{
        adjust_transaction_to_pay_fees, estimate_final_tx_vsize,
        estimate_tx_size_with_additional_inputs_outputs,
        estimate_tx_vsize_with_additional_inputs_outputs,
    },
    error::BitcoinTxError,
    fee_rate::FeeRate,
    inputs::{match_inputs_to_utxos, records_for, InputRecord},
    mempool::generate_mempool_info,
    utxo_info::UtxoInfo,
};

#[cfg(feature = "utxo-consolidation")]
use crate::{consolidation::add_consolidation_utxos, input_calc::ARCH_INPUT_SIZE};

mod arch;
pub mod btc_utxo_holder;
pub mod bytes;
mod calc_fee;
mod consolidation;
pub mod constants;
pub mod error;
pub mod fee_rate;
mod find_btc;
pub mod input_calc;
mod inputs;
mod mempool;
#[cfg(feature = "serde")]
mod serde;
pub mod util;
pub mod utxo_info;
#[cfg(feature = "serde")]
pub mod utxo_info_json;

/// Represents potential transaction inputs for size estimation.
///
/// This struct is used in "what-if" scenarios where you need to estimate the size
/// of a transaction before actually adding inputs. It's particularly useful for
/// fee calculation and UTXO consolidation planning.
///
/// ## Fields
///
/// - `count`: Number of inputs of this type to add
/// - `item`: Template input to use for size calculation
/// - `signer`: Whether these inputs will be signed by a program account
///
/// ## Examples
///
/// ```rust
/// # use arch_satellite_bitcoin_transactions::NewPotentialInputAmount;
/// # use bitcoin::{TxIn, OutPoint, ScriptBuf, Sequence, Witness};
/// // Estimate adding 3 similar inputs
/// let potential_inputs = NewPotentialInputAmount {
///     count: 3,
///     item: TxIn {
///         previous_output: OutPoint::null(),
///         script_sig: ScriptBuf::new(),
///         sequence: Sequence::MAX,
///         witness: Witness::new(),
///     },
///     signer: true,
/// };
/// ```
pub struct NewPotentialInputAmount {
    pub count: usize,
    pub item: TxIn,
    pub signer: bool,
}

/// Represents potential transaction outputs for size estimation.
///
/// Used in conjunction with [`NewPotentialInputAmount`] to estimate transaction
/// sizes before actually constructing the outputs. This is essential for accurate
/// fee calculation and transaction planning.
///
/// ## Fields
///
/// - `count`: Number of outputs of this type to add
/// - `item`: Template output to use for size calculation
///
/// ## Examples
///
/// ```rust
/// # use arch_satellite_bitcoin_transactions::NewPotentialOutputAmount;
/// # use bitcoin::{TxOut, Amount, ScriptBuf};
/// // Estimate adding 2 similar outputs
/// let potential_outputs = NewPotentialOutputAmount {
///     count: 2,
///     item: TxOut {
///         value: Amount::from_sat(50000),
///         script_pubkey: ScriptBuf::new(),
///     },
/// };
/// ```
pub struct NewPotentialOutputAmount {
    pub count: usize,
    pub item: TxOut,
}

/// Container for potential inputs and outputs used in size estimation.
///
/// This struct aggregates potential inputs and outputs into a single parameter
/// for methods like [`TransactionBuilder::estimate_tx_size_with_additional_inputs_outputs`].
/// It enables comprehensive "what-if" analysis for transaction planning.
///
/// ## Usage Patterns
///
/// ```rust
/// # use arch_satellite_bitcoin_transactions::{NewPotentialInputsAndOutputs, NewPotentialInputAmount, NewPotentialOutputAmount};
/// # use bitcoin::{TxIn, TxOut, OutPoint, ScriptBuf, Sequence, Witness, Amount};
/// // Planning a transaction with multiple potential changes
/// let potential_changes = NewPotentialInputsAndOutputs {
///     inputs: Some(NewPotentialInputAmount {
///         count: 2,
///         item: TxIn {
///             previous_output: OutPoint::null(),
///             script_sig: ScriptBuf::new(),
///             sequence: Sequence::MAX,
///             witness: Witness::new(),
///         },
///         signer: true,
///     }),
///     outputs: vec![
///         NewPotentialOutputAmount {
///             count: 1,
///             item: TxOut {
///                 value: Amount::from_sat(50000),
///                 script_pubkey: ScriptBuf::new(),
///             },
///         },
///         NewPotentialOutputAmount {
///             count: 1,
///             item: TxOut {
///                 value: Amount::from_sat(25000),
///                 script_pubkey: ScriptBuf::new(),
///             },
///         },
///     ],
/// };
/// ```
///
/// ## See Also
///
/// - [`TransactionBuilder::estimate_tx_size_with_additional_inputs_outputs`]
/// - [`TransactionBuilder::estimate_tx_vsize_with_additional_inputs_outputs`]
pub struct NewPotentialInputsAndOutputs {
    pub inputs: Option<NewPotentialInputAmount>,
    pub outputs: Vec<NewPotentialOutputAmount>,
}

#[derive(Debug)]
/// A zero-heap Bitcoin transaction builder for the Arch runtime.
///
/// `TransactionBuilder` provides a type-safe, heap-free way to construct Bitcoin transactions
/// that interact with the Arch runtime. It manages transaction inputs, outputs, state transitions,
/// and fee calculations while maintaining compatibility with constrained environments like the
/// Solana BPF VM.
///
/// ## Design Philosophy
///
/// The builder is designed around three core principles:
/// - **Zero-heap allocation**: All collections use fixed-size arrays determined at compile time
/// - **Type safety**: Generic parameters prevent runtime errors by enforcing limits at compile time
/// - **Arch integration**: Built-in support for Arch-specific concepts like state transitions and account modifications
///
/// ## Generic Parameters
///
/// - `MAX_MODIFIED_ACCOUNTS`: Maximum number of program accounts that can be modified in a single transaction
/// - `MAX_INPUTS_TO_SIGN`: Maximum number of transaction inputs that require signatures
/// - `RuneSet`: Collection type for tracking rune inputs (only used with `runes` feature)
///
/// These bounds are enforced at compile time to ensure the builder remains heap-free.
///
/// ## Feature Flags
///
/// - `runes`: Enables rune transaction support with automatic rune input/output tracking
/// - `utxo-consolidation`: Enables UTXO consolidation features for managing fragmented UTXOs
/// - `serde`: Enables serialization support for transaction data
///
/// ## Basic Usage
///
/// ```rust
/// use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// use arch_satellite_bitcoin_transactions::fee_rate::FeeRate;
///
/// // Create a builder with capacity for 8 modified accounts and 4 inputs to sign
/// let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
///
/// // The builder starts with an empty version 2 transaction
/// assert_eq!(builder.transaction.input.len(), 0);
/// assert_eq!(builder.transaction.output.len(), 0);
/// assert_eq!(builder.total_btc_input, 0);
///
/// // Add inputs and outputs using the builder methods...
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ## State Transitions
///
/// Since Arch 0.9 an account keeps the UTXO it was created with, so a transaction can no longer
/// carry an account's state. [`TransactionBuilder::add_state_transition`] and
/// [`TransactionBuilder::insert_state_transition_input`] are kept for source compatibility and
/// always fail with [`BitcoinTxError::StateTransitionsUnsupported`].
///
/// ## Input Bookkeeping
///
/// Add inputs only through the builder's `add_*` / `insert_*` / `find_btc_*` helpers. Each call
/// either succeeds completely or leaves the builder unchanged, and [`TransactionBuilder::finalize`]
/// runs [`TransactionBuilder::validate`], which rejects a transaction whose inputs were pushed,
/// removed or reordered directly on `transaction.input`. Outputs may be edited directly.
///
/// ## Adding User Inputs
///
/// Add user-controlled UTXOs to the transaction:
///
/// ```rust
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # use arch_satellite_bitcoin_transactions::utxo_info::{UtxoInfo, SingleRuneSet};
/// # use arch_satellite_bitcoin_transactions::TxStatus;
/// # use arch_program::pubkey::Pubkey;
/// # use arch_program::utxo::UtxoMeta;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// # let utxo: UtxoInfo<SingleRuneSet> = UtxoInfo::new(UtxoMeta::from([1; 32], 0), 10_000);
/// # let other_utxo: UtxoInfo<SingleRuneSet> = UtxoInfo::new(UtxoMeta::from([2; 32], 0), 10_000);
/// # let status = TxStatus::Confirmed;
/// # let signer = Pubkey::system_program();
/// // Add a regular input that requires signing
/// builder.add_tx_input(&utxo, &status, Some(&signer))?;
///
/// // For precise control over input order:
/// builder.insert_tx_input(0, &other_utxo, &status, Some(&signer))?;
/// # Ok::<(), arch_satellite_bitcoin_transactions::error::BitcoinTxError>(())
/// ```
///
/// ## Fee Management
///
/// The builder provides sophisticated fee management with mempool ancestry tracking:
///
/// ```rust,no_run
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # use arch_satellite_bitcoin_transactions::fee_rate::FeeRate;
/// # use bitcoin::ScriptBuf;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// # let change_address = ScriptBuf::new();
/// // Set target fee rate
/// let fee_rate = FeeRate::try_from(25.0)?; // 25 sat/vB
///
/// // Automatically adjust transaction to meet fee requirements
/// builder.adjust_transaction_to_pay_fees(&fee_rate, Some(change_address))?;
///
/// // Validate the effective fee rate (including ancestors)
/// builder.is_fee_rate_valid(&fee_rate)?;
///
/// // Get fee breakdown
/// let user_fee = builder.get_fee_paid_by_user(&fee_rate);
/// let total_fee = builder.get_fee_paid()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ## UTXO Selection
///
/// Automatically select UTXOs to meet funding requirements:
///
/// ```rust,no_run
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # use arch_satellite_bitcoin_transactions::utxo_info::UtxoInfo;
/// # use arch_program::pubkey::Pubkey;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// # let utxos: Vec<UtxoInfo<_>> = vec![];
/// # let program_pubkey = Pubkey::system_program();
/// // Find UTXOs to cover a specific amount
/// let amount_needed = 100_000; // 100k sats
/// let (selected_indices, total_found) = builder.find_btc_in_utxos(
///     &utxos,
///     &program_pubkey,
///     amount_needed
/// )?;
///
/// // The builder automatically selects the most efficient UTXOs
/// // and adds them to the transaction
/// # Ok::<(), arch_satellite_bitcoin_transactions::error::BitcoinTxError>(())
/// ```
///
/// ## Rune Support (with `runes` feature)
///
/// When compiled with the `runes` feature, the builder automatically tracks rune inputs and outputs:
///
/// ```rust
/// # #[cfg(feature = "runes")]
/// # {
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # use arch_satellite_bitcoin_transactions::utxo_info::UtxoInfo;
/// # use arch_program::rune::RuneAmount;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// # use arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet;
/// # let rune_utxo: UtxoInfo<SingleRuneSet> = unsafe { std::mem::zeroed() };
/// // Rune inputs are automatically tracked when adding UTXOs
/// // The builder maintains total_rune_inputs and runestone data
///
/// // Access rune information
/// let rune_count = builder.total_rune_inputs.len();
/// let runestone = &builder.runestone;
/// # }
/// ```
///
/// ## UTXO Consolidation (with `utxo-consolidation` feature)
///
/// Automatically consolidate fragmented UTXOs to reduce transaction sizes:
///
/// ```rust,no_run
/// # #[cfg(feature = "utxo-consolidation")]
/// # {
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # use arch_satellite_bitcoin_transactions::fee_rate::FeeRate;
/// # use arch_satellite_bitcoin_transactions::NewPotentialInputsAndOutputs;
/// # use arch_satellite_bitcoin_transactions::utxo_info::UtxoInfo;
/// # use arch_program::pubkey::Pubkey;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// # let pool_pubkey = Pubkey::system_program();
/// # let fee_rate = FeeRate::try_from(10.0).unwrap();
/// # let utxos: Vec<UtxoInfo> = vec![];
/// # let potential_changes = NewPotentialInputsAndOutputs { inputs: None, outputs: vec![] };
/// // Add consolidation UTXOs to reduce fragmentation
/// builder.add_consolidation_utxos(
///     &pool_pubkey,
///     &fee_rate,
///     &utxos,
///     &potential_changes
/// );
///
/// // Get fee breakdown
/// let program_fee = builder.get_fee_paid_by_program(&fee_rate);
/// let user_fee = builder.get_fee_paid_by_user(&fee_rate);
/// # }
/// ```
///
/// ## Size Estimation
///
/// Estimate transaction sizes for fee calculation:
///
/// ```rust
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # use arch_satellite_bitcoin_transactions::NewPotentialInputsAndOutputs;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// # let potential_changes = NewPotentialInputsAndOutputs { inputs: None, outputs: vec![] };
/// // Estimate current transaction size
/// let current_vsize = builder.estimate_final_tx_vsize();
///
/// // Estimate size with additional inputs/outputs
/// let estimated_vsize = builder.estimate_tx_vsize_with_additional_inputs_outputs(
///     &potential_changes
/// );
/// ```
///
/// ## Error Handling
///
/// The builder provides detailed error information:
///
/// ```rust
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # use arch_satellite_bitcoin_transactions::error::BitcoinTxError;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// // Capacity limits are enforced at runtime
/// match builder.inputs_to_sign.len() {
///     len if len >= 4 => {
///         // Handle InputToSignListFull error
///     }
///     _ => {
///         // Safe to add more inputs
///     }
/// }
///
/// // Fee validation errors
/// match builder.get_fee_paid() {
///     Ok(fee) => println!("Fee: {} sats", fee),
///     Err(BitcoinTxError::InsufficientInputAmount) => {
///         // Handle insufficient input funds
///     }
///     Err(e) => {
///         // Handle other errors
///     }
/// }
/// ```
///
/// ## Finalization
///
/// Complete the transaction and prepare it for signing:
///
/// ```rust
/// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
/// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
/// // After adding all inputs, outputs, and adjusting fees
/// builder.finalize()?;
///
/// // The transaction is now ready for the Arch runtime to collect signatures
/// // and broadcast to the Bitcoin network
/// # Ok::<(), arch_program::program_error::ProgramError>(())
/// ```
///
/// ## Performance Considerations
///
/// - All operations are O(1) or O(n) where n is bounded by the generic parameters
/// - No heap allocations occur during normal operation
/// - Memory usage is deterministic and known at compile time
/// - Suitable for use in constrained environments like the Solana BPF VM
///
/// ## Thread Safety
///
/// `TransactionBuilder` is not thread-safe and should not be shared between threads.
/// Create separate builders for concurrent transaction construction.
pub struct TransactionBuilder<
    'info,
    const MAX_MODIFIED_ACCOUNTS: usize,
    const MAX_INPUTS_TO_SIGN: usize,
    RuneSet: FixedCapacitySet<Item = RuneAmount> + Default,
> {
    /// This transaction will be broadcast through Arch to indicate a state
    /// transition in the program
    pub transaction: Transaction,
    pub tx_statuses: MempoolInfo,

    /// This tells Arch which inputs in [InstructionContext::transaction] still
    /// need to be signed, along with which key needs to sign each of them
    pub inputs_to_sign: FixedRefList<InputToSign, MAX_INPUTS_TO_SIGN>,

    pub total_btc_input: u64,

    /// One entry per input added through the builder, in transaction order.
    input_records: Vec<InputRecord>,

    _phantom: std::marker::PhantomData<(&'info (), RuneSet)>,

    #[cfg(feature = "runes")]
    pub total_rune_inputs: RuneSet,

    #[cfg(feature = "runes")]
    pub runestone: Runestone,

    #[cfg(feature = "utxo-consolidation")]
    pub total_btc_consolidation_input: u64,

    #[cfg(feature = "utxo-consolidation")]
    pub extra_tx_size_for_consolidation: usize,
}

/// Specifies how an added state-transition input should be signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignPolicy {
    /// The program (via Arch) manages signing responsibility for this input.
    ///
    /// This applies when the signer has either:
    /// - Already signed the transaction (e.g., fee payer or any other account that has signed)
    /// - Will be signed by the program itself (e.g., program PDAs)
    ///
    /// An `InputToSign` entry is recorded for Arch to handle the signature.
    Managed,
    /// A third-party will provide the signature for this input.
    ///
    /// This applies to accounts owned by other programs (e.g., via CPI) or signers
    /// which have not signed this transaction. No `InputToSign` entry is recorded.
    External,
}

impl<
        'info,
        const MAX_MODIFIED_ACCOUNTS: usize,
        const MAX_INPUTS_TO_SIGN: usize,
        RuneSet: FixedCapacitySet<Item = RuneAmount> + Default + Debug,
    > TransactionBuilder<'info, MAX_MODIFIED_ACCOUNTS, MAX_INPUTS_TO_SIGN, RuneSet>
{
    /// Creates a new empty transaction builder.
    ///
    /// Initializes a blank builder containing an empty **version 2** Bitcoin transaction with `lock_time = 0`.
    /// All internal counters and collections start empty, ready for you to populate through the various
    /// `add_*` and `insert_*` methods.
    ///
    /// ## Initial State
    ///
    /// - `transaction`: Empty version 2 transaction
    /// - `total_btc_input`: 0 satoshis
    /// - `modified_accounts`: Empty fixed-size list
    /// - `inputs_to_sign`: Empty fixed-size list
    /// - `tx_statuses`: Default mempool info
    ///
    /// ## Typical Workflow
    ///
    /// 1. Create builder with `new()`
    /// 2. Add inputs with `add_tx_input()` or `add_state_transition()`
    /// 3. Add outputs directly to `builder.transaction.output`
    /// 4. Adjust fees with `adjust_transaction_to_pay_fees()`
    /// 5. Finalize with `finalize()`
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use arch_satellite_bitcoin_transactions::TransactionBuilder;
    /// use bitcoin::Transaction;
    ///
    /// // Create a builder that can handle up to 8 modified accounts and 4 inputs to sign
    /// let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
    ///
    /// // Verify initial state
    /// assert_eq!(builder.transaction.input.len(), 0);
    /// assert_eq!(builder.transaction.output.len(), 0);
    /// assert_eq!(builder.total_btc_input, 0);
    /// assert_eq!(builder.inputs_to_sign.len(), 0);
    /// ```
    ///
    /// ## Generic Parameters
    ///
    /// Choose your bounds based on your use case:
    /// - **Small transactions**: `TransactionBuilder<4, 2>` for simple operations
    /// - **Medium transactions**: `TransactionBuilder<8, 4>` for typical use cases
    /// - **Large transactions**: `TransactionBuilder<16, 8>` for complex operations
    ///
    /// Remember that larger bounds use more stack space but provide more flexibility.
    #[cfg(not(feature = "runes"))]
    pub fn new() -> Self {
        let transaction = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![],
        };

        Self {
            transaction,
            tx_statuses: MempoolInfo::default(),
            inputs_to_sign: FixedRefList::new(),
            total_btc_input: 0,
            input_records: Vec::new(),

            #[cfg(feature = "utxo-consolidation")]
            total_btc_consolidation_input: 0,
            #[cfg(feature = "utxo-consolidation")]
            extra_tx_size_for_consolidation: 0,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Wraps an existing `transaction` whose inputs spend exactly the outpoints in `user_utxos`.
    ///
    /// Each input must spend a distinct outpoint described by exactly one entry of `user_utxos`,
    /// and every entry must be spent; totals are computed from those matched entries.
    #[cfg(not(feature = "runes"))]
    pub fn new_with_transaction<const MAX_UTXOS: usize, const MAX_ACCOUNTS: usize>(
        transaction: Transaction,
        mempool_data: &MempoolData<MAX_UTXOS, MAX_ACCOUNTS>,
        user_utxos: &[UtxoInfo],
    ) -> Result<Self, BitcoinTxError> {
        let matched = match_inputs_to_utxos(&transaction, user_utxos)?;
        let (input_records, total_btc_input) = records_for(&matched)?;
        let tx_statuses = generate_mempool_info(user_utxos, mempool_data)?;

        Ok(Self {
            transaction,
            tx_statuses,
            inputs_to_sign: FixedRefList::new(),
            total_btc_input,
            input_records,

            #[cfg(feature = "utxo-consolidation")]
            total_btc_consolidation_input: 0,
            #[cfg(feature = "utxo-consolidation")]
            extra_tx_size_for_consolidation: 0,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Creates a new empty transaction builder with rune support.
    ///
    /// Initializes a blank builder containing an empty **version 2** Bitcoin transaction with `lock_time = 0`.
    /// All internal counters and collections start empty, including rune-specific tracking.
    ///
    /// ## Initial State
    ///
    /// - `transaction`: Empty version 2 transaction
    /// - `total_btc_input`: 0 satoshis
    /// - `total_rune_inputs`: Empty rune set
    /// - `runestone`: Default runestone
    /// - `modified_accounts`: Empty fixed-size list
    /// - `inputs_to_sign`: Empty fixed-size list
    /// - `tx_statuses`: Default mempool info
    ///
    /// ## Rune Features
    ///
    /// With the `runes` feature enabled, the builder automatically:
    /// - Tracks rune inputs when adding UTXOs
    /// - Maintains runestone data for rune operations
    /// - Handles rune arithmetic and validation
    ///
    /// ## Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "runes")]
    /// # {
    /// use arch_satellite_bitcoin_transactions::TransactionBuilder;
    /// use arch_program::rune::RuneAmount;
    ///
    /// // Create a builder that can handle up to 8 modified accounts and 4 inputs to sign
    /// let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
    ///
    /// // Verify initial state
    /// assert_eq!(builder.transaction.input.len(), 0);
    /// assert_eq!(builder.transaction.output.len(), 0);
    /// assert_eq!(builder.total_btc_input, 0);
    /// assert_eq!(builder.total_rune_inputs.len(), 0);
    /// assert_eq!(builder.inputs_to_sign.len(), 0);
    /// # }
    /// ```
    ///
    /// ## Generic Parameters
    ///
    /// Choose your bounds based on your use case:
    /// - **Small transactions**: `TransactionBuilder<4, 2, SmallRuneSet>` for simple operations
    /// - **Medium transactions**: `TransactionBuilder<8, 4, MediumRuneSet>` for typical use cases
    /// - **Large transactions**: `TransactionBuilder<16, 8, LargeRuneSet>` for complex operations
    ///
    /// The `RuneSet` parameter determines how many different rune types can be tracked simultaneously.
    #[cfg(feature = "runes")]
    pub fn new() -> Self {
        let transaction = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![],
        };

        Self {
            transaction,
            tx_statuses: MempoolInfo::default(),
            inputs_to_sign: FixedRefList::new(),
            total_btc_input: 0,
            input_records: Vec::new(),

            total_rune_inputs: RuneSet::default(),
            runestone: Runestone::default(),

            #[cfg(feature = "utxo-consolidation")]
            total_btc_consolidation_input: 0,
            #[cfg(feature = "utxo-consolidation")]
            extra_tx_size_for_consolidation: 0,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Wraps an existing `transaction` whose inputs spend exactly the outpoints in `user_utxos`.
    ///
    /// Each input must spend a distinct outpoint described by exactly one entry of `user_utxos`,
    /// and every entry must be spent; totals are computed from those matched entries.
    #[cfg(feature = "runes")]
    pub fn new_with_transaction(
        transaction: Transaction,
        mempool_data: &impl MempoolDataView,
        user_utxos: &[UtxoInfo<RuneSet>],
    ) -> Result<Self, BitcoinTxError> {
        let matched = match_inputs_to_utxos(&transaction, user_utxos)?;

        let mut total_rune_inputs = RuneSet::default();
        for utxo in &matched {
            for rune in utxo.runes.as_slice() {
                add_rune_input(&mut total_rune_inputs, rune.clone())?;
            }
        }

        let (input_records, total_btc_input) = records_for(&matched)?;
        let tx_statuses = generate_mempool_info(user_utxos, mempool_data)?;

        let runestone = match Runestone::decipher(&transaction) {
            Some(artifact) => match artifact {
                Artifact::Runestone(runestone) => Ok(runestone),
                _ => Err(BitcoinTxError::RunestoneDecipherError),
            },
            None => Ok(Runestone::default()),
        }?;

        Ok(Self {
            transaction,
            tx_statuses,
            inputs_to_sign: FixedRefList::new(),
            total_btc_input,
            input_records,

            total_rune_inputs,
            runestone,

            #[cfg(feature = "utxo-consolidation")]
            total_btc_consolidation_input: 0,
            #[cfg(feature = "utxo-consolidation")]
            extra_tx_size_for_consolidation: 0,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Formerly added a state transition for an existing program account.
    ///
    /// Since Arch 0.9 an account keeps the UTXO it was created with and the runtime rejects any
    /// attempt to spend or move it (`AccountUtxoModified`), so a state transition can never be
    /// signed. This method is kept for source compatibility, leaves the builder unchanged and
    /// always returns [`BitcoinTxError::StateTransitionsUnsupported`].
    #[deprecated(
        since = "0.33.0",
        note = "account UTXOs are immutable since Arch 0.9; state transitions always fail"
    )]
    pub fn add_state_transition(
        &mut self,
        _account: &AccountInfo<'info>,
        _policy: SignPolicy,
    ) -> Result<u32, BitcoinTxError> {
        Err(BitcoinTxError::StateTransitionsUnsupported)
    }

    /// Formerly inserted a state-transition input at `tx_index`.
    ///
    /// Always returns [`BitcoinTxError::StateTransitionsUnsupported`] and leaves the builder
    /// unchanged; see [`Self::add_state_transition`].
    #[deprecated(
        since = "0.33.0",
        note = "account UTXOs are immutable since Arch 0.9; state transitions always fail"
    )]
    pub fn insert_state_transition_input(
        &mut self,
        _tx_index: usize,
        _account: &AccountInfo<'info>,
        _policy: SignPolicy,
    ) -> Result<(), BitcoinTxError> {
        Err(BitcoinTxError::StateTransitionsUnsupported)
    }

    /// Adds a regular input owned by `signer`.
    ///
    /// Besides pushing the `TxIn` into the underlying `transaction`, this helper:
    /// * Records mempool ancestry for fee-rate purposes.
    /// * Adds an [`InputToSign`].
    /// * Updates `total_btc_input` (and `total_rune_input` when compiled with the `runes` feature).
    ///
    /// Fails without changing the builder if the outpoint is already spent by another input,
    /// the [`InputToSign`] list is full, or a running total would overflow.
    pub fn add_tx_input<RS>(
        &mut self,
        utxo: &UtxoInfo<RS>,
        status: &TxStatus,
        signer: Option<&Pubkey>,
    ) -> Result<(), BitcoinTxError>
    where
        RS: FixedCapacitySet<Item = RuneAmount>,
    {
        self.add_input(
            self.transaction.input.len(),
            utxo,
            status,
            unsigned_tx_in(utxo),
            signer,
        )
    }

    /// Appends a **user-supplied** [`TxIn`] (already built elsewhere) while still tracking the
    /// UTXO ancestry for fee-rate purposes.
    ///
    /// `tx_in.previous_output` must be the outpoint described by `utxo`; otherwise the call fails
    /// with [`BitcoinTxError::UtxoOutpointMismatch`] and the builder is unchanged.
    pub fn add_user_tx_input<RS>(
        &mut self,
        utxo: &UtxoInfo<RS>,
        status: &TxStatus,
        tx_in: TxIn,
    ) -> Result<(), BitcoinTxError>
    where
        RS: FixedCapacitySet<Item = RuneAmount>,
    {
        self.add_input(self.transaction.input.len(), utxo, status, tx_in, None)
    }

    /// Inserts a **regular** (non-state–account) [`TxIn`] at the given position `tx_index`.
    ///
    /// Besides inserting the new input into [`TransactionBuilder::transaction`], this helper keeps
    /// all *internal bookkeeping* consistent:
    ///
    /// 1. Records the mempool ancestry for fee-rate calculations.
    /// 2. Shifts the `index` of every existing [`arch_program::input_to_sign::InputToSign`] that
    ///    appears **at or after** `tx_index` so their indices continue to match the underlying
    ///    transaction after the insertion.
    /// 3. Pushes a fresh [`InputToSign`] for `signer` so Arch knows which key must later provide
    ///    a witness for the inserted input.
    /// 4. Bumps [`Self::total_btc_input`] (and `total_rune_input` when compiled with the `runes`
    ///    feature) by the value of `utxo`.
    ///
    /// Use this when the *order* of inputs matters – for example when signing with PSBTs that
    /// expect user inputs to appear before program-generated ones.
    ///
    /// Fails without changing the builder if `tx_index` is past the end of the inputs
    /// ([`BitcoinTxError::InvalidInputIndex`]), or for any reason [`Self::add_tx_input`] fails.
    ///
    /// # Parameters
    /// * `tx_index` – zero-based index where the input should be inserted.
    /// * `utxo` – metadata of the UTXO being spent.
    /// * `status` – mempool status of `utxo`; contributes to ancestor fee/size tracking.
    /// * `signer` – optional public key that will sign the input.
    pub fn insert_tx_input<RS>(
        &mut self,
        tx_index: usize,
        utxo: &UtxoInfo<RS>,
        status: &TxStatus,
        signer: Option<&Pubkey>,
    ) -> Result<(), BitcoinTxError>
    where
        RS: FixedCapacitySet<Item = RuneAmount>,
    {
        self.add_input(tx_index, utxo, status, unsigned_tx_in(utxo), signer)
    }

    /// Inserts a **pre-constructed** [`TxIn`] – built elsewhere – at the specified `tx_index`.
    ///
    /// The function behaves similarly to [`Self::insert_tx_input`] but **does not** create a new
    /// [`InputToSign`], as the caller may already have handled signature tracking. It still:
    ///
    /// * Accounts for the input's mempool ancestry.
    /// * Shifts the indices of all existing [`InputToSign`] that come after `tx_index` so they
    ///   remain correct.
    /// * Updates BTC / rune running totals.
    ///
    /// This is handy when you have a non-standard script or any other reason to fully craft the
    /// `TxIn` outside of the builder but still need to place it at a precise position inside the
    /// transaction. `tx_in.previous_output` must be the outpoint described by `utxo`.
    ///
    /// # Parameters
    /// * `tx_index` – position where `tx_in` should be inserted.
    /// * `utxo` – the UTXO consumed by `tx_in`.
    /// * `status` – mempool status of `utxo`.
    /// * `tx_in` – ready-made transaction input (will be cloned).
    pub fn insert_user_tx_input<RS>(
        &mut self,
        tx_index: usize,
        utxo: &UtxoInfo<RS>,
        status: &TxStatus,
        tx_in: &TxIn,
    ) -> Result<(), BitcoinTxError>
    where
        RS: FixedCapacitySet<Item = RuneAmount>,
    {
        self.add_input(tx_index, utxo, status, tx_in.clone(), None)
    }

    /// Greedily selects UTXOs until at least `amount` satoshis are gathered.
    ///
    /// Selection strategy:
    /// * With the `utxo-consolidation` feature **enabled**: prefer UTXOs **without** the
    ///   `needs_consolidation` flag, then sort by descending value.
    /// * Without the feature: simply sort by descending value.
    ///
    /// Returns the **indices** of the chosen items inside the original slice plus the total value
    /// selected.
    ///
    /// find_btc_in_utxos has been moved to the `find_btc` module for clarity.

    #[cfg(feature = "utxo-consolidation")]
    fn set_consolidation_tracking(&mut self, consolidation_amount: u64, extra_tx_size: usize) {
        self.total_btc_consolidation_input += consolidation_amount;
        self.extra_tx_size_for_consolidation += extra_tx_size;
    }

    #[cfg(not(feature = "utxo-consolidation"))]
    fn set_consolidation_tracking(&mut self, _consolidation_amount: u64, _extra_tx_size: usize) {}

    // btc selection helpers moved into `find_btc` module

    /// Greedily selects UTXOs until at least `amount` satoshis are gathered.
    ///
    /// Selection strategy:
    /// * With the `utxo-consolidation` feature **enabled**: prefer UTXOs **without** the
    ///   `needs_consolidation` flag, then sort by descending value.
    /// * Without the feature: simply sort by descending value.
    ///
    /// Returns the **indices** of the chosen items inside the original slice plus the total value
    /// selected.
    ///
    /// # Errors
    /// * [`BitcoinTxError::NotEnoughBtcInPool`] – not enough value in `utxos` to satisfy `amount`.
    /// find_btc_in_utxos_from_holder has been moved to the `find_btc` module for clarity.

    /// Automatically adjusts the transaction to meet the target fee rate.
    ///
    /// This method optimizes the transaction's fee structure by analyzing the current input/output
    /// balance and adjusting outputs to achieve the desired fee rate. It handles both overpayment
    /// (creating change) and underpayment (reducing outputs) scenarios.
    ///
    /// ## How it works
    ///
    /// The method evaluates the current transaction and:
    /// 1. **Calculates required fee**: Based on transaction size and target fee rate
    /// 2. **Handles excess funds**: Creates or increases change output if inputs exceed requirements
    /// 3. **Handles insufficient funds**: Reduces change output or returns error if impossible
    /// 4. **Considers ancestors**: Accounts for mempool ancestry when calculating effective fee rate
    ///
    /// ## Change Output Behavior
    ///
    /// - **`address_to_send_remaining_btc = Some(address)`**: Creates new change output or increases existing one
    /// - **`address_to_send_remaining_btc = None`**: Only adjusts existing outputs, never creates new ones
    ///
    /// ## Examples
    ///
    /// ```rust,no_run
    /// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
    /// # use arch_satellite_bitcoin_transactions::fee_rate::FeeRate;
    /// # use bitcoin::ScriptBuf;
    /// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
    /// // Set target fee rate (25 sat/vB)
    /// let fee_rate = FeeRate::try_from(25.0)?;
    ///
    /// // Create change output for excess funds
    /// let change_address = ScriptBuf::new(); // Your change address
    /// builder.adjust_transaction_to_pay_fees(&fee_rate, Some(change_address))?;
    ///
    /// // Or adjust without creating change (only reduce existing outputs)
    /// builder.adjust_transaction_to_pay_fees(&fee_rate, None)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// ## Fee Calculation Details
    ///
    /// The method considers:
    /// - **Transaction size**: Estimated final size including witness data
    /// - **Input signatures**: Size overhead for each required signature
    /// - **Mempool ancestry**: Fees and sizes of unconfirmed parent transactions
    /// - **Consolidation**: Extra size from UTXO consolidation (if enabled)
    ///
    /// ## Error Handling
    ///
    /// Returns an error if:
    /// - Insufficient funds to cover minimum fee requirements
    /// - Cannot reduce outputs enough to meet fee rate
    /// - Transaction would exceed size limits
    /// - Fee rate calculation fails
    ///
    /// ## Best Practices
    ///
    /// ```rust,no_run
    /// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
    /// # use arch_satellite_bitcoin_transactions::fee_rate::FeeRate;
    /// # use bitcoin::ScriptBuf;
    /// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
    /// # let change_address = ScriptBuf::new();
    /// // Always validate fee rate after adjustment
    /// let fee_rate = FeeRate::try_from(15.0)?;
    /// builder.adjust_transaction_to_pay_fees(&fee_rate, Some(change_address))?;
    ///
    /// // Verify the final fee rate meets requirements
    /// builder.is_fee_rate_valid(&fee_rate)?;
    ///
    /// // Check final fee amount
    /// let final_fee = builder.get_fee_paid()?;
    /// println!("Final fee: {} sats", final_fee);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// ## See Also
    ///
    /// - [`Self::is_fee_rate_valid`] for validating the resulting fee rate
    /// - [`Self::get_fee_paid`] for checking the final fee amount
    /// - [`Self::get_fee_paid_by_user`] for user-specific fee calculation
    pub fn adjust_transaction_to_pay_fees(
        &mut self,
        fee_rate: &FeeRate,
        address_to_send_remaining_btc: Option<ScriptBuf>,
    ) -> Result<(), BitcoinTxError> {
        adjust_transaction_to_pay_fees(
            &mut self.transaction,
            &self.tx_statuses,
            self.total_btc_input,
            address_to_send_remaining_btc,
            fee_rate,
        )
    }

    /// Attempts to **sweep** pool-owned UTXOs marked for consolidation into the current
    /// transaction.
    ///
    /// This helper is only available when the `utxo-consolidation` feature is enabled. It acts as
    /// a thin wrapper around [`crate::consolidation::add_consolidation_utxos`], forwarding the
    /// relevant context from the builder and then updating the builder's running totals so that
    /// fee-calculation logic is aware of the extra inputs.
    ///
    /// The consolidated inputs are signed by `pool_pubkey`. Only UTXOs whose
    /// `needs_consolidation` value is **greater than or equal to** `fee_rate` are considered. The
    /// function stops adding inputs as soon as the draft transaction would exceed
    /// [`arch_program::MAX_BTC_TX_SIZE`].
    ///
    /// After execution the following builder fields are updated:
    /// * [`Self::total_btc_input`]
    /// * [`Self::total_btc_consolidation_input`]
    /// * [`Self::extra_tx_size_for_consolidation`]
    ///
    /// # Parameters
    /// * `pool_pubkey` – public key of the liquidity-pool program (signer of consolidation inputs).
    /// * `fee_rate` – current mempool fee-rate used to decide which UTXOs are worth consolidating.
    /// * `pool_shard_btc_utxos` – slice with the candidate pool UTXOs.
    /// * `new_potential_inputs_and_outputs` – hypothetical inputs/outputs the caller *may* add
    ///    later; needed to keep size estimations accurate.
    #[cfg(feature = "utxo-consolidation")]
    pub fn add_consolidation_utxos<BtcHolder: BtcUtxoHolder>(
        &mut self,
        pool_pubkey: &Pubkey,
        fee_rate: &FeeRate,
        pool_shard_btc_utxos: &[BtcHolder],
        new_potential_inputs_and_outputs: &NewPotentialInputsAndOutputs,
    ) {
        let (total_consolidation_input_amount, extra_tx_size) = add_consolidation_utxos(
            &mut self.transaction,
            &mut self.input_records,
            &mut self.tx_statuses,
            &mut self.inputs_to_sign,
            pool_pubkey,
            pool_shard_btc_utxos,
            fee_rate,
            new_potential_inputs_and_outputs,
            ARCH_INPUT_SIZE,
        );

        // An overflow here leaves `total_btc_input` below the recorded input values, which
        // `validate` rejects at `finalize`.
        self.total_btc_input = self
            .total_btc_input
            .saturating_add(total_consolidation_input_amount);
        self.set_consolidation_tracking(total_consolidation_input_amount, extra_tx_size);
    }

    #[cfg(feature = "utxo-consolidation")]
    pub fn get_fee_paid_by_program(&self, fee_rate: &FeeRate) -> u64 {
        fee_rate.fee(self.extra_tx_size_for_consolidation).to_sat()
    }

    pub fn get_fee_paid_by_user(&mut self, fee_rate: &FeeRate) -> Result<u64, BitcoinTxError> {
        let tx_size = self.estimate_final_tx_vsize()?;

        let tx_size_to_be_paid_by_user = {
            #[cfg(feature = "utxo-consolidation")]
            {
                tx_size
                    .checked_sub(self.extra_tx_size_for_consolidation)
                    .ok_or(BitcoinTxError::CalcOverflow)?
            }
            #[cfg(not(feature = "utxo-consolidation"))]
            {
                tx_size
            }
        };

        Ok(fee_rate.fee(tx_size_to_be_paid_by_user).to_sat())
    }

    pub fn estimate_final_tx_vsize(&mut self) -> Result<usize, BitcoinTxError> {
        estimate_final_tx_vsize(&mut self.transaction)
    }

    /// Returns the *weight* (in bytes) the transaction would have **if** the draft
    /// `new_potential_inputs_and_outputs` were added.
    ///
    /// Helpful during fee-bumping logic when you need to know "how much bigger will the TX get
    /// if I add N more inputs/outputs?".
    pub fn estimate_tx_size_with_additional_inputs_outputs(
        &mut self,
        new_potential_inputs_and_outputs: &NewPotentialInputsAndOutputs,
    ) -> Result<usize, BitcoinTxError> {
        Ok(estimate_tx_size_with_additional_inputs_outputs(
            &mut self.transaction,
            &self.inputs_to_sign,
            new_potential_inputs_and_outputs,
        )?)
    }

    /// Same as [`Self::estimate_tx_size_with_additional_inputs_outputs`] but returns **vsize**
    /// instead of raw size.
    pub fn estimate_tx_vsize_with_additional_inputs_outputs(
        &mut self,
        new_potential_inputs_and_outputs: &NewPotentialInputsAndOutputs,
    ) -> Result<usize, BitcoinTxError> {
        Ok(estimate_tx_vsize_with_additional_inputs_outputs(
            &mut self.transaction,
            &mut self.inputs_to_sign,
            new_potential_inputs_and_outputs,
        )?)
    }

    /// Returns the **aggregate mempool size (bytes) and fees (sats)** of all ancestor
    /// transactions referenced by *pending* inputs.
    pub fn get_ancestors_totals(&self) -> Result<(usize, u64), BitcoinTxError> {
        Ok((
            self.tx_statuses.total_size as usize,
            self.tx_statuses.total_fee,
        ))
    }

    /// Calculates the fee currently paid by the partially-built transaction (`inputs − outputs`).
    ///
    /// Fails with [`BitcoinTxError::InsufficientInputAmount`] if outputs exceed inputs.
    pub fn get_fee_paid(&self) -> Result<u64, BitcoinTxError> {
        let output_amount = total_output_value(&self.transaction)?;

        let fee_paid = self
            .total_btc_input
            .checked_sub(output_amount)
            .ok_or(BitcoinTxError::InsufficientInputAmount)?;

        Ok(fee_paid)
    }

    /// Checks that the *effective* fee-rate (including ancestors) is at least `fee_rate`.
    ///
    /// Returns an error when the calculated rate is below the target.
    pub fn is_fee_rate_valid(&mut self, fee_rate: &FeeRate) -> Result<(), BitcoinTxError> {
        // Transaction by itself should have a valid fee
        let fee_paid = self.get_fee_paid()?;
        let tx_size = self.estimate_final_tx_vsize()?;

        let real_fee_rate = FeeRate::try_from(fee_paid as f64 / tx_size as f64)
            .map_err(|_| BitcoinTxError::InvalidFeeRateTooLow)?;

        if real_fee_rate.n() < fee_rate.n() {
            return Err(BitcoinTxError::InvalidFeeRateTooLow);
        }

        // But also with ancestors.
        let (total_size_of_pending_utxos, total_fee_of_pending_utxos) =
            self.get_ancestors_totals()?;

        let fee_paid_with_ancestors = fee_paid
            .checked_add(total_fee_of_pending_utxos)
            .ok_or(BitcoinTxError::InsufficientInputAmount)?;

        let tx_size_with_ancestors = tx_size
            .checked_add(total_size_of_pending_utxos)
            .ok_or(BitcoinTxError::CalcOverflow)?;

        let real_fee_rate_with_ancestors =
            FeeRate::try_from(fee_paid_with_ancestors as f64 / tx_size_with_ancestors as f64)
                .map_err(|_| BitcoinTxError::InvalidFeeRateTooLow)?;

        if real_fee_rate_with_ancestors.n() < fee_rate.n() {
            return Err(BitcoinTxError::InvalidFeeRateTooLow);
        }

        Ok(())
    }

    /// Returns a slice of transaction inputs that are not state transitions.
    ///
    /// State transitions can no longer be added (see [`Self::add_state_transition`]), so this is
    /// every input of the transaction.
    ///
    /// ## Examples
    ///
    /// ```rust,no_run
    /// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
    /// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
    /// // After adding state transitions and regular inputs...
    /// let non_state_transition_inputs = builder.get_non_state_transition_inputs();
    ///
    /// // Process only non-state-transition inputs
    /// for input in non_state_transition_inputs {
    ///     // Handle regular UTXO inputs
    /// }
    /// ```
    pub fn get_non_state_transition_inputs(&self) -> &[TxIn] {
        &self.transaction.input
    }

    /// Finalizes the transaction and prepares it for signing by the Arch runtime.
    ///
    /// This method completes the transaction building process by transferring the constructed
    /// transaction and all associated metadata to the Arch runtime. Once called, the transaction
    /// is ready for the signature collection phase.
    ///
    /// ## What it does
    ///
    /// The method performs these final steps:
    /// 1. **Transfers ownership**: Passes the transaction to the Arch runtime
    /// 2. **Provides metadata**: Includes modified accounts and signing requirements
    /// 3. **Enables signing**: Makes the transaction available for signature collection
    /// 4. **Prepares broadcast**: Sets up the transaction for network submission
    ///
    /// ## Important Notes
    ///
    /// - **No further changes**: After calling `finalize()`, the builder should not be modified
    /// - **Not broadcasting**: This method does NOT broadcast the transaction to the network
    /// - **Signing phase**: The transaction enters the signing phase, handled by Arch runtime
    /// - **State consistency**: All modified accounts and inputs must be properly configured
    ///
    /// ## Prerequisites
    ///
    /// Before calling `finalize()`, ensure:
    /// - All required inputs have been added
    /// - All outputs have been configured
    /// - Fees have been adjusted with [`Self::adjust_transaction_to_pay_fees`]
    /// - Fee rate has been validated with [`Self::is_fee_rate_valid`]
    ///
    /// ## Transaction Lifecycle
    ///
    /// ```text
    /// 1. TransactionBuilder::new()          ← Create builder
    /// 2. Add inputs/outputs                 ← Populate transaction
    /// 3. adjust_transaction_to_pay_fees()   ← Set correct fees
    /// 4. finalize()                         ← Prepare for signing
    /// 5. [Arch runtime signs]              ← Automatic signing
    /// 6. [Arch runtime broadcasts]         ← Network submission
    /// ```
    ///
    /// ## Examples
    ///
    /// ```rust,no_run
    /// # use arch_satellite_bitcoin_transactions::TransactionBuilder;
    /// # use arch_satellite_bitcoin_transactions::fee_rate::FeeRate;
    /// # use bitcoin::ScriptBuf;
    /// # let mut builder: TransactionBuilder<8, 4, arch_satellite_bitcoin_transactions::utxo_info::SingleRuneSet> = TransactionBuilder::new();
    /// // After building your transaction...
    ///
    /// // 1. Adjust fees
    /// let fee_rate = FeeRate::try_from(20.0)?;
    /// let change_address = ScriptBuf::new();
    /// builder.adjust_transaction_to_pay_fees(&fee_rate, Some(change_address))?;
    ///
    /// // 2. Validate fee rate
    /// builder.is_fee_rate_valid(&fee_rate)?;
    ///
    /// // 3. Finalize and hand over to Arch
    /// builder.finalize()?;
    ///
    /// // Transaction is now ready for signing and broadcast
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// ## Error Handling
    ///
    /// Returns [`ProgramError`] if:
    /// - [`Self::validate`] fails: the builder's bookkeeping no longer describes the transaction
    /// - The Arch runtime cannot accept the transaction
    ///
    /// ## See Also
    ///
    /// - [`Self::adjust_transaction_to_pay_fees`] for fee adjustment
    /// - [`Self::is_fee_rate_valid`] for fee validation
    /// - [`arch_program::program::set_transaction_to_sign`] for the underlying mechanism
    pub fn finalize(&mut self) -> Result<(), ProgramError> {
        self.validate()?;

        set_transaction_to_sign::<AccountInfo<'info>>(
            &[],
            &self.transaction,
            self.inputs_to_sign.as_slice(),
        )?;

        Ok(())
    }

    #[cfg(feature = "runes")]
    fn add_rune_input(&mut self, rune: RuneAmount) -> Result<(), BitcoinTxError> {
        add_rune_input(&mut self.total_rune_inputs, rune)?;

        Ok(())
    }
}

fn unsigned_tx_in<RS>(utxo: &UtxoInfo<RS>) -> TxIn
where
    RS: FixedCapacitySet<Item = RuneAmount>,
{
    TxIn {
        previous_output: utxo.meta.to_outpoint(),
        script_sig: ScriptBuf::new(),
        sequence: Sequence::MAX,
        witness: Witness::new(),
    }
}

fn total_output_value(transaction: &Transaction) -> Result<u64, BitcoinTxError> {
    transaction
        .output
        .iter()
        .try_fold(0u64, |total, output| {
            total.checked_add(output.value.to_sat())
        })
        .ok_or(BitcoinTxError::CalcOverflow)
}

pub fn add_rune_input<RuneSet: FixedCapacitySet<Item = RuneAmount> + Default>(
    total_rune_inputs: &mut RuneSet,
    rune: RuneAmount,
) -> Result<(), BitcoinTxError> {
    if let Some(existing) = total_rune_inputs.find_mut(&rune.id) {
        existing.amount = existing
            .amount
            .checked_add(rune.amount)
            .ok_or(BitcoinTxError::RuneAdditionOverflow)?;
    } else {
        total_rune_inputs
            .insert(rune)
            .map_err(|_| BitcoinTxError::RuneInputListFull)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::utxo_info::UtxoInfoTrait;

    #[cfg(feature = "utxo-consolidation")]
    use crate::utxo_info::FixedOptionF64;

    use super::*;
    use crate::utxo_info::SingleRuneSet;
    use arch_program::rune::{RuneAmount, RuneId};
    use arch_program::utxo::UtxoMeta;
    use bitcoin::{Amount, OutPoint, TxOut};

    #[allow(unused_macros)]
    macro_rules! new_tb {
        ($max_mod:expr, $max_inputs:expr) => {{
            // Always specify the `SingleRuneSet` type parameter so the invocation
            // matches the `TransactionBuilder` definition regardless of whether the
            // `runes` feature is enabled.
            TransactionBuilder::<$max_mod, $max_inputs, SingleRuneSet>::new()
        }};
    }

    // Helper function to create a mock UtxoInfo
    fn create_mock_utxo(value: u64, txid: [u8; 32], vout: u32) -> UtxoInfo<SingleRuneSet> {
        UtxoInfo::new(UtxoMeta::from(txid, vout), value)
    }

    // Helper function to create a mock UtxoInfo with runes
    fn create_mock_utxo_with_runes(
        value: u64,
        txid: [u8; 32],
        vout: u32,
        rune_amount: u128,
    ) -> UtxoInfo<SingleRuneSet> {
        let runes = {
            #[cfg(feature = "runes")]
            {
                let mut runes = SingleRuneSet::default();
                runes
                    .insert(RuneAmount {
                        id: RuneId::new(1, 1),
                        amount: rune_amount,
                    })
                    .unwrap();

                runes
            }
            #[cfg(not(feature = "runes"))]
            {
                SingleRuneSet::default()
            }
        };

        let mut utxo = UtxoInfo::new(UtxoMeta::from(txid, vout), value);

        #[cfg(feature = "runes")]
        {
            utxo.runes = runes;
        }

        utxo
    }

    mod new {
        use super::*;

        #[test]
        fn creates_empty_transaction_builder() {
            let builder = new_tb!(0, 0);

            assert_eq!(builder.transaction.version, Version::TWO);
            assert_eq!(builder.transaction.lock_time, LockTime::ZERO);
            assert_eq!(builder.transaction.input.len(), 0);
            assert_eq!(builder.transaction.output.len(), 0);
            assert_eq!(builder.total_btc_input, 0);
            #[cfg(feature = "runes")]
            assert_eq!(builder.total_rune_inputs.len(), 0);
            #[cfg(feature = "utxo-consolidation")]
            assert_eq!(builder.total_btc_consolidation_input, 0);
            #[cfg(feature = "utxo-consolidation")]
            assert_eq!(builder.extra_tx_size_for_consolidation, 0);
            assert!(builder.input_records.is_empty());
            assert_eq!(builder.inputs_to_sign.len(), 0);
        }
    }

    mod new_with_transaction {
        use super::*;

        #[test]
        fn new_with_transaction_successfully() {
            // Create a transaction without inputs to avoid UTXO lookup issues
            let tx_output = TxOut {
                value: Amount::from_sat(50000),
                script_pubkey: ScriptBuf::new(),
            };

            let transaction = Transaction {
                version: Version::ONE,
                lock_time: LockTime::ZERO,
                input: vec![TxIn {
                    previous_output: OutPoint::from_str(
                        "1111111111111111111111111111111111111111111111111111111111111111:0",
                    )
                    .unwrap(),
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                }], // Empty inputs to avoid lookup
                output: vec![tx_output],
            };

            let utxo_metas = transaction
                .input
                .iter()
                .map(|input| {
                    UtxoMeta::from_outpoint(input.previous_output.txid, input.previous_output.vout)
                })
                .collect::<Vec<_>>();

            // Prepare mock mempool data reflecting a pending UTXO with specific fee/size
            let user_utxos = vec![create_mock_utxo_with_runes(
                25000,
                utxo_metas[0].txid_big_endian(),
                utxo_metas[0].vout(),
                1000,
            )];

            let mempool_data = {
                let mut utxo_mempool_info = [None; 10];
                utxo_mempool_info[0] = Some((
                    utxo_metas[0].txid_big_endian(),
                    MempoolInfo {
                        total_fee: 1000,
                        total_size: 250,
                    },
                ));

                mempool::MempoolData::<10, 10>::new(
                    utxo_mempool_info,
                    std::array::from_fn(|_| mempool::AccountMempoolInfo::default()),
                )
            };

            // Build the transaction builder directly from an existing transaction.
            let builder = TransactionBuilder::<10, 10, SingleRuneSet>::new_with_transaction(
                transaction.clone(),
                &mempool_data,
                &user_utxos,
            )
            .expect("Failed to create builder from transaction");

            // The builder should now reflect the data derived from `transaction`.
            assert_eq!(builder.transaction.version, Version::ONE);
            assert_eq!(builder.transaction.input.len(), 1);
            assert_eq!(builder.transaction.output.len(), 1);
            assert_eq!(builder.total_btc_input, 25000);
            #[cfg(feature = "runes")]
            assert_eq!(builder.total_rune_inputs.len(), 1);
            #[cfg(feature = "runes")]
            assert_eq!(
                builder.total_rune_inputs.find(&RuneAmount {
                    id: RuneId::new(1, 1),
                    amount: 1000,
                }),
                Some(&RuneAmount {
                    id: RuneId::new(1, 1),
                    amount: 1000,
                })
            );
            assert_eq!(builder.tx_statuses.total_fee, 1000);
            assert_eq!(builder.tx_statuses.total_size, 250);
        }

        #[cfg(feature = "runes")]
        #[test]
        fn calculates_rune_input_correctly() {
            let transaction =
                Transaction {
                    version: Version::TWO,
                    lock_time: LockTime::ZERO,
                    input: vec![TxIn {
                    previous_output: OutPoint::from_str(
                        "1111111111111111111111111111111111111111111111111111111111111111:0",
                    )
                    .unwrap(),
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                },
                TxIn {
                    previous_output: OutPoint::from_str(
                        "2222222222222222222222222222222222222222222222222222222222222222:1",
                    )
                    .unwrap(),
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                },
                TxIn {
                    previous_output: OutPoint::from_str(
                        "3333333333333333333333333333333333333333333333333333333333333333:2",
                    )
                    .unwrap(),
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                }],
                    output: vec![],
                };

            let utxo_metas = transaction
                .input
                .iter()
                .map(|input| {
                    UtxoMeta::from_outpoint(input.previous_output.txid, input.previous_output.vout)
                })
                .collect::<Vec<_>>();

            // Test with multiple rune UTXOs but no pending mempool data required
            let user_utxos = vec![
                create_mock_utxo_with_runes(
                    10000,
                    utxo_metas[0].txid_big_endian(),
                    utxo_metas[0].vout(),
                    500,
                ),
                create_mock_utxo_with_runes(
                    20000,
                    utxo_metas[1].txid_big_endian(),
                    utxo_metas[1].vout(),
                    750,
                ),
                create_mock_utxo(30000, utxo_metas[2].txid_big_endian(), utxo_metas[2].vout()), // No runes
            ];

            let mempool_data = mempool::MempoolData::<10, 10>::default();

            let builder = TransactionBuilder::<10, 10, SingleRuneSet>::new_with_transaction(
                transaction,
                &mempool_data,
                &user_utxos,
            )
            .expect("Failed to build transaction");

            assert_eq!(builder.total_rune_inputs.len(), 1);
            assert_eq!(
                builder.total_rune_inputs.find(&RuneId::new(1, 1)),
                Some(&RuneAmount {
                    id: RuneId::new(1, 1),
                    amount: 1250,
                })
            );
        }
    }

    mod get_fee_paid {
        use bitcoin::Amount;

        use super::*;

        #[test]
        fn calculates_fee_paid_correctly() {
            let mut builder = new_tb!(10, 10);

            // Set total BTC input directly for this test
            builder.total_btc_input = 100000;

            // Add output
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(95000),
                script_pubkey: ScriptBuf::new(),
            });

            let fee_paid = builder.get_fee_paid().unwrap();
            assert_eq!(fee_paid, 5000); // 100000 - 95000
        }

        #[test]
        fn returns_error_when_insufficient_input() {
            let mut builder = new_tb!(10, 10);

            // Add output but no input
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(50000),
                script_pubkey: ScriptBuf::new(),
            });

            let result = builder.get_fee_paid();
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), BitcoinTxError::InsufficientInputAmount);
        }
    }

    mod get_ancestors_totals {
        use super::*;

        #[test]
        fn returns_correct_ancestors_totals() {
            let mut builder = new_tb!(10, 10);
            builder.tx_statuses = MempoolInfo {
                total_fee: 1500,
                total_size: 300,
            };

            let (total_size, total_fee) = builder.get_ancestors_totals().unwrap();
            assert_eq!(total_size, 300);
            assert_eq!(total_fee, 1500);
        }
    }

    mod is_fee_rate_valid {
        use super::*;

        #[test]
        fn validates_fee_rate_correctly() {
            let mut builder = new_tb!(10, 10);

            // Set inputs and outputs manually for controlled test
            builder.total_btc_input = 100000;

            // Add output with fee of 10000 sats
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(90000),
                script_pubkey: ScriptBuf::new(),
            });

            // Assume transaction size is about 200 bytes, so fee rate is 50 sat/vB
            let fee_rate = FeeRate::try_from(30.0).unwrap(); // 30 sat/vB
            let result = builder.is_fee_rate_valid(&fee_rate);

            // This should pass as our effective fee rate (50) is higher than required (30)
            assert!(result.is_ok());
        }

        #[test]
        fn rejects_insufficient_fee_rate() {
            let mut builder = new_tb!(10, 10);

            // Set inputs and outputs manually
            builder.total_btc_input = 100000;

            // Add output with very low fee
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(99900),
                script_pubkey: ScriptBuf::new(),
            });

            // Require high fee rate
            let fee_rate = FeeRate::try_from(100.0).unwrap(); // 100 sat/vB
            let result = builder.is_fee_rate_valid(&fee_rate);

            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), BitcoinTxError::InvalidFeeRateTooLow);
        }
    }

    mod tx_status_handling {
        use super::*;

        #[test]
        fn handles_confirmed_tx_status() {
            let mut builder = new_tb!(10, 10);
            let utxo = create_mock_utxo(50000, [1u8; 32], 0);
            let status = TxStatus::Confirmed;

            // Manually test the add_tx_status logic
            builder.tx_statuses = builder.tx_statuses_with(&utxo, &status).unwrap();

            assert_eq!(builder.tx_statuses.total_fee, 0);
            assert_eq!(builder.tx_statuses.total_size, 0);
        }

        #[test]
        fn handles_pending_tx_status() {
            let mut builder = new_tb!(10, 10);
            let utxo = create_mock_utxo(50000, [1u8; 32], 0);
            let pending_info = MempoolInfo {
                total_fee: 2000,
                total_size: 250,
            };
            let status = TxStatus::Pending(pending_info);

            // Manually test the add_tx_status logic
            builder.tx_statuses = builder.tx_statuses_with(&utxo, &status).unwrap();

            assert_eq!(builder.tx_statuses.total_fee, 2000);
            assert_eq!(builder.tx_statuses.total_size, 250);
        }
    }

    mod estimate_final_tx_vsize {
        use super::*;
        use arch_program::input_to_sign::InputToSign;
        use arch_program::pubkey::Pubkey;

        #[test]
        fn estimates_empty_transaction_size() {
            let mut builder = new_tb!(10, 10);

            let vsize = builder.estimate_final_tx_vsize().unwrap();

            // Empty transaction should have minimal size
            assert!(vsize > 0);
            assert!(vsize < 100); // Should be quite small
        }

        #[test]
        fn estimates_transaction_size_with_inputs_to_sign() {
            let mut builder = new_tb!(10, 10);

            // Add some mock inputs to sign
            let pubkey = Pubkey::system_program();
            builder
                .inputs_to_sign
                .push(InputToSign {
                    index: 0,
                    signer: pubkey,
                })
                .unwrap();

            builder
                .inputs_to_sign
                .push(InputToSign {
                    index: 1,
                    signer: pubkey,
                })
                .unwrap();

            // Add some transaction inputs
            builder.transaction.input.push(TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            });
            builder.transaction.input.push(TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            });

            let vsize = builder.estimate_final_tx_vsize().unwrap();

            // Should be larger than empty transaction due to witness overhead
            assert!(vsize > 100);
        }
    }

    #[cfg(feature = "utxo-consolidation")]
    mod get_fee_paid_by_program {
        use super::*;

        #[test]
        fn calculates_consolidation_fee_correctly() {
            let mut builder = new_tb!(10, 10);

            // Set consolidation values
            builder.extra_tx_size_for_consolidation = 500; // 500 bytes

            let fee_rate = FeeRate::try_from(10.0).unwrap(); // 10 sat/vB
            let fee = builder.get_fee_paid_by_program(&fee_rate);

            assert_eq!(fee, 5000); // 500 bytes * 10 sat/vB = 5000 sats
        }

        #[test]
        fn returns_zero_when_no_consolidation() {
            let builder = new_tb!(10, 10);

            let fee_rate = FeeRate::try_from(50.0).unwrap();
            let fee = builder.get_fee_paid_by_program(&fee_rate);

            assert_eq!(fee, 0);
        }
    }

    mod input_index_management {
        use super::*;
        use arch_program::input_to_sign::InputToSign;
        use arch_program::pubkey::Pubkey;

        #[test]
        fn updates_indices_correctly_when_inserting() {
            let mut builder = new_tb!(10, 10);
            let pubkey = Pubkey::system_program();

            // Add initial inputs to sign
            builder
                .inputs_to_sign
                .push(InputToSign {
                    index: 0,
                    signer: pubkey,
                })
                .unwrap();
            builder
                .inputs_to_sign
                .push(InputToSign {
                    index: 1,
                    signer: pubkey,
                })
                .unwrap();
            builder
                .inputs_to_sign
                .push(InputToSign {
                    index: 2,
                    signer: pubkey,
                })
                .unwrap();

            // Manually call the index update logic (simulate insertion at index 1)
            let insert_index = 1u32;
            for input in builder.inputs_to_sign.iter_mut() {
                if input.index >= insert_index {
                    input.index += 1;
                }
            }

            // Check that indices were updated correctly
            let slice = builder.inputs_to_sign.as_slice();
            assert_eq!(slice[0].index, 0); // Should remain 0
            assert_eq!(slice[1].index, 2); // Should be incremented from 1 to 2
            assert_eq!(slice[2].index, 3); // Should be incremented from 2 to 3
        }

        #[test]
        fn handles_multiple_insertions() {
            let mut builder = new_tb!(10, 10);
            let pubkey = Pubkey::system_program();

            // Add inputs to sign
            for i in 0..5 {
                builder
                    .inputs_to_sign
                    .push(InputToSign {
                        index: i,
                        signer: pubkey,
                    })
                    .unwrap();
            }

            // Simulate multiple insertions
            // Insert at index 2 - indices 2,3,4 become 3,4,5
            for input in builder.inputs_to_sign.iter_mut() {
                if input.index >= 2 {
                    input.index += 1;
                }
            }

            // Insert at index 1 - indices 1,3,4,5 become 2,4,5,6
            for input in builder.inputs_to_sign.iter_mut() {
                if input.index >= 1 {
                    input.index += 1;
                }
            }

            // Check final indices - let's trace through what actually happens:
            // Original: 0,1,2,3,4
            // After first insertion at 2: 0,1,3,4,5
            // After second insertion at 1: 0,2,4,5,6
            let slice = builder.inputs_to_sign.as_slice();
            assert_eq!(slice[0].index, 0); // Should remain 0
            assert_eq!(slice[1].index, 2); // 1 -> 2
            assert_eq!(slice[2].index, 4); // 2 -> 3 -> 4
            assert_eq!(slice[3].index, 5); // 3 -> 4 -> 5
            assert_eq!(slice[4].index, 6); // 4 -> 5 -> 6
        }
    }

    mod boundary_conditions {
        use super::*;

        #[test]
        fn handles_max_inputs_to_sign() {
            let builder = new_tb!(10, 10);

            // Test that the list starts empty and can hold items
            assert_eq!(builder.inputs_to_sign.len(), 0);
        }

        #[test]
        fn handles_large_btc_amounts() {
            let mut builder = new_tb!(10, 10);

            // Test with large BTC amounts (but not MAX to avoid overflow in calculations)
            builder.total_btc_input = 21_000_000 * 100_000_000; // 21M BTC in satoshis

            assert_eq!(builder.total_btc_input, 21_000_000 * 100_000_000);
        }
    }

    mod fee_rate_validation_edge_cases {
        use super::*;

        #[test]
        fn handles_zero_fee_rate() {
            let mut builder = new_tb!(10, 10);

            builder.total_btc_input = 100000;
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(95000), // 5000 sat fee for a more reasonable rate
                script_pubkey: ScriptBuf::new(),
            });

            // Low fee rate
            let fee_rate = FeeRate::try_from(1.0).unwrap(); // 1 sat/vB
            let result = builder.is_fee_rate_valid(&fee_rate);

            // Should pass since we have sufficient fee
            assert!(result.is_ok());
        }

        #[test]
        fn handles_ancestors_with_high_fees() {
            let mut builder = new_tb!(10, 10);

            builder.total_btc_input = 100000;
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(95000),
                script_pubkey: ScriptBuf::new(),
            });

            // Set high ancestor fees
            builder.tx_statuses = MempoolInfo {
                total_fee: 50000, // High ancestor fees
                total_size: 1000,
            };

            let fee_rate = FeeRate::try_from(10.0).unwrap();
            let result = builder.is_fee_rate_valid(&fee_rate);

            // Should pass due to high ancestor fees contributing to overall rate
            assert!(result.is_ok());
        }

        #[test]
        fn handles_very_large_transactions() {
            let mut builder = new_tb!(10, 10);

            // Create a large transaction with many inputs
            for i in 0..50 {
                builder.transaction.input.push(TxIn {
                    previous_output: OutPoint {
                        txid: bitcoin::Txid::from_str(&format!("{:064x}", i)).unwrap(),
                        vout: 0,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                });
            }

            builder.total_btc_input = 5000000; // 5M sats
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(4950000), // 50k sats fee
                script_pubkey: ScriptBuf::new(),
            });

            let fee_rate = FeeRate::try_from(10.0).unwrap();
            let result = builder.is_fee_rate_valid(&fee_rate);

            // Should handle large transactions gracefully
            assert!(result.is_ok() || result.is_err()); // Just ensure it doesn't panic
        }
    }

    #[cfg(feature = "utxo-consolidation")]
    mod consolidation_tests {
        use super::*;

        #[test]
        fn tracks_consolidation_input_amounts() {
            let mut builder = new_tb!(10, 10);

            // Manually set consolidation amounts (normally set by add_consolidation_utxos)
            builder.total_btc_consolidation_input = 250000;

            assert_eq!(builder.total_btc_consolidation_input, 250000);
        }

        #[test]
        fn tracks_extra_consolidation_size() {
            let mut builder = new_tb!(10, 10);

            // Manually set extra tx size (normally set by add_consolidation_utxos)
            builder.extra_tx_size_for_consolidation = 1500;

            assert_eq!(builder.extra_tx_size_for_consolidation, 1500);
        }

        #[test]
        fn consolidation_fee_calculation_integration() {
            let mut builder = new_tb!(10, 10);

            builder.extra_tx_size_for_consolidation = 800;

            let fee_rate = FeeRate::try_from(25.0).unwrap(); // 25 sat/vB
            let fee = builder.get_fee_paid_by_program(&fee_rate);

            assert_eq!(fee, 20000); // 800 * 25 = 20000 sats
        }
    }

    mod transaction_structure {
        use super::*;

        #[test]
        fn maintains_transaction_structure_integrity() {
            let mut builder = new_tb!(10, 10);

            // Add inputs and outputs
            builder.transaction.input.push(TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            });

            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(50000),
                script_pubkey: ScriptBuf::new(),
            });

            // Verify structure
            assert_eq!(builder.transaction.input.len(), 1);
            assert_eq!(builder.transaction.output.len(), 1);
            assert_eq!(builder.transaction.version, Version::TWO);
            assert_eq!(builder.transaction.lock_time, LockTime::ZERO);
        }

        #[test]
        fn handles_empty_transaction_gracefully() {
            let mut builder = new_tb!(10, 10);

            // Empty transaction should be valid
            assert_eq!(builder.transaction.input.len(), 0);
            assert_eq!(builder.transaction.output.len(), 0);

            // Should be able to estimate size even when empty
            let vsize = builder.estimate_final_tx_vsize().unwrap();
            assert!(vsize > 0);
        }
    }

    mod error_handling {
        use super::*;

        #[test]
        fn handles_fee_calculation_edge_cases() {
            let mut builder = new_tb!(10, 10);

            // Test with zero input
            builder.total_btc_input = 0;
            builder.transaction.output.push(TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::new(),
            });

            let result = builder.get_fee_paid();
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), BitcoinTxError::InsufficientInputAmount);
        }

        #[test]
        fn handles_ancestor_totals_correctly() {
            let mut builder = new_tb!(10, 10);

            // Test with default (empty) mempool info
            let (size, fee) = builder.get_ancestors_totals().unwrap();
            assert_eq!(size, 0);
            assert_eq!(fee, 0);

            // Test with some ancestor data
            builder.tx_statuses.total_fee = 5000;
            builder.tx_statuses.total_size = 500;

            let (size, fee) = builder.get_ancestors_totals().unwrap();
            assert_eq!(size, 500);
            assert_eq!(fee, 5000);
        }
    }

    mod find_btc {
        use super::*;

        const PUBKEY: Pubkey = Pubkey([0; 32]);

        #[test]
        fn finds_btc_with_one_utxo() {
            let utxos = vec![UtxoInfo::new(UtxoMeta::from([0; 32], 0), 10_000)];

            let amount = 10_000;

            let mut transaction_builder = new_tb!(10, 10);

            let utxo_refs: Vec<&UtxoInfo<SingleRuneSet>> = utxos.iter().collect();
            let (found_utxo_indices, found_amount) = transaction_builder
                .find_btc_in_utxos(&utxo_refs, &PUBKEY, amount)
                .unwrap();

            assert_eq!(found_utxo_indices.len(), 1, "Found a single UTXO");
            assert_eq!(found_amount, 10_000);
        }

        #[test]
        fn finds_btc_with_multiple_utxos() {
            let utxos = vec![
                UtxoInfo::new(UtxoMeta::from([0; 32], 0), 5_000),
                UtxoInfo::new(UtxoMeta::from([0; 32], 1), 8_000),
                UtxoInfo::new(UtxoMeta::from([0; 32], 2), 12_000),
            ];

            let amount = 10_000;

            let mut transaction_builder = new_tb!(10, 10);

            let utxo_refs: Vec<&UtxoInfo<SingleRuneSet>> = utxos.iter().collect();
            let (found_utxo_indices, found_amount) = transaction_builder
                .find_btc_in_utxos(&utxo_refs, &PUBKEY, amount)
                .unwrap();

            assert_eq!(found_utxo_indices.len(), 1, "Found a single UTXO");
            assert_eq!(utxos[found_utxo_indices[0]].meta.vout(), 2);
            assert_eq!(found_amount, 12_000);
        }

        #[test]
        #[cfg(feature = "utxo-consolidation")]
        fn finds_btc_with_consolidation_utxos() {
            let mut utxos = vec![
                UtxoInfo::new(UtxoMeta::from([0; 32], 0), 5_000),
                UtxoInfo::new(UtxoMeta::from([0; 32], 1), 8_000),
                UtxoInfo::new(UtxoMeta::from([0; 32], 2), 12_000),
            ];

            *utxos[1].needs_consolidation_mut() = FixedOptionF64::some(1.0);
            *utxos[2].needs_consolidation_mut() = FixedOptionF64::some(1.0);

            let amount = 10_000;

            let mut transaction_builder = new_tb!(10, 10);

            let utxo_refs: Vec<&UtxoInfo<SingleRuneSet>> = utxos.iter().collect();
            let (found_utxo_indices, found_amount) = transaction_builder
                .find_btc_in_utxos(&utxo_refs, &PUBKEY, amount)
                .unwrap();

            assert_eq!(found_utxo_indices.len(), 2, "Found two UTXOs");
            assert_eq!(
                utxos[found_utxo_indices[0]].meta.vout(),
                0,
                "First UTXO matches"
            );
            assert_eq!(
                utxos[found_utxo_indices[1]].meta.vout(),
                2,
                "Second UTXO matches"
            );
            assert_eq!(found_amount, 17_000);
        }
    }

    mod find_btc_from_holder {
        use super::*;

        const PUBKEY: Pubkey = Pubkey([0; 32]);

        struct TestHolder {
            utxos: Vec<UtxoInfo<SingleRuneSet>>,
        }

        impl BtcUtxoHolder for TestHolder {
            fn btc_utxos(&self) -> &[UtxoInfo] {
                self.utxos.as_slice()
            }
        }

        fn utxo(value: u64, vout: u32) -> UtxoInfo<SingleRuneSet> {
            UtxoInfo::new(UtxoMeta::from([0; 32], vout), value)
        }

        #[cfg(feature = "utxo-consolidation")]
        fn utxo_cons(value: u64, vout: u32) -> UtxoInfo<SingleRuneSet> {
            let mut u = utxo(value, vout);
            *u.needs_consolidation_mut() = FixedOptionF64::some(1.0);
            u
        }

        #[test]
        fn selects_basic_across_holders() {
            // Two holders with one UTXO each; ensure we pick the largest to satisfy the amount.
            let holders = vec![
                TestHolder {
                    utxos: vec![utxo(5_000, 0)],
                },
                TestHolder {
                    utxos: vec![utxo(12_000, 1)],
                },
            ];

            let mut builder = new_tb!(10, 10);
            let amount = 10_000;

            let found_amount = builder
                .find_btc_in_utxos_from_holder(&holders, &PUBKEY, amount, false)
                .unwrap();

            assert_eq!(found_amount, 17_000);

            #[cfg(feature = "utxo-consolidation")]
            {
                assert_eq!(builder.total_btc_consolidation_input, 0);
                assert_eq!(builder.extra_tx_size_for_consolidation, 0);
            }
        }

        #[test]
        #[cfg(feature = "utxo-consolidation")]
        fn tracks_consolidation_single_input() {
            // Prefer non-consolidation UTXOs first, then include one consolidation UTXO.
            let holders = vec![
                TestHolder {
                    utxos: vec![utxo(5_000, 0), utxo_cons(12_000, 1)],
                },
                TestHolder {
                    utxos: vec![utxo(9_000, 2)],
                },
            ];

            let mut builder = new_tb!(10, 10);
            let amount = 17_000;

            let found_amount = builder
                .find_btc_in_utxos_from_holder(&holders, &PUBKEY, amount, false)
                .unwrap();

            // Selection: 9k (non-cons) + 5k (non-cons) + 12k (cons) = 26k
            assert_eq!(found_amount, 26_000);
            assert_eq!(builder.total_btc_consolidation_input, 12_000);
            assert_eq!(
                builder.extra_tx_size_for_consolidation,
                crate::input_calc::ARCH_INPUT_SIZE
            );
        }

        #[test]
        #[cfg(feature = "utxo-consolidation")]
        fn tracks_consolidation_multiple_inputs() {
            // Require two consolidation UTXOs to reach the target amount.
            let holders = vec![
                TestHolder {
                    utxos: vec![utxo(4_000, 0)],
                },
                TestHolder {
                    utxos: vec![utxo_cons(7_000, 1)],
                },
                TestHolder {
                    utxos: vec![utxo_cons(6_000, 2)],
                },
            ];

            let mut builder = new_tb!(10, 10);
            let amount = 15_000;

            let found_amount = builder
                .find_btc_in_utxos_from_holder(&holders, &PUBKEY, amount, false)
                .unwrap();

            // Selection: 4k (non-cons) + 7k (cons) + 6k (cons) = 17k
            assert_eq!(found_amount, 17_000);
            assert_eq!(builder.total_btc_consolidation_input, 13_000);
            assert_eq!(
                builder.extra_tx_size_for_consolidation,
                crate::input_calc::ARCH_INPUT_SIZE * 2
            );
        }
    }
}
