use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Local;

pub struct FileLayer {
    root: PathBuf,
}

impl FileLayer {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn ensure_workspace_initialized(&self) -> Result<()> {
        fs::create_dir_all(self.root.join("memories"))?;
        for name in ["OWNER.md", "PUPS.md", "RULES.md"] {
            let path = self.root.join(name);
            if !path.exists() {
                fs::write(&path, format!("# {name}\n"))?;
            }
        }
        Ok(())
    }

    pub fn read_owner_profile(&self) -> Result<String> {
        Ok(fs::read_to_string(self.root.join("OWNER.md"))?)
    }

    pub fn read_pups_profile(&self) -> Result<String> {
        Ok(fs::read_to_string(self.root.join("PUPS.md"))?)
    }

    /// Write structured onboarding data to OWNER.md.
    pub fn write_owner_profile(&self, content: &str) -> Result<()> {
        fs::write(self.root.join("OWNER.md"), content)?;
        Ok(())
    }

    /// Onboarding is complete when OWNER.md contains the structured sections
    /// written by the onboarding flow (not just the blank template).
    pub fn is_onboarding_completed(&self) -> bool {
        let path = self.root.join("OWNER.md");
        fs::read_to_string(&path)
            .map(|c| c.contains("## Boundaries") && c.len() > 80)
            .unwrap_or(false)
    }

    /// List all diary dates that have entries, sorted newest-first.
    pub fn list_diary_dates(&self) -> Vec<String> {
        let dir = self.root.join("memories");
        let mut dates = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        dates.push(stem.to_string());
                    }
                }
            }
        }
        dates.sort_unstable_by(|a, b| b.cmp(a)); // newest first
        dates
    }

    /// Read the diary for a specific date (YYYY-MM-DD). Returns error if not found.
    pub fn read_diary(&self, date: &str) -> Result<String> {
        // Validate date format to prevent path traversal
        if !date.chars().all(|c| c.is_ascii_digit() || c == '-') || date.len() != 10 {
            return Err(anyhow::anyhow!("invalid date format"));
        }
        let path = self.root.join("memories").join(format!("{date}.md"));
        Ok(fs::read_to_string(path)?)
    }

    /// Append a block of text to RULES.md (used by Alpha heartbeat self-evolution).
    pub fn append_rules(&self, content: &str) -> Result<()> {
        let path = self.root.join("RULES.md");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        use std::io::Write;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    /// Append one or more bullet entries to today's memory diary.
    /// Creates `memories/YYYY-MM-DD.md` if it doesn't exist.
    pub fn append_daily_diary(&self, entries: &[String]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let today = Local::now().format("%Y-%m-%d").to_string();
        let path = self.root.join("memories").join(format!("{today}.md"));

        let header = if path.exists() {
            String::new()
        } else {
            format!("# 记忆日记 {today}\n\n")
        };

        let now = Local::now().format("%H:%M").to_string();
        let bullet_block = entries
            .iter()
            .map(|e| format!("- [{now}] {e}"))
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!("{header}{bullet_block}\n");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        use std::io::Write;
        file.write_all(content.as_bytes())?;
        Ok(())
    }
}
