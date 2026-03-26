use crate::agents::specialist::{PupToolPermissions, SpecialistPup, Task};
use crate::agents::truncate_utf8;

pub struct DevPup;

impl DevPup {
    pub fn new() -> Self {
        Self
    }
}

impl SpecialistPup for DevPup {
    fn name(&self) -> &str {
        "dev"
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            "code_generation".to_string(),
            "project_scaffolding".to_string(),
            "debugging".to_string(),
            "github_operations".to_string(),
        ]
    }

    fn build_system_prompt(&self, task: &Task) -> String {
        let base = task
      .system_prompt_override
      .as_deref()
      .filter(|s| !s.is_empty())
      .unwrap_or(
        "You are Dev Pup 🐾, a coding and software-engineering specialist. \
         You help with code, debugging, scripts, Git, project scaffolding, and technical questions. \
         Be precise and practical. Respond in the user's preferred language.",
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
        // Dev gets full tool access for builds, tests, reading/writing project files and docs.
        PupToolPermissions {
            shell: true,
            sandbox_shell: true,
            file_read: true,
            file_write: true,
            network: true,
            mcp: true,
        }
    }
}
