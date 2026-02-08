use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SolanaCliConfig {
    pub json_rpc_url: Option<String>,
    pub keypair_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuoteResponse {
    pub input_mint: String,
    pub in_amount: String,
    pub output_mint: String,
    pub out_amount: String,
    pub other_amount_threshold: String,
    pub swap_mode: String,
    pub slippage_bps: u16,
    pub price_impact_pct: String,
    pub route_plan: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRequest {
    pub user_public_key: String,
    pub quote_response: QuoteResponse,
    pub wrap_and_unwrap_sol: bool,
    pub dynamic_compute_unit_limit: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapResponse {
    pub swap_transaction: String, // base64
    pub last_valid_block_height: u64,
    #[serde(default)]
    pub prioritization_fee_lamports: Option<u64>,
}
