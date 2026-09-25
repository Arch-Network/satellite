use crate::error::{Error, ErrorCode};
use crate::prelude::{Id, System};
use crate::{AnchorDeserialize, Result};
use arch_program::account::AccountInfo;
use arch_program::pubkey::Pubkey;
use arch_program::system_program;

pub fn close<'info>(info: AccountInfo<'info>, sol_destination: AccountInfo<'info>) -> Result<()> {
    // Transfer tokens from the account to the sol_destination.
    let dest_starting_lamports = sol_destination.lamports();
    **sol_destination.lamports.borrow_mut() =
        dest_starting_lamports.checked_add(info.lamports()).unwrap();
    **info.lamports.borrow_mut() = 0;

    info.assign(&system_program::SYSTEM_PROGRAM_ID);
    info.realloc(0, false).map_err(Into::into)
}

pub fn is_closed(info: &AccountInfo) -> bool {
    info.owner == &System::id() && info.data_is_empty()
}

/// Decodes CPI return data as `T`, accepting it only if `expected_program_id` produced it.
///
/// Return data records the last program that set it, which can be a program the callee invoked
/// rather than the callee itself.
pub fn decode_cpi_return<T: AnchorDeserialize>(
    expected_program_id: &Pubkey,
    return_data: Option<(Pubkey, impl AsRef<[u8]>)>,
) -> Result<T> {
    let (producer, data) = return_data.ok_or(ErrorCode::CpiReturnDataMissing)?;
    if producer != *expected_program_id {
        return Err(Error::from(ErrorCode::CpiReturnProgramMismatch)
            .with_pubkeys((producer, *expected_program_id)));
    }
    T::try_from_slice(data.as_ref()).map_err(|_| ErrorCode::CpiReturnDataInvalid.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALLEE: Pubkey = Pubkey([1; 32]);
    const INNER: Pubkey = Pubkey([2; 32]);

    fn code(result: Result<u64>) -> u32 {
        match result.unwrap_err() {
            Error::AnchorError(e) => e.error_code_number,
            Error::ProgramError(e) => panic!("unexpected program error {e:?}"),
        }
    }

    #[test]
    fn accepts_data_set_by_the_callee() {
        let data = 7u64.to_le_bytes().to_vec();
        assert_eq!(
            decode_cpi_return::<u64>(&CALLEE, Some((CALLEE, data))).unwrap(),
            7
        );
    }

    #[test]
    fn rejects_data_set_by_a_nested_program() {
        let data = 7u64.to_le_bytes().to_vec();
        assert_eq!(
            code(decode_cpi_return::<u64>(&CALLEE, Some((INNER, data)))),
            ErrorCode::CpiReturnProgramMismatch as u32
        );
    }

    #[test]
    fn rejects_missing_data() {
        assert_eq!(
            code(decode_cpi_return::<u64>(&CALLEE, None::<(Pubkey, Vec<u8>)>)),
            ErrorCode::CpiReturnDataMissing as u32
        );
    }

    #[test]
    fn rejects_undecodable_data() {
        assert_eq!(
            code(decode_cpi_return::<u64>(
                &CALLEE,
                Some((CALLEE, vec![1, 2, 3]))
            )),
            ErrorCode::CpiReturnDataInvalid as u32
        );
    }
}
