use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::{self, ChannelsConfig, FeishuChannelConfig};
use crate::tools::net;

#[derive(Debug, Deserialize)]
struct TenantAccessTokenResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    tenant_access_token: String,
}

#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    code: i32,
    #[serde(default)]
    msg: String,
}

/// 从配置加载 Feishu 通道设置。
fn load_feishu_config() -> Result<FeishuChannelConfig> {
    let cfg = config::load_or_init()?;
    let channels: ChannelsConfig = cfg.channels.unwrap_or_default();
    let feishu = channels.feishu.context(
        "feishu channel is not configured. Add [channels.feishu] to ~/.openpup/config.toml first.",
    )?;
    Ok(feishu)
}

fn feishu_app_from_env(feishu_cfg: &FeishuChannelConfig) -> Result<(String, String)> {
    let app_id_var = feishu_cfg.app_id_env.trim().to_string();
    let app_secret_var = feishu_cfg.app_secret_env.trim().to_string();

    let app_id_name = if app_id_var.is_empty() {
        "FEISHU_APP_ID"
    } else {
        app_id_var.as_str()
    };
    let app_secret_name = if app_secret_var.is_empty() {
        "FEISHU_APP_SECRET"
    } else {
        "FEISHU_APP_SECRET"
    };

    let app_id = std::env::var(app_id_name)
        .with_context(|| format!("missing Feishu app_id in env {}", app_id_name))?;
    let app_secret = std::env::var(app_secret_name)
        .with_context(|| format!("missing Feishu app_secret in env {}", app_secret_name))?;

    Ok((app_id, app_secret))
}

async fn get_tenant_access_token(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<String> {
    let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal/";
    let resp = client
        .post(url)
        .json(&serde_json::json!({
            "app_id": app_id,
            "app_secret": app_secret,
        }))
        .send()
        .await
        .context("feishu tenant_access_token request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "feishu tenant_access_token http error: status={} body={}",
            status,
            body
        ));
    }

    let body: TenantAccessTokenResponse = resp
        .json()
        .await
        .context("parse feishu tenant_access_token response")?;
    if body.code != 0 {
        return Err(anyhow::anyhow!(
            "feishu tenant_access_token error: code={} msg={}",
            body.code,
            body.msg
        ));
    }

    Ok(body.tenant_access_token)
}

async fn send_text_message(
    client: &reqwest::Client,
    tenant_access_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<()> {
    let url = "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id";
    let resp = client
        .post(url)
        .bearer_auth(tenant_access_token)
        .json(&serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": serde_json::json!({ "text": text }).to_string(),
        }))
        .send()
        .await
        .context("feishu send message failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "feishu send message http error: status={} body={}",
            status,
            body
        ));
    }

    let body: SendMessageResponse = resp
        .json()
        .await
        .context("parse feishu send message response")?;
    if body.code != 0 {
        return Err(anyhow::anyhow!(
            "feishu send message error: code={} msg={}",
            body.code,
            body.msg
        ));
    }

    Ok(())
}

/// 向 Feishu 默认群聊发送一条纯文本消息。
///
/// 目前仅作为「通知通道」使用，不接收消息回流；后续可以扩展为完整的事件通道。
pub async fn notify_default_chat(text: &str) -> Result<()> {
    let feishu_cfg = load_feishu_config()?;
    if feishu_cfg.default_chat_id.trim().is_empty() {
        anyhow::bail!("feishu.default_chat_id is empty in config");
    }
    let (app_id, app_secret) = feishu_app_from_env(&feishu_cfg)?;
    let client = net::async_client()?;
    let token = get_tenant_access_token(&client, &app_id, &app_secret).await?;
    send_text_message(&client, &token, feishu_cfg.default_chat_id.trim(), text).await
}

/// 向指定 Feishu chat_id 发送一条纯文本消息（不依赖 default_chat_id）。
pub async fn send_text_to_chat(chat_id: &str, text: &str) -> Result<()> {
    let feishu_cfg = load_feishu_config()?;
    let (app_id, app_secret) = feishu_app_from_env(&feishu_cfg)?;
    let client = net::async_client()?;
    let token = get_tenant_access_token(&client, &app_id, &app_secret).await?;
    send_text_message(&client, &token, chat_id, text).await
}
