use arch_satellite_lang::arch_program::account::AccountInfo;
use arch_satellite_lang::arch_program::pubkey::Pubkey;
use arch_satellite_lang::Result;
use arch_satellite_lang::{context::CpiContext, Accounts};

pub fn non_transferable_mint_initialize<'info>(
    ctx: CpiContext<'_, '_, '_, 'info, NonTransferableMintInitialize<'info>>,
) -> Result<()> {
    let ix = spl_token_2022::instruction::initialize_non_transferable_mint(
        ctx.accounts.token_program_id.key,
        ctx.accounts.mint.key,
    )?;
    arch_satellite_lang::arch_program::program::invoke_signed(
        &ix,
        &[ctx.accounts.token_program_id, ctx.accounts.mint],
        ctx.signer_seeds,
    )
    .map_err(Into::into)
}

#[derive(Accounts)]
pub struct NonTransferableMintInitialize<'info> {
    pub token_program_id: AccountInfo<'info>,
    pub mint: AccountInfo<'info>,
}
