use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use qrcode::{render::svg, QrCode};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tracing::{debug, warn};

use crate::bridge::types::WeixinConfig;

use super::session_guard::{assert_session_active, pause_session, SESSION_EXPIRED_ERRCODE};
use super::types::{
    BaseInfo, GetConfigReq, GetConfigResp, GetUpdatesReq, GetUpdatesResp, QrCodeResponse,
    QrStatusResponse, SendMessageReq, SendTypingReq,
};

pub const DEFAULT_LONG_POLL_TIMEOUT_MS: u64 = 35_000;
pub const DEFAULT_API_TIMEOUT_MS: u64 = 15_000;
pub const DEFAULT_CONFIG_TIMEOUT_MS: u64 = 10_000;
pub const DEFAULT_ILINK_BOT_TYPE: &str = "3";

#[derive(Clone)]
pub struct WeixinApiClient {
    client: Client,
    base_url: String,
    access_token: String,
    route_tag: Option<String>,
    account_id: String,
}

impl WeixinApiClient {
    pub fn from_config(config: &WeixinConfig) -> Self {
        let mut builder = Client::builder();
        if let Some(proxy_url) = config
            .proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            match reqwest::Proxy::all(proxy_url) {
                Ok(proxy) => builder = builder.proxy(proxy),
                Err(error) => warn!("[weixin] invalid proxy `{proxy_url}`: {error}"),
            }
        }
        let client = builder.build().unwrap_or_else(|error| {
            warn!("[weixin] failed to build client: {error}");
            Client::new()
        });
        Self {
            client,
            base_url: config.base_url.trim().trim_end_matches('/').to_string(),
            access_token: config.access_token.trim().to_string(),
            route_tag: config
                .route_tag
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string),
            account_id: config
                .account_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or("default")
                .to_string(),
        }
    }

    pub fn for_login(
        base_url: &str,
        proxy_url: Option<&str>,
        route_tag: Option<&str>,
        account_id: Option<&str>,
    ) -> Self {
        let mut builder = Client::builder();
        if let Some(proxy_url) = proxy_url.map(str::trim).filter(|v| !v.is_empty()) {
            match reqwest::Proxy::all(proxy_url) {
                Ok(proxy) => builder = builder.proxy(proxy),
                Err(error) => warn!("[weixin] invalid proxy `{proxy_url}`: {error}"),
            }
        }
        let client = builder.build().unwrap_or_else(|error| {
            warn!("[weixin] failed to build client: {error}");
            Client::new()
        });
        Self {
            client,
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            access_token: String::new(),
            route_tag: route_tag
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string),
            account_id: account_id.unwrap_or("default").to_string(),
        }
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn get_updates(&self, cursor: &str, timeout_ms: u64) -> Result<GetUpdatesResp> {
        let body = GetUpdatesReq {
            get_updates_buf: Some(cursor.to_string()),
            base_info: Some(base_info()),
        };
        match self
            .post_json(
                "ilink/bot/getupdates",
                &body,
                timeout_ms.max(DEFAULT_LONG_POLL_TIMEOUT_MS),
                true,
            )
            .await
        {
            Ok(resp) => Ok(resp),
            Err(error) => {
                if error
                    .downcast_ref::<reqwest::Error>()
                    .map(|req_err| req_err.is_timeout())
                    .unwrap_or(false)
                {
                    debug!("[weixin] long poll timed out after {timeout_ms}ms");
                    Ok(GetUpdatesResp {
                        ret: Some(0),
                        errcode: Some(0),
                        errmsg: None,
                        msgs: Some(Vec::new()),
                        get_updates_buf: Some(cursor.to_string()),
                        longpolling_timeout_ms: None,
                    })
                } else {
                    Err(error)
                }
            }
        }
    }

    pub async fn send_message(&self, body: &SendMessageReq) -> Result<()> {
        let _: Value = self
            .post_json("ilink/bot/sendmessage", body, DEFAULT_API_TIMEOUT_MS, true)
            .await?;
        Ok(())
    }

    pub async fn get_config(
        &self,
        user_id: &str,
        context_token: Option<&str>,
    ) -> Result<GetConfigResp> {
        let body = GetConfigReq {
            ilink_user_id: Some(user_id.to_string()),
            context_token: context_token.map(ToString::to_string),
            base_info: Some(base_info()),
        };
        self.post_json(
            "ilink/bot/getconfig",
            &body,
            DEFAULT_CONFIG_TIMEOUT_MS,
            true,
        )
        .await
    }

    pub async fn send_typing(&self, body: &SendTypingReq) -> Result<()> {
        let _: Value = self
            .post_json(
                "ilink/bot/sendtyping",
                body,
                DEFAULT_CONFIG_TIMEOUT_MS,
                true,
            )
            .await?;
        Ok(())
    }

    pub async fn fetch_qrcode(&self, bot_type: Option<&str>) -> Result<QrCodeResponse> {
        let bot_type = bot_type.unwrap_or(DEFAULT_ILINK_BOT_TYPE);
        let mut url = reqwest::Url::parse(&format!("{}/ilink/bot/get_bot_qrcode", self.base_url))?;
        url.query_pairs_mut().append_pair("bot_type", bot_type);
        let mut resp: QrCodeResponse = self
            .get_json(url.as_str(), DEFAULT_CONFIG_TIMEOUT_MS)
            .await?;
        if !resp.qrcode_img_content.trim().is_empty() {
            resp.qrcode_img_content = self
                .normalize_qrcode_payload(&resp.qrcode_img_content)
                .await
                .unwrap_or(resp.qrcode_img_content);
        }
        Ok(resp)
    }

    pub async fn poll_qrcode_status(
        &self,
        qrcode: &str,
        timeout_ms: u64,
    ) -> Result<QrStatusResponse> {
        let mut url =
            reqwest::Url::parse(&format!("{}/ilink/bot/get_qrcode_status", self.base_url))?;
        url.query_pairs_mut().append_pair("qrcode", qrcode);
        match self.get_json(url.as_str(), timeout_ms).await {
            Ok(resp) => Ok(resp),
            Err(error) => {
                if error
                    .downcast_ref::<reqwest::Error>()
                    .map(|req_err| req_err.is_timeout())
                    .unwrap_or(false)
                {
                    Ok(QrStatusResponse {
                        status: "wait".to_string(),
                        ..Default::default()
                    })
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn get_json<T: DeserializeOwned>(&self, url: &str, timeout_ms: u64) -> Result<T> {
        let mut req = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .header("X-WECHAT-UIN", random_wechat_uin());
        if let Some(route_tag) = &self.route_tag {
            req = req.header("SKRouteTag", route_tag);
        }
        let response = req.send().await?;
        let status = response.status();
        let raw = response.text().await.unwrap_or_default();
        debug!("[weixin] GET {} status={} body={}", url, status, raw);
        if !status.is_success() {
            return Err(anyhow!("weixin GET {} HTTP {}: {}", url, status, raw));
        }
        Ok(serde_json::from_str(&raw)?)
    }

    async fn normalize_qrcode_payload(&self, raw: &str) -> Result<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with("data:image/") {
            return Ok(trimmed.to_string());
        }
        if trimmed.starts_with("data:text/html") {
            return Ok(trimmed.to_string());
        }
        render_qrcode_data_url(trimmed)
    }

    async fn post_json<TReq: Serialize, TResp: DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &TReq,
        timeout_ms: u64,
        require_auth: bool,
    ) -> Result<TResp> {
        if require_auth {
            assert_session_active(&self.account_id)?;
        }
        let body_text = serde_json::to_string(body)?;
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("AuthorizationType", "ilink_bot_token")
            .header("Content-Length", body_text.len().to_string())
            .header("X-WECHAT-UIN", random_wechat_uin())
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .body(body_text.clone());
        if let Some(route_tag) = &self.route_tag {
            req = req.header("SKRouteTag", route_tag);
        }
        if require_auth && !self.access_token.is_empty() {
            req = req.bearer_auth(&self.access_token);
        }
        let response = req.send().await?;
        let status = response.status();
        let raw = response.text().await.unwrap_or_default();
        debug!("[weixin] POST {} status={} body={}", endpoint, status, raw);
        if !status.is_success() {
            return Err(anyhow!("weixin {} HTTP {}: {}", endpoint, status, raw));
        }
        let value = serde_json::from_str::<Value>(&raw)?;
        let errcode = value.get("errcode").and_then(Value::as_i64).unwrap_or(0);
        let ret = value.get("ret").and_then(Value::as_i64).unwrap_or(0);
        if errcode == SESSION_EXPIRED_ERRCODE || ret == SESSION_EXPIRED_ERRCODE {
            pause_session(&self.account_id);
        }
        if errcode != 0 || ret != 0 {
            let errmsg = value
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(anyhow!(
                "weixin {} failed: ret={} errcode={} errmsg={}",
                endpoint,
                ret,
                errcode,
                errmsg
            ));
        }
        Ok(serde_json::from_value(value)?)
    }
}

pub fn base_info() -> BaseInfo {
    BaseInfo {
        channel_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    }
}

fn random_wechat_uin() -> String {
    let value = rand::random::<u32>().to_string();
    STANDARD.encode(value.as_bytes())
}

fn render_qrcode_data_url(content: &str) -> Result<String> {
    let code = QrCode::new(content.as_bytes())?;
    let image = code
        .render::<svg::Color<'_>>()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#111111"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        STANDARD.encode(image.as_bytes())
    ))
}
