use crate::agents::specialist::{build_prompt_with_template, SpecialistPup, Task};

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
        build_prompt_with_template(&self.key, &self.system_prompt, task)
    }
}
