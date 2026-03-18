use crate::agents::specialist::{PupToolPermissions, SpecialistPup, Task};

pub struct ResearchPup;

impl ResearchPup {
    pub fn new() -> Self {
        Self
    }
}

impl SpecialistPup for ResearchPup {
    fn name(&self) -> &str {
        "research"
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            "information_gathering".to_string(),
            "synthesis".to_string(),
            "report_writing".to_string(),
            "fact_checking".to_string(),
        ]
    }

    fn build_system_prompt(&self, task: &Task) -> String {
        let base = task
      .system_prompt_override
      .as_deref()
      .filter(|s| !s.is_empty())
      .unwrap_or(
        "You are Research Pup 🐾, a research and knowledge specialist. \
         You help with finding information, synthesising sources, fact-checking, and producing structured reports. \
         Be thorough, cite your reasoning, and organise output clearly.",
      );

        let mut system = base.to_string();
        if task.owner_context.contains("## Boundaries") {
            system.push_str(&format!("\n\nOwner profile:\n{}", task.owner_context));
        }
        if !task.relevant_memories.is_empty() {
            let bullets: String = task
                .relevant_memories
                .iter()
                .map(|m| format!("- {}", if m.len() > 200 { &m[..200] } else { m.as_str() }))
                .collect::<Vec<_>>()
                .join("\n");
            system.push_str(&format!("\n\n## Relevant Memories\n{bullets}"));
        }
        system
    }

    fn tool_permissions(&self) -> PupToolPermissions {
        // Research gets network access so it can fetch URLs.
        PupToolPermissions {
            shell: false,
            filesystem: false,
            network: true,
            mcp: true,
        }
    }
}
