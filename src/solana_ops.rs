use anyhow::{anyhow, Context, Result};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
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

pub fn ix_create_ata(payer: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> Instruction {
    let ata_program = Pubkey::from_str(ATA_PROGRAM_ID).unwrap();
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let ata = associated_token_address(owner, mint);

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
        data: vec![],
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

pub fn get_token_decimals(rpc: &RpcClient, mint: &Pubkey) -> Result<u8> {
    let supply = rpc.get_token_supply(mint)?;
    Ok(supply.decimals)
}

pub fn ensure_ata_exists(
    rpc: &RpcClient,
    owner: &Pubkey,
    keypair: &Keypair,
    mint: &Pubkey,
    ata: &Pubkey,
) -> Result<()> {
    let ata_exists = rpc
        .get_account_with_commitment(ata, CommitmentConfig::processed())?
        .value
        .is_some();

    if ata_exists {
        return Ok(());
    }

    println!("\nUSDT ATA не існує. Створюю ATA: {ata}");
    let ix = ix_create_ata(owner, owner, mint);

    let latest = rpc.get_latest_blockhash()?;
    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[ix],
        Some(owner),
        &[keypair],
        latest,
    );

    let sig = rpc
        .send_and_confirm_transaction(&tx)
        .context("Не вдалося створити USDT ATA (перевір, чи є SOL на fee)")?;

    println!("✅ ATA створено. Tx: {sig}");
    Ok(())
}
