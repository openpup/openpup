use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ── Job mode ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum JobMode {
    #[default]
    Single,
    Sequential,
    Parallel,
}

// ── Job step ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStep {
    /// Name of the skill to execute.
    pub skill: String,
    /// Input passed to the skill.  In sequential mode an empty string means
    /// "inherit the previous step's output".
    #[serde(default)]
    pub input: String,
}

// ── Scheduled job ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: String,
    pub name: String,
    /// Standard 5-field cron expression: min hour dom month dow
    /// Examples: "0 * * * *" (every hour), "0 8 * * *" (8 AM daily)
    pub schedule: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub mode: JobMode,
    pub steps: Vec<JobStep>,
}

fn default_true() -> bool {
    true
}

// ── Registry ──────────────────────────────────────────────────────────────────

pub struct JobRegistry {
    path: PathBuf,
}

impl JobRegistry {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load all jobs from disk.  Returns an empty list if the file does not
    /// exist or cannot be parsed.
    pub fn load(&self) -> Vec<ScheduledJob> {
        if !self.path.exists() {
            return Vec::new();
        }
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, jobs: &[ScheduledJob]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(jobs)?)?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut jobs = self.load();
        jobs.retain(|j| j.id != id);
        self.save(&jobs)
    }

    pub fn toggle(&self, id: &str, enabled: bool) -> Result<()> {
        let mut jobs = self.load();
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            job.enabled = enabled;
        }
        self.save(&jobs)
    }
}

// ── Cron helpers ──────────────────────────────────────────────────────────────

/// Calculate the next fire time for a cron schedule.
pub fn next_fire_time(schedule: &str) -> Option<i64> {
    use cron::Schedule;
    use std::str::FromStr;
    let seven_field = format!("0 {} *", schedule);
    let sched = Schedule::from_str(&seven_field).ok()?;
    sched.upcoming(chrono::Utc).next().map(|t| t.timestamp())
}

/// Return `true` if the 5-field cron expression `schedule` was due within the
/// 60-second window ending at `now`.  This lets the scheduler run once per
/// minute and correctly fire jobs without double-firing on restart.
pub fn is_due(schedule: &str, now: &chrono::DateTime<chrono::Utc>) -> bool {
    use cron::Schedule;
    use std::str::FromStr;

    // The `cron` crate uses a 7-field format: sec min hour dom month dow year.
    // We accept the familiar 5-field format and expand it.
    let seven_field = format!("0 {} *", schedule);
    let Ok(sched) = Schedule::from_str(&seven_field) else {
        return false;
    };

    let one_minute_ago = *now - chrono::Duration::minutes(1);
    sched
        .after(&one_minute_ago)
        .next()
        .map(|t| t <= *now)
        .unwrap_or(false)
}
