//! Test helpers and mock types shared between `split_loader` and `update_loader` unit tests.

// NOTE: The entire module is only compiled when running tests.
#![cfg(test)]

use super::super::shard::StateShard;

use arch_program::program_pack::{Pack, Sealed};
use arch_program::utxo;
use arch_program::{account::AccountInfo, pubkey::Pubkey, utxo::UtxoMeta};
use bytemuck::{Pod, Zeroable};
use satellite_bitcoin::utxo_info::UtxoInfoTrait;
use satellite_bitcoin::utxo_info::{SingleRuneSet, UtxoInfo};
use satellite_lang::prelude::Owner;
use satellite_lang::prelude::{AccountLoader, ProgramError};
use satellite_lang::Discriminator;
use satellite_lang::ZeroCopy;

// Increased capacity to comfortably cover edge-case tests that require up to
// 5 UTXOs per shard × 10 shards. 64 is a convenient power-of-two that leaves
// plenty of head-room for future tests.
pub const MAX_BTC_UTXOS: usize = 64;

/// Size of a single `UtxoInfo<SingleRuneSet>` in bytes.
const UTXO_INFO_SIZE: usize = core::mem::size_of::<UtxoInfo<SingleRuneSet>>();
/// Total byte size of the mock shard (must match the layout of the logical
/// fields). We round up to the next multiple of 8 to match the struct's
/// alignment requirements (UtxoInfo is align(8)).
const MOCK_SHARD_RAW_SIZE: usize =
    UTXO_INFO_SIZE * MAX_BTC_UTXOS + UTXO_INFO_SIZE + 1 + 1 + 5;
const MOCK_SHARD_SIZE: usize =
    (MOCK_SHARD_RAW_SIZE + 7) & !7; // round up to multiple of 8

/// Zero-copy mock shard used exclusively in unit tests.
///
/// Stored as a raw byte array so the struct can derive `Copy` + `Pod` (needed
/// by `AccountLoader` / `ZeroCopy`).  Accessor methods reinterpret the bytes
/// into the typed fields.
#[repr(C, align(8))]
#[derive(Clone, Copy, Zeroable, Pod)]
pub struct MockShardZc {
    data: [u8; MOCK_SHARD_SIZE],
}

impl MockShardZc {
    const BTC_UTXOS_OFFSET: usize = 0;
    const RUNE_UTXO_OFFSET: usize = UTXO_INFO_SIZE * MAX_BTC_UTXOS;
    const BTC_UTXO_LEN_OFFSET: usize = Self::RUNE_UTXO_OFFSET + UTXO_INFO_SIZE;
    const HAS_RUNE_OFFSET: usize = Self::BTC_UTXO_LEN_OFFSET + 1;

    fn btc_utxo_len_raw(&self) -> u8 {
        self.data[Self::BTC_UTXO_LEN_OFFSET]
    }

    fn set_btc_utxo_len_raw(&mut self, len: u8) {
        self.data[Self::BTC_UTXO_LEN_OFFSET] = len;
    }

    fn has_rune_raw(&self) -> u8 {
        self.data[Self::HAS_RUNE_OFFSET]
    }

    fn set_has_rune_raw(&mut self, val: u8) {
        self.data[Self::HAS_RUNE_OFFSET] = val;
    }

    fn btc_utxo_ref(&self, idx: usize) -> &UtxoInfo<SingleRuneSet> {
        let start = Self::BTC_UTXOS_OFFSET + idx * UTXO_INFO_SIZE;
        unsafe { &*(self.data[start..].as_ptr() as *const UtxoInfo<SingleRuneSet>) }
    }

    fn btc_utxo_mut(&mut self, idx: usize) -> &mut UtxoInfo<SingleRuneSet> {
        let start = Self::BTC_UTXOS_OFFSET + idx * UTXO_INFO_SIZE;
        unsafe { &mut *(self.data[start..].as_mut_ptr() as *mut UtxoInfo<SingleRuneSet>) }
    }

    fn write_btc_utxo(&mut self, idx: usize, utxo: UtxoInfo<SingleRuneSet>) {
        let start = Self::BTC_UTXOS_OFFSET + idx * UTXO_INFO_SIZE;
        unsafe {
            core::ptr::write(
                self.data[start..].as_mut_ptr() as *mut UtxoInfo<SingleRuneSet>,
                utxo,
            );
        }
    }

    fn rune_utxo_ref(&self) -> &UtxoInfo<SingleRuneSet> {
        unsafe {
            &*(self.data[Self::RUNE_UTXO_OFFSET..].as_ptr() as *const UtxoInfo<SingleRuneSet>)
        }
    }

    fn rune_utxo_mut(&mut self) -> &mut UtxoInfo<SingleRuneSet> {
        unsafe {
            &mut *(self.data[Self::RUNE_UTXO_OFFSET..].as_mut_ptr()
                as *mut UtxoInfo<SingleRuneSet>)
        }
    }

    fn write_rune_utxo(&mut self, utxo: UtxoInfo<SingleRuneSet>) {
        unsafe {
            core::ptr::write(
                self.data[Self::RUNE_UTXO_OFFSET..].as_mut_ptr()
                    as *mut UtxoInfo<SingleRuneSet>,
                utxo,
            );
        }
    }

    pub fn btc_utxos_max_len(&self) -> usize {
        MAX_BTC_UTXOS
    }

    pub fn btc_utxos_len(&self) -> usize {
        self.btc_utxo_len_raw() as usize
    }
}

impl ZeroCopy for MockShardZc {}

impl Sealed for MockShardZc {}

impl Pack for MockShardZc {
    const LEN: usize = core::mem::size_of::<MockShardZc>();

    fn pack_into_slice(&self, dst: &mut [u8]) {
        dst.copy_from_slice(&self.data);
    }

    fn unpack_from_slice(src: &[u8]) -> std::result::Result<Self, ProgramError> {
        let mut shard = Self::default();
        shard.data.copy_from_slice(&src[..Self::LEN]);
        Ok(shard)
    }
}

impl Discriminator for MockShardZc {
    // 16-byte discriminator so the subsequent struct bytes start naturally aligned.
    const DISCRIMINATOR: &'static [u8] = b"mockshard_zc____";
}

impl Owner for MockShardZc {
    fn owner() -> Pubkey {
        Pubkey::default()
    }
}

impl Default for MockShardZc {
    fn default() -> Self {
        Self {
            data: [0u8; MOCK_SHARD_SIZE],
        }
    }
}

// ---------------------------------------------------------------------
// `StateShard` implementation so the mock can be used with library helpers.
// ---------------------------------------------------------------------
impl StateShard<UtxoInfo<SingleRuneSet>, SingleRuneSet> for MockShardZc {
    fn btc_utxos(&self) -> &[UtxoInfo<SingleRuneSet>] {
        let len = self.btc_utxo_len_raw() as usize;
        let start = Self::BTC_UTXOS_OFFSET;
        unsafe {
            core::slice::from_raw_parts(
                self.data[start..].as_ptr() as *const UtxoInfo<SingleRuneSet>,
                len,
            )
        }
    }

    fn btc_utxos_mut(&mut self) -> &mut [UtxoInfo<SingleRuneSet>] {
        let len = self.btc_utxo_len_raw() as usize;
        let start = Self::BTC_UTXOS_OFFSET;
        unsafe {
            core::slice::from_raw_parts_mut(
                self.data[start..].as_mut_ptr() as *mut UtxoInfo<SingleRuneSet>,
                len,
            )
        }
    }

    fn btc_utxos_retain(&mut self, f: &mut dyn FnMut(&UtxoInfo<SingleRuneSet>) -> bool) {
        let len = self.btc_utxo_len_raw() as usize;
        let mut write_idx = 0usize;
        for read_idx in 0..len {
            let keep = f(self.btc_utxo_ref(read_idx));
            if keep {
                if write_idx != read_idx {
                    let cloned = self.btc_utxo_ref(read_idx).clone();
                    self.write_btc_utxo(write_idx, cloned);
                }
                write_idx += 1;
            }
        }
        self.set_btc_utxo_len_raw(write_idx as u8);
    }

    fn add_btc_utxo(&mut self, utxo: UtxoInfo<SingleRuneSet>) -> Option<usize> {
        let len = self.btc_utxo_len_raw() as usize;
        if len >= MAX_BTC_UTXOS {
            return None;
        }
        self.write_btc_utxo(len, utxo);
        self.set_btc_utxo_len_raw((len + 1) as u8);
        Some(len)
    }

    fn btc_utxos_len(&self) -> usize {
        self.btc_utxo_len_raw() as usize
    }

    fn btc_utxos_max_len(&self) -> usize {
        MAX_BTC_UTXOS
    }

    fn rune_utxo(&self) -> Option<&UtxoInfo<SingleRuneSet>> {
        if self.has_rune_raw() == 1 {
            Some(self.rune_utxo_ref())
        } else {
            None
        }
    }

    fn rune_utxo_mut(&mut self) -> Option<&mut UtxoInfo<SingleRuneSet>> {
        if self.has_rune_raw() == 1 {
            Some(self.rune_utxo_mut())
        } else {
            None
        }
    }

    fn clear_rune_utxo(&mut self) {
        self.set_has_rune_raw(0);
    }

    fn set_rune_utxo(&mut self, utxo: UtxoInfo<SingleRuneSet>) {
        self.write_rune_utxo(utxo);
        self.set_has_rune_raw(1);
    }
}

// ---------------------------------------------------------------------
// Account-loader factory helpers
// ---------------------------------------------------------------------

/// Builds an in-memory [`AccountLoader<MockShardZc>`].
pub fn create_loader() -> AccountLoader<'static, MockShardZc> {
    // Leak all heap allocations → `'static` lifetime suitable for tests.
    let key = Box::leak(Box::new(Pubkey::default()));
    let owner = Box::leak(Box::new(Pubkey::default()));
    let utxo = Box::leak(Box::new(UtxoMeta::default()));
    let lamports = Box::leak(Box::new(0u64));

    // ----- Alignment-aware manual buffer --------------------------------------------------
    let disc = MockShardZc::DISCRIMINATOR;
    let struct_size = core::mem::size_of::<MockShardZc>();
    let struct_align = core::mem::align_of::<MockShardZc>();

    // Calculate padding after the discriminator so that the start of MockShardZc
    // is properly aligned.
    let padding = (struct_align - (disc.len() % struct_align)) % struct_align;
    let offset = disc.len() + padding; // where MockShardZc bytes will start

    let total_len = offset + struct_size;

    // Allocate raw memory with the required alignment and size, zero-fill it, write the
    // discriminator, then convert it into a leaked `&'static mut [u8]` slice.

    use std::alloc::{alloc_zeroed, Layout};

    let data: &'static mut [u8] = unsafe {
        let layout = Layout::from_size_align(total_len, struct_align).expect("layout");
        let ptr = alloc_zeroed(layout);
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        // Copy discriminator.
        std::ptr::copy_nonoverlapping(disc.as_ptr(), ptr, disc.len());

        // Turn the allocation into a boxed slice so it will be freed on process exit, then leak
        // it to obtain a `'static` slice reference for the lifetime of the tests.
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(ptr, total_len);
        let boxed_slice: Box<[u8]> = Box::from_raw(slice_ptr);
        Box::leak(boxed_slice)
    };

    // Assemble `AccountInfo`.
    let account_info = AccountInfo::new(
        key, lamports, data, owner, utxo, /* is_signer   = */ false,
        /* is_writable = */ true, /* is_executable = */ false,
    );

    let account_ref: &'static AccountInfo<'static> = Box::leak(Box::new(account_info));

    AccountLoader::try_from_unchecked(account_ref).expect("create loader")
}

// ------------------------------------------------------------------
// Helper functions mirroring `split::common` for test convenience
// ------------------------------------------------------------------

/// Creates a deterministic `UtxoMeta` (txid pattern) for tests.
pub fn random_utxo_meta(vout: u32) -> UtxoMeta {
    UtxoMeta::from([vout as u8; 32], vout)
}

/// Builds a BTC `UtxoInfo` with the given value and vout.
pub fn create_btc_utxo(value: u64, vout: u32) -> UtxoInfo<SingleRuneSet> {
    let utxo_info = UtxoInfo::new(random_utxo_meta(vout), value);
    utxo_info
}

/// Constructs a `MockShardZc` pre-populated with one BTC-UTXO of `initial_btc`.
pub fn create_shard(initial_btc: u64) -> MockShardZc {
    let mut shard = MockShardZc::default();
    if initial_btc > 0 {
        shard.add_btc_utxo(create_btc_utxo(initial_btc, 0));
    }
    shard
}

/// Creates a loader that is pre-initialised with the provided `shard` data.
pub fn create_loader_from(shard: &MockShardZc) -> AccountLoader<'static, MockShardZc> {
    let loader = create_loader();
    {
        let mut mut_ref = loader.load_mut().expect("zero-copy borrow");
        *mut_ref = *shard;
    }
    loader
}

/// Utility: leak an array of loaders built from a `Vec<MockShardZc>` and return a `'static` slice.
pub fn leak_loaders_from_vec(
    shards: Vec<MockShardZc>,
) -> &'static [AccountLoader<'static, MockShardZc>] {
    let mut boxed_vec: Vec<AccountLoader<'static, MockShardZc>> = Vec::with_capacity(shards.len());
    for shard in shards {
        let loader = create_loader_from(&shard);
        boxed_vec.push(loader);
    }
    Box::leak(boxed_vec.into_boxed_slice())
}

// pub fn create_loaders(shards: Vec<MockShardZc>) -> Vec<AccountLoader<'static, MockShardZc>> {
//     shards
//         .into_iter()
//         .map(|shard| create_loader_from(&shard))
//         .collect()
// }

/// Adds multiple BTC-UTXOs to the provided `shard`.
/// Each new UTXO will use a sequential `vout` value starting after the current last index.
/// The helper silently stops once the shard reaches its maximum capacity.
pub fn add_btc_utxos_bulk(shard: &mut MockShardZc, sats_values: &[u64]) {
    let mut next_vout = shard.btc_utxos_len() as u32;
    for &value in sats_values {
        // Respect shard capacity – abort once full.
        if shard
            .add_btc_utxo(create_btc_utxo(value, next_vout))
            .is_none()
        {
            break;
        }
        next_vout = next_vout.saturating_add(1);
    }
}

/// Convenience wrapper to obtain a **mutable** reference to the underlying
/// `MockShardZc` inside an `AccountLoader` and run an arbitrary closure against it.
// pub fn with_loader_mut<R, F: FnOnce(&mut MockShardZc) -> R>(
//     loader: &'static AccountLoader<'static, MockShardZc>,
//     f: F,
// ) -> R {
//     let mut borrow = loader.load_mut().expect("zero-copy borrow");
//     f(&mut borrow)
// }

/// Builds a Rune-bearing `UtxoInfo` with the specified `amount` (whole runes) and `vout`.
#[cfg(feature = "runes")]
pub fn create_rune_utxo(amount: u128, vout: u32) -> UtxoInfo<SingleRuneSet> {
    use arch_program::rune::{RuneAmount, RuneId};
    use satellite_bitcoin::constants::DUST_LIMIT;

    // Single-entry rune set (capacity = 1)
    let mut runes = SingleRuneSet::default();
    // Safe: capacity is 1, so insertion cannot overflow.
    let _ = runes.insert(RuneAmount {
        id: RuneId::new(1, 1),
        amount,
    });

    UtxoInfo::<SingleRuneSet> {
        meta: random_utxo_meta(vout),
        value: DUST_LIMIT, // minimal on-chain value for a valid output
        runes,
        #[cfg(feature = "utxo-consolidation")]
        needs_consolidation: Default::default(),
    }
}
