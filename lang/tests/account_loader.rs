use arch_satellite_lang::arch_program::{account::AccountInfo, pubkey::Pubkey, utxo::UtxoMeta};
use arch_satellite_lang::error::{Error, ErrorCode};
use arch_satellite_lang::prelude::*;
use arch_satellite_lang::ZeroCopy;

declare_id!("da075cb2ff5ec6817613de530b692a8735477769da47430cbd8154335c4a8327");

#[account(zero_copy)]
pub struct Data {
    pub a: u64,
    pub b: u64,
}

#[account(zero_copy, discriminator = [])]
pub struct NoDisc {
    pub a: u64,
}

const DATA_LEN: usize = 8 + 16;

fn code(err: Error) -> u32 {
    match err {
        Error::AnchorError(e) => e.error_code_number,
        Error::ProgramError(e) => panic!("unexpected program error {e:?}"),
    }
}

fn account(data: Vec<u8>) -> &'static AccountInfo<'static> {
    Box::leak(Box::new(AccountInfo::new(
        Box::leak(Box::new(Pubkey::new_unique())),
        Box::leak(Box::new(0)),
        data.leak(),
        &ID,
        Box::leak(Box::new(UtxoMeta::default())),
        false,
        true,
        false,
    )))
}

fn loader<T: ZeroCopy + Owner>(data: Vec<u8>) -> AccountLoader<'static, T> {
    AccountLoader::try_from_unchecked(account(data)).unwrap()
}

fn prefixed(disc: &[u8], len: usize) -> Vec<u8> {
    let mut data = vec![0u8; len];
    let n = disc.len().min(len);
    data[..n].copy_from_slice(&disc[..n]);
    data
}

#[test]
fn try_from_rejects_every_undersized_length() {
    for len in 0..DATA_LEN {
        let info = account(prefixed(Data::DISCRIMINATOR, len));
        let err = AccountLoader::<Data>::try_from(info)
            .map(|_| ())
            .unwrap_err();
        let expected = if len < 8 {
            ErrorCode::AccountDiscriminatorNotFound
        } else {
            ErrorCode::AccountDataTooSmall
        };
        assert_eq!(code(err), expected as u32, "len {len}");
    }
}

#[test]
fn load_and_load_mut_reject_every_undersized_length() {
    for len in 8..DATA_LEN {
        let loader = loader::<Data>(prefixed(Data::DISCRIMINATOR, len));
        assert_eq!(
            code(loader.load().map(|_| ()).unwrap_err()),
            ErrorCode::AccountDataTooSmall as u32,
            "load len {len}"
        );
        assert_eq!(
            code(loader.load_mut().map(|_| ()).unwrap_err()),
            ErrorCode::AccountDataTooSmall as u32,
            "load_mut len {len}"
        );
    }
}

#[test]
fn load_init_rejects_every_undersized_length() {
    for len in 0..DATA_LEN {
        let err = loader::<Data>(vec![0u8; len])
            .load_init()
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            code(err),
            ErrorCode::AccountDataTooSmall as u32,
            "len {len}"
        );
    }
}

#[test]
fn exact_length_loads() {
    let info = account(prefixed(Data::DISCRIMINATOR, DATA_LEN));
    let loader = AccountLoader::<Data>::try_from(info).unwrap();
    loader.load_mut().unwrap().b = 7;
    assert_eq!(loader.load().unwrap().b, 7);
}

#[test]
fn empty_discriminator_still_requires_the_full_type() {
    for len in 0..8 {
        let err = AccountLoader::<NoDisc>::try_from(account(vec![0u8; len]))
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            code(err),
            ErrorCode::AccountDataTooSmall as u32,
            "len {len}"
        );
    }
}

#[test]
fn exit_errors_instead_of_panicking_when_the_discriminator_does_not_fit() {
    let err = loader::<Data>(vec![0u8; 4]).exit(&ID).unwrap_err();
    assert_eq!(code(err), ErrorCode::AccountDidNotSerialize as u32);
}
