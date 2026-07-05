//! Shared strict-JSON LLM call over the configured OpenAI-compatible
//! chat-completions endpoint, with the full LLM audit trail. Used by query
//! answer synthesis and the reinforcement validation verdict provider.

use crate::handlers::query::token_usage_from_chat_body;
use crate::prelude::*;
use crate::repository::events::emit_llm_audit_activity;
use crate::state::AppState;

/// One strict-JSON chat call.
pub(crate) struct LlmStrictJsonRequest<'a> {
    /// Project slug for the audit trail.
    pub project: &'a str,
    /// Audit operation label, e.g. `query_answer` or `memory_validation`.
    pub purpose: &'a str,
    /// Human-readable audit summary line.
    pub subject: String,
    pub system_prompt: &'a str,
    pub user_prompt: String,
    /// Caller-side clamp on top of `llm.max_output_tokens`.
    pub max_output_tokens_cap: u32,
}

pub(crate) struct LlmCallOutcome {
    /// Trimmed content of the first choice message.
    pub content: String,
    pub token_usage: Option<TokenUsage>,
    /// The request body, so callers can audit their own parse failures.
    pub request_body: serde_json::Value,
    pub started: std::time::Instant,
}

/// Sends the call, checks status, extracts message content, and audits
/// every transport/protocol failure plus success. Semantic parse failures
/// of the returned JSON belong to the caller (audit with
/// `outcome.request_body`).
pub(crate) async fn call_llm_strict_json(
    state: &AppState,
    request: &LlmStrictJsonRequest<'_>,
) -> Result<LlmCallOutcome> {
    if !is_supported_llm_provider(&state.config.llm.provider)
        || state.config.llm.model.trim().is_empty()
    {
        anyhow::bail!("llm is not configured");
    }
    let api_key = resolve_llm_api_key(&state.config.llm);
    if llm_requires_api_key(&state.config.llm) && api_key.is_none() {
        anyhow::bail!("read llm api key {}", state.config.llm.api_key_env);
    }
    let url = format!(
        "{}/chat/completions",
        effective_llm_base_url(&state.config.llm)
    );
    let mut request_body = serde_json::json!({
        "model": state.config.llm.model,
        "temperature": 0.0,
        "messages": [
            { "role": "system", "content": request.system_prompt },
            { "role": "user", "content": request.user_prompt }
        ]
    });
    request_body[llm_max_output_tokens_field(&state.config.llm.provider)] = serde_json::json!(
        state
            .config
            .llm
            .max_output_tokens
            .min(request.max_output_tokens_cap)
    );

    let started = std::time::Instant::now();
    let audit_error = |error: &str, token_usage: Option<TokenUsage>, body: &serde_json::Value| {
        emit_llm_audit_activity(
            state,
            request.project,
            request.purpose,
            request.subject.clone(),
            body,
            "error",
            Some(error),
            Some(started.elapsed().as_millis() as u64),
            token_usage,
        );
    };

    let mut builder = state.http_client.post(url);
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key);
    }
    let http_response = match builder.json(&request_body).send().await {
        Ok(response) => response,
        Err(error) => {
            audit_error(&format!("send llm request: {error}"), None, &request_body);
            return Err(error).context("send llm request");
        }
    };
    let status = http_response.status();
    let body = match http_response.text().await {
        Ok(body) => body,
        Err(error) => {
            audit_error(&format!("read llm body: {error}"), None, &request_body);
            return Err(error).context("read llm body");
        }
    };
    let token_usage = token_usage_from_chat_body(&body);
    if !status.is_success() {
        let error = format!("llm call failed: {status} {body}");
        audit_error(&error, token_usage.clone(), &request_body);
        anyhow::bail!(error);
    }
    let content = match chat_content_from_body(&body) {
        Ok(content) => content,
        Err(error) => {
            audit_error(&error.to_string(), token_usage.clone(), &request_body);
            return Err(error);
        }
    };
    emit_llm_audit_activity(
        state,
        request.project,
        request.purpose,
        request.subject.clone(),
        &request_body,
        "success",
        None,
        Some(started.elapsed().as_millis() as u64),
        token_usage.clone(),
    );
    Ok(LlmCallOutcome {
        content,
        token_usage,
        request_body,
        started,
    })
}

/// Extracts the first choice's trimmed message content from a
/// chat-completions response body.
pub(crate) fn chat_content_from_body(body: &str) -> Result<String> {
    let payload: serde_json::Value =
        serde_json::from_str(body).context("parse llm response body")?;
    payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("llm response missing content"))
}
