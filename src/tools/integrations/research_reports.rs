//! 研报搜索与抓取（示例实现，默认返回 mock 数据）。
//!
//! 设计目标：
//! - 提供两个最小可用的 L1 工具：`research_reports_search` 与 `research_reports_fetch`；
//! - 先用本地 mock 数据把链路跑通，后续可以无缝替换为真实数据源（券商官网 / 第三方聚合）。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

/// 研报元数据的统一结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchReportMeta {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub publish_time: String,
    pub summary: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,
}

/// 按关键词 + 时间窗口搜索最新研报（当前为 mock，实现稳定后可接真实 API）。
pub fn search_reports(keywords: &[String], _since_hours: Option<u32>, limit: usize) -> Result<Value> {
    let now = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let joined = if keywords.is_empty() {
        "综合".to_string()
    } else {
        keywords.join(" / ")
    };

    let mut out = Vec::new();
    let max_items = limit.max(1).min(10);
    for idx in 0..max_items {
        let meta = ResearchReportMeta {
            id: format!("mock-{}-{}", joined.replace(' ', "_"), idx + 1),
            title: format!("{}：主题研报示例 {}", joined, idx + 1),
            provider: "Mock Securities".to_string(),
            publish_time: now.clone(),
            summary: format!("围绕「{}」的示例研报摘要，用于验证 openpup 工具链。", joined),
            url: format!("https://example.com/reports/{}/{}", joined.replace(' ', "_"), idx + 1),
            rating: Some("BUY".to_string()),
            target_price: None,
            topics: Some(keywords.to_vec()),
        };
        out.push(serde_json::to_value(meta)?);
    }

    Ok(Value::Array(out))
}

/// 抓取单份研报的“核心内容”（当前为 mock，返回一段示例正文）。
pub fn fetch_report(_url: &str) -> Result<Value> {
    // 真实实现中，这里会：
    // - 下载 PDF / HTML；
    // - 解析「投资观点 / 核心结论 / 风险提示」等章节；
    // - 返回结构化 JSON，供上层 Agent 做进一步提炼。
    Ok(serde_json::json!({
        "content": "（示例正文）这里是研报的核心内容：包含投资观点、行业趋势、盈利预测假设以及主要风险提示等，用于验证研报分析 Agent 的摘要能力。",
    }))
}

