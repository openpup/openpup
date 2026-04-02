pub mod alpha;
pub mod context_builder;
pub mod custom_pup;
pub mod dev_pup;
pub mod life_admin_pup;
pub mod ops_pup;
pub mod research_pup;
pub mod router;
pub mod specialist;
pub mod writer_pup;

/// Safely truncate a UTF-8 string to a maximum byte length without panicking on boundaries.
pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    // Find a valid UTF-8 boundary
    for i in (0..=max_bytes).rev() {
        if s.is_char_boundary(i) {
            return &s[..i];
        }
    }
    // Should never reach here in practice, but fallback to safe empty string
    ""
}
