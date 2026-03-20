use std::sync::Arc;

use chrono::Utc;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use tracing::warn;

use crate::llm::client::LlmMessage;
use crate::memory::file_layer::FileLayer;
use crate::memory::system::MemorySystem;
use crate::skills::executor::SkillExecutor;
use crate::skills::permissions::PermissionChecker;

pub struct SkillScheduler {
    pub executor: Arc<SkillExecutor>,
    pub memory: Arc<MemorySystem>,
    pub permissions: PermissionChecker,
    pub file_layer: Arc<FileLayer>,
}

impl SkillScheduler {
    /// Spawn a background task that runs the Alpha heartbeat once every 24 hours.
    pub fn start(self, app_handle: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut last_heartbeat: Option<chrono::DateTime<Utc>> = None;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;

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

    async fn tick_alpha_heartbeat(&self, app_handle: &AppHandle) {
        let owner_profile = self.file_layer.read_owner_profile().unwrap_or_default();
        let recent_convs = self
            .memory
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

        let response = match self
            .executor
            .llm
            .chat(vec![
                LlmMessage {
                    role: "system".to_string(),
                    content: "You are Alpha, a loyal personal AI assistant. Analyze interaction patterns \
                              and extract behavioral rules for your owner."
                        .to_string(),
                },
                LlmMessage { role: "user".to_string(), content: prompt },
            ])
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
                let _ = self.file_layer.append_rules(&entry);
            }
        }

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
}
