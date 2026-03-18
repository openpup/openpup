use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use cron::Schedule;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::llm::client::LlmMessage;
use crate::memory::file_layer::FileLayer;
use crate::memory::system::MemorySystem;
use crate::skills::executor::SkillExecutor;
use crate::skills::permissions::{ExecutionMode, PermissionChecker};

pub struct SkillScheduler {
    pub executor: Arc<SkillExecutor>,
    pub memory: Arc<MemorySystem>,
    pub permissions: PermissionChecker,
    pub file_layer: Arc<FileLayer>,
}

impl SkillScheduler {
    /// Spawn a background Tokio task that runs every 60 s.
    /// Must be called from within Tauri's `.setup()` closure (after AppHandle is available).
    pub fn start(self, app_handle: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut last_heartbeat: Option<chrono::DateTime<Utc>> = None;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

                // Layer 1 — run skills that have a matching cron trigger
                self.tick_scheduled_skills(&app_handle).await;

                // Layer 2 — Alpha heartbeat (once every 24 h)
                let now = Utc::now();
                let elapsed_hours = last_heartbeat
                    .map(|last| (now - last).num_hours())
                    .unwrap_or(i64::MAX);

                if elapsed_hours >= 24 {
                    last_heartbeat = Some(now);
                    self.tick_alpha_heartbeat(&app_handle).await;
                }
            }
        });
    }

    // ── Layer 1: scheduled skills ──────────────────────────────────────────────

    async fn tick_scheduled_skills(&self, app_handle: &AppHandle) {
        let now = Utc::now();
        let window_start = now - chrono::Duration::seconds(62); // slight buffer

        let installed = self.executor.registry.list_installed().await;
        for skill in installed.iter().filter(|s| s.enabled) {
            let Some(manifest) = self.executor.registry.get(&skill.name).await else {
                continue;
            };
            let Some(sched_cfg) = &manifest.schedule else {
                continue;
            };
            let Some(cron_expr) = &sched_cfg.cron else {
                continue;
            };

            // Normalize 5-field "min hour day month weekday" → 6-field by prepending "0 "
            let expr = if cron_expr.split_whitespace().count() == 5 {
                format!("0 {}", cron_expr)
            } else {
                cron_expr.clone()
            };

            let Ok(schedule) = Schedule::from_str(&expr) else {
                continue;
            };

            // Check if any scheduled tick falls inside the last 62-second window
            if let Some(next) = schedule.after(&window_start).next() {
                if next <= now {
                    self.run_skill_with_history(&skill.name, "cron", "", app_handle)
                        .await;
                }
            }
        }
    }

    // ── Layer 2: Alpha daily heartbeat ────────────────────────────────────────

    async fn tick_alpha_heartbeat(&self, app_handle: &AppHandle) {
        // Gather context
        let owner_profile = self.file_layer.read_owner_profile().unwrap_or_default();
        let recent_convs = self
            .memory
            .recent_conversations_global(40)
            .await
            .unwrap_or_default();

        // Discover new skills from all configured registries (incl. github://)
        let discoverable = self.executor.registry.fetch_discoverable().await;
        let installed_names: HashSet<String> = self
            .executor
            .registry
            .list_installed()
            .await
            .into_iter()
            .map(|s| s.name)
            .collect();

        let new_skills: Vec<_> = discoverable
            .iter()
            .filter(|d| !installed_names.contains(&d.name) && !d.repo_url.is_empty())
            .collect();

        // Build context strings for the LLM
        let conv_summary = recent_convs
            .iter()
            .take(20)
            .map(|(role, content)| {
                let snippet: String = content.chars().take(200).collect();
                format!("{role}: {snippet}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let skills_list = new_skills
            .iter()
            .map(|d| format!("- {} ({}): {}", d.name, d.category, d.description))
            .collect::<Vec<_>>()
            .join("\n");

        let profile_snippet: String = owner_profile.chars().take(600).collect();

        let prompt = format!(
      "Owner profile:\n{profile_snippet}\n\n\
       Recent interactions (last 40 messages):\n{conv_summary}\n\n\
       New skills available to install:\n{}\n\n\
       Task A — extract 1-3 new behavioral preferences or rules observed in the interactions. \
       Output as: RULES:\n- <rule>\n- <rule>\n\n\
       Task B — from the available skills list, recommend 0-2 that are genuinely useful for this owner. \
       Only recommend if highly relevant. \
       Output as: RECOMMEND: <skill_name> - <one-sentence reason>\n\n\
       Be concise. Only output the two sections above.",
      if skills_list.is_empty() { "（无新技能）".to_string() } else { skills_list }
    );

        let response = match self
            .executor
            .llm
            .chat(vec![
        LlmMessage {
          role: "system".to_string(),
          content: "You are Alpha, a loyal personal AI assistant. Analyze interaction patterns \
                    and make intelligent suggestions for your owner."
            .to_string(),
        },
        LlmMessage { role: "user".to_string(), content: prompt },
      ])
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[heartbeat] LLM call failed: {e}");
                return;
            }
        };

        // ── Parse and persist RULES section ─────────────────────────────────────
        if let Some(rules_start) = response.find("RULES:") {
            let rules_block = &response[rules_start + "RULES:".len()..];
            let rules_text = rules_block
                .split("RECOMMEND:")
                .next()
                .unwrap_or(rules_block)
                .trim();
            if !rules_text.is_empty() {
                let entry = format!(
                    "\n## Heartbeat {}\n{}\n",
                    chrono::Local::now().format("%Y-%m-%d"),
                    rules_text
                );
                let _ = self.file_layer.append_rules(&entry);
            }
        }

        // ── Parse RECOMMEND lines and act ────────────────────────────────────────
        let mode = self.permissions.get_mode().await;
        for line in response.lines() {
            let Some(rest) = line.trim().strip_prefix("RECOMMEND:") else {
                continue;
            };
            let mut parts = rest.splitn(2, '-');
            let skill_name = parts.next().unwrap_or("").trim().to_string();
            let reason = parts.next().unwrap_or("").trim().to_string();
            if skill_name.is_empty() {
                continue;
            }

            let Some(discovered) = new_skills.iter().find(|d| d.name == skill_name) else {
                continue;
            };

            match mode {
                ExecutionMode::FreeRun => {
                    // Auto-install and run
                    if self
                        .executor
                        .registry
                        .install_from_git(&discovered.repo_url, None)
                        .await
                        .is_ok()
                    {
                        self.run_skill_with_history(
                            &skill_name,
                            "heartbeat",
                            "heartbeat auto-run",
                            app_handle,
                        )
                        .await;
                    }
                }
                ExecutionMode::Leashed => {
                    // Suggest to the user via a frontend event
                    let _ = app_handle.emit(
                        "skill_suggestion",
                        serde_json::json!({
                          "skill_name": skill_name,
                          "repo_url": discovered.repo_url,
                          "reason": reason,
                        }),
                    );
                }
            }
        }

        // Record the heartbeat itself as a skill run
        let run_id = Uuid::new_v4().to_string();
        let _ = self
            .memory
            .record_skill_run(&run_id, "alpha_heartbeat", "heartbeat")
            .await;
        let _ = self
            .memory
            .complete_skill_run(&run_id, "completed", &response[..response.len().min(1000)])
            .await;

        let _ = app_handle.emit("heartbeat_completed", serde_json::json!({}));
    }

    // ── Layer 3: shared helper — run + record history ─────────────────────────

    async fn run_skill_with_history(
        &self,
        skill_name: &str,
        triggered_by: &str,
        input: &str,
        app_handle: &AppHandle,
    ) {
        let run_id = Uuid::new_v4().to_string();
        let _ = self
            .memory
            .record_skill_run(&run_id, skill_name, triggered_by)
            .await;

        let result = self.executor.execute_skill(skill_name, input).await;
        let (status, output) = match result {
            Ok(o) => ("completed".to_string(), o),
            Err(e) => ("failed".to_string(), e.to_string()),
        };

        let _ = self
            .memory
            .complete_skill_run(&run_id, &status, &output)
            .await;

        let _ = app_handle.emit(
            "skill_run_completed",
            serde_json::json!({
              "skill_name": skill_name,
              "triggered_by": triggered_by,
              "status": status,
            }),
        );
    }
}
