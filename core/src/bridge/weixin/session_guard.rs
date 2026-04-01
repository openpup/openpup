use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};

const SESSION_PAUSE_DURATION_MS: u64 = 60 * 60 * 1000;
pub const SESSION_EXPIRED_ERRCODE: i64 = -14;

fn pause_map() -> &'static Mutex<HashMap<String, u64>> {
    static MAP: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn pause_session(account_id: &str) {
    let until = chrono::Utc::now().timestamp_millis() as u64 + SESSION_PAUSE_DURATION_MS;
    if let Ok(mut map) = pause_map().lock() {
        map.insert(account_id.to_string(), until);
    }
}

pub fn get_remaining_pause_ms(account_id: &str) -> u64 {
    let now = chrono::Utc::now().timestamp_millis() as u64;
    let Ok(mut map) = pause_map().lock() else {
        return 0;
    };
    match map.get(account_id).copied() {
        Some(until) if until > now => until - now,
        Some(_) => {
            map.remove(account_id);
            0
        }
        None => 0,
    }
}

pub fn assert_session_active(account_id: &str) -> Result<()> {
    let remaining = get_remaining_pause_ms(account_id);
    if remaining == 0 {
        return Ok(());
    }
    let minutes = ((remaining as f64) / 60_000.0).ceil() as u64;
    Err(anyhow!(
        "session paused for account `{account_id}`, {minutes} min remaining (errcode {SESSION_EXPIRED_ERRCODE})"
    ))
}
