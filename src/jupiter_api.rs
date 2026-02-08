use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue};

use crate::types::{QuoteResponse, SwapRequest, SwapResponse};

pub fn jup_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("Content-Type", HeaderValue::from_static("application/json"));
    h
}

pub async fn get_quote(
    client: &reqwest::Client,
    headers: &HeaderMap,
    base: &str,
    input_mint: &str,
    output_mint: &str,
    amount_raw: u64,
    slippage_bps: u16,
    dex_param: &str,
) -> Result<QuoteResponse> {
    let quote_url = format!("{}/swap/v1/quote", base);

    let quote_resp = client
        .get(&quote_url)
        .headers(headers.clone())
        .query(&[
            ("inputMint", input_mint),
            ("outputMint", output_mint),
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

    Ok(quote_resp.json().await?)
}

pub async fn get_swap(
    client: &reqwest::Client,
    headers: &HeaderMap,
    swap_url: &str,
    swap_req: &SwapRequest,
) -> Result<SwapResponse> {
    let swap_resp = client
        .post(swap_url)
        .headers(headers.clone())
        .json(swap_req)
        .send()
        .await?;

    if !swap_resp.status().is_success() {
        let t = swap_resp.text().await.unwrap_or_default();
        return Err(anyhow!("Swap build error: {t}"));
    }

    Ok(swap_resp.json().await?)
}
