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

// ── Notification config ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NotifyWhen {
    Always,
    #[default]
    OnFailure,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    #[serde(default)]
    pub when: NotifyWhen,
    /// Channels to notify: "weixin", "qqbot". Empty = app event only.
    #[serde(default)]
    pub channels: Vec<String>,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            when: NotifyWhen::OnFailure,
            channels: vec![],
        }
    }
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
    #[serde(default)]
    pub notify: NotifyConfig,
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

/// Calculate the next fire time for a cron schedule (local timezone).
pub fn next_fire_time(schedule: &str) -> Option<i64> {
    next_fire_time_after(schedule, chrono::Local::now())
}

fn next_fire_time_after(schedule: &str, after: chrono::DateTime<chrono::Local>) -> Option<i64> {
    use cron::Schedule;
    use std::str::FromStr;
    let seven_field = to_cron_crate_schedule(schedule)?;
    let sched = Schedule::from_str(&seven_field).ok()?;
    sched.after(&after).next().map(|t| t.timestamp())
}

/// Return `true` if the 5-field cron expression `schedule` was due within the
/// 60-second window ending at `now`.  This lets the scheduler run once per
/// minute and correctly fire jobs without double-firing on restart.
///
/// Cron expressions are evaluated in the **local timezone** so that users can
/// write schedules like `30 8 * * 1-5` and have them fire at 08:30 local time.
pub fn is_due(schedule: &str, now: &chrono::DateTime<chrono::Local>) -> bool {
    use cron::Schedule;
    use std::str::FromStr;

    // The `cron` crate uses a 7-field format: sec min hour dom month dow year.
    // We accept the familiar 5-field cron format where 0/7=Sunday and
    // 1=Monday .. 6=Saturday, then convert numeric days to unambiguous weekday
    // names before handing the expression to the crate.
    let Some(seven_field) = to_cron_crate_schedule(schedule) else {
        return false;
    };
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

fn to_cron_crate_schedule(schedule: &str) -> Option<String> {
    let parts: Vec<_> = schedule.split_whitespace().collect();
    if parts.len() != 5 {
        return None;
    }

    let dow = translate_unix_dow_field(parts[4])?;
    Some(format!(
        "0 {} {} {} {} {} *",
        parts[0], parts[1], parts[2], parts[3], dow
    ))
}

fn translate_unix_dow_field(field: &str) -> Option<String> {
    if field == "*" || field == "?" {
        return Some(field.to_string());
    }

    field
        .split(',')
        .map(translate_unix_dow_item)
        .collect::<Option<Vec<_>>>()
        .map(|items| items.join(","))
}

fn translate_unix_dow_item(item: &str) -> Option<String> {
    let (base, step) = item
        .split_once('/')
        .map(|(base, step)| Some((base, Some(step.parse::<usize>().ok()?))))
        .unwrap_or(Some((item, None)))?;

    match step {
        Some(0) => None,
        Some(step)
            if base
                .chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '*') =>
        {
            expand_unix_dow_base(base, step).map(|days| {
                days.into_iter()
                    .map(unix_dow_name)
                    .collect::<Vec<_>>()
                    .join(",")
            })
        }
        Some(step) => Some(format!("{}/{}", translate_unix_dow_base(base)?, step)),
        None => translate_unix_dow_base(base),
    }
}

fn translate_unix_dow_base(base: &str) -> Option<String> {
    if base == "*" || base == "?" {
        return Some(base.to_string());
    }

    match base.split_once('-') {
        Some((start, end)) => Some(format!(
            "{}-{}",
            translate_unix_dow_atom(start)?,
            translate_unix_dow_atom(end)?
        )),
        None => translate_unix_dow_atom(base),
    }
}

fn translate_unix_dow_atom(atom: &str) -> Option<String> {
    if atom.chars().any(|c| c.is_ascii_alphabetic()) {
        Some(atom.to_ascii_uppercase())
    } else {
        Some(unix_dow_name(parse_unix_dow(atom)?).to_string())
    }
}

fn expand_unix_dow_base(base: &str, step: usize) -> Option<Vec<u32>> {
    let (start, end) = match base {
        "*" => (0, 6),
        _ => match base.split_once('-') {
            Some((start, end)) => (parse_unix_dow(start)?, parse_unix_dow(end)?),
            None => {
                return Some(vec![parse_unix_dow(base)?]);
            }
        },
    };

    if start > end {
        return None;
    }

    Some((start..=end).step_by(step).collect())
}

fn parse_unix_dow(value: &str) -> Option<u32> {
    let day = value.parse::<u32>().ok()?;
    if day <= 7 {
        Some(if day == 7 { 0 } else { day })
    } else {
        None
    }
}

fn unix_dow_name(day: u32) -> &'static str {
    match day {
        0 => "SUN",
        1 => "MON",
        2 => "TUE",
        3 => "WED",
        4 => "THU",
        5 => "FRI",
        6 => "SAT",
        _ => unreachable!("parse_unix_dow only accepts 0 through 6"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    #[test]
    fn test_is_due_every_minute() {
        // "* * * * *" should be due at any whole minute
        let now = Local.with_ymd_and_hms(2026, 4, 16, 10, 30, 5).unwrap();
        assert!(is_due("* * * * *", &now));
    }

    #[test]
    fn test_numeric_dow_translates_to_names() {
        assert_eq!(
            to_cron_crate_schedule("50 19 * * 1-5").as_deref(),
            Some("0 50 19 * * MON-FRI *")
        );
        assert_eq!(
            to_cron_crate_schedule("0 8 * * 0,6").as_deref(),
            Some("0 0 8 * * SUN,SAT *")
        );
        assert_eq!(
            to_cron_crate_schedule("0 8 * * 7").as_deref(),
            Some("0 0 8 * * SUN *")
        );
        assert_eq!(
            to_cron_crate_schedule("0 8 * * MON,5").as_deref(),
            Some("0 0 8 * * MON,FRI *")
        );
        assert_eq!(
            to_cron_crate_schedule("0 8 * * mon-fri").as_deref(),
            Some("0 0 8 * * MON-FRI *")
        );
        assert_eq!(
            to_cron_crate_schedule("0 8 * * 1-5/2").as_deref(),
            Some("0 0 8 * * MON,WED,FRI *")
        );
    }

    #[test]
    fn test_is_due_every_2_min() {
        // "*/2 * * * *" → due at even minutes
        let at_even = Local.with_ymd_and_hms(2026, 4, 16, 10, 30, 5).unwrap();
        assert!(is_due("*/2 * * * *", &at_even));

        let at_odd = Local.with_ymd_and_hms(2026, 4, 16, 10, 31, 5).unwrap();
        assert!(!is_due("*/2 * * * *", &at_odd));
    }

    #[test]
    fn test_is_due_specific_time() {
        // "30 8 * * *" → 08:30 daily (local time)
        let at_830 = Local.with_ymd_and_hms(2026, 4, 16, 8, 30, 30).unwrap();
        assert!(is_due("30 8 * * *", &at_830));

        let at_831 = Local.with_ymd_and_hms(2026, 4, 16, 8, 31, 30).unwrap();
        assert!(!is_due("30 8 * * *", &at_831));
    }

    #[test]
    fn test_is_due_weekday_only() {
        // "30 8 * * 1-5" → 08:30 Mon-Fri (local time)
        // 2026-04-16 is Thursday (weekday)
        let thu = Local.with_ymd_and_hms(2026, 4, 16, 8, 30, 30).unwrap();
        assert!(is_due("30 8 * * 1-5", &thu));

        // 2026-04-17 is Friday
        let fri = Local.with_ymd_and_hms(2026, 4, 17, 8, 30, 30).unwrap();
        assert!(is_due("30 8 * * 1-5", &fri));

        // 2026-04-18 is Saturday
        let sat = Local.with_ymd_and_hms(2026, 4, 18, 8, 30, 30).unwrap();
        assert!(!is_due("30 8 * * 1-5", &sat));

        // 2026-04-19 is Sunday
        let sun = Local.with_ymd_and_hms(2026, 4, 19, 8, 30, 30).unwrap();
        assert!(!is_due("30 8 * * 1-5", &sun));
    }

    #[test]
    fn test_next_fire_time_weekday_after_friday() {
        let before_friday_run = Local.with_ymd_and_hms(2026, 4, 17, 19, 49, 0).unwrap();
        let next = next_fire_time_after("50 19 * * 1-5", before_friday_run).unwrap();
        assert_eq!(
            chrono::DateTime::from_timestamp(next, 0)
                .unwrap()
                .with_timezone(&Local),
            Local.with_ymd_and_hms(2026, 4, 17, 19, 50, 0).unwrap()
        );

        let after_friday_run = Local.with_ymd_and_hms(2026, 4, 17, 19, 50, 30).unwrap();
        let next = next_fire_time_after("50 19 * * 1-5", after_friday_run).unwrap();
        assert_eq!(
            chrono::DateTime::from_timestamp(next, 0)
                .unwrap()
                .with_timezone(&Local),
            Local.with_ymd_and_hms(2026, 4, 20, 19, 50, 0).unwrap()
        );
    }

    #[test]
    fn test_next_fire_time_returns_some() {
        assert!(next_fire_time("* * * * *").is_some());
        assert!(next_fire_time("*/2 * * * *").is_some());
        assert!(next_fire_time("30 8 * * 1-5").is_some());
    }

    #[test]
    fn test_invalid_cron() {
        assert!(!is_due("invalid", &Local::now()));
        assert!(next_fire_time("invalid").is_none());
    }
}
