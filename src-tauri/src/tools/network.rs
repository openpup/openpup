/// Network tool implementations for ToolRegistry.

use anyhow::{anyhow, Result};
use scraper::{Html, Selector};
use tracing::debug;

use super::primitive::{truncate_chars, ToolRegistry};

impl ToolRegistry {
    pub(crate) async fn http_get(&self, url: &str) -> Result<String> {
        debug!("[tool/http_get] {}", truncate_chars(url, 120));
        let resp = self
            .http
            .get(url)
            .header("User-Agent", "openpup/0.1")
            .send()
            .await
            .map_err(|e| anyhow!("http_get '{url}': {e}"))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("http_get '{url}': HTTP {status}"));
        }
        Ok(self.dynamic_truncate(&body))
    }

    pub(crate) async fn web_fetch(&self, url: &str) -> Result<String> {
        debug!("[tool/web_fetch] {}", truncate_chars(url, 120));
        let resp = self
            .http
            .get(url)
            .header("User-Agent", "openpup/0.1")
            .send()
            .await
            .map_err(|e| anyhow!("web_fetch '{url}': {e}"))?;
        let status = resp.status();
        let final_url = resp.url().to_string();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("web_fetch '{url}': HTTP {status}"));
        }

        let document = Html::parse_document(&body);
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
            "final_url: {final_url}\ntitle: {}\ncontent:\n{}",
            if title.is_empty() {
                "(untitled)"
            } else {
                &title
            },
            normalized
        );
        Ok(self.dynamic_truncate(&result))
    }
}
