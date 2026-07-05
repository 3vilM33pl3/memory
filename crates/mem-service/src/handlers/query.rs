use crate::prelude::*;
use crate::*;
use mem_api::GlobalQueryRequest;

pub(crate) async fn query(
    State(state): State<AppState>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<mem_api::QueryResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/query", &request, false).await?,
        ));
    }
    let pool = &state.pool()?;
    let embedders = state.embedders.read().await;
    match mem_search::query_memory_with_configs(
        pool,
        &request,
        embedders.active(),
        &state.config.provenance,
        &mem_search::ReinforcementRankParams::from(&state.config.reinforcement),
    )
    .await
    {
        Ok(mut response) => {
            if should_enrich_query_answer_with_llm(&request) {
                enrich_query_answer_with_llm(&state, &request, &mut response).await;
            }
            crate::reinforcement::record_query_access(&state, &response);
            notify_project_changed_with_metadata(
                &state,
                request.project.clone(),
                None,
                ActivityKind::Query,
                format!("Query: {}", summarize_query(&request.query)),
                Some(query_activity_details(&request, &response)),
                None,
                None,
                Some("query".to_string()),
                None,
                Some(response.answer_generation.duration_ms),
                Some(state.config.llm.provider.clone()),
                Some(state.config.llm.model.clone()),
                response.answer_generation.token_usage.clone(),
            );
            Ok(Json(response))
        }
        Err(error) => {
            let diagnostic =
                classify_anyhow_diagnostic(&error, "search", "query", DiagnosticSeverity::Error);
            notify_project_changed(
                &state,
                request.project.clone(),
                None,
                ActivityKind::QueryError,
                format!("Query error: {}", summarize_query(&request.query)),
                Some(ActivityDetails::Query {
                    query: request.query.clone(),
                    top_k: request.top_k,
                    result_count: 0,
                    confidence: 0.0,
                    insufficient_evidence: true,
                    total_duration_ms: 0,
                    graph_status: None,
                    graph_candidates: 0,
                    graph_augmented_candidates: 0,
                    graph_duration_ms: 0,
                    graph_result_count: 0,
                    graph_connection_count: 0,
                    graph_connections: Vec::new(),
                    answer: None,
                    error: Some(error.to_string()),
                }),
            );
            notify_project_diagnostic(&state, request.project.clone(), diagnostic.clone());
            Err(ApiError::diagnostic(
                StatusCode::INTERNAL_SERVER_ERROR,
                diagnostic,
            ))
        }
    }
}

pub(crate) async fn query_global(
    State(state): State<AppState>,
    Json(request): Json<GlobalQueryRequest>,
) -> Result<Json<mem_api::QueryResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/query/global", &request, false).await?,
        ));
    }
    let pool = &state.pool()?;
    let embedders = state.embedders.read().await;
    mem_search::query_memory_global_with_configs(
        pool,
        &request,
        embedders.active(),
        &state.config.provenance,
        &mem_search::ReinforcementRankParams::from(&state.config.reinforcement),
    )
    .await
    .map(|response| {
        crate::reinforcement::record_query_access(&state, &response);
        Json(response)
    })
    .map_err(|error| {
        let diagnostic =
            classify_anyhow_diagnostic(&error, "search", "query_global", DiagnosticSeverity::Error);
        ApiError::diagnostic(StatusCode::INTERNAL_SERVER_ERROR, diagnostic)
    })
}

pub(crate) fn should_enrich_query_answer_with_llm(request: &QueryRequest) -> bool {
    matches!(
        request.answer_mode.unwrap_or_default(),
        QueryAnswerMode::Auto | QueryAnswerMode::Llm
    )
}

pub(crate) fn query_activity_details(
    request: &QueryRequest,
    response: &QueryResponse,
) -> ActivityDetails {
    let graph_connections = query_activity_graph_connections(response);
    let graph_connection_count = response
        .results
        .iter()
        .map(|result| result.graph_connections.len())
        .sum();
    let graph_result_count = response
        .results
        .iter()
        .filter(|result| !result.graph_connections.is_empty() || result.debug.graph_boost > 0.0)
        .count();

    ActivityDetails::Query {
        query: request.query.clone(),
        top_k: request.top_k,
        result_count: response.results.len(),
        confidence: response.confidence,
        insufficient_evidence: response.insufficient_evidence,
        total_duration_ms: response.diagnostics.total_duration_ms,
        graph_status: if response.diagnostics.graph_status.is_empty() {
            None
        } else {
            Some(response.diagnostics.graph_status.clone())
        },
        graph_candidates: response.diagnostics.graph_candidates,
        graph_augmented_candidates: response.diagnostics.graph_augmented_candidates,
        graph_duration_ms: response.diagnostics.graph_duration_ms,
        graph_result_count,
        graph_connection_count,
        graph_connections,
        answer: Some(response.answer.clone()),
        error: None,
    }
}

pub(crate) fn query_activity_graph_connections(
    response: &QueryResponse,
) -> Vec<QueryGraphConnection> {
    response
        .results
        .iter()
        .flat_map(|result| result.graph_connections.iter().cloned())
        .take(QUERY_ACTIVITY_GRAPH_CONNECTION_LIMIT)
        .collect()
}

pub(crate) async fn enrich_query_answer_with_llm(
    state: &AppState,
    request: &QueryRequest,
    response: &mut QueryResponse,
) {
    let started = std::time::Instant::now();
    let result = synthesize_query_answer_with_llm(state, request, response).await;
    match result {
        Ok(answer) => {
            response.answer = answer.answer;
            response.confidence = answer.confidence;
            response.insufficient_evidence = answer.insufficient_evidence;
            response.answer_citations = answer.citations;
            response.answer_generation = QueryAnswerGeneration {
                method: QueryAnswerMethod::Llm,
                cited_result_numbers: answer.cited_result_numbers,
                evidence_count: response.answer_citations.len(),
                duration_ms: started.elapsed().as_millis() as u64,
                fallback_reason: None,
                token_usage: answer.token_usage,
            };
        }
        Err(error) => {
            let cited_result_numbers = response
                .answer_citations
                .iter()
                .map(|citation| citation.result_number)
                .collect::<Vec<_>>();
            response.answer_generation = QueryAnswerGeneration {
                method: QueryAnswerMethod::Fallback,
                cited_result_numbers,
                evidence_count: response.answer_citations.len(),
                duration_ms: started.elapsed().as_millis() as u64,
                fallback_reason: Some(error.to_string()),
                token_usage: None,
            };
        }
    }
}

#[derive(Debug)]
pub(crate) struct LlmQueryAnswer {
    pub(crate) answer: String,
    pub(crate) confidence: f32,
    pub(crate) insufficient_evidence: bool,
    pub(crate) cited_result_numbers: Vec<usize>,
    pub(crate) citations: Vec<QueryAnswerCitation>,
    pub(crate) token_usage: Option<TokenUsage>,
}

#[derive(Debug, SerdeDeserialize)]
pub(crate) struct LlmQueryAnswerPayload {
    pub(crate) answer: String,
    #[serde(default)]
    pub(crate) confidence: f32,
    #[serde(default)]
    pub(crate) insufficient_evidence: bool,
    #[serde(default)]
    pub(crate) citations: Vec<usize>,
}

pub(crate) async fn synthesize_query_answer_with_llm(
    state: &AppState,
    request: &QueryRequest,
    response: &QueryResponse,
) -> Result<LlmQueryAnswer> {
    if response.results.is_empty() {
        anyhow::bail!("no query memories available for llm answer synthesis");
    }
    if !is_supported_llm_provider(&state.config.llm.provider)
        || state.config.llm.model.trim().is_empty()
    {
        anyhow::bail!("llm query answer is not configured");
    }
    let api_key = resolve_llm_api_key(&state.config.llm);
    if llm_requires_api_key(&state.config.llm) && api_key.is_none() {
        anyhow::bail!(
            "read llm api key {} for query answer",
            state.config.llm.api_key_env
        );
    }
    let url = format!(
        "{}/chat/completions",
        effective_llm_base_url(&state.config.llm)
    );
    let mut request_body = serde_json::json!({
        "model": state.config.llm.model,
        "temperature": 0.0,
        "messages": [
            {
                "role": "system",
                "content": "Answer project-memory questions using only the numbered memories supplied by the user. Return strict JSON with keys: answer (string), citations (array of result numbers), confidence (0..1), insufficient_evidence (boolean). Cite only memories that directly support the answer. If evidence is weak, say so and set insufficient_evidence true."
            },
            {
                "role": "user",
                "content": build_query_answer_prompt(request, response)
            }
        ]
    });
    request_body[llm_max_output_tokens_field(&state.config.llm.provider)] =
        serde_json::json!(state.config.llm.max_output_tokens.min(800));
    let started = std::time::Instant::now();
    let mut builder = state.http_client.post(url);
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key);
    }
    let http_response = match builder.json(&request_body).send().await {
        Ok(response) => response,
        Err(error) => {
            emit_llm_audit_activity(
                state,
                &request.project,
                "query_answer",
                format!("Question: {}", summarize_query(&request.query)),
                &request_body,
                "error",
                Some(&format!("send llm query answer request: {error}")),
                Some(started.elapsed().as_millis() as u64),
                None,
            );
            return Err(error).context("send llm query answer request");
        }
    };
    let status = http_response.status();
    let body = match http_response.text().await {
        Ok(body) => body,
        Err(error) => {
            emit_llm_audit_activity(
                state,
                &request.project,
                "query_answer",
                format!("Question: {}", summarize_query(&request.query)),
                &request_body,
                "error",
                Some(&format!("read llm query answer body: {error}")),
                Some(started.elapsed().as_millis() as u64),
                None,
            );
            return Err(error).context("read llm query answer body");
        }
    };
    let token_usage = token_usage_from_chat_body(&body);
    if !status.is_success() {
        let error = format!("llm query answer failed: {status} {body}");
        emit_llm_audit_activity(
            state,
            &request.project,
            "query_answer",
            format!("Question: {}", summarize_query(&request.query)),
            &request_body,
            "error",
            Some(&error),
            Some(started.elapsed().as_millis() as u64),
            token_usage,
        );
        anyhow::bail!("llm query answer failed: {status} {body}");
    }
    let mut answer = match parse_llm_query_answer_body(&body, response) {
        Ok(answer) => answer,
        Err(error) => {
            emit_llm_audit_activity(
                state,
                &request.project,
                "query_answer",
                format!("Question: {}", summarize_query(&request.query)),
                &request_body,
                "error",
                Some(&error.to_string()),
                Some(started.elapsed().as_millis() as u64),
                token_usage,
            );
            return Err(error);
        }
    };
    answer.token_usage = token_usage;
    emit_llm_audit_activity(
        state,
        &request.project,
        "query_answer",
        format!("Question: {}", summarize_query(&request.query)),
        &request_body,
        "success",
        None,
        Some(started.elapsed().as_millis() as u64),
        answer.token_usage.clone(),
    );
    Ok(answer)
}

pub(crate) fn build_query_answer_prompt(
    request: &QueryRequest,
    response: &QueryResponse,
) -> String {
    let mut lines = vec![
        format!("Project: {}", request.project),
        format!("Question: {}", request.query),
        String::new(),
        "Returned memories:".to_string(),
    ];
    for (index, result) in response.results.iter().enumerate() {
        let route = match (&result.project, &result.repo_root) {
            (Some(project), Some(repo_root)) => format!(" project={project} repo_root={repo_root}"),
            (Some(project), None) => format!(" project={project}"),
            (None, Some(repo_root)) => format!(" repo_root={repo_root}"),
            (None, None) => String::new(),
        };
        lines.push(format!(
            "[{}] type={} score={:.2}{} summary={}",
            index + 1,
            result.memory_type,
            result.score,
            route,
            result.summary
        ));
        lines.push(format!("snippet: {}", result.snippet));
        if !result.sources.is_empty() {
            let sources = result
                .sources
                .iter()
                .take(3)
                .map(|source| {
                    let mut parts = vec![source_kind_name(&source.source_kind).to_string()];
                    if let Some(path) = &source.file_path {
                        parts.push(path.clone());
                    }
                    if let Some(excerpt) = &source.excerpt {
                        parts.push(excerpt.clone());
                    }
                    parts.join(" | ")
                })
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("sources: {sources}"));
        }
        if !result.graph_connections.is_empty() {
            let graph_connections = result
                .graph_connections
                .iter()
                .take(3)
                .map(|connection| {
                    let mut parts = vec![connection.reason.clone(), connection.file_path.clone()];
                    if let Some(symbol) = &connection.symbol {
                        parts.push(format!("symbol={symbol}"));
                    }
                    if let Some(edge_kind) = &connection.edge_kind {
                        parts.push(format!("edge={edge_kind}"));
                    }
                    if let Some(neighbor) = &connection.neighbor_symbol {
                        parts.push(format!("neighbor={neighbor}"));
                    }
                    parts.push(format!("boost={:.2}", connection.score_boost));
                    parts.join(" | ")
                })
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("graph: {graph_connections}"));
        }
        lines.push(String::new());
    }
    lines.push(
        "Return JSON only, for example: {\"answer\":\"... [1]\",\"citations\":[1],\"confidence\":0.82,\"insufficient_evidence\":false}"
            .to_string(),
    );
    lines.join("\n")
}

pub(crate) fn parse_llm_query_answer_body(
    body: &str,
    response: &QueryResponse,
) -> Result<LlmQueryAnswer> {
    let payload: serde_json::Value =
        serde_json::from_str(body).context("parse llm query answer response")?;
    let content = payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| anyhow::anyhow!("llm query answer missing content"))?;
    parse_llm_query_answer_content(content, response)
}

pub(crate) fn parse_llm_query_answer_content(
    content: &str,
    response: &QueryResponse,
) -> Result<LlmQueryAnswer> {
    let json = content
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            content
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim)
        .unwrap_or(content);
    let payload: LlmQueryAnswerPayload =
        serde_json::from_str(json).context("parse llm query answer content")?;
    let answer = payload.answer.trim();
    if answer.is_empty() {
        anyhow::bail!("llm query answer was empty");
    }
    let cited_result_numbers = validate_query_answer_citations(&payload.citations, response)?;
    let citations = citations_from_result_numbers(&cited_result_numbers, response);
    Ok(LlmQueryAnswer {
        answer: answer.to_string(),
        confidence: payload.confidence.clamp(0.0, 1.0),
        insufficient_evidence: payload.insufficient_evidence || citations.is_empty(),
        cited_result_numbers,
        citations,
        token_usage: None,
    })
}

pub(crate) fn token_usage_from_chat_body(body: &str) -> Option<TokenUsage> {
    let payload: serde_json::Value = serde_json::from_str(body).ok()?;
    let usage = payload.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.get("cached_input_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens);
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
        && total_tokens == 0
    {
        return None;
    }
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
    })
}

pub(crate) fn validate_query_answer_citations(
    citations: &[usize],
    response: &QueryResponse,
) -> Result<Vec<usize>> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for citation in citations {
        if *citation == 0 || *citation > response.results.len() {
            anyhow::bail!("llm query answer cited unavailable result {citation}");
        }
        if seen.insert(*citation) {
            result.push(*citation);
        }
    }
    Ok(result)
}

pub(crate) fn citations_from_result_numbers(
    cited_result_numbers: &[usize],
    response: &QueryResponse,
) -> Vec<QueryAnswerCitation> {
    cited_result_numbers
        .iter()
        .filter_map(|number| {
            let result = response.results.get(number.saturating_sub(1))?;
            Some(QueryAnswerCitation {
                result_number: *number,
                memory_id: result.memory_id,
                project: result.project.clone(),
                project_name: result.project_name.clone(),
                repo_root: result.repo_root.clone(),
                memory_type: result.memory_type.clone(),
                summary: result.summary.clone(),
                snippet: result.snippet.clone(),
            })
        })
        .collect()
}
