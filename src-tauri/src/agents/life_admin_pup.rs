use crate::agents::specialist::{SpecialistPup, Task};
use crate::agents::truncate_utf8;

pub struct LifeAdminPup;

impl LifeAdminPup {
    pub fn new() -> Self {
        Self
    }
}

impl SpecialistPup for LifeAdminPup {
    fn name(&self) -> &str {
        "life_admin"
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            "email_routing".to_string(),
            "bills_and_expenses".to_string(),
            "shopping_lists".to_string(),
            "calendar".to_string(),
        ]
    }

    fn build_system_prompt(&self, task: &Task) -> String {
        let base = task
      .system_prompt_override
      .as_deref()
      .filter(|s| !s.is_empty())
      .unwrap_or(
        "You are Life Admin Pup 🐾, a personal life-administration specialist. \
         You help with email triage, bills, shopping lists, calendar planning, and everyday personal tasks. \
         Be concise and practical. Never take real-world actions without explicit confirmation.",
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
}
