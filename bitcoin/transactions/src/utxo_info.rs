use arch_program::rune::{RuneAmount, RuneId};

use arch_program::{
    program::get_bitcoin_tx_output_value, program_error::ProgramError, utxo::UtxoMeta,
};

use bytemuck::{Pod, Zeroable};
use satellite_collections::declare_fixed_option;
use satellite_collections::generic::fixed_set::{FixedCapacitySet, FixedSet};

use crate::{bytes::txid_to_bytes_big_endian, error::BitcoinTxError};

#[cfg(feature = "runes")]
use crate::arch::get_runes;

/// Trait defining the essential operations needed by StateShard for UTXO types.
/// This allows StateShard to work with different concrete UtxoInfo implementations
/// while maintaining a consistent interface.
pub trait UtxoInfoTrait<RuneSet: FixedCapacitySet<Item = RuneAmount>> {
    /// Create a new UtxoInfo with the given metadata and value.
    fn new(meta: UtxoMeta, value: u64) -> Self;

    /// Get the UTXO metadata (txid, vout, etc.)
    fn meta(&self) -> &UtxoMeta;

    /// Get the satoshi value of this UTXO
    fn value(&self) -> u64;

    /// Check if this UTXO is equal to another based on metadata
    fn eq_meta(&self, other: &Self) -> bool;

    /// Get access to the runes information when the "runes" feature is enabled
    #[cfg(feature = "runes")]
    fn runes(&self) -> &RuneSet;

    /// Get mutable access to the runes information when the "runes" feature is enabled
    #[cfg(feature = "runes")]
    fn runes_mut(&mut self) -> &mut RuneSet;

    /// Get access to the consolidation information when the "utxo-consolidation" feature is enabled
    #[cfg(feature = "utxo-consolidation")]
    fn needs_consolidation(&self) -> &FixedOptionF64;

    /// Get mutable access to the consolidation information when the "utxo-consolidation" feature is enabled
    #[cfg(feature = "utxo-consolidation")]
    fn needs_consolidation_mut(&mut self) -> &mut FixedOptionF64;
}

#[cfg(feature = "utxo-consolidation")]
declare_fixed_option!(FixedOptionF64, f64, 7);

#[cfg(feature = "runes")]
pub type SingleRuneSet = FixedSet<RuneAmount, 1>;

#[cfg(not(feature = "runes"))]
pub type SingleRuneSet = FixedSet<RuneAmount, 0>;

#[repr(C, align(8))]
#[derive(Clone, Debug)]
// Provide a default generic parameter so callers can simply use `UtxoInfo` without specifying
// the rune set type. When the `runes` feature is enabled the default is `SingleRuneSet`; when it
// is disabled the default is `()`.
pub struct UtxoInfo<RuneSet: FixedCapacitySet<Item = RuneAmount> = SingleRuneSet> {
    pub meta: UtxoMeta,
    pub value: u64,

    #[cfg(feature = "runes")]
    pub runes: RuneSet,

    #[cfg(feature = "utxo-consolidation")]
    pub needs_consolidation: FixedOptionF64,

    // Ensure the generic parameter is referenced even when the `runes` feature is disabled.
    #[cfg(not(feature = "runes"))]
    _phantom: std::marker::PhantomData<RuneSet>,
}

impl<RuneSet: FixedCapacitySet<Item = RuneAmount> + Default> UtxoInfo<RuneSet> {
    /// Public constructor that initializes [`UtxoInfo`] with `meta` and `value`,
    /// filling the remaining fields from `Default`.
    pub fn new(meta: UtxoMeta, value: u64) -> Self {
        Self {
            meta,
            value,
            ..Default::default()
        }
    }

    #[cfg(feature = "runes")]
    pub fn new_with_runes(meta: UtxoMeta, value: u64, runes: RuneSet) -> Self {
        Self {
            meta,
            value,
            runes,
            ..Default::default()
        }
    }

    #[cfg(feature = "utxo-consolidation")]
    pub fn new_with_consolidation(
        meta: UtxoMeta,
        value: u64,
        needs_consolidation: FixedOptionF64,
    ) -> Self {
        Self {
            meta,
            value,
            needs_consolidation,
            ..Default::default()
        }
    }

    #[cfg(feature = "utxo-consolidation")]
    #[cfg(feature = "runes")]
    pub fn new_with_runes_and_consolidation(
        meta: UtxoMeta,
        value: u64,
        runes: RuneSet,
        needs_consolidation: FixedOptionF64,
    ) -> Self {
        Self {
            meta,
            value,
            runes,
            needs_consolidation,
            ..Default::default()
        }
    }
}

// Implement the UtxoInfoTrait for UtxoInfo
impl<RuneSet: FixedCapacitySet<Item = RuneAmount> + Default> UtxoInfoTrait<RuneSet>
    for UtxoInfo<RuneSet>
{
    fn new(meta: UtxoMeta, value: u64) -> Self {
        Self {
            meta,
            value,
            ..Default::default()
        }
    }

    fn meta(&self) -> &UtxoMeta {
        &self.meta
    }

    fn value(&self) -> u64 {
        self.value
    }

    fn eq_meta(&self, other: &Self) -> bool {
        self.meta == other.meta
    }

    /// Get access to the runes information when the "runes" feature is enabled
    #[cfg(feature = "runes")]
    fn runes(&self) -> &RuneSet {
        &self.runes
    }

    /// Get mutable access to the runes information when the "runes" feature is enabled
    #[cfg(feature = "runes")]
    fn runes_mut(&mut self) -> &mut RuneSet {
        &mut self.runes
    }

    /// Get access to the consolidation information when the "utxo-consolidation" feature is enabled
    #[cfg(feature = "utxo-consolidation")]
    fn needs_consolidation(&self) -> &FixedOptionF64 {
        &self.needs_consolidation
    }

    /// Get mutable access to the consolidation information when the "utxo-consolidation" feature is enabled
    #[cfg(feature = "utxo-consolidation")]
    fn needs_consolidation_mut(&mut self) -> &mut FixedOptionF64 {
        &mut self.needs_consolidation
    }
}

// Pod and Zeroable are only available when RuneSet itself is Pod/Zeroable
// AND UtxoInfo implements Copy (which requires RuneSet: Copy).
// After arch_program 0.6.2, RuneAmount no longer implements Copy/Pod/Zeroable,
// so UtxoInfo<SingleRuneSet> is no longer Pod/Zeroable.
unsafe impl<RuneSet: FixedCapacitySet<Item = RuneAmount> + Pod + Copy> Pod for UtxoInfo<RuneSet> where
    UtxoInfo<RuneSet>: Copy
{
}
unsafe impl<RuneSet: FixedCapacitySet<Item = RuneAmount> + Zeroable> Zeroable
    for UtxoInfo<RuneSet>
{
}

impl<RuneSet: FixedCapacitySet<Item = RuneAmount>> PartialEq for UtxoInfo<RuneSet> {
    fn eq(&self, other: &Self) -> bool {
        self.meta == other.meta
    }
}

impl<RuneSet: FixedCapacitySet<Item = RuneAmount>> Eq for UtxoInfo<RuneSet> {}

impl<RuneSet: FixedCapacitySet<Item = RuneAmount>> std::fmt::Display for UtxoInfo<RuneSet> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", hex::encode(&self.meta.txid()), self.meta.vout())
    }
}

impl<RuneSet: FixedCapacitySet<Item = RuneAmount>> AsRef<UtxoInfo<RuneSet>> for UtxoInfo<RuneSet> {
    fn as_ref(&self) -> &UtxoInfo<RuneSet> {
        self
    }
}

impl<RuneSet: FixedCapacitySet<Item = RuneAmount>> AsRef<UtxoMeta> for UtxoInfo<RuneSet> {
    fn as_ref(&self) -> &UtxoMeta {
        &self.meta
    }
}

impl<RuneSet: FixedCapacitySet<Item = RuneAmount>> Default for UtxoInfo<RuneSet>
where
    RuneSet: Default,
{
    fn default() -> Self {
        Self {
            meta: UtxoMeta::from([0; 32], 0),
            value: u64::default(),
            #[cfg(feature = "runes")]
            runes: RuneSet::default(),
            #[cfg(feature = "utxo-consolidation")]
            needs_consolidation: FixedOptionF64::default(),
            // Ensure the generic parameter is referenced even when the `runes` feature is disabled.
            #[cfg(not(feature = "runes"))]
            _phantom: std::marker::PhantomData::<RuneSet>,
        }
    }
}

#[cfg(feature = "runes")]
impl<RS> TryFrom<&UtxoMeta> for UtxoInfo<RS>
where
    RS: FixedCapacitySet<Item = RuneAmount> + Default,
{
    type Error = ProgramError;

    fn try_from(value: &UtxoMeta) -> std::result::Result<Self, ProgramError> {
        // Fetch rune amount (at most one) from the UTXO.
        let runes = get_runes(value)?;

        let outpoint = value.to_outpoint();

        let ui_value =
            get_bitcoin_tx_output_value(txid_to_bytes_big_endian(&outpoint.txid), outpoint.vout)
                .ok_or(BitcoinTxError::TransactionNotFound)?;

        Ok(UtxoInfo {
            meta: value.clone(),
            value: ui_value,
            runes: runes,
            #[cfg(feature = "utxo-consolidation")]
            needs_consolidation: FixedOptionF64::none(),
        })
    }
}

#[cfg(feature = "runes")]
impl<RS> TryFrom<UtxoMeta> for UtxoInfo<RS>
where
    RS: FixedCapacitySet<Item = RuneAmount> + Default,
{
    type Error = ProgramError;

    fn try_from(value: UtxoMeta) -> std::result::Result<Self, ProgramError> {
        // Fetch rune amount (at most one) from the UTXO.
        let runes = get_runes(&value)?;

        let outpoint = value.to_outpoint();

        let ui_value =
            get_bitcoin_tx_output_value(txid_to_bytes_big_endian(&outpoint.txid), outpoint.vout)
                .ok_or(BitcoinTxError::TransactionNotFound)?;

        Ok(UtxoInfo {
            meta: value.clone(),
            value: ui_value,
            runes: runes,
            #[cfg(feature = "utxo-consolidation")]
            needs_consolidation: FixedOptionF64::none(),
        })
    }
}

// When the "runes" feature is disabled, fallback implementation without rune handling.
#[cfg(not(feature = "runes"))]
impl TryFrom<&UtxoMeta> for UtxoInfo<SingleRuneSet> {
    type Error = ProgramError;

    fn try_from(value: &UtxoMeta) -> std::result::Result<Self, ProgramError> {
        let outpoint = value.to_outpoint();

        let ui_value =
            get_bitcoin_tx_output_value(txid_to_bytes_big_endian(&outpoint.txid), outpoint.vout)
                .ok_or(ProgramError::Custom(
                    BitcoinTxError::TransactionNotFound.into(),
                ))?;

        Ok(UtxoInfo {
            meta: value.clone(),
            value: ui_value,
            #[cfg(feature = "utxo-consolidation")]
            needs_consolidation: FixedOptionF64::none(),
            _phantom: std::marker::PhantomData::<SingleRuneSet>,
        })
    }
}

// === Default helper types ==================================================
// For the most common case – a single-rune set (SingleRuneSet) and up to 50
// plain-BTC UTXOs per account – we provide ready-made fixed-capacity array and
// fixed-option wrappers so downstream crates can simply use
// `FixedArrayUtxoInfo` / `FixedOptionUtxoInfo` without additional boilerplate.

// Manual definitions replacing the macros because UtxoInfo is no longer Copy/Pod
// after the arch_program 0.6.2 upgrade (RuneAmount lost Copy/Default/Pod).

#[repr(C)]
#[derive(Clone, Debug)]
pub struct FixedArrayUtxoInfo {
    items: [UtxoInfo<SingleRuneSet>; 50],
    count: u16,
    _padding: [u8; 14],
}

impl FixedArrayUtxoInfo {
    pub fn new() -> Self {
        Self {
            items: core::array::from_fn(|_| UtxoInfo::default()),
            count: 0,
            _padding: [0; 14],
        }
    }

    #[cfg(not(target_os = "solana"))]
    pub fn from_slice(input_slice: &[UtxoInfo<SingleRuneSet>]) -> Self {
        let mut fa = Self::new();
        let num_to_copy = core::cmp::min(input_slice.len(), 50);
        for i in 0..num_to_copy {
            fa.add(input_slice[i].clone());
        }
        fa
    }

    pub fn len(&self) -> usize {
        self.count as usize
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn capacity(&self) -> usize {
        50
    }

    pub fn is_full(&self) -> bool {
        (self.count as usize) == 50
    }

    pub fn get(&self, index: usize) -> Option<&UtxoInfo<SingleRuneSet>> {
        if index < self.len() {
            Some(&self.items[index])
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut UtxoInfo<SingleRuneSet>> {
        if index < self.len() {
            Some(&mut self.items[index])
        } else {
            None
        }
    }

    pub fn add(&mut self, item: UtxoInfo<SingleRuneSet>) -> Option<usize> {
        if self.is_full() {
            None
        } else {
            let index = self.count as usize;
            self.items[index] = item;
            self.count += 1;
            Some(index)
        }
    }

    pub fn remove_at(&mut self, index: usize) -> Option<UtxoInfo<SingleRuneSet>> {
        if index >= self.len() {
            return None;
        }
        let removed_item = self.items[index].clone();
        for i in index..(self.len() - 1) {
            self.items[i] = self.items[i + 1].clone();
        }
        self.count -= 1;
        if 50 > 0 {
            self.items[self.len()] = UtxoInfo::default();
        }
        Some(removed_item)
    }

    pub fn remove_item(&mut self, item_to_remove: &UtxoInfo<SingleRuneSet>) -> bool {
        let mut found_index: Option<usize> = None;
        for i in 0..self.len() {
            if self.items[i] == *item_to_remove {
                found_index = Some(i);
                break;
            }
        }
        if let Some(index) = found_index {
            self.remove_at(index);
            true
        } else {
            false
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &UtxoInfo<SingleRuneSet>> + '_ {
        self.items.iter().take(self.len())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut UtxoInfo<SingleRuneSet>> + '_ {
        let len = self.len();
        self.items.iter_mut().take(len)
    }

    pub fn clear(&mut self) {
        for i in 0..self.len() {
            self.items[i] = UtxoInfo::default();
        }
        self.count = 0;
    }

    pub fn as_slice(&self) -> &[UtxoInfo<SingleRuneSet>] {
        &self.items[0..self.len()]
    }

    pub fn as_mut_slice(&mut self) -> &mut [UtxoInfo<SingleRuneSet>] {
        let len = self.len();
        &mut self.items[0..len]
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&UtxoInfo<SingleRuneSet>) -> bool,
    {
        let original_len = self.len();
        let mut write_idx = 0;
        let mut read_idx = 0;
        while read_idx < original_len {
            if f(&self.items[read_idx]) {
                if read_idx != write_idx {
                    self.items[write_idx] = self.items[read_idx].clone();
                }
                write_idx += 1;
            }
            read_idx += 1;
        }
        for i in write_idx..original_len {
            self.items[i] = UtxoInfo::default();
        }
        self.count = write_idx as u16;
    }

    pub fn as_vec(&self) -> Vec<UtxoInfo<SingleRuneSet>> {
        self.items
            .iter()
            .take(self.count as usize)
            .cloned()
            .collect()
    }
}

impl Default for FixedArrayUtxoInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for FixedArrayUtxoInfo {
    fn eq(&self, other: &Self) -> bool {
        self.count == other.count && self.as_slice() == other.as_slice()
    }
}

impl Eq for FixedArrayUtxoInfo {}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct FixedOptionUtxoInfo {
    item: UtxoInfo<SingleRuneSet>,
    present: u8,
    _padding: [u8; 15],
}

impl FixedOptionUtxoInfo {
    pub fn none() -> Self {
        Self {
            item: UtxoInfo::default(),
            present: 0,
            _padding: [0; 15],
        }
    }

    pub fn some(data: UtxoInfo<SingleRuneSet>) -> Self {
        Self {
            item: data,
            present: 1,
            _padding: [0; 15],
        }
    }

    pub fn is_some(&self) -> bool {
        self.present != 0
    }

    pub fn is_none(&self) -> bool {
        self.present == 0
    }

    pub fn get(&self) -> Option<UtxoInfo<SingleRuneSet>> {
        if self.is_some() {
            Some(self.item.clone())
        } else {
            None
        }
    }

    pub fn as_ref(&self) -> Option<&UtxoInfo<SingleRuneSet>> {
        if self.is_some() {
            Some(&self.item)
        } else {
            None
        }
    }

    pub fn as_mut(&mut self) -> Option<&mut UtxoInfo<SingleRuneSet>> {
        if self.is_some() {
            Some(&mut self.item)
        } else {
            None
        }
    }

    pub fn unwrap(self) -> UtxoInfo<SingleRuneSet> {
        let option: Option<_> = self.into();
        option.unwrap()
    }
}

impl Default for FixedOptionUtxoInfo {
    fn default() -> Self {
        Self::none()
    }
}

impl From<FixedOptionUtxoInfo> for Option<UtxoInfo<SingleRuneSet>> {
    fn from(item: FixedOptionUtxoInfo) -> Option<UtxoInfo<SingleRuneSet>> {
        if item.is_some() {
            Some(item.item)
        } else {
            None
        }
    }
}

impl From<Option<UtxoInfo<SingleRuneSet>>> for FixedOptionUtxoInfo {
    fn from(item: Option<UtxoInfo<SingleRuneSet>>) -> FixedOptionUtxoInfo {
        match item {
            Some(data) => FixedOptionUtxoInfo::some(data),
            None => FixedOptionUtxoInfo::none(),
        }
    }
}

impl From<Option<&UtxoInfo<SingleRuneSet>>> for FixedOptionUtxoInfo {
    fn from(item: Option<&UtxoInfo<SingleRuneSet>>) -> FixedOptionUtxoInfo {
        match item {
            Some(data) => FixedOptionUtxoInfo::some(data.clone()),
            None => FixedOptionUtxoInfo::none(),
        }
    }
}

impl PartialEq for FixedOptionUtxoInfo {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}

// Add helper methods to validate runes contained in a UTXO
#[cfg(feature = "runes")]
impl<RuneSet> UtxoInfo<RuneSet>
where
    RuneSet: FixedCapacitySet<Item = RuneAmount>,
{
    /// Returns the number of `RuneAmount` entries stored in this UTXO.
    ///
    /// NOTE: With the default `SingleRuneSet` this will be either **0** or **1**.
    pub fn rune_entry_count(&self) -> usize {
        self.runes.len()
    }

    /// Returns the total amount of runes across **all** [`RuneAmount`] entries
    /// stored in this UTXO.
    ///
    /// When `SingleRuneSet` is used this is equivalent to `self.rune_amount().unwrap_or(0)`.
    pub fn total_rune_amount(&self) -> u128 {
        self.runes.iter().map(|r| r.amount).sum()
    }

    /// If the UTXO contains a [`RuneAmount`] with the given [`RuneId`], returns
    /// the amount, otherwise `None`.
    pub fn rune_amount(&self, rune_id: &RuneId) -> Option<u128> {
        self.runes.find(rune_id).map(|r| r.amount)
    }

    /// Convenience check that this UTXO holds **exactly** `amount` of the rune
    /// identified by `rune_id`.
    pub fn contains_exact_rune(&self, rune_id: &RuneId, amount: u128) -> bool {
        self.rune_amount(rune_id) == Some(amount)
    }
}

#[cfg(not(feature = "runes"))]
impl<RuneSet> UtxoInfo<RuneSet>
where
    RuneSet: FixedCapacitySet<Item = RuneAmount>,
{
    /// Returns zero because rune information is unavailable when the `runes` feature is disabled.
    pub fn rune_entry_count(&self) -> usize {
        0
    }

    /// Returns zero because rune information is unavailable when the `runes` feature is disabled.
    pub fn total_rune_amount(&self) -> u128 {
        0
    }

    /// Always returns `None` because rune information is unavailable when the `runes` feature is disabled.
    pub fn rune_amount(&self, _rune_id: &RuneId) -> Option<u128> {
        None
    }

    /// Always returns `false` because rune information is unavailable when the `runes` feature is disabled.
    pub fn contains_exact_rune(&self, _rune_id: &RuneId, _amount: u128) -> bool {
        false
    }
}
