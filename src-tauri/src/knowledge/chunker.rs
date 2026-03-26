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
            target_size: 800,
            overlap: 100,
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
    let sentences = split_sentences(text);
    let mut result = Vec::new();
    let mut current = String::new();

    for sentence in &sentences {
        let cur_len = current.chars().count();
        let sent_len = sentence.chars().count();

        if cur_len + sent_len > target && !current.is_empty() {
            result.push(current.clone());
            // Keep tail as overlap for next chunk
            let tail: String = current
                .chars()
                .rev()
                .take(overlap)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            current = tail;
        }
        current.push_str(sentence);
    }

    if !current.trim().is_empty() {
        result.push(current);
    }

    result
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '.' | '!' | '?') {
            result.push(std::mem::take(&mut current));
        } else if ch == '\n' && current.ends_with("\n\n") {
            result.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}
