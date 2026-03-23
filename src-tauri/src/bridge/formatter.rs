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

/// Truncate long results for mobile screens.
pub fn format_result(output: &str) -> String {
    let truncated: String = output.chars().take(800).collect();
    if truncated.len() < output.len() {
        format!("{truncated}\n\n\u{2026}(full result saved locally, open openpup to view)")
    } else {
        output.to_string()
    }
}

pub fn format_perm_request(action_desc: &str, skill: &str) -> String {
    format!(
        "\u{26a0}\u{fe0f} {} needs your permission\n\n{}\n\nReply yes to allow, no to deny",
        skill, action_desc
    )
}
