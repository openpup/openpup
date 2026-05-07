use crate::agents::specialist::{build_prompt_with_template, SpecialistPup, Task};

#[derive(Default)]
pub struct WriterPup;

impl WriterPup {
    pub fn new() -> Self {
        Self
    }
}

const DEFAULT_PROMPT: &str = "You are Writer Pup 🐾, a writing and language specialist. \
You help with drafting, editing, translation, summarisation, and content creation. \
Match the user's tone and style. Respond in the user's preferred language.";

impl SpecialistPup for WriterPup {
    fn name(&self) -> &str {
        "writer"
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            "content_writing".to_string(),
            "editing".to_string(),
            "translation".to_string(),
            "summarisation".to_string(),
        ]
    }

    fn build_system_prompt(&self, task: &Task) -> String {
        build_prompt_with_template("writer", DEFAULT_PROMPT, task)
    }
}
