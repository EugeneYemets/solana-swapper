use anyhow::{anyhow, Result};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
#[allow(deprecated)]
use solana_sdk::system_program;
use std::str::FromStr;

use crate::constants::{ATA_PROGRAM_ID, TOKEN_PROGRAM_ID};

pub fn associated_token_address(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let ata_program = Pubkey::from_str(ATA_PROGRAM_ID).unwrap();
    let seeds = &[owner.as_ref(), token_program.as_ref(), mint.as_ref()];
    Pubkey::find_program_address(seeds, &ata_program).0
}

// Create ATA instruction (без додаткових крейтів)
pub fn ix_create_ata(payer: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> Instruction {
    let ata_program = Pubkey::from_str(ATA_PROGRAM_ID).unwrap();
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let ata = associated_token_address(owner, mint);

    // accounts:
    // [0] payer (signer, writable)
    // [1] ata (writable)
    // [2] owner (readonly)
    // [3] mint (readonly)
    // [4] system program (readonly)
    // [5] token program (readonly)
    // [6] rent sysvar (readonly)
    // data: empty
    let rent_sysvar = solana_sdk::sysvar::rent::id();

    Instruction {
        program_id: ata_program,
        accounts: vec![
            solana_sdk::instruction::AccountMeta::new(*payer, true),
            solana_sdk::instruction::AccountMeta::new(ata, false),
            solana_sdk::instruction::AccountMeta::new_readonly(*owner, false),
            solana_sdk::instruction::AccountMeta::new_readonly(*mint, false),
            solana_sdk::instruction::AccountMeta::new_readonly(system_program::id(), false),
            solana_sdk::instruction::AccountMeta::new_readonly(token_program, false),
            solana_sdk::instruction::AccountMeta::new_readonly(rent_sysvar, false),
        ],
        data: vec![], // create_associated_token_account має порожні data
    }
}

pub fn decimal_to_u64_exact(amount_ui: &Decimal, decimals: u8) -> Result<u64> {
    if amount_ui.is_sign_negative() || amount_ui.is_zero() {
        return Err(anyhow!("Сума має бути > 0"));
    }
    let pow10 = 10u64
        .checked_pow(decimals as u32)
        .ok_or_else(|| anyhow!("decimals overflow"))?;
    let scaled = *amount_ui * Decimal::from(pow10);
    if !scaled.fract().is_zero() {
        return Err(anyhow!(
            "Забагато знаків після коми для цього токена (decimals={decimals})"
        ));
    }
    scaled.to_u64().ok_or_else(|| anyhow!("Не влізло в u64"))
}
