mod app;
mod config;
mod constants;
mod jupiter_api;
mod solana_ops;
mod types;
mod ui;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await
}
