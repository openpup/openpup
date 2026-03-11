//! 市场行情与资讯（L1 只读）。

use anyhow::{Context, Result};
use rss::Channel;
use serde_json::Value;

use crate::tools::integrations::net;

async fn http_get_text_async(url: &str) -> Result<String> {
    let client = net::async_client()?;
    let resp = client.get(url).send().await.context("http get failed")?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} for {}", resp.status(), url);
    }
    Ok(resp.text().await.context("failed to read body")?)
}

fn http_get_text(url: &str) -> Result<String> {
    net::block_on_async(http_get_text_async(url))
}

/// Stooq 日线报价（无需 key）：https://stooq.com/q/l/?s=aapl.us&i=d
pub fn stooq_quote_daily(symbol: &str) -> Result<Value> {
    let s = symbol.trim().to_lowercase();
    if s.is_empty() {
        anyhow::bail!("symbol is required");
    }
    let url = format!("https://stooq.com/q/l/?s={}&i=d", s);
    let body = http_get_text(&url)?;
    // CSV: Symbol,Date,Time,Open,High,Low,Close,Volume
    let mut lines = body.lines();
    let header = lines.next().unwrap_or("");
    let row = lines.next().unwrap_or("");
    if !header.to_lowercase().contains("symbol") || row.trim().is_empty() {
        anyhow::bail!("unexpected stooq response");
    }
    let cols: Vec<&str> = row.split(',').collect();
    if cols.len() < 8 {
        anyhow::bail!("unexpected stooq csv columns");
    }
    Ok(serde_json::json!({
        "provider": "stooq",
        "symbol": cols[0],
        "date": cols[1],
        "time": cols[2],
        "open": cols[3],
        "high": cols[4],
        "low": cols[5],
        "close": cols[6],
        "volume": cols[7],
        "url": url,
    }))
}

pub fn rss_headlines(feed_url: &str, limit: usize) -> Result<Vec<Value>> {
    let xml = http_get_text(feed_url)?;
    let channel = Channel::read_from(xml.as_bytes()).context("failed to parse RSS")?;
    let mut out = Vec::new();
    for item in channel.items().iter().take(limit) {
        out.push(serde_json::json!({
            "title": item.title().unwrap_or(""),
            "link": item.link().unwrap_or(""),
            "pub_date": item.pub_date().unwrap_or(""),
        }));
    }
    Ok(out)
}

