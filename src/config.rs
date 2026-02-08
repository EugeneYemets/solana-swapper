use anyhow::Result;
use std::{fs, path::PathBuf};

use crate::types::SolanaCliConfig;

fn load_cli_config() -> Option<SolanaCliConfig> {
    let mut p = dirs::home_dir()?;
    p.push(".config/solana/cli/config.yml");
    let data = fs::read_to_string(p).ok()?;
    serde_yaml::from_str(&data).ok()
}

pub fn resolve_rpc_and_keypair() -> Result<(String, PathBuf)> {
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
