use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use futures_util::future::join_all;
use uuid::Uuid;

use tracing::{info, warn};

use crate::bridge::types::{BridgeOutbox, OutboundMessage, OutboundType, Platform};
use crate::llm::client::LlmMessage;
use crate::memory::file_layer::FileLayer;
use crate::memory::system::MemorySystem;
use crate::runtime::{emit_event, SharedEventSink};
use crate::skills::executor::SkillExecutor;
use crate::skills::job_registry::{is_due, next_fire_time, JobMode, JobRegistry, NotifyWhen, ScheduledJob};
use crate::skills::permissions::PermissionChecker;

pub struct SkillScheduler {
    pub executor: Arc<SkillExecutor>,
    pub memory: Arc<MemorySystem>,
    pub permissions: PermissionChecker,
    pub file_layer: Arc<FileLayer>,
    /// Path to ~/.openpup/scheduled_jobs.json
    pub jobs_path: PathBuf,
    /// Optional bridge outbox for sending job notifications to messaging platforms.
    pub bridge_outbox: Option<BridgeOutbox>,
}

impl SkillScheduler {
    /// Spawn a background task that ticks every minute.
    ///
    /// Each due job is spawned as an independent Tokio task so a slow LLM call
    /// never blocks the scheduler loop or delays other jobs.
    pub fn start(self, events: Option<SharedEventSink>) {
        let executor = self.executor;
        let memory = self.memory;
        let file_layer = self.file_layer;
        let jobs_path = self.jobs_path;
        let bridge_outbox = self.bridge_outbox;

        tokio::spawn(async move {
            // Restore last heartbeat time from DB so restarts don't re-trigger.
            // If no prior run exists (fresh install), seed with `now` so the first
            // heartbeat waits a full 24 hours — there are no conversations to extract from yet.
            let mut last_heartbeat: Option<chrono::DateTime<Utc>> = Some(
                memory
                    .last_skill_run_time("alpha_heartbeat")
                    .await
                    .ok()
                    .flatten()
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .unwrap_or_else(Utc::now),
            );
            let job_registry = JobRegistry::new(jobs_path);

            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

                let now = Utc::now();

                // ── Alpha heartbeat (once per 24 hours) ─────────────────────
                let elapsed_hours = last_heartbeat
                    .map(|last| (now - last).num_hours())
                    .unwrap_or(i64::MAX);
                if elapsed_hours >= 24 {
                    last_heartbeat = Some(now);
                    let executor = executor.clone();
                    let memory = memory.clone();
                    let file_layer = file_layer.clone();
                    let heartbeat_events = events.clone();
                    tokio::spawn(async move {
                        tick_alpha_heartbeat(&executor, &memory, &file_layer, heartbeat_events)
                            .await;
                    });
                }

                // ── Dynamic scheduled jobs ───────────────────────────────────
                // Each due job is spawned independently — the loop never awaits
                // job execution, so a slow job cannot block the minute tick.
                for job in job_registry.load().into_iter().filter(|j| j.enabled) {
                    if is_due(&job.schedule, &now) {
                        let executor = executor.clone();
                        let memory = memory.clone();
                        let job_events = events.clone();
                        let outbox = bridge_outbox.clone();
                        info!(
                            "[scheduler] spawning job '{}' (schedule: '{}')",
                            job.name, job.schedule
                        );
                        tokio::spawn(async move {
                            run_job(job, &executor, &memory, job_events, outbox).await;
                        });
                    }
                }
            }
        });
    }
}

// ── Job execution (free functions — owned by spawned tasks) ──────────────────

async fn run_job(
    job: ScheduledJob,
    executor: &Arc<SkillExecutor>,
    memory: &Arc<MemorySystem>,
    events: Option<SharedEventSink>,
    bridge_outbox: Option<BridgeOutbox>,
) {
    if job.steps.is_empty() {
        return;
    }

    if let Some(sink) = events.as_ref() {
        emit_event(
            sink.as_ref(),
            "scheduled_job_started",
            serde_json::json!({ "id": job.id, "name": job.name }),
        );
    }

    let run_id = Uuid::new_v4().to_string();
    let _ = memory
        .record_skill_run(&run_id, &job.name, "scheduled", Some(&job.id))
        .await;

    let started = std::time::Instant::now();

    let result = match job.mode {
        JobMode::Single | JobMode::Sequential => run_sequential(&job, executor).await,
        JobMode::Parallel => run_parallel(&job, executor).await,
    };

    let elapsed_secs = started.elapsed().as_secs();

    let (status, output) = match result {
        Ok(o) => ("completed".to_string(), o),
        Err(e) => ("failed".to_string(), format!("Error: {e}")),
    };

    let output_preview: String = output.chars().take(1000).collect();
    let _ = memory
        .complete_skill_run(&run_id, &status, &output_preview)
        .await;

    if let Some(sink) = events.as_ref() {
        emit_event(
            sink.as_ref(),
            "scheduled_job_completed",
            serde_json::json!({
                "id": job.id,
                "name": job.name,
                "status": status,
                "output": output,
            }),
        );
    }

    // ── Notification dispatch ─────────────────────────────────────────────
    let should_notify = match job.notify.when {
        NotifyWhen::Never => false,
        NotifyWhen::OnFailure => status == "failed",
        NotifyWhen::Always => true,
    };

    if should_notify && !job.notify.channels.is_empty() {
        let message = if status == "completed" {
            let preview: String = output.chars().take(500).collect();
            format!(
                "✅ {} 完成 · 耗时 {}s\n──\n{}",
                job.name, elapsed_secs, preview
            )
        } else {
            let next_time = next_fire_time(&job.schedule)
                .map(|t| {
                    chrono::DateTime::from_timestamp(t, 0)
                        .map(|dt| dt.format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| t.to_string())
                })
                .unwrap_or_else(|| "-".to_string());
            format!(
                "❌ {} 失败\n{}\n──\ncron: {}\n下次执行: {}",
                job.name, output, job.schedule, next_time
            )
        };

        if let Some(outbox) = bridge_outbox.as_ref() {
            send_job_notification(outbox, &job.notify.channels, &message).await;
        }
    }
}

/// Send a notification message to the specified bridge channels.
async fn send_job_notification(outbox: &BridgeOutbox, channels: &[String], message: &str) {
    let tx = match outbox.lock().await.clone() {
        Some(tx) => tx,
        None => {
            warn!("[scheduler] bridge outbox not available, skipping notification");
            return;
        }
    };

    let cfg = crate::config::load_with_env().bridge.unwrap_or_default();

    for channel in channels {
        let target: Option<(Platform, String)> = match channel.as_str() {
            "weixin" => cfg
                .weixin
                .as_ref()
                .map(|wx| (Platform::Weixin, wx.owner_user_id.clone())),
            "qqbot" => cfg
                .qqbot
                .as_ref()
                .map(|qq| (Platform::QQBot, format!("c2c:{}", qq.owner_user_id))),
            "telegram" => cfg
                .telegram
                .as_ref()
                .map(|tg| (Platform::Telegram, tg.owner_user_id.clone())),
            other => {
                warn!("[scheduler] unknown notification channel: {other}");
                None
            }
        };

        if let Some((platform, chat_id)) = target {
            let _ = tx
                .send(OutboundMessage {
                    platform,
                    chat_id,
                    text: message.to_string(),
                    reply_to_id: None,
                    msg_type: OutboundType::Result,
                })
                .await;
        }
    }
}

/// Run steps serially; a step with an empty `input` inherits the previous
/// step's output (pipeline behaviour).
async fn run_sequential(
    job: &ScheduledJob,
    executor: &Arc<SkillExecutor>,
) -> anyhow::Result<String> {
    let mut prev_output = String::new();
    for step in &job.steps {
        if executor.registry.get(&step.skill).await.is_none() {
            warn!(
                "[scheduler] skill '{}' not found, skipping step in job '{}'",
                step.skill, job.name
            );
            continue;
        }
        let input = if step.input.is_empty() {
            prev_output.clone()
        } else {
            step.input.clone()
        };
        prev_output = executor
            .execute_skill(&step.skill, &input)
            .await
            .unwrap_or_else(|e| format!("[{}] error: {e}", step.skill));
    }
    Ok(prev_output)
}

/// Run all steps concurrently and join their outputs.
async fn run_parallel(job: &ScheduledJob, executor: &Arc<SkillExecutor>) -> anyhow::Result<String> {
    let futures: Vec<_> = job
        .steps
        .iter()
        .map(|step| {
            let executor = executor.clone();
            let skill = step.skill.clone();
            let input = step.input.clone();
            let job_name = job.name.clone();
            async move {
                if executor.registry.get(&skill).await.is_none() {
                    warn!(
                        "[scheduler] skill '{skill}' not found, skipping step in job '{job_name}'"
                    );
                    return format!("[{skill}] skipped: skill not found");
                }
                executor
                    .execute_skill(&skill, &input)
                    .await
                    .unwrap_or_else(|e| format!("[{skill}] error: {e}"))
            }
        })
        .collect();

    Ok(join_all(futures).await.join("\n\n---\n\n"))
}

// ── Alpha heartbeat ───────────────────────────────────────────────────────────

async fn tick_alpha_heartbeat(
    executor: &Arc<SkillExecutor>,
    memory: &Arc<MemorySystem>,
    file_layer: &Arc<FileLayer>,
    events: Option<SharedEventSink>,
) {
    let owner_profile = file_layer.read_owner_profile().unwrap_or_default();
    let recent_convs = memory
        .recent_conversations_global(40)
        .await
        .unwrap_or_default();

    let conv_summary = recent_convs
        .iter()
        .take(20)
        .map(|(role, content)| {
            let snippet: String = content.chars().take(200).collect();
            format!("{role}: {snippet}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let profile_snippet: String = owner_profile.chars().take(600).collect();
    let prompt = format!(
        "Owner profile:\n{profile_snippet}\n\n\
         Recent interactions (last 40 messages):\n{conv_summary}\n\n\
         Task — extract 1-3 new behavioral preferences or rules observed in the interactions. \
         Output as: RULES:\n- <rule>\n- <rule>\n\n\
         Be concise. Only output the RULES section.",
    );

    let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let response = match executor
        .llm
        .chat_stream(
            vec![
                LlmMessage {
                    role: "system".to_string(),
                    content: "You are Alpha, a loyal personal AI assistant. Analyze interaction \
                              patterns and extract behavioral rules for your owner."
                        .to_string(),
                },
                LlmMessage {
                    role: "user".to_string(),
                    content: prompt,
                },
            ],
            |_, _| {},
            &abort,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!("[heartbeat] LLM call failed: {e}");
            return;
        }
    };

    if let Some(rules_start) = response.find("RULES:") {
        let rules_text = response[rules_start + "RULES:".len()..].trim();
        if !rules_text.is_empty() {
            let entry = format!(
                "\n## Heartbeat {}\n{}\n",
                chrono::Local::now().format("%Y-%m-%d"),
                rules_text
            );
            let _ = file_layer.append_rules(&entry);
        }
    }

    let run_id = Uuid::new_v4().to_string();
    let _ = memory
        .record_skill_run(&run_id, "alpha_heartbeat", "heartbeat", None)
        .await;
    let response_preview: String = response.chars().take(1000).collect();
    let _ = memory
        .complete_skill_run(&run_id, "completed", &response_preview)
        .await;
    if let Some(sink) = events.as_ref() {
        emit_event(sink.as_ref(), "heartbeat_completed", serde_json::json!({}));
    }
}
