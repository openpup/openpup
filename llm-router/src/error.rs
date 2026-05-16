use thiserror::Error;

pub type Result<T> = std::result::Result<T, RouterError>;

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("no available provider configured")]
    NoProvider,
    #[error("route `{0}` is not configured")]
    RouteNotConfigured(&'static str),
    #[error("provider `{0}` not found")]
    ProviderNotFound(String),
    #[error("provider `{0}` is disabled")]
    ProviderDisabled(String),
    #[error("provider `{0}` has no model configured")]
    ProviderModelMissing(String),
    #[error("provider `{provider}` uses unsupported protocol `{protocol}`")]
    UnsupportedProtocol { provider: String, protocol: String },
    #[error("provider `{0}` is missing api_key")]
    MissingApiKey(String),
    #[error("request failed: {0}")]
    Request(String),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("parse error: {0}")]
    Parse(String),
}
