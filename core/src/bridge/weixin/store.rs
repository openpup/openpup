use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::types::StoredWeixinAccount;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinAccountData {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub saved_at: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

fn state_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openpup")
        .join("weixin")
}

fn accounts_dir() -> PathBuf {
    state_root().join("accounts")
}

fn index_path() -> PathBuf {
    state_root().join("accounts.json")
}

fn sync_path(account_id: &str) -> PathBuf {
    accounts_dir().join(format!("{account_id}.sync.json"))
}

fn account_path(account_id: &str) -> PathBuf {
    accounts_dir().join(format!("{account_id}.json"))
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create directory `{}`", parent.display()))?;
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    ensure_parent(path)?;
    let raw = serde_json::to_string_pretty(value)?;
    std::fs::write(path, raw).with_context(|| format!("write `{}`", path.display()))?;
    Ok(())
}

pub fn list_account_ids() -> Vec<String> {
    read_json::<Vec<String>>(&index_path()).unwrap_or_default()
}

pub fn register_account_id(account_id: &str) -> Result<()> {
    let mut ids = list_account_ids();
    if !ids.iter().any(|id| id == account_id) {
        ids.push(account_id.to_string());
        write_json(&index_path(), &ids)?;
    }
    Ok(())
}

pub fn load_account(account_id: &str) -> Option<WeixinAccountData> {
    read_json(&account_path(account_id))
}

pub fn save_account(account_id: &str, update: WeixinAccountData) -> Result<()> {
    register_account_id(account_id)?;
    let path = account_path(account_id);
    let existing = load_account(account_id).unwrap_or_default();
    let merged = WeixinAccountData {
        token: update.token.or(existing.token),
        saved_at: update
            .saved_at
            .or_else(|| Some(chrono::Utc::now().to_rfc3339())),
        base_url: update.base_url.or(existing.base_url),
        user_id: update.user_id.or(existing.user_id),
    };
    write_json(&path, &merged)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn list_accounts() -> Vec<StoredWeixinAccount> {
    list_account_ids()
        .into_iter()
        .map(|account_id| {
            let data = load_account(&account_id).unwrap_or_default();
            StoredWeixinAccount {
                account_id,
                base_url: data.base_url,
                user_id: data.user_id,
                configured: data
                    .token
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|v| !v.is_empty()),
                saved_at: data.saved_at,
            }
        })
        .collect()
}

pub fn load_get_updates_buf(account_id: &str) -> Option<String> {
    let path = sync_path(account_id);
    read_json::<serde_json::Value>(&path)?
        .get("get_updates_buf")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub fn save_get_updates_buf(account_id: &str, get_updates_buf: &str) -> Result<()> {
    write_json(
        &sync_path(account_id),
        &serde_json::json!({ "get_updates_buf": get_updates_buf }),
    )
}

pub fn clear_get_updates_buf(account_id: &str) -> Result<()> {
    let path = sync_path(account_id);
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("remove `{}`", path.display()))?;
    }
    Ok(())
}
