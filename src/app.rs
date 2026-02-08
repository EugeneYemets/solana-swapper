use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rust_decimal::Decimal;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    transaction::VersionedTransaction,
};
use std::str::FromStr;

use crate::{
    config::resolve_rpc_and_keypair,
    constants::{JUP_BASE, USDT_MINT, WSOL_MINT},
    jupiter_api::{get_quote, get_swap, jup_headers},
    solana_ops::{associated_token_address, decimal_to_u64_exact, ensure_ata_exists, get_token_decimals},
    types::{QuoteResponse, SwapRequest},
    ui::read_line,
};

pub async fn run() -> Result<()> {
    println!("=== Solana Interactive Swap (Jupiter -> Raydium/Meteora) ===");

    let (rpc_url, keypair_path) = resolve_rpc_and_keypair()?;
    let keypair: Keypair = read_keypair_file(&keypair_path).map_err(|e| {
        anyhow!(
            "Не можу прочитати keypair: {} | {}",
            keypair_path.display(),
            e
        )
    })?;
    let owner = keypair.pubkey();

    println!("RPC: {rpc_url}");
    println!("Keypair: {}", keypair_path.display());
    println!("Wallet: {owner}");

    let rpc = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    let dex_param = choose_dex().await?;

    // decimals USDT з мережі
    let usdt_mint = Pubkey::from_str(USDT_MINT)?;
    let usdt_decimals = get_token_decimals(&rpc, &usdt_mint)?;

    // USDT ATA
    let usdt_ata = associated_token_address(&owner, &usdt_mint);

    // якщо ATA не існує — створюємо автоматично
    ensure_ata_exists(&rpc, &owner, &keypair, &usdt_mint, &usdt_ata)?;

    // Тепер баланс
    let usdt_bal = rpc
        .get_token_account_balance(&usdt_ata)
        .map_err(|e| anyhow!("Не можу отримати баланс USDT ATA: {usdt_ata} | {e}"))?;

    println!("\nUSDT decimals: {usdt_decimals}");
    println!("USDT ATA: {usdt_ata}");
    println!("USDT баланс: {}", usdt_bal.ui_amount_string);

    let (amount_ui, amount_raw) = ask_amount(&usdt_bal.amount, usdt_decimals).await?;

    let client = reqwest::Client::new();
    let headers = jup_headers();

    // QUOTE
    let slippage_bps = 50u16;
    let quote: QuoteResponse = get_quote(
        &client,
        &headers,
        JUP_BASE,
        USDT_MINT,
        WSOL_MINT,
        amount_raw,
        slippage_bps,
        &dex_param,
    )
    .await?;

    print_quote(&dex_param, amount_ui, &quote)?;

    confirm_swap().await?;

    // SWAP
    let swap_url = format!("{}/swap/v1/swap", JUP_BASE);
    let swap_req = SwapRequest {
        user_public_key: owner.to_string(),
        quote_response: quote.clone(),
        wrap_and_unwrap_sol: true,
        dynamic_compute_unit_limit: Some(true),
    };

    let swap = get_swap(&client, &headers, &swap_url, &swap_req).await?;

    let tx_bytes = STANDARD
        .decode(swap.swap_transaction)
        .context("Не можу base64 decode swapTransaction")?;

    let unsigned_tx: VersionedTransaction =
        bincode::deserialize(&tx_bytes).context("Не можу deserialize VersionedTransaction")?;

    let message = unsigned_tx.message.clone();
    let signed_tx = VersionedTransaction::try_new(message, &[&keypair])
        .context("Не можу підписати транзакцію")?;

    let sig = rpc
        .send_transaction(&signed_tx)
        .context("RPC send_transaction помилка")?;

    println!("\n✅ Відправлено! Tx signature: {sig}");
    println!("lastValidBlockHeight: {}", swap.last_valid_block_height);
    if let Some(p) = swap.prioritization_fee_lamports {
        println!("prioritizationFeeLamports: {p}");
    }

    Ok(())
}

async fn choose_dex() -> Result<String> {
    println!("\nОбери DEX (маршрут буде обмежено тільки ним):");
    println!("  1) Raydium");
    println!("  2) Meteora DLMM");
    let dex_choice = read_line("Твій вибір (1/2): ")?;
    let dex_param = match dex_choice.as_str() {
        "1" => "Raydium",
        "2" => "Meteora+DLMM",
        _ => return Err(anyhow!("Невірний вибір DEX")),
    };
    Ok(dex_param.to_string())
}

async fn ask_amount(balance_raw_str: &str, usdt_decimals: u8) -> Result<(Decimal, u64)> {
    let amount_ui_str = read_line("\nСкільки USDT свапнути в SOL? (наприклад 12.34): ")?;
    let amount_ui =
        Decimal::from_str(&amount_ui_str).map_err(|_| anyhow!("Не можу розпарсити число"))?;
    let amount_raw = decimal_to_u64_exact(&amount_ui, usdt_decimals)?;

    let bal_raw: u64 = balance_raw_str.parse()?;
    if amount_raw > bal_raw {
        return Err(anyhow!(
            "Недостатньо USDT. Потрібно {amount_ui_str}, доступно (raw) {bal_raw}"
        ));
    }

    Ok((amount_ui, amount_raw))
}

fn print_quote(dex_param: &str, amount_ui: Decimal, quote: &QuoteResponse) -> Result<()> {
    let out_lamports: u64 = quote.out_amount.parse()?;
    let out_sol = Decimal::from(out_lamports) / Decimal::from(1_000_000_000u64);

    println!("\nКотирування (DEX={dex_param}):");
    println!("  In:  {} USDT", amount_ui);
    println!("  Out: ~{} SOL (попередньо)", out_sol);
    println!("  priceImpactPct: {}", quote.price_impact_pct);
    println!("  slippageBps: {}", quote.slippage_bps);

    Ok(())
}

async fn confirm_swap() -> Result<()> {
    let confirm = read_line("\nНабери SWAP щоб виконати реальний свап (інакше вихід): ")?;
    if confirm != "SWAP" {
        println!("Вихід без транзакції.");
        return Err(anyhow!("User cancelled"));
    }
    Ok(())
}
