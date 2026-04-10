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

/// Maximum characters per outbound message segment.
/// Telegram limit is 4096 chars; we stay well under to leave room for formatting.
const SEGMENT_MAX_CHARS: usize = 3800;

/// Split a long result into segments that fit within platform message limits.
/// Splits at paragraph boundaries (`\n\n`) when possible, falling back to
/// line boundaries (`\n`), and finally at char boundaries as a last resort.
pub fn format_result(output: &str) -> Vec<String> {
    if output.is_empty() {
        return vec![];
    }
    if output.chars().count() <= SEGMENT_MAX_CHARS {
        return vec![output.to_string()];
    }

    let mut segments = Vec::new();
    let mut remaining = output;

    while !remaining.is_empty() {
        if remaining.chars().count() <= SEGMENT_MAX_CHARS {
            segments.push(remaining.to_string());
            break;
        }

        // Find the byte offset corresponding to SEGMENT_MAX_CHARS chars.
        let byte_limit = remaining
            .char_indices()
            .nth(SEGMENT_MAX_CHARS)
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
        let split_byte = if split_byte == 0 { byte_limit } else { split_byte };

        segments.push(remaining[..split_byte].to_string());
        remaining = remaining[split_byte..].trim_start_matches('\n');
    }

    // Add segment indicators when there are multiple parts.
    let total = segments.len();
    if total > 1 {
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
