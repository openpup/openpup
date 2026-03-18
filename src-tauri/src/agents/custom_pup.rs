use crate::agents::specialist::{SpecialistPup, Task};

/// A user-defined pup with a fully custom system prompt.
pub struct CustomPup {
    pub key: String,
    pub display_name: String,
    pub system_prompt: String,
}

impl SpecialistPup for CustomPup {
    fn name(&self) -> &str {
        &self.key
    }

    fn capabilities(&self) -> Vec<String> {
        vec![]
    }

    fn build_system_prompt(&self, task: &Task) -> String {
        let base = task
            .system_prompt_override
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.system_prompt);

        let mut system = base.to_string();
        if !task.owner_context.is_empty() {
            system.push_str(&format!("\n\nOwner profile:\n{}", task.owner_context));
        }
        if !task.relevant_memories.is_empty() {
            let bullets: String = task
                .relevant_memories
                .iter()
                .map(|m| {
                    let capped = if m.len() > 200 { &m[..200] } else { m.as_str() };
                    format!("- {capped}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            system.push_str(&format!("\n\n## Relevant Memories\n{bullets}"));
        }
        system
    }
}
