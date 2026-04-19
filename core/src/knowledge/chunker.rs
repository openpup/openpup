use super::parser::Section;

/// A chunk ready to be embedded and stored.
pub struct ChunkDraft {
    pub content: String,
    pub heading_path: Option<String>,
    pub char_start: Option<usize>,
    pub char_end: Option<usize>,
}

/// Chunk configuration.
pub struct ChunkConfig {
    /// Target chunk size in characters.
    pub target_size: usize,
    /// Overlap between adjacent chunks in characters.
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            // Roughly maps to a mid-sized semantic chunk for mixed zh/en text.
            target_size: 2400,
            overlap: 320,
        }
    }
}

/// Split parsed sections into overlapping chunks.
pub fn chunk_sections(sections: &[Section], config: &ChunkConfig) -> Vec<ChunkDraft> {
    let mut chunks = Vec::new();

    for section in sections {
        let char_count = section.content.chars().count();

        // Short section: single chunk
        if char_count <= config.target_size {
            chunks.push(ChunkDraft {
                content: section.content.clone(),
                heading_path: section.heading_path.clone(),
                char_start: Some(section.char_start),
                char_end: Some(section.char_end),
            });
            continue;
        }

        // Long section: split with overlap
        let sub_chunks = split_with_overlap(&section.content, config.target_size, config.overlap);
        for sub in sub_chunks {
            chunks.push(ChunkDraft {
                content: sub,
                heading_path: section.heading_path.clone(),
                char_start: None,
                char_end: None,
            });
        }
    }

    chunks
}

fn split_with_overlap(text: &str, target: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut result = Vec::new();
    let mut start = 0usize;

    while start < len {
        while start < len && chars[start].is_whitespace() {
            start += 1;
        }
        if start >= len {
            break;
        }

        let mut end = (start + target).min(len);
        if end < len {
            let search_start = start + target.saturating_mul(7) / 10;
            if let Some(boundary) = find_boundary(&chars, search_start.min(end), end) {
                end = boundary;
            }
        }

        if end <= start {
            end = (start + target).min(len);
        }

        let chunk: String = chars[start..end].iter().collect();
        let chunk = chunk.trim().to_string();
        if !chunk.is_empty() {
            result.push(chunk);
        }

        if end >= len {
            break;
        }

        let raw_next_start = end.saturating_sub(overlap.min(end.saturating_sub(start)));
        start = align_overlap_start(&chars, raw_next_start, end);
    }

    result
}

fn find_boundary(chars: &[char], search_start: usize, end: usize) -> Option<usize> {
    for idx in (search_start..end).rev() {
        if is_paragraph_break(chars, idx) {
            return Some(idx + 1);
        }
    }
    for idx in (search_start..end).rev() {
        if is_sentence_break(chars[idx]) {
            return Some(idx + 1);
        }
    }
    for idx in (search_start..end).rev() {
        if chars[idx].is_whitespace() {
            return Some(idx + 1);
        }
    }
    None
}

fn align_overlap_start(chars: &[char], start: usize, end: usize) -> usize {
    let mut idx = start.min(chars.len());

    while idx < end && idx < chars.len() && chars[idx].is_whitespace() {
        idx += 1;
    }

    let window_end = (idx + 160).min(end).min(chars.len());
    for probe in idx..window_end {
        if probe > 0 && is_paragraph_break(chars, probe - 1) {
            return probe;
        }
    }
    for probe in idx..window_end {
        if probe > 0 && is_sentence_break(chars[probe - 1]) {
            return probe;
        }
    }

    idx
}

fn is_sentence_break(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '.' | '!' | '?' | ';' | '；')
}

fn is_paragraph_break(chars: &[char], idx: usize) -> bool {
    chars.get(idx) == Some(&'\n') && chars.get(idx + 1) == Some(&'\n')
}

#[cfg(test)]
mod tests {
    use super::{chunk_sections, ChunkConfig};
    use crate::knowledge::parser::Section;

    #[test]
    fn keeps_short_section_as_single_chunk() {
        let sections = vec![Section {
            heading_path: Some("a".to_string()),
            content: "short text".to_string(),
            char_start: 0,
            char_end: 10,
        }];
        let chunks = chunk_sections(&sections, &ChunkConfig::default());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "short text");
    }

    #[test]
    fn splits_large_section_into_multiple_chunks() {
        let text = format!(
            "{}\n\n{}\n\n{}",
            "第一段。".repeat(400),
            "第二段。".repeat(400),
            "第三段。".repeat(400)
        );
        let sections = vec![Section {
            heading_path: None,
            content: text,
            char_start: 0,
            char_end: 0,
        }];
        let chunks = chunk_sections(&sections, &ChunkConfig::default());
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn overlap_starts_on_trimmed_boundary() {
        let text = format!(
            "{}\n\n{}",
            "A sentence.".repeat(300),
            "B sentence.".repeat(300)
        );
        let sections = vec![Section {
            heading_path: None,
            content: text,
            char_start: 0,
            char_end: 0,
        }];
        let chunks = chunk_sections(&sections, &ChunkConfig::default());
        assert!(chunks
            .iter()
            .all(|c| !c.content.starts_with(char::is_whitespace)));
    }
}
