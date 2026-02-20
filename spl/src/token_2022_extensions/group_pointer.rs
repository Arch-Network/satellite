use arch_satellite_lang::arch_program::account::AccountInfo;
use arch_satellite_lang::arch_program::pubkey::Pubkey;
use arch_satellite_lang::Result;
use arch_satellite_lang::{context::CpiContext, Accounts};

pub fn group_pointer_initialize<'info>(
    ctx: CpiContext<'_, '_, '_, 'info, GroupPointerInitialize<'info>>,
    authority: Option<Pubkey>,
    group_address: Option<Pubkey>,
) -> Result<()> {
    let ix = spl_token_2022::extension::group_pointer::instruction::initialize(
        ctx.accounts.token_program_id.key,
        ctx.accounts.mint.key,
        authority,
        group_address,
    )?;
    arch_satellite_lang::arch_program::program::invoke_signed(
        &ix,
        &[ctx.accounts.token_program_id, ctx.accounts.mint],
        ctx.signer_seeds,
    )
    .map_err(Into::into)
}

#[derive(Accounts)]
pub struct GroupPointerInitialize<'info> {
    pub token_program_id: AccountInfo<'info>,
    pub mint: AccountInfo<'info>,
}

pub fn group_pointer_update<'info>(
    ctx: CpiContext<'_, '_, '_, 'info, GroupPointerUpdate<'info>>,
    group_address: Option<Pubkey>,
) -> Result<()> {
    let ix = spl_token_2022::extension::group_pointer::instruction::update(
        ctx.accounts.token_program_id.key,
        ctx.accounts.mint.key,
        ctx.accounts.authority.key,
        &[ctx.accounts.authority.key],
        group_address,
    )?;
    arch_satellite_lang::arch_program::program::invoke_signed(
        &ix,
        &[ctx.accounts.token_program_id, ctx.accounts.mint],
        ctx.signer_seeds,
    )
    .map_err(Into::into)
}

#[derive(Accounts)]
pub struct GroupPointerUpdate<'info> {
    pub token_program_id: AccountInfo<'info>,
    pub mint: AccountInfo<'info>,
    pub authority: AccountInfo<'info>,
}
