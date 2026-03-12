//! CalDAV L1（只读）：拉取今日事件与 VTODO（基础实现）。
//! 凭证走 env，不落盘。

use anyhow::{Context, Result};
use ical::IcalParser;
use serde_json::Value;

use crate::config::{CaldavConfig, OpenpupConfig};
use crate::tools::integrations::net;

pub fn get_caldav_config(cfg: &OpenpupConfig) -> Result<CaldavConfig> {
    cfg.integrations
        .as_ref()
        .and_then(|i| i.caldav.clone())
        .ok_or_else(|| anyhow::anyhow!("caldav is not configured. Run `openpup add-tool caldav`."))
}

fn basic_auth(cfg: &CaldavConfig) -> Result<(String, String)> {
    let u = std::env::var(&cfg.username_env)
        .with_context(|| format!("missing env var {}", cfg.username_env))?;
    let p = std::env::var(&cfg.password_env)
        .with_context(|| format!("missing env var {}", cfg.password_env))?;
    Ok((u, p))
}

async fn http_get_ics_async(url: &str, user: &str, pass: &str) -> Result<String> {
    let client = net::async_client()?;
    let resp = client
        .get(url)
        .basic_auth(user, Some(pass))
        .send()
        .await
        .context("failed to call CalDAV endpoint")?;
    if !resp.status().is_success() {
        anyhow::bail!("CalDAV HTTP {}", resp.status());
    }
    Ok(resp.text().await.context("failed to read CalDAV body")?)
}

fn http_get_ics(url: &str, user: &str, pass: &str) -> Result<String> {
    net::block_on_async(http_get_ics_async(url, user, pass))
}

/// 解析 ICS，提取简单事件列表（summary, dtstart, dtend）。
pub fn parse_events(ics: &str, limit: usize) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for cal in IcalParser::new(ics.as_bytes()) {
        let cal = cal.context("ical parse error")?;
        for ev in cal.events {
            let mut summary = String::new();
            let mut dtstart = String::new();
            let mut dtend = String::new();
            for p in ev.properties {
                match p.name.as_str() {
                    "SUMMARY" => summary = p.value.unwrap_or_default(),
                    "DTSTART" => dtstart = p.value.unwrap_or_default(),
                    "DTEND" => dtend = p.value.unwrap_or_default(),
                    _ => {}
                }
            }
            if !summary.is_empty() {
                out.push(
                    serde_json::json!({ "summary": summary, "dtstart": dtstart, "dtend": dtend }),
                );
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

/// 解析 ICS，提取简单 task 列表（summary, due, status）。
pub fn parse_tasks(ics: &str, limit: usize) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for cal in IcalParser::new(ics.as_bytes()) {
        let cal = cal.context("ical parse error")?;
        for td in cal.todos {
            let mut summary = String::new();
            let mut due = String::new();
            let mut status = String::new();
            for p in td.properties {
                match p.name.as_str() {
                    "SUMMARY" => summary = p.value.unwrap_or_default(),
                    "DUE" => due = p.value.unwrap_or_default(),
                    "STATUS" => status = p.value.unwrap_or_default(),
                    _ => {}
                }
            }
            if !summary.is_empty() {
                out.push(serde_json::json!({ "summary": summary, "due": due, "status": status }));
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

/// 基础 GET：对每个 calendar url 拉取 ICS（需要你提供能 GET 到 .ics 的 URL；具体 CalDAV REPORT 在后续增强）。
pub fn fetch_ics_blobs(cfg: &CaldavConfig, limit_urls: usize) -> Result<Vec<String>> {
    let (u, p) = basic_auth(cfg)?;
    let urls = if cfg.calendar_urls.is_empty() {
        vec![cfg.base_url.clone()]
    } else {
        cfg.calendar_urls.clone()
    };
    let mut out = Vec::new();
    for url in urls.into_iter().take(limit_urls) {
        out.push(http_get_ics(&url, &u, &p)?);
    }
    Ok(out)
}
