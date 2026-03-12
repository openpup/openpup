//! Home Assistant L1（只读）集成：读取实体状态。
//! token 不落盘：从 config 指定的 env 变量读取。

use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::{HomeAssistantConfig, OpenpupConfig};
use crate::tools::integrations::net;

fn normalize_base_url(s: &str) -> String {
    s.trim_end_matches('/').to_string()
}

pub fn get_home_assistant_config(cfg: &OpenpupConfig) -> Result<HomeAssistantConfig> {
    cfg.integrations
        .as_ref()
        .and_then(|i| i.home_assistant.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "home_assistant is not configured. Run `openpup add-tool home-assistant`."
            )
        })
}

pub fn get_state(entity_id: &str, ha: &HomeAssistantConfig) -> Result<Value> {
    if !ha.allowed_entities.is_empty() && !ha.allowed_entities.iter().any(|e| e == entity_id) {
        anyhow::bail!(
            "entity_id {} is not in allowed_entities whitelist (edit ~/.openpup/config.toml)",
            entity_id
        );
    }

    let token = std::env::var(&ha.token_env).with_context(|| {
        format!(
            "missing Home Assistant token env var {} (set it before running)",
            ha.token_env
        )
    })?;

    let base = normalize_base_url(&ha.base_url);
    net::block_on_async(async move {
        let url = format!("{}/api/states/{}", base, entity_id);
        let client = net::async_client()?;
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .send()
            .await
            .context("failed to call Home Assistant API")?;

        if !resp.status().is_success() {
            anyhow::bail!("Home Assistant API returned {}", resp.status());
        }

        let v: Value = resp
            .json()
            .await
            .context("failed to parse Home Assistant JSON")?;
        Ok(v)
    })
}
