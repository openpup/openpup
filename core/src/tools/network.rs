/// Network tool implementations for ToolRegistry.

use anyhow::{anyhow, Result};
use openpup_capabilities::HttpRequest;
use scraper::{Html, Selector};
use tracing::debug;

use super::primitive::{truncate_chars, ToolRegistry};

impl ToolRegistry {
    pub(crate) async fn http_get(&self, url: &str) -> Result<String> {
        debug!("[tool/http_get] {}", truncate_chars(url, 120));
        let resp = self
            .capabilities
            .net
            .get(HttpRequest {
                url: url.to_string(),
                user_agent: Some("openpup/0.1".to_string()),
            })
            .await
            .map_err(|e| anyhow!("http_get '{url}': {e}"))?;
        if !(200..300).contains(&resp.status) {
            return Err(anyhow!("http_get '{url}': HTTP {}", resp.status));
        }
        Ok(self.dynamic_truncate(&resp.body))
    }

    pub(crate) async fn web_fetch(&self, url: &str) -> Result<String> {
        debug!("[tool/web_fetch] {}", truncate_chars(url, 120));
        let resp = self
            .capabilities
            .net
            .get(HttpRequest {
                url: url.to_string(),
                user_agent: Some("openpup/0.1".to_string()),
            })
            .await
            .map_err(|e| anyhow!("web_fetch '{url}': {e}"))?;
        if !(200..300).contains(&resp.status) {
            return Err(anyhow!("web_fetch '{url}': HTTP {}", resp.status));
        }

        let document = Html::parse_document(&resp.body);
        let title = Selector::parse("title")
            .ok()
            .and_then(|selector| document.select(&selector).next())
            .map(|node| node.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
            .trim()
            .to_string();
        let text = document.root_element().text().collect::<Vec<_>>().join(" ");
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");

        let result = format!(
            "final_url: {final_url}\ntitle: {title}\ncontent:\n{content}",
            final_url = resp.final_url,
            title = if title.is_empty() {
                "(untitled)"
            } else {
                &title
            },
            content = normalized,
        );
        Ok(self.dynamic_truncate(&result))
    }
}
