//! Input bookkeeping for [`TransactionBuilder`].
//!
//! Every input the builder adds goes through [`TransactionBuilder::add_input`], which runs all of
//! its checks before touching any state, so a failed call leaves the builder unchanged. Each
//! added input is recorded as an [`InputRecord`]; [`TransactionBuilder::validate`] re-derives the
//! builder's bookkeeping from those records before the transaction is handed to Arch.

use std::marker::PhantomData;

use arch_program::{input_to_sign::InputToSign, pubkey::Pubkey, rune::RuneAmount};
use arch_satellite_collections::generic::fixed_set::FixedCapacitySet;
use bitcoin::{OutPoint, Transaction, TxIn};

use crate::{
    bytes::txid_to_bytes_big_endian,
    error::BitcoinTxError,
    mempool::{MempoolInfo, TxStatus},
    utxo_info::UtxoInfo,
    TransactionBuilder,
};

/// An input added through the builder: the outpoint it spends and the value credited for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputRecord {
    pub outpoint: OutPoint,
    pub value: u64,
}

/// Builder state captured by [`TransactionBuilder::checkpoint`].
pub(crate) struct Checkpoint<RuneSet> {
    inputs: usize,
    inputs_to_sign: usize,
    total_btc_input: u64,
    tx_statuses: MempoolInfo,
    #[cfg(feature = "runes")]
    total_rune_inputs: RuneSet,
    _phantom: PhantomData<RuneSet>,
}

impl<
        'info,
        const MAX_MODIFIED_ACCOUNTS: usize,
        const MAX_INPUTS_TO_SIGN: usize,
        RuneSet: FixedCapacitySet<Item = RuneAmount> + Default,
    > TransactionBuilder<'info, MAX_MODIFIED_ACCOUNTS, MAX_INPUTS_TO_SIGN, RuneSet>
{
    /// Inserts `tx_in` at `tx_index`, spending `utxo`, and registers `signer` for it.
    pub(crate) fn add_input<RS>(
        &mut self,
        tx_index: usize,
        utxo: &UtxoInfo<RS>,
        status: &TxStatus,
        tx_in: TxIn,
        signer: Option<&Pubkey>,
    ) -> Result<(), BitcoinTxError>
    where
        RS: FixedCapacitySet<Item = RuneAmount>,
    {
        let outpoint = utxo.meta.to_outpoint();
        if tx_in.previous_output != outpoint {
            return Err(BitcoinTxError::UtxoOutpointMismatch);
        }
        if tx_index > self.transaction.input.len() {
            return Err(BitcoinTxError::InvalidInputIndex);
        }
        let index = u32::try_from(tx_index).map_err(|_| BitcoinTxError::InvalidInputIndex)?;
        if self
            .transaction
            .input
            .iter()
            .any(|input| input.previous_output == outpoint)
        {
            return Err(BitcoinTxError::DuplicateInput);
        }
        if signer.is_some() && self.inputs_to_sign.len() >= MAX_INPUTS_TO_SIGN {
            return Err(BitcoinTxError::InputToSignListFull);
        }
        let total_btc_input = self
            .total_btc_input
            .checked_add(utxo.value)
            .ok_or(BitcoinTxError::CalcOverflow)?;
        let tx_statuses = self.tx_statuses_with(utxo, status)?;
        #[cfg(feature = "runes")]
        let total_rune_inputs = {
            let mut runes = self.total_rune_inputs.clone();
            for rune in utxo.runes.as_slice() {
                crate::add_rune_input(&mut runes, rune.clone())?;
            }
            runes
        };

        for input in self.inputs_to_sign.iter_mut() {
            if input.index >= index {
                input.index += 1;
            }
        }
        if let Some(signer) = signer {
            self.inputs_to_sign
                .push(InputToSign {
                    index,
                    signer: *signer,
                })
                .map_err(|_| BitcoinTxError::InputToSignListFull)?;
        }
        self.transaction.input.insert(tx_index, tx_in);
        self.input_records.insert(
            tx_index,
            InputRecord {
                outpoint,
                value: utxo.value,
            },
        );
        self.total_btc_input = total_btc_input;
        self.tx_statuses = tx_statuses;
        #[cfg(feature = "runes")]
        {
            self.total_rune_inputs = total_rune_inputs;
        }

        Ok(())
    }

    /// Returns `tx_statuses` with `utxo`'s mempool ancestry added, counting each parent
    /// transaction once.
    pub(crate) fn tx_statuses_with<RS>(
        &self,
        utxo: &UtxoInfo<RS>,
        status: &TxStatus,
    ) -> Result<MempoolInfo, BitcoinTxError>
    where
        RS: FixedCapacitySet<Item = RuneAmount>,
    {
        let mut tx_statuses = self.tx_statuses;
        let txid = utxo.meta.txid_big_endian();
        let already_counted = self
            .transaction
            .input
            .iter()
            .any(|input| txid_to_bytes_big_endian(&input.previous_output.txid) == txid);

        if let (false, TxStatus::Pending(info)) = (already_counted, status) {
            tx_statuses.total_fee = tx_statuses
                .total_fee
                .checked_add(info.total_fee)
                .ok_or(BitcoinTxError::CalcOverflow)?;
            tx_statuses.total_size = tx_statuses
                .total_size
                .checked_add(info.total_size)
                .ok_or(BitcoinTxError::CalcOverflow)?;
        }

        Ok(tx_statuses)
    }

    /// Checks that the builder's bookkeeping still describes [`Self::transaction`].
    ///
    /// [`Self::finalize`] runs this before handing the transaction to Arch. It fails when:
    /// - the transaction's inputs are not exactly the inputs added through the builder, in the
    ///   same order (for example after pushing, removing or reordering `transaction.input`
    ///   directly) — [`BitcoinTxError::InputBookkeepingMismatch`];
    /// - two inputs spend the same outpoint — [`BitcoinTxError::DuplicateInput`];
    /// - [`Self::total_btc_input`] differs from the sum of the recorded input values —
    ///   [`BitcoinTxError::InputBookkeepingMismatch`];
    /// - an [`InputToSign`] points past the last input, or two point at the same input —
    ///   [`BitcoinTxError::InvalidInputToSign`];
    /// - the outputs spend more than the inputs provide.
    pub fn validate(&self) -> Result<(), BitcoinTxError> {
        let inputs = &self.transaction.input;
        if inputs.len() != self.input_records.len()
            || inputs
                .iter()
                .zip(&self.input_records)
                .any(|(input, record)| input.previous_output != record.outpoint)
        {
            return Err(BitcoinTxError::InputBookkeepingMismatch);
        }

        for (i, input) in inputs.iter().enumerate() {
            if inputs[..i]
                .iter()
                .any(|prev| prev.previous_output == input.previous_output)
            {
                return Err(BitcoinTxError::DuplicateInput);
            }
        }

        let recorded_total = self
            .input_records
            .iter()
            .try_fold(0u64, |total, record| total.checked_add(record.value))
            .ok_or(BitcoinTxError::CalcOverflow)?;
        if recorded_total != self.total_btc_input {
            return Err(BitcoinTxError::InputBookkeepingMismatch);
        }

        let inputs_to_sign = self.inputs_to_sign.as_slice();
        for (i, input_to_sign) in inputs_to_sign.iter().enumerate() {
            if input_to_sign.index as usize >= inputs.len()
                || inputs_to_sign[..i]
                    .iter()
                    .any(|prev| prev.index == input_to_sign.index)
            {
                return Err(BitcoinTxError::InvalidInputToSign);
            }
        }

        self.get_fee_paid()?;

        Ok(())
    }

    /// Captures the state [`Self::rollback`] restores.
    pub(crate) fn checkpoint(&self) -> Checkpoint<RuneSet> {
        Checkpoint {
            inputs: self.transaction.input.len(),
            inputs_to_sign: self.inputs_to_sign.len(),
            total_btc_input: self.total_btc_input,
            tx_statuses: self.tx_statuses,
            #[cfg(feature = "runes")]
            total_rune_inputs: self.total_rune_inputs.clone(),
            _phantom: PhantomData,
        }
    }

    /// Restores the state captured by `checkpoint`. Only correct when every change since the
    /// checkpoint appended inputs, which is all the selection helpers do.
    pub(crate) fn rollback(&mut self, checkpoint: Checkpoint<RuneSet>) {
        self.transaction.input.truncate(checkpoint.inputs);
        self.input_records.truncate(checkpoint.inputs);
        while self.inputs_to_sign.len() > checkpoint.inputs_to_sign {
            self.inputs_to_sign.remove(self.inputs_to_sign.len() - 1);
        }
        self.total_btc_input = checkpoint.total_btc_input;
        self.tx_statuses = checkpoint.tx_statuses;
        #[cfg(feature = "runes")]
        {
            self.total_rune_inputs = checkpoint.total_rune_inputs;
        }
    }
}

/// Pairs each input of `transaction` with the UTXO it spends. Every input must spend a distinct
/// outpoint described by exactly one entry of `user_utxos`, and every entry must be spent.
pub(crate) fn match_inputs_to_utxos<'u, RS>(
    transaction: &Transaction,
    user_utxos: &'u [UtxoInfo<RS>],
) -> Result<Vec<&'u UtxoInfo<RS>>, BitcoinTxError>
where
    RS: FixedCapacitySet<Item = RuneAmount>,
{
    if transaction.input.len() != user_utxos.len() {
        return Err(BitcoinTxError::TransactionInputLengthMustMatchUserUtxosLength);
    }

    let mut matched = Vec::with_capacity(user_utxos.len());
    for (i, input) in transaction.input.iter().enumerate() {
        let outpoint = input.previous_output;
        if transaction.input[..i]
            .iter()
            .any(|prev| prev.previous_output == outpoint)
        {
            return Err(BitcoinTxError::DuplicateInput);
        }

        let mut candidates = user_utxos
            .iter()
            .filter(|utxo| utxo.meta.to_outpoint() == outpoint);
        let utxo = candidates
            .next()
            .ok_or(BitcoinTxError::UtxoNotFoundInUserUtxos)?;
        if candidates.next().is_some() {
            return Err(BitcoinTxError::DuplicateInput);
        }
        matched.push(utxo);
    }

    Ok(matched)
}

/// Records and total BTC value for inputs matched by [`match_inputs_to_utxos`].
pub(crate) fn records_for<RS>(
    matched: &[&UtxoInfo<RS>],
) -> Result<(Vec<InputRecord>, u64), BitcoinTxError>
where
    RS: FixedCapacitySet<Item = RuneAmount>,
{
    let mut total = 0u64;
    let mut records = Vec::with_capacity(matched.len());
    for utxo in matched {
        total = total
            .checked_add(utxo.value)
            .ok_or(BitcoinTxError::CalcOverflow)?;
        records.push(InputRecord {
            outpoint: utxo.meta.to_outpoint(),
            value: utxo.value,
        });
    }

    Ok((records, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc_utxo_holder::BtcUtxoHolder;
    use crate::mempool::MempoolData;
    use crate::utxo_info::SingleRuneSet;
    use arch_program::utxo::UtxoMeta;
    use bitcoin::{
        absolute::LockTime, transaction::Version, Amount, ScriptBuf, Sequence, TxOut, Witness,
    };

    type Builder = TransactionBuilder<'static, 4, 2, SingleRuneSet>;

    const SIGNER: Pubkey = Pubkey([7; 32]);

    fn utxo(txid: u8, vout: u32, value: u64) -> UtxoInfo<SingleRuneSet> {
        UtxoInfo::new(UtxoMeta::from([txid; 32], vout), value)
    }

    fn tx_in(outpoint: OutPoint) -> TxIn {
        TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }
    }

    #[derive(Debug, PartialEq)]
    struct State {
        transaction: Transaction,
        inputs_to_sign: Vec<InputToSign>,
        total_btc_input: u64,
        total_fee: u64,
        total_size: u64,
        records: Vec<InputRecord>,
        #[cfg(feature = "runes")]
        runes: Vec<RuneAmount>,
    }

    fn state(builder: &Builder) -> State {
        State {
            transaction: builder.transaction.clone(),
            inputs_to_sign: builder.inputs_to_sign.as_slice().to_vec(),
            total_btc_input: builder.total_btc_input,
            total_fee: builder.tx_statuses.total_fee,
            total_size: builder.tx_statuses.total_size,
            records: builder.input_records.clone(),
            #[cfg(feature = "runes")]
            runes: builder.total_rune_inputs.as_slice().to_vec(),
        }
    }

    fn builder_with_one_input() -> Builder {
        let mut builder = Builder::new();
        builder
            .add_tx_input(&utxo(1, 0, 10_000), &TxStatus::Confirmed, Some(&SIGNER))
            .unwrap();
        builder
    }

    #[test]
    fn user_input_must_spend_the_described_outpoint() {
        let mut builder = builder_with_one_input();
        let before = state(&builder);
        let described = utxo(2, 0, 1_000_000);
        let actual = utxo(3, 0, 1_000).meta.to_outpoint();

        assert_eq!(
            builder.add_user_tx_input(&described, &TxStatus::Confirmed, tx_in(actual)),
            Err(BitcoinTxError::UtxoOutpointMismatch)
        );
        assert_eq!(
            builder.insert_user_tx_input(0, &described, &TxStatus::Confirmed, &tx_in(actual)),
            Err(BitcoinTxError::UtxoOutpointMismatch)
        );
        assert_eq!(state(&builder), before);
    }

    #[test]
    fn rejects_spending_an_outpoint_twice() {
        let mut builder = builder_with_one_input();
        let before = state(&builder);

        assert_eq!(
            builder.add_tx_input(&utxo(1, 0, 10_000), &TxStatus::Confirmed, None),
            Err(BitcoinTxError::DuplicateInput)
        );
        assert_eq!(state(&builder), before);
    }

    #[test]
    fn rejects_insert_past_the_end() {
        let mut builder = builder_with_one_input();
        let before = state(&builder);

        assert_eq!(
            builder.insert_tx_input(2, &utxo(2, 0, 1), &TxStatus::Confirmed, Some(&SIGNER)),
            Err(BitcoinTxError::InvalidInputIndex)
        );
        assert_eq!(state(&builder), before);
    }

    #[test]
    fn full_signer_list_leaves_indices_unshifted() {
        let mut builder = builder_with_one_input();
        builder
            .add_tx_input(&utxo(2, 0, 1), &TxStatus::Confirmed, Some(&SIGNER))
            .unwrap();
        let before = state(&builder);

        assert_eq!(
            builder.insert_tx_input(0, &utxo(3, 0, 1), &TxStatus::Confirmed, Some(&SIGNER)),
            Err(BitcoinTxError::InputToSignListFull)
        );
        assert_eq!(state(&builder), before);
    }

    #[test]
    fn value_overflow_leaves_builder_unchanged() {
        let mut builder = builder_with_one_input();
        let before = state(&builder);

        assert_eq!(
            builder.add_tx_input(&utxo(2, 0, u64::MAX), &TxStatus::Confirmed, None),
            Err(BitcoinTxError::CalcOverflow)
        );
        assert_eq!(state(&builder), before);
    }

    #[test]
    fn ancestry_overflow_leaves_builder_unchanged() {
        let mut builder = builder_with_one_input();
        builder.tx_statuses.total_fee = u64::MAX;
        let before = state(&builder);
        let pending = TxStatus::Pending(MempoolInfo {
            total_fee: 1,
            total_size: 1,
        });

        assert_eq!(
            builder.add_tx_input(&utxo(2, 0, 1), &pending, None),
            Err(BitcoinTxError::CalcOverflow)
        );
        assert_eq!(state(&builder), before);
    }

    #[cfg(feature = "runes")]
    #[test]
    fn rune_overflow_leaves_builder_unchanged() {
        use arch_program::rune::RuneId;

        fn rune_utxo(txid: u8) -> UtxoInfo<SingleRuneSet> {
            let mut utxo = utxo(txid, 0, 1_000);
            utxo.runes
                .insert(RuneAmount {
                    id: RuneId::new(1, 1),
                    amount: u128::MAX,
                })
                .unwrap();
            utxo
        }

        let mut builder = Builder::new();
        builder
            .add_tx_input(&rune_utxo(1), &TxStatus::Confirmed, Some(&SIGNER))
            .unwrap();
        let before = state(&builder);

        assert_eq!(
            builder.add_tx_input(&rune_utxo(2), &TxStatus::Confirmed, Some(&SIGNER)),
            Err(BitcoinTxError::RuneAdditionOverflow)
        );
        assert_eq!(state(&builder), before);
    }

    #[test]
    fn insert_shifts_signer_indices_and_records() {
        let mut builder = builder_with_one_input();
        builder
            .insert_tx_input(0, &utxo(2, 0, 5), &TxStatus::Confirmed, Some(&SIGNER))
            .unwrap();

        let indices: Vec<u32> = builder.inputs_to_sign.iter().map(|i| i.index).collect();
        assert_eq!(indices, vec![1, 0]);
        assert_eq!(
            builder.input_records[0].outpoint,
            utxo(2, 0, 5).meta.to_outpoint()
        );
        assert_eq!(builder.total_btc_input, 10_005);
        assert_eq!(builder.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_raw_input_mutation() {
        let mut pushed = builder_with_one_input();
        pushed
            .transaction
            .input
            .push(tx_in(utxo(9, 0, 0).meta.to_outpoint()));
        assert_eq!(
            pushed.validate(),
            Err(BitcoinTxError::InputBookkeepingMismatch)
        );

        let mut removed = builder_with_one_input();
        removed.transaction.input.clear();
        assert_eq!(
            removed.validate(),
            Err(BitcoinTxError::InputBookkeepingMismatch)
        );

        let mut reordered = builder_with_one_input();
        reordered
            .add_tx_input(&utxo(2, 0, 1), &TxStatus::Confirmed, None)
            .unwrap();
        reordered.transaction.input.swap(0, 1);
        assert_eq!(
            reordered.validate(),
            Err(BitcoinTxError::InputBookkeepingMismatch)
        );

        let mut substituted = builder_with_one_input();
        substituted.transaction.input[0].previous_output = utxo(9, 0, 0).meta.to_outpoint();
        assert_eq!(
            substituted.validate(),
            Err(BitcoinTxError::InputBookkeepingMismatch)
        );
    }

    #[test]
    fn validate_rejects_inflated_or_deflated_totals() {
        let mut builder = builder_with_one_input();
        builder.total_btc_input += 1;
        assert_eq!(
            builder.validate(),
            Err(BitcoinTxError::InputBookkeepingMismatch)
        );
        builder.total_btc_input -= 2;
        assert_eq!(
            builder.validate(),
            Err(BitcoinTxError::InputBookkeepingMismatch)
        );
    }

    #[test]
    fn validate_rejects_bad_signer_indices() {
        let mut out_of_range = builder_with_one_input();
        out_of_range.inputs_to_sign.as_mut_slice()[0].index = 1;
        assert_eq!(
            out_of_range.validate(),
            Err(BitcoinTxError::InvalidInputToSign)
        );

        let mut duplicated = builder_with_one_input();
        duplicated
            .add_tx_input(&utxo(2, 0, 1), &TxStatus::Confirmed, Some(&SIGNER))
            .unwrap();
        duplicated.inputs_to_sign.as_mut_slice()[1].index = 0;
        assert_eq!(
            duplicated.validate(),
            Err(BitcoinTxError::InvalidInputToSign)
        );
    }

    #[test]
    fn validate_rejects_outputs_exceeding_inputs() {
        let mut builder = builder_with_one_input();
        builder.transaction.output.push(TxOut {
            value: Amount::from_sat(10_001),
            script_pubkey: ScriptBuf::new(),
        });
        assert_eq!(
            builder.validate(),
            Err(BitcoinTxError::InsufficientInputAmount)
        );
    }

    struct Holder(Vec<UtxoInfo<SingleRuneSet>>);

    impl BtcUtxoHolder for Holder {
        fn btc_utxos(&self) -> &[UtxoInfo] {
            &self.0
        }
    }

    #[test]
    fn failed_holder_selection_adds_nothing() {
        let mut builder = Builder::new();
        let before = state(&builder);
        let holders = [Holder(vec![utxo(1, 0, 100), utxo(1, 1, 100)])];

        assert_eq!(
            builder.find_btc_in_utxos_from_holder(&holders, &SIGNER, 1_000, false),
            Err(BitcoinTxError::NotEnoughBtcInPool)
        );
        assert_eq!(state(&builder), before);
    }

    #[test]
    fn failed_list_selection_adds_nothing() {
        let mut builder = Builder::new();
        let before = state(&builder);
        let utxos = [utxo(1, 0, 100), utxo(1, 1, 100), utxo(1, 2, 100)];

        // The third input overflows the two-entry signer list after two were added.
        assert_eq!(
            builder.find_btc_in_utxos(&utxos, &SIGNER, 1_000),
            Err(BitcoinTxError::InputToSignListFull)
        );
        assert_eq!(state(&builder), before);
    }

    fn transaction_spending(outpoints: &[OutPoint]) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: outpoints.iter().copied().map(tx_in).collect(),
            output: vec![],
        }
    }

    fn from_transaction(
        transaction: Transaction,
        user_utxos: &[UtxoInfo<SingleRuneSet>],
    ) -> Result<Builder, BitcoinTxError> {
        Builder::new_with_transaction(transaction, &MempoolData::<1, 1>::default(), user_utxos)
    }

    #[test]
    fn new_with_transaction_totals_come_from_matched_utxos() {
        let (a, b) = (utxo(1, 0, 3_000), utxo(2, 0, 4_000));
        let transaction = transaction_spending(&[b.meta.to_outpoint(), a.meta.to_outpoint()]);

        let builder = from_transaction(transaction, &[a, b]).unwrap();
        assert_eq!(builder.total_btc_input, 7_000);
        assert_eq!(builder.input_records[0].value, 4_000);
        assert_eq!(builder.validate(), Ok(()));
    }

    #[test]
    fn new_with_transaction_rejects_mismatched_metadata() {
        let (a, b) = (utxo(1, 0, 3_000), utxo(2, 0, 4_000));
        let a_out = a.meta.to_outpoint();

        assert_eq!(
            from_transaction(
                transaction_spending(&[a_out, a_out]),
                &[a.clone(), b.clone()]
            )
            .unwrap_err(),
            BitcoinTxError::DuplicateInput
        );
        assert_eq!(
            from_transaction(
                transaction_spending(&[a_out, a_out]),
                &[a.clone(), a.clone()]
            )
            .unwrap_err(),
            BitcoinTxError::DuplicateInput
        );
        assert_eq!(
            from_transaction(transaction_spending(&[a_out]), &[a.clone(), b.clone()]).unwrap_err(),
            BitcoinTxError::TransactionInputLengthMustMatchUserUtxosLength
        );
        let substituted = utxo(3, 0, u64::MAX / 2);
        assert_eq!(
            from_transaction(
                transaction_spending(&[a_out, substituted.meta.to_outpoint()]),
                &[a.clone(), b.clone()]
            )
            .unwrap_err(),
            BitcoinTxError::UtxoNotFoundInUserUtxos
        );
        let big = utxo(4, 0, u64::MAX);
        assert_eq!(
            from_transaction(
                transaction_spending(&[a_out, big.meta.to_outpoint()]),
                &[a, big]
            )
            .unwrap_err(),
            BitcoinTxError::CalcOverflow
        );
    }
}
