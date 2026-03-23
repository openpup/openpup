use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::client::WeixinApiClient;
use super::types::CachedConfig;

const CONFIG_CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;
const CONFIG_CACHE_INITIAL_RETRY_MS: i64 = 2_000;
const CONFIG_CACHE_MAX_RETRY_MS: i64 = 60 * 60 * 1000;

#[derive(Clone)]
pub struct WeixinConfigManager {
    client: WeixinApiClient,
    cache: Arc<RwLock<HashMap<String, ConfigCacheEntry>>>,
}

#[derive(Debug, Clone)]
struct ConfigCacheEntry {
    config: CachedConfig,
    next_fetch_at: i64,
    retry_delay_ms: i64,
}

impl WeixinConfigManager {
    pub fn new(client: WeixinApiClient) -> Self {
        Self {
            client,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_for_user(&self, user_id: &str, context_token: Option<&str>) -> CachedConfig {
        let now = chrono::Utc::now().timestamp_millis();
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(user_id) {
                if now < entry.next_fetch_at {
                    return entry.config.clone();
                }
            }
        }

        let mut next_entry = ConfigCacheEntry {
            config: CachedConfig::default(),
            next_fetch_at: now + CONFIG_CACHE_INITIAL_RETRY_MS,
            retry_delay_ms: CONFIG_CACHE_INITIAL_RETRY_MS,
        };
        if let Ok(resp) = self.client.get_config(user_id, context_token).await {
            if resp.ret.unwrap_or(0) == 0 {
                next_entry.config.typing_ticket = resp.typing_ticket.unwrap_or_default();
                next_entry.next_fetch_at =
                    now + (rand::random::<u32>() as i64 % CONFIG_CACHE_TTL_MS.max(1));
                next_entry.retry_delay_ms = CONFIG_CACHE_INITIAL_RETRY_MS;
            }
        } else {
            let old_retry = {
                let cache = self.cache.read().await;
                cache
                    .get(user_id)
                    .map(|entry| entry.retry_delay_ms)
                    .unwrap_or(CONFIG_CACHE_INITIAL_RETRY_MS)
            };
            next_entry.retry_delay_ms = (old_retry * 2).min(CONFIG_CACHE_MAX_RETRY_MS);
            next_entry.next_fetch_at = now + next_entry.retry_delay_ms;
        }

        let config = next_entry.config.clone();
        self.cache
            .write()
            .await
            .insert(user_id.to_string(), next_entry);
        config
    }
}
