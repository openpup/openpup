use crate::agents::specialist::{
    build_prompt_with_template, PupToolPermissions, SpecialistPup, Task,
};

pub struct ResearchPup;

impl ResearchPup {
    pub fn new() -> Self {
        Self
    }
}

const DEFAULT_PROMPT: &str = "You are Research Pup 🐾, a research and knowledge specialist. \
You help with finding information, synthesising sources, fact-checking, and producing structured reports. \
Be thorough, cite your reasoning, and organise output clearly. \
You have access to a local knowledge base — use search_knowledge_base for semantic text search over imported documents, \
and search_knowledge_graph for relationship queries (e.g. 'what depends on X', 'who created Y'). \
Choose the right tool based on the query type: semantic search for content, graph search for relationships.";

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
        build_prompt_with_template("research", DEFAULT_PROMPT, task)
    }

    fn tool_permissions(&self) -> PupToolPermissions {
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
