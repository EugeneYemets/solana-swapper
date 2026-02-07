use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::header::{HeaderMap, HeaderValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    transaction::VersionedTransaction,
};
#[allow(deprecated)]
use solana_sdk::system_program;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    str::FromStr,
};

const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const ATA_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

const JUP_BASE: &str = "https://lite-api.jup.ag";

#[derive(Debug, Deserialize)]
struct SolanaCliConfig {
    json_rpc_url: Option<String>,
    keypair_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct QuoteResponse {
    input_mint: String,
    in_amount: String,
    output_mint: String,
    out_amount: String,
    other_amount_threshold: String,
    swap_mode: String,
    slippage_bps: u16,
    price_impact_pct: String,
    route_plan: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SwapRequest {
    user_public_key: String,
    quote_response: QuoteResponse,
    wrap_and_unwrap_sol: bool,
    dynamic_compute_unit_limit: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapResponse {
    swap_transaction: String, // base64
    last_valid_block_height: u64,
    #[serde(default)]
    prioritization_fee_lamports: Option<u64>,
}
// задає питання та читає 
fn read_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush().ok();
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

fn load_cli_config() -> Option<SolanaCliConfig> {
    let mut p = dirs::home_dir()?;
    p.push(".config/solana/cli/config.yml");
    let data = fs::read_to_string(p).ok()?;
    serde_yaml::from_str(&data).ok()
}

fn resolve_rpc_and_keypair() -> Result<(String, PathBuf)> {
    let env_rpc = std::env::var("SOLANA_RPC_URL").ok();
    let env_keypair = std::env::var("SOLANA_KEYPAIR").ok();
    let cfg = load_cli_config();

    let rpc = env_rpc
        .or_else(|| cfg.as_ref().and_then(|c| c.json_rpc_url.clone()))
        .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string());

    let kp = env_keypair
        .or_else(|| cfg.as_ref().and_then(|c| c.keypair_path.clone()))
        .unwrap_or_else(|| {
            let mut d = dirs::home_dir().expect("home");
            d.push(".config/solana/id.json");
            d.to_string_lossy().to_string()
        });

    Ok((rpc, PathBuf::from(kp)))
}

fn associated_token_address(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let ata_program = Pubkey::from_str(ATA_PROGRAM_ID).unwrap();
    let seeds = &[owner.as_ref(), token_program.as_ref(), mint.as_ref()];
    Pubkey::find_program_address(seeds, &ata_program).0
}

// Create ATA instruction (без додаткових крейтів)
fn ix_create_ata(payer: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> Instruction {
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

fn decimal_to_u64_exact(amount_ui: &Decimal, decimals: u8) -> Result<u64> {
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

fn jup_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("Content-Type", HeaderValue::from_static("application/json"));
    h
}

#[tokio::main]
async fn main() -> Result<()> {
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

    println!("\nОбери DEX (маршрут буде обмежено тільки ним):");
    println!("  1) Raydium");
    println!("  2) Meteora DLMM");
    let dex_choice = read_line("Твій вибір (1/2): ")?;
    let dex_param = match dex_choice.as_str() {
        "1" => "Raydium",
        "2" => "Meteora+DLMM",
        _ => return Err(anyhow!("Невірний вибір DEX")),
    };

    // decimals USDT з мережі
    let usdt_mint = Pubkey::from_str(USDT_MINT)?;
    let supply = rpc.get_token_supply(&usdt_mint)?;
    let usdt_decimals = supply.decimals;

    // USDT ATA
    let usdt_ata = associated_token_address(&owner, &usdt_mint);

    // ✅ НОВЕ: якщо ATA не існує — створюємо автоматично
    let ata_exists = rpc
        .get_account_with_commitment(&usdt_ata, CommitmentConfig::processed())?
        .value
        .is_some();

    if !ata_exists {
        println!("\nUSDT ATA не існує. Створюю ATA: {usdt_ata}");
        let ix = ix_create_ata(&owner, &owner, &usdt_mint);

        let latest = rpc.get_latest_blockhash()?;
        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[ix],
            Some(&owner),
            &[&keypair],
            latest,
        );

        let sig = rpc
            .send_and_confirm_transaction(&tx)
            .context("Не вдалося створити USDT ATA (перевір, чи є SOL на fee)")?;

        println!("✅ ATA створено. Tx: {sig}");
    }

    // Тепер баланс
    let usdt_bal = rpc
        .get_token_account_balance(&usdt_ata)
        .map_err(|e| anyhow!("Не можу отримати баланс USDT ATA: {usdt_ata} | {e}"))?;

    println!("\nUSDT decimals: {usdt_decimals}");
    println!("USDT ATA: {usdt_ata}");
    println!("USDT баланс: {}", usdt_bal.ui_amount_string);

    let amount_ui_str = read_line("\nСкільки USDT свапнути в SOL? (наприклад 12.34): ")?;
    let amount_ui =
        Decimal::from_str(&amount_ui_str).map_err(|_| anyhow!("Не можу розпарсити число"))?;
    let amount_raw = decimal_to_u64_exact(&amount_ui, usdt_decimals)?;

    let bal_raw: u64 = usdt_bal.amount.parse()?;
    if amount_raw > bal_raw {
        return Err(anyhow!(
            "Недостатньо USDT. Потрібно {amount_ui_str}, доступно {}",
            usdt_bal.ui_amount_string
        ));
    }

    let client = reqwest::Client::new();
    let headers = jup_headers();

    // QUOTE
    let quote_url = format!("{}/swap/v1/quote", JUP_BASE);
    let slippage_bps = 50u16;

    let quote_resp = client
        .get(&quote_url)
        .headers(headers.clone())
        .query(&[
            ("inputMint", USDT_MINT),
            ("outputMint", WSOL_MINT),
            ("amount", &amount_raw.to_string()),
            ("swapMode", "ExactIn"),
            ("slippageBps", &slippage_bps.to_string()),
            ("dexes", dex_param),
        ])
        .send()
        .await?;

    if !quote_resp.status().is_success() {
        let t = quote_resp.text().await.unwrap_or_default();
        return Err(anyhow!("Quote error: {t}"));
    }
    let quote: QuoteResponse = quote_resp.json().await?;

    let out_lamports: u64 = quote.out_amount.parse()?;
    let out_sol = Decimal::from(out_lamports) / Decimal::from(1_000_000_000u64);

    println!("\nКотирування (DEX={dex_param}):");
    println!("  In:  {} USDT", amount_ui);
    println!("  Out: ~{} SOL (попередньо)", out_sol);
    println!("  priceImpactPct: {}", quote.price_impact_pct);
    println!("  slippageBps: {}", quote.slippage_bps);

    let confirm = read_line("\nНабери SWAP щоб виконати реальний свап (інакше вихід): ")?;
    if confirm != "SWAP" {
        println!("Вихід без транзакції.");
        return Ok(());
    }

    // SWAP
    let swap_url = format!("{}/swap/v1/swap", JUP_BASE);
    let swap_req = SwapRequest {
        user_public_key: owner.to_string(),
        quote_response: quote.clone(),
        wrap_and_unwrap_sol: true,
        dynamic_compute_unit_limit: Some(true),
    };

    let swap_resp = client
        .post(&swap_url)
        .headers(headers)
        .json(&swap_req)
        .send()
        .await?;

    if !swap_resp.status().is_success() {
        let t = swap_resp.text().await.unwrap_or_default();
        return Err(anyhow!("Swap build error: {t}"));
    }
    let swap: SwapResponse = swap_resp.json().await?;

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



