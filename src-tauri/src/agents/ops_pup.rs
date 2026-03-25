use crate::agents::specialist::{PupToolPermissions, SpecialistPup, Task};
use crate::agents::truncate_utf8;

pub struct OpsPup;

impl OpsPup {
    pub fn new() -> Self {
        Self
    }
}

impl SpecialistPup for OpsPup {
    fn name(&self) -> &str {
        "ops"
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            "scheduling".to_string(),
            "automation".to_string(),
            "system_monitoring".to_string(),
            "reminders".to_string(),
        ]
    }

    fn build_system_prompt(&self, task: &Task) -> String {
        let base = task
      .system_prompt_override
      .as_deref()
      .filter(|s| !s.is_empty())
      .unwrap_or(
        "You are Ops Pup 🐾, an operations and automation specialist. \
         You help with scheduling, reminders, automation scripts, system monitoring, and workflow optimisation. \
         Be structured and action-oriented. Respect the user's work schedule and do-not-disturb windows.",
      );

        let mut system = base.to_string();
        if task.owner_context.contains("## Boundaries") {
            system.push_str(&format!("\n\nOwner profile:\n{}", task.owner_context));
        }
        if !task.relevant_memories.is_empty() {
            let bullets: String = task
                .relevant_memories
                .iter()
                .map(|m| format!("- {}", truncate_utf8(m, 200)))
                .collect::<Vec<_>>()
                .join("\n");
            system.push_str(&format!("\n\n## Relevant Memories\n{bullets}"));
        }
        system
    }

    fn tool_permissions(&self) -> PupToolPermissions {
        // Ops gets full tool access — running commands, automation, and fetching resources.
        PupToolPermissions {
            shell: true,
            file_read: true,
            file_write: true,
            network: true,
            mcp: true,
        }
    }
}
