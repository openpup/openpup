use crate::agents::specialist::{build_prompt_with_template, PupToolPermissions, SpecialistPup, Task};

pub struct OpsPup;

impl OpsPup {
    pub fn new() -> Self {
        Self
    }
}

const DEFAULT_PROMPT: &str = "You are Ops Pup 🐾, an operations and automation specialist. \
You help with scheduling, reminders, automation scripts, system monitoring, and workflow optimisation. \
Be structured and action-oriented. Respect the user's work schedule and do-not-disturb windows.";

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
        build_prompt_with_template("ops", DEFAULT_PROMPT, task)
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
