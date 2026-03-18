use crate::agents::specialist::{SpecialistPup, Task};

pub struct WriterPup;

impl WriterPup {
  pub fn new() -> Self {
    Self
  }
}

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
    let base = task
      .system_prompt_override
      .as_deref()
      .filter(|s| !s.is_empty())
      .unwrap_or(
        "You are Writer Pup 🐾, a writing and language specialist. \
         You help with drafting, editing, translation, summarisation, and content creation. \
         Match the user's tone and style. Respond in the user's preferred language.",
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
}
