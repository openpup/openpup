use crate::bridge::types::{OutboundMessage, OutboundType, Platform};

/// Format a layer completion summary for external platform push.
pub fn format_progress(done_pups: &[String], layer_index: usize) -> String {
    if done_pups.is_empty() {
        return format!("Layer {} running\u{2026}", layer_index + 1);
    }
    let items = done_pups
        .iter()
        .map(|p| format!("\u{2713} {}", p))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Layer {} done:\n{}", layer_index + 1, items)
}

/// Default maximum characters per outbound message segment.
/// Telegram limit is 4096 chars; we stay well under to leave room for formatting.
const DEFAULT_SEGMENT_MAX_CHARS: usize = 3800;
/// Discord's Create Message docs allow content up to 2000 characters, so keep
/// some headroom for safe replies and segment markers.
const DISCORD_SEGMENT_MAX_CHARS: usize = 1800;

/// Split a long result into segments that fit within platform message limits.
/// Splits at paragraph boundaries (`\n\n`) when possible, falling back to
/// line boundaries (`\n`), and finally at char boundaries as a last resort.
pub fn format_result(output: &str) -> Vec<String> {
    split_text(output, DEFAULT_SEGMENT_MAX_CHARS, true)
}

/// Expand one logical outbound message into one or more platform-sized messages.
pub fn expand_outbound_message(msg: OutboundMessage) -> Vec<OutboundMessage> {
    let max_chars = match msg.platform {
        Platform::Discord => DISCORD_SEGMENT_MAX_CHARS,
        Platform::Telegram | Platform::Weixin | Platform::QQBot => DEFAULT_SEGMENT_MAX_CHARS,
    };
    let append_segment_index = matches!(
        msg.msg_type,
        OutboundType::Result | OutboundType::Error | OutboundType::PermRequest
    );
    let segments = split_text(&msg.text, max_chars, append_segment_index);
    if segments.is_empty() {
        return vec![];
    }
    segments
        .into_iter()
        .enumerate()
        .map(|(i, text)| OutboundMessage {
            platform: msg.platform.clone(),
            chat_id: msg.chat_id.clone(),
            text,
            reply_to_id: if i == 0 {
                msg.reply_to_id.clone()
            } else {
                None
            },
            msg_type: msg.msg_type.clone(),
        })
        .collect()
}

fn split_text(output: &str, segment_max_chars: usize, append_segment_index: bool) -> Vec<String> {
    if output.is_empty() {
        return vec![];
    }
    if output.chars().count() <= segment_max_chars {
        return vec![output.to_string()];
    }

    let mut segments = Vec::new();
    let mut remaining = output;

    while !remaining.is_empty() {
        if remaining.chars().count() <= segment_max_chars {
            segments.push(remaining.to_string());
            break;
        }

        // Find the byte offset corresponding to the segment budget.
        let byte_limit = remaining
            .char_indices()
            .nth(segment_max_chars)
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());
        let window = &remaining[..byte_limit];

        // Try to split at a paragraph boundary.
        let split_byte = window
            .rmatch_indices("\n\n")
            .next()
            .map(|(i, _)| i)
            // Fall back to a line boundary.
            .or_else(|| window.rfind('\n'))
            // Last resort: split exactly at the char boundary.
            .unwrap_or(byte_limit);

        // Avoid zero-length segments (e.g. leading \n\n).
        let split_byte = if split_byte == 0 {
            byte_limit
        } else {
            split_byte
        };

        segments.push(remaining[..split_byte].to_string());
        remaining = remaining[split_byte..].trim_start_matches('\n');
    }

    // Add segment indicators when there are multiple parts.
    let total = segments.len();
    if total > 1 && append_segment_index {
        for (i, seg) in segments.iter_mut().enumerate() {
            seg.push_str(&format!("\n\n[{}/{}]", i + 1, total));
        }
    }

    segments
}

pub fn format_perm_request(action_desc: &str, skill: &str) -> String {
    format!(
        "\u{26a0}\u{fe0f} {} needs your permission\n\n{}\n\nReply yes to allow, no to deny",
        skill, action_desc
    )
}
