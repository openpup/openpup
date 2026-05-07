use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::client::{WeixinApiClient, DEFAULT_ILINK_BOT_TYPE, DEFAULT_LONG_POLL_TIMEOUT_MS};
use super::store::{save_account, WeixinAccountData};
use super::types::{QrStatusResponse, WeixinQrStartResult, WeixinQrWaitResult};

const ACTIVE_LOGIN_TTL_MS: i64 = 5 * 60_000;
const DEFAULT_WAIT_TIMEOUT_MS: i64 = 8 * 60_000;
const MAX_QR_REFRESH_COUNT: usize = 3;

#[derive(Debug, Clone)]
struct ActiveLogin {
    qrcode: String,
    qrcode_url: String,
    started_at_ms: i64,
}

#[derive(Clone, Default)]
pub struct LoginManager {
    active_logins: Arc<RwLock<HashMap<String, ActiveLogin>>>,
}

impl LoginManager {
    pub async fn start_login(
        &self,
        base_url: &str,
        proxy_url: Option<&str>,
        route_tag: Option<&str>,
        account_id: Option<&str>,
        bot_type: Option<&str>,
        force: bool,
    ) -> Result<WeixinQrStartResult> {
        let session_key = account_id
            .map(ToString::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.purge_expired().await;

        if !force {
            let active = self.active_logins.read().await;
            if let Some(existing) = active
                .get(&session_key)
                .filter(|login| is_login_fresh(login))
            {
                return Ok(WeixinQrStartResult {
                    session_key,
                    qrcode_url: Some(existing.qrcode_url.clone()),
                    message: "二维码已就绪，请使用微信扫描。".to_string(),
                });
            }
        }

        let client = WeixinApiClient::for_login(base_url, proxy_url, route_tag, account_id);
        let qr = client
            .fetch_qrcode(bot_type.or(Some(DEFAULT_ILINK_BOT_TYPE)))
            .await?;
        let login = ActiveLogin {
            qrcode: qr.qrcode.clone(),
            qrcode_url: qr.qrcode_img_content.clone(),
            started_at_ms: now_ms(),
        };
        self.active_logins
            .write()
            .await
            .insert(session_key.clone(), login);
        Ok(WeixinQrStartResult {
            session_key,
            qrcode_url: Some(qr.qrcode_img_content),
            message: "使用微信扫描二维码，以完成连接。".to_string(),
        })
    }

    pub async fn wait_login(
        &self,
        session_key: &str,
        base_url: &str,
        proxy_url: Option<&str>,
        route_tag: Option<&str>,
        bot_type: Option<&str>,
        timeout_ms: Option<i64>,
    ) -> Result<WeixinQrWaitResult> {
        let deadline = now_ms() + timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS).max(1_000);
        let client = WeixinApiClient::for_login(base_url, proxy_url, route_tag, Some(session_key));
        let mut refresh_count = 1usize;

        loop {
            let Some(active) = self.active_logins.read().await.get(session_key).cloned() else {
                return Err(anyhow!("当前没有进行中的登录，请先发起登录。"));
            };
            if !is_login_fresh(&active) {
                self.active_logins.write().await.remove(session_key);
                return Ok(WeixinQrWaitResult {
                    connected: false,
                    status: "expired".to_string(),
                    session_key: session_key.to_string(),
                    qrcode_url: None,
                    account_id: None,
                    base_url: None,
                    user_id: None,
                    message: "二维码已过期，请重新生成。".to_string(),
                });
            }
            if now_ms() >= deadline {
                return Ok(WeixinQrWaitResult {
                    connected: false,
                    status: "wait".to_string(),
                    session_key: session_key.to_string(),
                    qrcode_url: Some(active.qrcode_url),
                    account_id: None,
                    base_url: None,
                    user_id: None,
                    message: "登录仍在等待扫码，可继续轮询。".to_string(),
                });
            }

            let status = client
                .poll_qrcode_status(&active.qrcode, DEFAULT_LONG_POLL_TIMEOUT_MS)
                .await?;
            match status.status.as_str() {
                "wait" => continue,
                "scaned" => {
                    return Ok(wait_result(
                        session_key,
                        &active,
                        status,
                        false,
                        "已扫码，请在微信中确认登录。",
                    ));
                }
                "expired" => {
                    refresh_count += 1;
                    if refresh_count > MAX_QR_REFRESH_COUNT {
                        self.active_logins.write().await.remove(session_key);
                        return Ok(WeixinQrWaitResult {
                            connected: false,
                            status: "expired".to_string(),
                            session_key: session_key.to_string(),
                            qrcode_url: None,
                            account_id: None,
                            base_url: None,
                            user_id: None,
                            message: "登录超时：二维码多次过期，请重新开始登录流程。".to_string(),
                        });
                    }
                    let refreshed = client
                        .fetch_qrcode(bot_type.or(Some(DEFAULT_ILINK_BOT_TYPE)))
                        .await?;
                    self.active_logins.write().await.insert(
                        session_key.to_string(),
                        ActiveLogin {
                            qrcode: refreshed.qrcode.clone(),
                            qrcode_url: refreshed.qrcode_img_content.clone(),
                            started_at_ms: now_ms(),
                        },
                    );
                    return Ok(WeixinQrWaitResult {
                        connected: false,
                        status: "expired".to_string(),
                        session_key: session_key.to_string(),
                        qrcode_url: Some(refreshed.qrcode_img_content),
                        account_id: None,
                        base_url: None,
                        user_id: None,
                        message: format!(
                            "二维码已刷新（{refresh_count}/{MAX_QR_REFRESH_COUNT}）。"
                        ),
                    });
                }
                "confirmed" => {
                    let account_id = status
                        .ilink_bot_id
                        .clone()
                        .filter(|v| !v.trim().is_empty())
                        .unwrap_or_else(|| session_key.to_string());
                    save_account(
                        &account_id,
                        WeixinAccountData {
                            token: status.bot_token.clone(),
                            base_url: status
                                .baseurl
                                .clone()
                                .or_else(|| Some(base_url.to_string())),
                            user_id: status.ilink_user_id.clone(),
                            saved_at: Some(chrono::Utc::now().to_rfc3339()),
                        },
                    )?;
                    self.active_logins.write().await.remove(session_key);
                    return Ok(WeixinQrWaitResult {
                        connected: true,
                        status: "confirmed".to_string(),
                        session_key: session_key.to_string(),
                        qrcode_url: None,
                        account_id: Some(account_id),
                        base_url: status.baseurl.or_else(|| Some(base_url.to_string())),
                        user_id: status.ilink_user_id,
                        message: "微信 Bridge 已连接。".to_string(),
                    });
                }
                _ => {
                    return Ok(wait_result(
                        session_key,
                        &active,
                        status,
                        false,
                        "登录状态未知，请继续轮询。",
                    ));
                }
            }
        }
    }

    pub async fn cancel_login(&self, session_key: &str) {
        self.active_logins.write().await.remove(session_key);
    }

    async fn purge_expired(&self) {
        self.active_logins
            .write()
            .await
            .retain(|_, login| is_login_fresh(login));
    }
}

fn wait_result(
    session_key: &str,
    active: &ActiveLogin,
    status: QrStatusResponse,
    connected: bool,
    message: &str,
) -> WeixinQrWaitResult {
    WeixinQrWaitResult {
        connected,
        status: status.status,
        session_key: session_key.to_string(),
        qrcode_url: Some(active.qrcode_url.clone()),
        account_id: status.ilink_bot_id,
        base_url: status.baseurl,
        user_id: status.ilink_user_id,
        message: message.to_string(),
    }
}

fn is_login_fresh(login: &ActiveLogin) -> bool {
    now_ms() - login.started_at_ms < ACTIVE_LOGIN_TTL_MS
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
