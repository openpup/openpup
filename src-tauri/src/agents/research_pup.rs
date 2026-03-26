use crate::agents::specialist::{PupToolPermissions, SpecialistPup, Task};
use crate::agents::truncate_utf8;

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
         Be thorough, cite your reasoning, and organise output clearly. \
         You have access to a local knowledge base — use search_knowledge_base for semantic text search over imported documents, \
         and search_knowledge_graph for relationship queries (e.g. 'what depends on X', 'who created Y'). \
         Choose the right tool based on the query type: semantic search for content, graph search for relationships.",
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
        // Research gets network + file_read — can fetch URLs and read project files for context.
        PupToolPermissions {
            shell: false,
            sandbox_shell: true,
            file_read: true,
            file_write: false,
            network: true,
            mcp: true,
        }
    }
}
