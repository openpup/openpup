use std::sync::Arc;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{debug, warn};

const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const API_BASE: &str = "https://api.sgroup.qq.com";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(deserialize_with = "string_or_u64")]
    expires_in: u64,
}

fn string_or_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("invalid number")),
        serde_json::Value::String(s) => s
            .parse::<u64>()
            .map_err(|_| serde::de::Error::custom(format!("cannot parse \"{s}\" as u64"))),
        _ => Err(serde::de::Error::custom("expected number or string")),
    }
}

#[derive(Debug)]
struct TokenState {
    token: String,
    expires_at: std::time::Instant,
}

#[derive(Clone)]
pub struct QQBotClient {
    app_id: String,
    client_secret: String,
    http: Client,
    token_state: Arc<RwLock<Option<TokenState>>>,
}

impl QQBotClient {
    pub fn new(app_id: String, client_secret: String) -> Self {
        Self {
            app_id,
            client_secret,
            http: Client::new(),
            token_state: Arc::new(RwLock::new(None)),
        }
    }

    /// Ensure a valid access token is available, refreshing if needed.
    pub async fn ensure_token(&self) -> Result<()> {
        {
            let guard = self.token_state.read().await;
            if let Some(state) = guard.as_ref() {
                if std::time::Instant::now() < state.expires_at {
                    return Ok(());
                }
            }
        }
        self.refresh_token().await
    }

    /// Force-refresh the access token.
    pub async fn refresh_token(&self) -> Result<()> {
        debug!("[qqbot] refreshing access token");
        let resp = self
            .http
            .post(TOKEN_URL)
            .json(&json!({
                "appId": self.app_id,
                "clientSecret": self.client_secret,
            }))
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("token request failed HTTP {status}: {body}"));
        }

        let token_resp: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| anyhow!("failed to parse token response: {e}, body: {body}"))?;

        // Refresh at 2/3 of lifetime to avoid edge races
        let refresh_in = (token_resp.expires_in * 2 / 3).max(60);
        let expires_at = std::time::Instant::now() + std::time::Duration::from_secs(refresh_in);

        let mut guard = self.token_state.write().await;
        *guard = Some(TokenState {
            token: token_resp.access_token,
            expires_at,
        });
        debug!("[qqbot] token refreshed, valid for ~{refresh_in}s");
        Ok(())
    }

    /// Get the current access token string.
    pub async fn get_token(&self) -> Result<String> {
        let guard = self.token_state.read().await;
        guard
            .as_ref()
            .map(|s| s.token.clone())
            .ok_or_else(|| anyhow!("no access token available"))
    }

    /// Get the WebSocket gateway URL.
    pub async fn get_gateway(&self) -> Result<String> {
        let token = self.get_token().await?;
        let resp = self
            .http
            .get(format!("{API_BASE}/gateway"))
            .header("Authorization", format!("QQBot {token}"))
            .send()
            .await?
            .json::<Value>()
            .await?;
        resp["url"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("gateway response missing url: {resp}"))
    }

    /// Send a text message. Determines endpoint by chat_id prefix convention:
    /// - `c2c:{openid}` → private message
    /// - `group:{group_openid}` → group message
    /// - `channel:{channel_id}` → guild channel message
    pub async fn send_message(
        &self,
        chat_id: &str,
        content: &str,
        reply_to_id: Option<&str>,
    ) -> Result<()> {
        let token = self.get_token().await?;
        let (url, body) = self.build_send_request(chat_id, content, reply_to_id)?;

        debug!("[qqbot] send to {chat_id}: {} chars", content.len());
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            // Rate limited or quota exceeded — log but don't crash
            if status.as_u16() == 429 || status.as_u16() == 304 {
                warn!("[qqbot] send rate limited ({status}): {body_text}");
                return Ok(());
            }
            return Err(anyhow!("qqbot send HTTP {status}: {body_text}"));
        }
        Ok(())
    }

    fn build_send_request(
        &self,
        chat_id: &str,
        content: &str,
        reply_to_id: Option<&str>,
    ) -> Result<(String, Value)> {
        let msg_seq = (chrono::Utc::now().timestamp_millis() & 0xFFFF) as u16;

        if let Some(openid) = chat_id.strip_prefix("c2c:") {
            let mut body = json!({
                "content": content,
                "msg_type": 0,
                "msg_seq": msg_seq,
            });
            if let Some(mid) = reply_to_id {
                body["msg_id"] = json!(mid);
            }
            Ok((format!("{API_BASE}/v2/users/{openid}/messages"), body))
        } else if let Some(group_openid) = chat_id.strip_prefix("group:") {
            let mut body = json!({
                "content": content,
                "msg_type": 0,
                "msg_seq": msg_seq,
            });
            if let Some(mid) = reply_to_id {
                body["msg_id"] = json!(mid);
            }
            Ok((
                format!("{API_BASE}/v2/groups/{group_openid}/messages"),
                body,
            ))
        } else if let Some(channel_id) = chat_id.strip_prefix("channel:") {
            let mut body = json!({
                "content": content,
            });
            if let Some(mid) = reply_to_id {
                body["msg_id"] = json!(mid);
            }
            Ok((format!("{API_BASE}/channels/{channel_id}/messages"), body))
        } else {
            Err(anyhow!(
                "unknown chat_id format: {chat_id} (expected c2c:, group:, or channel: prefix)"
            ))
        }
    }
}
