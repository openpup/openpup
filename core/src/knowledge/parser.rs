use anyhow::Result;
use std::path::Path;

/// A parsed section of a document.
pub struct Section {
    pub heading_path: Option<String>,
    pub content: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// Result of parsing a file.
pub struct ParsedDocument {
    pub title: String,
    pub mime_type: String,
    pub sections: Vec<Section>,
    pub raw_len: usize,
}

/// Parse a file into sections. Supports .md and .txt.
pub fn parse_file(path: &Path) -> Result<ParsedDocument> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "md" | "markdown" => parse_markdown(path),
        _ => parse_plaintext(path),
    }
}

fn parse_markdown(path: &Path) -> Result<ParsedDocument> {
    let raw = std::fs::read_to_string(path)?;
    let title = extract_md_title(&raw).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    });
    let sections = split_by_headings(&raw);
    let raw_len = raw.len();

    Ok(ParsedDocument {
        title,
        mime_type: "text/markdown".to_string(),
        sections,
        raw_len,
    })
}

fn parse_plaintext(path: &Path) -> Result<ParsedDocument> {
    let raw = std::fs::read_to_string(path)?;
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();
    let raw_len = raw.len();

    let sections = vec![Section {
        heading_path: None,
        content: raw,
        char_start: 0,
        char_end: raw_len,
    }];

    Ok(ParsedDocument {
        title,
        mime_type: "text/plain".to_string(),
        sections,
        raw_len,
    })
}

fn split_by_headings(text: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current_headings: Vec<String> = Vec::new();
    let mut current_start = 0usize;
    let mut current_content = String::new();
    let mut byte_pos = 0usize;

    for line in text.lines() {
        let line_bytes = line.len() + 1; // +1 for newline

        if let Some(level) = heading_level(line) {
            if !current_content.trim().is_empty() {
                let end = byte_pos;
                sections.push(Section {
                    heading_path: heading_path_string(&current_headings),
                    content: current_content.clone(),
                    char_start: current_start,
                    char_end: end,
                });
            }
            current_start = byte_pos;
            current_content = String::new();
            // Update heading stack
            current_headings.truncate(level.saturating_sub(1));
            current_headings.push(line.trim_start_matches('#').trim().to_string());
        }
        current_content.push_str(line);
        current_content.push('\n');
        byte_pos += line_bytes;
    }

    if !current_content.trim().is_empty() {
        sections.push(Section {
            heading_path: heading_path_string(&current_headings),
            content: current_content,
            char_start: current_start,
            char_end: text.len(),
        });
    }

    // If no sections were created (no headings), treat entire text as one section
    if sections.is_empty() && !text.trim().is_empty() {
        sections.push(Section {
            heading_path: None,
            content: text.to_string(),
            char_start: 0,
            char_end: text.len(),
        });
    }

    sections
}

fn heading_level(line: &str) -> Option<usize> {
    if !line.starts_with('#') {
        return None;
    }
    let count = line.chars().take_while(|&c| c == '#').count();
    if count > 0 && count <= 6 && line.chars().nth(count) == Some(' ') {
        Some(count)
    } else {
        None
    }
}

fn heading_path_string(headings: &[String]) -> Option<String> {
    if headings.is_empty() {
        None
    } else {
        Some(headings.join(" > "))
    }
}

fn extract_md_title(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.starts_with("# ") && !l.starts_with("##"))
        .map(|l| l.trim_start_matches('#').trim().to_string())
}
