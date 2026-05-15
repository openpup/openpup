use pulldown_cmark::{html, Options, Parser};
use serde::Serialize;
use serde::Deserialize;
use tauri::State;

use super::AppState;

#[derive(Serialize)]
pub struct TimelineEvent {
    pub role: String,
    pub pup_key: String,
    pub pup_name: String,
    pub content: String,
    pub timestamp: i64,
}

#[derive(Debug, Deserialize)]
pub struct TimelineExportEvent {
    pub role: String,
    pub pup_key: String,
    pub pup_name: String,
    pub content: String,
    pub timestamp: i64,
}

fn format_timestamp_local(timestamp: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "invalid-timestamp".to_string())
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn markdown_to_html(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(input, options);
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

fn event_icon(role: &str, pup_key: &str) -> &'static str {
    if role == "user" {
        "👤"
    } else if pup_key == "alpha" {
        "🐶"
    } else {
        "🤖"
    }
}

#[tauri::command]
pub async fn list_timeline_events(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<TimelineEvent>, String> {
    let rows = state
        .app
        .timeline_events(limit)
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|(role, content, timestamp)| TimelineEvent {
            pup_key: if role == "assistant" {
                "alpha".to_string()
            } else {
                "you".to_string()
            },
            pup_name: if role == "assistant" {
                "Alpha".to_string()
            } else {
                "You".to_string()
            },
            role,
            content,
            timestamp,
        })
        .collect())
}

#[tauri::command]
pub async fn export_timeline_content(
    format: String,
    events: Vec<TimelineExportEvent>,
) -> Result<String, String> {
    match format.as_str() {
        "markdown" => {
            let mut out = String::from("# Timeline Export\n\n");
            for event in events {
                let icon = event_icon(&event.role, &event.pup_key);
                out.push_str(&format!(
                    "## {} {} · {} · {}\n\n{}\n\n",
                    icon,
                    event.pup_name,
                    event.role,
                    format_timestamp_local(event.timestamp),
                    event.content
                ));
            }
            Ok(out)
        }
        "html" => {
            let mut out = String::from(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>Timeline Export</title><style>\
body{font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif;max-width:900px;margin:32px auto;padding:0 20px;color:#111827;background:#fafaf9;}\
h1{font-size:28px;margin-bottom:24px;}\
.item{border:1px solid #e5e7eb;border-radius:12px;background:#fff;padding:16px 18px;margin-bottom:14px;}\
.meta{font-size:12px;color:#6b7280;margin-bottom:10px;}\
.meta .icon{margin-right:6px;}\
.content{line-height:1.7;font-size:14px;color:#111827;}\
.content p{margin:0 0 10px;}\
.content pre{padding:12px 14px;background:#111827;color:#f3f4f6;border-radius:10px;overflow:auto;}\
.content code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;}\
.content :last-child{margin-bottom:0;}\
</style></head><body><h1>Timeline Export</h1>",
            );
            for event in events {
                let icon = event_icon(&event.role, &event.pup_key);
                out.push_str(&format!(
                    "<section class=\"item\"><div class=\"meta\"><span class=\"icon\">{}</span>{} · {} · {}</div><div class=\"content\">{}</div></section>",
                    icon,
                    html_escape(&event.pup_name),
                    html_escape(&event.role),
                    html_escape(&format_timestamp_local(event.timestamp)),
                    markdown_to_html(&event.content)
                ));
            }
            out.push_str("</body></html>");
            Ok(out)
        }
        _ => Err("unsupported export format".to_string()),
    }
}

#[tauri::command]
pub async fn list_diary_dates(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.app.list_diary_dates())
}

#[tauri::command]
pub async fn read_diary_entry(state: State<'_, AppState>, date: String) -> Result<String, String> {
    state.app.read_diary(&date).map_err(|e| e.to_string())
}
