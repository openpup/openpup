use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ProviderProtocol {
    #[default]
    OpenAiCompatible,
    OpenAiResponses,
    AnthropicMessages,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub provider_key: String,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutingConfig {
    pub primary: RouteTarget,
    pub mini: RouteTarget,
    pub embedding: RouteTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteTarget {
    pub provider_id: String,
    pub model: String,
}
