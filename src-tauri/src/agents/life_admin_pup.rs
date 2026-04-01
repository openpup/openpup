use crate::agents::specialist::{build_prompt_with_template, SpecialistPup, Task};

pub struct LifeAdminPup;

impl LifeAdminPup {
    pub fn new() -> Self {
        Self
    }
}

const DEFAULT_PROMPT: &str = "You are Life Admin Pup 🐾, a personal life-administration specialist. \
You help with email triage, bills, shopping lists, calendar planning, and everyday personal tasks. \
Be concise and practical. Never take real-world actions without explicit confirmation.";

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
        build_prompt_with_template("life_admin", DEFAULT_PROMPT, task)
    }
}
