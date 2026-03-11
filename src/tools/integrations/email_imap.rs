//! IMAP L1（只读）：只读取未读邮件的 envelope（标题/发件人/日期），不下载正文。
//! 凭证走 env，不落盘。

use anyhow::{Context, Result};
use imap::types::Fetch;
use native_tls::TlsConnector;

use crate::config::{ImapConfig, OpenpupConfig};

pub fn get_imap_config(cfg: &OpenpupConfig) -> Result<ImapConfig> {
    cfg.integrations
        .as_ref()
        .and_then(|i| i.imap.clone())
        .ok_or_else(|| anyhow::anyhow!("imap is not configured. Run `openpup add-tool imap`."))
}

pub fn unread_envelopes(
    cfg: &ImapConfig,
    mailbox: &str,
    limit: usize,
) -> Result<Vec<(String, String, String)>> {
    if !cfg.allowed_mailboxes.is_empty()
        && !cfg
            .allowed_mailboxes
            .iter()
            .any(|m| m.eq_ignore_ascii_case(mailbox))
    {
        anyhow::bail!(
            "mailbox {} is not allowed (edit ~/.openpup/config.toml)",
            mailbox
        );
    }

    let username = std::env::var(&cfg.username_env)
        .with_context(|| format!("missing env var {}", cfg.username_env))?;
    let password = std::env::var(&cfg.password_env)
        .with_context(|| format!("missing env var {}", cfg.password_env))?;

    let tls = TlsConnector::builder()
        .build()
        .context("failed to build TLS connector")?;
    let client = imap::connect((cfg.host.as_str(), cfg.port), cfg.host.as_str(), &tls)
        .context("failed to connect to IMAP")?;

    let mut session = client
        .login(username, password)
        .map_err(|e| e.0)
        .context("failed to login to IMAP")?;

    session
        .select(mailbox)
        .context("failed to select mailbox")?;

    // UNSEEN messages
    let ids = session
        .search("UNSEEN")
        .context("failed to search UNSEEN")?;
    if ids.is_empty() {
        let _ = session.logout();
        return Ok(vec![]);
    }

    let mut out = Vec::new();
    let mut v: Vec<u32> = ids.iter().copied().collect();
    v.sort_unstable();
    let fetch_ids = v
        .into_iter()
        .rev()
        .take(limit)
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .fetch(fetch_ids, "ENVELOPE")
        .context("failed to fetch ENVELOPE")?;
    for f in fetches.iter() {
        if let Some((subject, from, date)) = extract_envelope(f) {
            out.push((subject, from, date));
        }
    }
    let _ = session.logout();
    Ok(out)
}

fn extract_envelope(f: &Fetch) -> Option<(String, String, String)> {
    let env = f.envelope()?;
    let subject = env
        .subject
        .as_ref()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();
    let date = env
        .date
        .as_ref()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();
    let from = env
        .from
        .as_ref()
        .and_then(|addrs| addrs.get(0))
        .and_then(|a| a.mailbox.as_ref().or(a.name.as_ref()))
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();
    Some((subject, from, date))
}

