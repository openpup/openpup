use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use futures_util::Stream;

use crate::config::{ProviderConfig, ProviderProtocol, RouteTarget, RoutingConfig};
use crate::error::{Result, RouterError};
use crate::protocols::anthropic::AnthropicProvider;
use crate::protocols::openai_compatible::OpenAiCompatibleProvider;
use crate::protocols::openai_responses::OpenAiResponsesProvider;
use crate::protocols::ollama::OllamaProvider;
use crate::types::{ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse, StreamEvent};

#[derive(Clone, Copy)]
enum Slot {
    Primary,
    Mini,
    Embedding,
}

#[derive(Clone)]
struct ClientConfig {
    providers: HashMap<String, ProviderConfig>,
    routing: RoutingConfig,
}

#[derive(Clone)]
pub struct Client {
    config: Arc<RwLock<ClientConfig>>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(providers: Vec<ProviderConfig>, routing: RoutingConfig) -> Self {
        let providers = providers
            .into_iter()
            .map(|provider| (provider.id.clone(), provider))
            .collect();
        Self {
            config: Arc::new(RwLock::new(ClientConfig { providers, routing })),
            http: reqwest::Client::new(),
        }
    }

    pub fn reconfigure(&self, providers: Vec<ProviderConfig>, routing: RoutingConfig) {
        let providers = providers
            .into_iter()
            .map(|provider| (provider.id.clone(), provider))
            .collect();
        let mut guard = self.config.write().unwrap();
        guard.providers = providers;
        guard.routing = routing;
    }

    pub fn routing_config(&self) -> (Vec<ProviderConfig>, RoutingConfig) {
        let guard = self.config.read().unwrap();
        (
            guard.providers.values().cloned().collect(),
            guard.routing.clone(),
        )
    }

    pub fn has_primary_provider(&self) -> bool {
        self.resolve_provider(Slot::Primary).is_ok()
    }

    pub fn primary_provider_name(&self) -> String {
        self.resolve_provider(Slot::Primary)
            .map(|provider| provider.provider_key)
            .unwrap_or_else(|_| "unconfigured".to_string())
    }

    pub async fn chat(&self, messages: Vec<crate::types::Message>) -> Result<ChatResponse> {
        let (provider, model) = self.resolve_provider_and_model(Slot::Primary)?;
        self.chat_on_provider(&provider, model, messages, Vec::new()).await
    }

    pub async fn chat_mini(&self, messages: Vec<crate::types::Message>) -> Result<ChatResponse> {
        let (provider, model) = self.resolve_provider_and_model(Slot::Mini)?;
        self.chat_on_provider(&provider, model, messages, Vec::new()).await
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<crate::types::Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let (provider, model) = self.resolve_provider_and_model(Slot::Primary)?;
        self.stream_on_provider(&provider, model, messages, Vec::new()).await
    }

    pub async fn chat_stream_mini(
        &self,
        messages: Vec<crate::types::Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let (provider, model) = self.resolve_provider_and_model(Slot::Mini)?;
        self.stream_on_provider(&provider, model, messages, Vec::new()).await
    }

    pub async fn chat_with_tools(
        &self,
        messages: Vec<crate::types::Message>,
        tools: Vec<crate::types::ToolDefinition>,
    ) -> Result<ChatResponse> {
        let (provider, model) = self.resolve_provider_and_model(Slot::Primary)?;
        self.chat_on_provider(&provider, model, messages, tools).await
    }

    pub async fn chat_with_tools_stream(
        &self,
        messages: Vec<crate::types::Message>,
        tools: Vec<crate::types::ToolDefinition>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let (provider, model) = self.resolve_provider_and_model(Slot::Primary)?;
        self.stream_on_provider(&provider, model, messages, tools).await
    }

    pub async fn embed(&self, input: Vec<String>) -> Result<EmbeddingResponse> {
        let (provider, model) = self.resolve_provider_and_model(Slot::Embedding)?;
        match &provider.protocol {
            ProviderProtocol::OpenAiCompatible => {
                OpenAiCompatibleProvider::new(provider, self.http.clone())
                    .embed(EmbeddingRequest { model, input })
                    .await
            }
            ProviderProtocol::AnthropicMessages => {
                AnthropicProvider::new(provider, self.http.clone())
                    .embed(EmbeddingRequest { model, input })
                    .await
            }
            ProviderProtocol::Ollama => {
                OllamaProvider::new(provider, self.http.clone())
                    .embed(EmbeddingRequest { model, input })
                    .await
            }
            ProviderProtocol::OpenAiResponses => {
                OpenAiResponsesProvider::new(provider, self.http.clone())
                    .embed(EmbeddingRequest { model, input })
                    .await
            }
        }
    }

    fn resolve_provider_and_model(&self, slot: Slot) -> Result<(ProviderConfig, String)> {
        let provider = self.resolve_provider(slot)?;
        let route = self.route_for_slot(slot)?.clone();
        let model = if route.model.trim().is_empty() {
            provider
                .models
                .first()
                .cloned()
                .ok_or_else(|| RouterError::ProviderModelMissing(provider.id.clone()))?
        } else {
            route.model
        };
        Ok((provider, model))
    }

    fn resolve_provider(&self, slot: Slot) -> Result<ProviderConfig> {
        let guard = self.config.read().unwrap();
        let route = match slot {
            Slot::Primary => &guard.routing.primary,
            Slot::Mini => &guard.routing.mini,
            Slot::Embedding => &guard.routing.embedding,
        };
        let provider = if !route.provider_id.trim().is_empty() {
            guard
                .providers
                .get(&route.provider_id)
                .cloned()
                .ok_or_else(|| RouterError::ProviderNotFound(route.provider_id.clone()))?
        } else {
            guard
                .providers
                .values()
                .find(|provider| provider.enabled)
                .cloned()
                .ok_or(RouterError::NoProvider)?
        };
        if !provider.enabled {
            return Err(RouterError::ProviderDisabled(provider.id));
        }
        Ok(provider)
    }

    fn route_for_slot(&self, slot: Slot) -> Result<RouteTarget> {
        let guard = self.config.read().unwrap();
        let route = match slot {
            Slot::Primary => guard.routing.primary.clone(),
            Slot::Mini => guard.routing.mini.clone(),
            Slot::Embedding => guard.routing.embedding.clone(),
        };
        Ok(route)
    }

    async fn chat_on_provider(
        &self,
        provider: &ProviderConfig,
        model: String,
        messages: Vec<crate::types::Message>,
        tools: Vec<crate::types::ToolDefinition>,
    ) -> Result<ChatResponse> {
        match &provider.protocol {
            ProviderProtocol::OpenAiCompatible => {
                OpenAiCompatibleProvider::new(provider.clone(), self.http.clone())
                    .chat(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
            ProviderProtocol::AnthropicMessages => {
                AnthropicProvider::new(provider.clone(), self.http.clone())
                    .chat(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
            ProviderProtocol::OpenAiResponses => {
                OpenAiResponsesProvider::new(provider.clone(), self.http.clone())
                    .chat(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
            ProviderProtocol::Ollama => {
                OllamaProvider::new(provider.clone(), self.http.clone())
                    .chat(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
        }
    }

    async fn stream_on_provider(
        &self,
        provider: &ProviderConfig,
        model: String,
        messages: Vec<crate::types::Message>,
        tools: Vec<crate::types::ToolDefinition>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        match &provider.protocol {
            ProviderProtocol::OpenAiCompatible => {
                OpenAiCompatibleProvider::new(provider.clone(), self.http.clone())
                    .chat_stream(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
            ProviderProtocol::AnthropicMessages => {
                AnthropicProvider::new(provider.clone(), self.http.clone())
                    .chat_stream(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
            ProviderProtocol::OpenAiResponses => {
                OpenAiResponsesProvider::new(provider.clone(), self.http.clone())
                    .chat_stream(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
            ProviderProtocol::Ollama => {
                OllamaProvider::new(provider.clone(), self.http.clone())
                    .chat_stream(ChatRequest {
                        model,
                        messages,
                        tools,
                        temperature: None,
                        max_tokens: None,
                    })
                    .await
            }
        }
    }
}
