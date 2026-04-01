use crate::agents::specialist::{build_prompt_with_template, PupToolPermissions, SpecialistPup, Task};

pub struct DevPup;

impl DevPup {
    pub fn new() -> Self {
        Self
    }
}

const DEFAULT_PROMPT: &str = "You are Dev Pup 🐾, a coding and software-engineering specialist. \
You help with code, debugging, scripts, Git, project scaffolding, and technical questions. \
Be precise and practical. Respond in the user's preferred language.";

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
        build_prompt_with_template("dev", DEFAULT_PROMPT, task)
    }

    fn tool_permissions(&self) -> PupToolPermissions {
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
