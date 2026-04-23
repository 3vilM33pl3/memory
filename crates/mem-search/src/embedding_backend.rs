//! HTTP backends that turn a batch of strings into vectors. Each provider
//! speaks its own dialect — the trait is the common denominator the rest of
//! the crate depends on.
//!
//! When adding a provider:
//!
//! 1. Implement `EmbeddingBackend` for a new struct.
//! 2. Return a sensible default `base_url` from `default_base_url` so
//!    deployments can override without reading a doc.
//! 3. Register the provider string in `build_backend`.
//! 4. Add a unit test covering request shape + response parsing.
//!
//! Every backend speaks the same `EmbeddingPurpose` contract. OpenAI-style
//! providers ignore it; Voyage, Cohere, and Gemini all use it for retrieval
//! quality.

use anyhow::{Context, Result};
use async_trait::async_trait;
use mem_api::{EmbeddingBackendConfig, resolve_secret_value};
use pgvector::Vector;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Which side of retrieval a batch is for. Provider backends that distinguish
/// between indexing-time embeddings and query-time embeddings use this to
/// pass the right hint (e.g. Voyage's `input_type`, Cohere's `input_type`,
/// Gemini's `taskType`). OpenAI-compatible providers ignore it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingPurpose {
    Document,
    Query,
}

/// Identity of a vector space — stored alongside every embedding so multiple
/// providers can coexist in `memory_chunk_embeddings` without collision.
#[derive(Debug, Clone)]
pub struct EmbeddingSpace {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub space_key: String,
}

impl EmbeddingSpace {
    pub fn new(provider: &str, base_url: &str, model: &str) -> Self {
        let provider = provider.trim().to_string();
        let base_url = base_url.trim_end_matches('/').to_string();
        let model = model.trim().to_string();
        let space_key = format!("{provider}|{base_url}|{model}");
        Self {
            provider,
            base_url,
            model,
            space_key,
        }
    }
}

#[async_trait]
pub trait EmbeddingBackend: Send + Sync {
    fn space(&self) -> &EmbeddingSpace;
    async fn embed(&self, input: &[String], purpose: EmbeddingPurpose) -> Result<Vec<Vector>>;
}

/// Resolve a provider string + config into a concrete backend. Returns `None`
/// when the config is unusable (unknown provider, empty model, missing API
/// key) so the caller can decide to run without semantic search.
pub fn build_backend(config: &EmbeddingBackendConfig) -> Option<Arc<dyn EmbeddingBackend>> {
    if config.model.trim().is_empty() {
        return None;
    }
    let api_key = resolve_secret_value(&config.api_key_env)?;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return None;
    }
    let provider = config.provider.trim();
    let base_url = if config.base_url.trim().is_empty() {
        default_base_url(provider)?.to_string()
    } else {
        config.base_url.trim_end_matches('/').to_string()
    };
    let client = Client::new();
    match provider {
        "openai_compatible" | "openai" => Some(Arc::new(OpenAiBackend::new(
            client,
            base_url,
            config.model.trim().to_string(),
            api_key.to_string(),
        ))),
        "voyage" => Some(Arc::new(VoyageBackend::new(
            client,
            base_url,
            config.model.trim().to_string(),
            api_key.to_string(),
        ))),
        "cohere" => Some(Arc::new(CohereBackend::new(
            client,
            base_url,
            config.model.trim().to_string(),
            api_key.to_string(),
        ))),
        "gemini" => Some(Arc::new(GeminiBackend::new(
            client,
            base_url,
            config.model.trim().to_string(),
            api_key.to_string(),
        ))),
        _ => None,
    }
}

fn default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai_compatible" | "openai" => Some("https://api.openai.com/v1"),
        "voyage" => Some("https://api.voyageai.com"),
        "cohere" => Some("https://api.cohere.com"),
        "gemini" => Some("https://generativelanguage.googleapis.com/v1beta"),
        _ => None,
    }
}

// ---------- OpenAI / OpenAI-compatible ----------

pub struct OpenAiBackend {
    client: Client,
    space: EmbeddingSpace,
    api_key: String,
}

impl OpenAiBackend {
    fn new(client: Client, base_url: String, model: String, api_key: String) -> Self {
        Self {
            client,
            space: EmbeddingSpace::new("openai_compatible", &base_url, &model),
            api_key,
        }
    }
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct OpenAiResponse {
    data: Vec<OpenAiItem>,
}

#[derive(Deserialize)]
struct OpenAiItem {
    index: usize,
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingBackend for OpenAiBackend {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    async fn embed(&self, input: &[String], _purpose: EmbeddingPurpose) -> Result<Vec<Vector>> {
        let body = OpenAiRequest {
            model: &self.space.model,
            input,
        };
        let url = format!("{}/embeddings", self.space.base_url);
        let response = self
            .client
            .post(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("openai embedding request")?;
        let status = response.status();
        let text = response.text().await.context("read openai response")?;
        if !status.is_success() {
            anyhow::bail!("openai embedding request failed: {status} {text}");
        }
        let parsed: OpenAiResponse =
            serde_json::from_str(&text).context("parse openai embedding response")?;
        let mut data = parsed.data;
        data.sort_by_key(|item| item.index);
        Ok(data.into_iter().map(|item| Vector::from(item.embedding)).collect())
    }
}

// ---------- Voyage AI ----------
//
// Voyage's REST contract is extremely close to OpenAI's: same URL shape,
// same Bearer auth, same response envelope. The only meaningful difference
// is the `input_type` hint, which significantly improves retrieval quality
// when you're careful to pass `document` at indexing time and `query` at
// query time.

pub struct VoyageBackend {
    client: Client,
    space: EmbeddingSpace,
    api_key: String,
}

impl VoyageBackend {
    fn new(client: Client, base_url: String, model: String, api_key: String) -> Self {
        Self {
            client,
            space: EmbeddingSpace::new("voyage", &base_url, &model),
            api_key,
        }
    }
}

#[derive(Serialize)]
struct VoyageRequest<'a> {
    model: &'a str,
    input: &'a [String],
    input_type: &'static str,
}

#[async_trait]
impl EmbeddingBackend for VoyageBackend {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    async fn embed(&self, input: &[String], purpose: EmbeddingPurpose) -> Result<Vec<Vector>> {
        let body = VoyageRequest {
            model: &self.space.model,
            input,
            input_type: match purpose {
                EmbeddingPurpose::Document => "document",
                EmbeddingPurpose::Query => "query",
            },
        };
        let url = format!("{}/v1/embeddings", self.space.base_url);
        let response = self
            .client
            .post(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("voyage embedding request")?;
        let status = response.status();
        let text = response.text().await.context("read voyage response")?;
        if !status.is_success() {
            anyhow::bail!("voyage embedding request failed: {status} {text}");
        }
        // Response shape matches OpenAI's; reuse its types.
        let parsed: OpenAiResponse =
            serde_json::from_str(&text).context("parse voyage embedding response")?;
        let mut data = parsed.data;
        data.sort_by_key(|item| item.index);
        Ok(data.into_iter().map(|item| Vector::from(item.embedding)).collect())
    }
}

// ---------- Cohere ----------

pub struct CohereBackend {
    client: Client,
    space: EmbeddingSpace,
    api_key: String,
}

impl CohereBackend {
    fn new(client: Client, base_url: String, model: String, api_key: String) -> Self {
        Self {
            client,
            space: EmbeddingSpace::new("cohere", &base_url, &model),
            api_key,
        }
    }
}

#[derive(Serialize)]
struct CohereRequest<'a> {
    model: &'a str,
    texts: &'a [String],
    input_type: &'static str,
    embedding_types: [&'static str; 1],
}

#[derive(Deserialize)]
struct CohereResponse {
    embeddings: CohereEmbeddings,
}

#[derive(Deserialize)]
struct CohereEmbeddings {
    float: Vec<Vec<f32>>,
}

#[async_trait]
impl EmbeddingBackend for CohereBackend {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    async fn embed(&self, input: &[String], purpose: EmbeddingPurpose) -> Result<Vec<Vector>> {
        let body = CohereRequest {
            model: &self.space.model,
            texts: input,
            input_type: match purpose {
                EmbeddingPurpose::Document => "search_document",
                EmbeddingPurpose::Query => "search_query",
            },
            embedding_types: ["float"],
        };
        let url = format!("{}/v2/embed", self.space.base_url);
        let response = self
            .client
            .post(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("cohere embedding request")?;
        let status = response.status();
        let text = response.text().await.context("read cohere response")?;
        if !status.is_success() {
            anyhow::bail!("cohere embedding request failed: {status} {text}");
        }
        let parsed: CohereResponse =
            serde_json::from_str(&text).context("parse cohere embedding response")?;
        Ok(parsed
            .embeddings
            .float
            .into_iter()
            .map(Vector::from)
            .collect())
    }
}

// ---------- Gemini ----------

pub struct GeminiBackend {
    client: Client,
    space: EmbeddingSpace,
    api_key: String,
}

impl GeminiBackend {
    fn new(client: Client, base_url: String, model: String, api_key: String) -> Self {
        Self {
            client,
            space: EmbeddingSpace::new("gemini", &base_url, &model),
            api_key,
        }
    }
}

#[derive(Serialize)]
struct GeminiBatchRequest<'a> {
    requests: Vec<GeminiSingleRequest<'a>>,
}

#[derive(Serialize)]
struct GeminiSingleRequest<'a> {
    model: String,
    content: GeminiContent<'a>,
    #[serde(rename = "taskType")]
    task_type: &'static str,
}

#[derive(Serialize)]
struct GeminiContent<'a> {
    parts: Vec<GeminiPart<'a>>,
}

#[derive(Serialize)]
struct GeminiPart<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct GeminiResponse {
    embeddings: Vec<GeminiEmbedding>,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

#[async_trait]
impl EmbeddingBackend for GeminiBackend {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    async fn embed(&self, input: &[String], purpose: EmbeddingPurpose) -> Result<Vec<Vector>> {
        // Gemini wants the model name fully qualified in both URL and body
        // and will reject a bare "text-embedding-004" in the body field —
        // prefix it with "models/" for the JSON while leaving the raw model
        // name in the URL path.
        let qualified_model = if self.space.model.starts_with("models/") {
            self.space.model.clone()
        } else {
            format!("models/{}", self.space.model)
        };
        let task_type = match purpose {
            EmbeddingPurpose::Document => "RETRIEVAL_DOCUMENT",
            EmbeddingPurpose::Query => "RETRIEVAL_QUERY",
        };
        let requests = input
            .iter()
            .map(|text| GeminiSingleRequest {
                model: qualified_model.clone(),
                content: GeminiContent {
                    parts: vec![GeminiPart { text }],
                },
                task_type,
            })
            .collect();
        let body = GeminiBatchRequest { requests };
        let url = format!(
            "{}/models/{}:batchEmbedContents",
            self.space.base_url, self.space.model
        );
        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .context("gemini embedding request")?;
        let status = response.status();
        let text = response.text().await.context("read gemini response")?;
        if !status.is_success() {
            anyhow::bail!("gemini embedding request failed: {status} {text}");
        }
        let parsed: GeminiResponse =
            serde_json::from_str(&text).context("parse gemini embedding response")?;
        Ok(parsed
            .embeddings
            .into_iter()
            .map(|e| Vector::from(e.values))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

    fn config(provider: &str, base: &str, model: &str, key_env: &str) -> EmbeddingBackendConfig {
        EmbeddingBackendConfig {
            provider: provider.to_string(),
            base_url: base.to_string(),
            api_key_env: key_env.to_string(),
            model: model.to_string(),
            batch_size: 16,
            ..EmbeddingBackendConfig::default()
        }
    }

    #[tokio::test]
    async fn openai_backend_roundtrips_batch() {
        const KEY_ENV: &str = "TEST_EMBED_KEY_OPENAI";
        unsafe {
            std::env::set_var(KEY_ENV, "sk-test");
        }
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/embeddings"))
            .and(matchers::header(
                "authorization",
                "Bearer sk-test",
            ))
            .and(matchers::body_json(serde_json::json!({
                "model": "text-embedding-3-small",
                "input": ["hello", "world"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"index": 0, "embedding": [0.1, 0.2]},
                    {"index": 1, "embedding": [0.3, 0.4]},
                ]
            })))
            .mount(&server)
            .await;
        let backend = build_backend(&config(
            "openai_compatible",
            &server.uri(),
            "text-embedding-3-small",
            KEY_ENV,
        ))
        .expect("backend resolves");
        let vectors = backend
            .embed(
                &["hello".to_string(), "world".to_string()],
                EmbeddingPurpose::Document,
            )
            .await
            .unwrap();
        assert_eq!(vectors.len(), 2);
    }

    #[tokio::test]
    async fn voyage_backend_sends_input_type_for_queries() {
        const KEY_ENV: &str = "TEST_EMBED_KEY_VOYAGE";
        unsafe {
            std::env::set_var(KEY_ENV, "vo-test");
        }
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/v1/embeddings"))
            .and(matchers::body_json(serde_json::json!({
                "model": "voyage-3-large",
                "input": ["hi"],
                "input_type": "query",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [ {"index": 0, "embedding": [0.9, 0.8, 0.7]} ]
            })))
            .mount(&server)
            .await;
        let backend =
            build_backend(&config("voyage", &server.uri(), "voyage-3-large", KEY_ENV))
                .expect("backend");
        let vectors = backend
            .embed(&["hi".to_string()], EmbeddingPurpose::Query)
            .await
            .unwrap();
        assert_eq!(vectors.len(), 1);
    }

    #[tokio::test]
    async fn cohere_backend_parses_float_envelope() {
        const KEY_ENV: &str = "TEST_EMBED_KEY_COHERE";
        unsafe {
            std::env::set_var(KEY_ENV, "co-test");
        }
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/v2/embed"))
            .and(matchers::body_json(serde_json::json!({
                "model": "embed-english-v3.0",
                "texts": ["doc"],
                "input_type": "search_document",
                "embedding_types": ["float"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "embeddings": { "float": [[0.1, 0.2, 0.3]] }
            })))
            .mount(&server)
            .await;
        let backend =
            build_backend(&config("cohere", &server.uri(), "embed-english-v3.0", KEY_ENV))
                .expect("backend");
        let vectors = backend
            .embed(&["doc".to_string()], EmbeddingPurpose::Document)
            .await
            .unwrap();
        assert_eq!(vectors.len(), 1);
    }

    #[tokio::test]
    async fn gemini_backend_qualifies_model_and_sends_task_type() {
        const KEY_ENV: &str = "TEST_EMBED_KEY_GEMINI";
        unsafe {
            std::env::set_var(KEY_ENV, "ai-test");
        }
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path(
                "/models/text-embedding-004:batchEmbedContents",
            ))
            .and(matchers::header("x-goog-api-key", "ai-test"))
            .and(matchers::body_json(serde_json::json!({
                "requests": [
                    {
                        "model": "models/text-embedding-004",
                        "content": { "parts": [{"text": "hello"}] },
                        "taskType": "RETRIEVAL_DOCUMENT"
                    }
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "embeddings": [ { "values": [0.5, 0.5] } ]
            })))
            .mount(&server)
            .await;
        let backend = build_backend(&config(
            "gemini",
            &server.uri(),
            "text-embedding-004",
            KEY_ENV,
        ))
        .expect("backend");
        let vectors = backend
            .embed(&["hello".to_string()], EmbeddingPurpose::Document)
            .await
            .unwrap();
        assert_eq!(vectors.len(), 1);
    }

    #[test]
    fn unknown_provider_yields_no_backend() {
        const KEY_ENV: &str = "TEST_EMBED_KEY_UNKNOWN";
        unsafe {
            std::env::set_var(KEY_ENV, "k");
        }
        let cfg = config("wishful-ai", "https://example.com", "some-model", KEY_ENV);
        assert!(build_backend(&cfg).is_none());
    }
}
