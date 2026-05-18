use crate::prelude::*;
use crate::*;

pub(crate) async fn reindex(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReindexRequest>,
) -> Result<Json<ReindexResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/reindex", &request, true).await?,
        ));
    }
    let embedders = state.embedders.read().await;
    let selected_keys: Vec<String> = if let Some(name) = request.backend.as_deref() {
        let service = embedders.get(name).ok_or_else(|| {
            ApiError::validation(ValidationError::new(format!(
                "unknown embedding backend: {name}"
            )))
        })?;
        vec![service.embedding_space_key()]
    } else {
        embedders
            .iter()
            .map(|(_, service)| service.embedding_space_key())
            .collect()
    };
    let project = request.project.clone();
    let count = if request.dry_run {
        if request.backend.is_some() {
            count_missing_embedding_chunks(state.pool()?, &request.project, &selected_keys).await?
        } else {
            sqlx::query(
                r#"
                SELECT COUNT(*) AS count
                FROM memory_entries m
                JOIN projects p ON p.id = m.project_id
                WHERE p.slug = $1
                "#,
            )
            .bind(&request.project)
            .fetch_one(state.pool()?)
            .await
            .map_err(ApiError::sql)?
            .try_get::<i64, _>("count")
            .map_err(ApiError::sql)? as u64
        }
    } else {
        rebuild_chunks(
            state.pool()?,
            &request.project,
            &embedders,
            request.backend.as_deref(),
        )
        .await
        .map_err(|error| {
            ApiError::diagnostic(
                StatusCode::INTERNAL_SERVER_ERROR,
                classify_anyhow_diagnostic(
                    &error,
                    "embeddings",
                    "reindex",
                    DiagnosticSeverity::Error,
                ),
            )
        })?
    };
    if request.dry_run {
        return Ok(Json(ReindexResponse {
            reindexed_entries: count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Reindex,
        format!("Reindexed {count} memory entry/entries."),
        Some(ActivityDetails::Reindex {
            reindexed_entries: count,
        }),
    );
    Ok(Json(ReindexResponse {
        reindexed_entries: count,
        dry_run: false,
    }))
}

pub(crate) async fn reembed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReembedRequest>,
) -> Result<Json<ReembedResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/reembed", &request, true).await?,
        ));
    }
    let embedders = state.embedders.read().await;
    if embedders.is_empty() {
        return Err(ApiError::validation(ValidationError::new(
            "embeddings are not configured; cannot re-embed",
        )));
    }
    let selected_keys: Vec<(String, String)> = match request.backend.as_deref() {
        Some(name) => {
            let service = embedders.get(name).ok_or_else(|| {
                ApiError::validation(ValidationError::new(format!(
                    "unknown embedding backend: {name}"
                )))
            })?;
            vec![(name.to_string(), service.embedding_space_key())]
        }
        None => embedders
            .iter()
            .map(|(name, service)| (name.to_string(), service.embedding_space_key()))
            .collect(),
    };
    let project = request.project.clone();
    let count = if request.dry_run {
        let space_keys = selected_keys
            .iter()
            .map(|(_, space_key)| space_key.clone())
            .collect::<Vec<_>>();
        count_missing_embedding_chunks(state.pool()?, &request.project, &space_keys).await?
    } else {
        reembed_project_chunks(
            state.pool()?,
            &request.project,
            &embedders,
            request.backend.as_deref(),
        )
        .await
        .map_err(|error| {
            ApiError::diagnostic(
                StatusCode::INTERNAL_SERVER_ERROR,
                classify_anyhow_diagnostic(
                    &error,
                    "embeddings",
                    "reembed",
                    DiagnosticSeverity::Error,
                ),
            )
        })?
    };
    if request.dry_run {
        return Ok(Json(ReembedResponse {
            reembedded_chunks: count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Reembed,
        format!("Re-embedded {count} chunk(s)."),
        Some(ActivityDetails::Reembed {
            reembedded_chunks: count,
        }),
    );
    Ok(Json(ReembedResponse {
        reembedded_chunks: count,
        dry_run: false,
    }))
}

pub(crate) async fn count_missing_embedding_chunks(
    pool: &PgPool,
    project: &str,
    space_keys: &[String],
) -> Result<u64, ApiError> {
    let mut total: i64 = 0;
    for space_key in space_keys {
        total += sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM memory_chunks mc
            JOIN memory_entries m ON m.id = mc.memory_entry_id
            JOIN projects p ON p.id = m.project_id
            LEFT JOIN memory_chunk_embeddings mce
              ON mce.chunk_id = mc.id
             AND mce.embedding_space = $2
            WHERE p.slug = $1
              AND m.status = 'active'
              AND m.is_tombstone = FALSE
              AND (
                    mce.chunk_id IS NULL
                    OR mce.embedding_dimension IS NULL
                  )
            "#,
        )
        .bind(project)
        .bind(space_key)
        .fetch_one(pool)
        .await
        .map_err(ApiError::sql)?
        .try_get::<i64, _>("count")
        .map_err(ApiError::sql)?;
    }
    Ok(total as u64)
}

pub(crate) async fn prune_embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PruneEmbeddingsRequest>,
) -> Result<Json<PruneEmbeddingsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/prune-embeddings", &request, true).await?,
        ));
    }
    let embedders = state.embedders.read().await;
    if embedders.is_empty() {
        return Err(ApiError::validation(ValidationError::new(
            "embeddings are not configured; cannot prune inactive spaces",
        )));
    }
    let keep: Vec<String> = embedders
        .iter()
        .map(|(_, service)| service.embedding_space_key())
        .collect();
    let project = request.project.clone();
    let count = if request.dry_run {
        sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM memory_chunk_embeddings mce
            JOIN memory_chunks mc ON mc.id = mce.chunk_id
            JOIN memory_entries m ON m.id = mc.memory_entry_id
            JOIN projects p ON p.id = m.project_id
            WHERE p.slug = $1
              AND m.status = 'active'
              AND mce.embedding_space <> ALL($2)
            "#,
        )
        .bind(&request.project)
        .bind(&keep)
        .fetch_one(state.pool()?)
        .await
        .map_err(ApiError::sql)?
        .try_get::<i64, _>("count")
        .map_err(ApiError::sql)? as u64
    } else {
        prune_project_embeddings(state.pool()?, &request.project, &embedders)
            .await
            .map_err(|error| {
                ApiError::diagnostic(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    classify_anyhow_diagnostic(
                        &error,
                        "embeddings",
                        "prune_embeddings",
                        DiagnosticSeverity::Error,
                    ),
                )
            })?
    };
    if request.dry_run {
        return Ok(Json(PruneEmbeddingsResponse {
            pruned_embeddings: count,
            dry_run: true,
        }));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Reembed,
        format!("Pruned {count} inactive embedding row(s)."),
        Some(ActivityDetails::Reembed {
            reembedded_chunks: count,
        }),
    );
    Ok(Json(PruneEmbeddingsResponse {
        pruned_embeddings: count,
        dry_run: false,
    }))
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct EmbeddingBackendsQuery {
    project: Option<String>,
}

pub(crate) async fn list_embedding_backends(
    State(state): State<AppState>,
    Query(params): Query<EmbeddingBackendsQuery>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    build_embedding_backends_response(&state, params.project.as_deref()).await
}

pub(crate) async fn build_embedding_backends_response(
    state: &AppState,
    project: Option<&str>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    let embedders = state.embedders.read().await;
    let active_name = embedders.active_name().map(|s| s.to_string());
    // Map name -> space_key for ready backends so we can merge coverage
    // counts (which are grouped by embedding_space) back by name.
    let space_by_name: std::collections::HashMap<String, String> = embedders
        .iter()
        .map(|(name, service)| (name.to_string(), service.embedding_space_key()))
        .collect();
    let ready: std::collections::HashSet<String> = space_by_name.keys().cloned().collect();

    let coverage_by_space: std::collections::HashMap<String, (i64, i64)> = match project {
        Some(slug) => fetch_project_embedding_coverage(state, slug).await?,
        None => std::collections::HashMap::new(),
    };

    let backends = state
        .config
        .embeddings
        .backends
        .iter()
        .map(|backend| {
            let base_url = effective_embedding_base_url(&backend.provider, &backend.base_url)
                .unwrap_or_else(|| backend.base_url.trim_end_matches('/').to_string());
            let (project_chunk_count, project_memory_count) = if project.is_some() {
                match space_by_name
                    .get(&backend.name)
                    .and_then(|key| coverage_by_space.get(key))
                {
                    Some((chunks, memories)) => (Some(*chunks), Some(*memories)),
                    None => (Some(0), Some(0)),
                }
            } else {
                (None, None)
            };
            EmbeddingBackendInfo {
                name: backend.name.clone(),
                provider: backend.provider.clone(),
                base_url,
                model: backend.model.clone(),
                active: active_name.as_deref() == Some(backend.name.as_str()),
                ready: ready.contains(&backend.name),
                create_enabled: if ready.contains(&backend.name) {
                    embedders.create_enabled(&backend.name)
                } else {
                    backend.create_enabled
                },
                project_chunk_count,
                project_memory_count,
            }
        })
        .collect();
    Ok(Json(EmbeddingBackendsResponse {
        backends,
        active: active_name,
        create_enabled: state
            .automated_embedding_creation_enabled
            .load(Ordering::Relaxed),
    }))
}

pub(crate) async fn fetch_project_embedding_coverage(
    state: &AppState,
    slug: &str,
) -> Result<std::collections::HashMap<String, (i64, i64)>, ApiError> {
    let Some(pool) = state.pool.as_ref() else {
        return Ok(std::collections::HashMap::new());
    };
    let rows = sqlx::query(
        r#"
        SELECT mce.embedding_space,
               COUNT(*)::bigint                       AS chunk_count,
               COUNT(DISTINCT mc.memory_entry_id)::bigint AS memory_count
        FROM memory_chunk_embeddings mce
        JOIN memory_chunks mc ON mc.id = mce.chunk_id
        JOIN memory_entries m ON m.id = mc.memory_entry_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND m.status = 'active'
          AND m.is_tombstone = FALSE
        GROUP BY mce.embedding_space
        "#,
    )
    .bind(slug)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;

    let mut map = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        let space: String = row.try_get("embedding_space").map_err(ApiError::sql)?;
        let chunk_count: i64 = row.try_get("chunk_count").map_err(ApiError::sql)?;
        let memory_count: i64 = row.try_get("memory_count").map_err(ApiError::sql)?;
        insert_embedding_coverage_count(&mut map, space.clone(), chunk_count, memory_count);
        if let Some(alias) = equivalent_openai_embedding_space_key(&space) {
            insert_embedding_coverage_count(&mut map, alias, chunk_count, memory_count);
        }
    }
    Ok(map)
}

pub(crate) fn insert_embedding_coverage_count(
    map: &mut std::collections::HashMap<String, (i64, i64)>,
    space: String,
    chunk_count: i64,
    memory_count: i64,
) {
    map.entry(space)
        .and_modify(|(chunks, memories)| {
            *chunks = (*chunks).max(chunk_count);
            *memories = (*memories).max(memory_count);
        })
        .or_insert((chunk_count, memory_count));
}

pub(crate) fn equivalent_openai_embedding_space_key(space: &str) -> Option<String> {
    space
        .strip_prefix("openai|")
        .map(|suffix| format!("openai_compatible|{suffix}"))
        .or_else(|| {
            space
                .strip_prefix("openai_compatible|")
                .map(|suffix| format!("openai|{suffix}"))
        })
}

pub(crate) async fn activate_embedding_backend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ActivateEmbeddingBackendRequest>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/embeddings/activate", &request, true).await?,
        ));
    }

    let previous_active = {
        let mut embedders = state.embedders.write().await;
        let previous = embedders.active_name().map(|s| s.to_string());
        embedders
            .set_active(&request.name)
            .map_err(|err| ApiError::validation(ValidationError::new(err.to_string())))?;
        previous
    };

    if let Err(err) = persist_active_embedding_backend(&state, Some(&request.name)).await {
        // Revert in-memory state so config and registry stay in sync.
        let mut embedders = state.embedders.write().await;
        if let Some(name) = previous_active {
            let _ = embedders.set_active(&name);
        }
        return Err(err);
    }

    build_embedding_backends_response(&state, None).await
}

pub(crate) async fn deactivate_embedding_backend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(_request): Json<mem_api::DeactivateEmbeddingBackendRequest>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                "/v1/embeddings/deactivate",
                &mem_api::DeactivateEmbeddingBackendRequest::default(),
                true,
            )
            .await?,
        ));
    }

    let previous_active = {
        let mut embedders = state.embedders.write().await;
        let previous = embedders.active_name().map(|s| s.to_string());
        embedders.clear_active();
        previous
    };

    if let Err(err) = persist_active_embedding_backend(&state, None).await {
        if let Some(name) = previous_active {
            let mut embedders = state.embedders.write().await;
            let _ = embedders.set_active(&name);
        }
        return Err(err);
    }

    build_embedding_backends_response(&state, None).await
}

pub(crate) async fn set_embedding_creation_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetEmbeddingCreationRequest>,
) -> Result<Json<EmbeddingBackendsResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/embeddings/create-enabled", &request, true).await?,
        ));
    }

    let name = request.name.trim();
    if name.is_empty() {
        return Err(ApiError::validation(ValidationError::new(
            "name must be non-empty",
        )));
    }
    if !state
        .config
        .embeddings
        .backends
        .iter()
        .any(|backend| backend.name == name)
    {
        return Err(ApiError::validation(ValidationError::new(format!(
            "unknown embedding backend: {name}"
        ))));
    }

    let previous = {
        let mut embedders = state.embedders.write().await;
        let previous = embedders.create_enabled(name);
        if embedders.get(name).is_some() {
            embedders
                .set_create_enabled(name, request.enabled)
                .map_err(|err| ApiError::validation(ValidationError::new(err.to_string())))?;
        }
        previous
    };
    let previous_global = state
        .automated_embedding_creation_enabled
        .swap(true, Ordering::Relaxed);
    if let Err(err) = persist_embedding_creation_enabled(&state, name, request.enabled).await {
        let mut embedders = state.embedders.write().await;
        if embedders.get(name).is_some() {
            let _ = embedders.set_create_enabled(name, previous);
        }
        state
            .automated_embedding_creation_enabled
            .store(previous_global, Ordering::Relaxed);
        return Err(err);
    }

    build_embedding_backends_response(&state, None).await
}

pub(crate) async fn persist_active_embedding_backend(
    state: &AppState,
    active_name: Option<&str>,
) -> Result<(), ApiError> {
    let Some(config_path) = state.config.resolved_config_path.clone() else {
        // Ephemeral (env-var only) config — no file to rewrite. The
        // in-memory activation is still applied, but it will not survive
        // a restart. Surface this to the caller as a soft warning via
        // tracing rather than an error.
        tracing::warn!(
            "changed active embedding backend without persistence: no TOML config file is resolved"
        );
        return Ok(());
    };
    let existing = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("read {}: {err}", config_path.display())))?;
    let rendered = set_active_embedding_backend_in_toml(&existing, active_name)
        .map_err(|err| ApiError::io(anyhow::anyhow!("update {}: {err}", config_path.display())))?;
    let tmp_path = config_path.with_extension("toml.tmp");
    tokio::fs::write(&tmp_path, rendered.as_bytes())
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("write {}: {err}", tmp_path.display())))?;
    tokio::fs::rename(&tmp_path, &config_path)
        .await
        .map_err(|err| {
            ApiError::io(anyhow::anyhow!(
                "rename {} -> {}: {err}",
                tmp_path.display(),
                config_path.display()
            ))
        })?;
    Ok(())
}

pub(crate) async fn persist_embedding_creation_enabled(
    state: &AppState,
    name: &str,
    enabled: bool,
) -> Result<(), ApiError> {
    let Some(config_path) = state.config.resolved_config_path.clone() else {
        tracing::warn!(
            "changed automatic embedding creation without persistence: no TOML config file is resolved"
        );
        return Ok(());
    };
    let existing = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("read {}: {err}", config_path.display())))?;
    let rendered = set_embedding_creation_enabled_in_toml(&existing, name, enabled)
        .map_err(|err| ApiError::io(anyhow::anyhow!("update {}: {err}", config_path.display())))?;
    let tmp_path = config_path.with_extension("toml.tmp");
    tokio::fs::write(&tmp_path, rendered.as_bytes())
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("write {}: {err}", tmp_path.display())))?;
    tokio::fs::rename(&tmp_path, &config_path)
        .await
        .map_err(|err| {
            ApiError::io(anyhow::anyhow!(
                "rename {} -> {}: {err}",
                tmp_path.display(),
                config_path.display()
            ))
        })?;
    Ok(())
}

pub(crate) async fn llm_audit_status(
    State(state): State<AppState>,
) -> Result<Json<LlmAuditStatusResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/config/llm-audit").await?));
    }
    Ok(Json(build_llm_audit_status_response(&state)))
}

pub(crate) async fn set_llm_audit_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetLlmAuditRequest>,
) -> Result<Json<LlmAuditStatusResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/config/llm-audit", &request, true).await?,
        ));
    }

    let previous = state
        .llm_audit
        .read()
        .expect("llm audit config lock poisoned")
        .clone();
    let mut next = previous.clone();
    next.enabled = request.enabled;
    persist_llm_audit_config(&state, &next).await?;
    {
        let mut guard = state
            .llm_audit
            .write()
            .expect("llm audit config lock poisoned");
        *guard = next;
    }

    Ok(Json(build_llm_audit_status_response(&state)))
}

pub(crate) fn build_llm_audit_status_response(state: &AppState) -> LlmAuditStatusResponse {
    let audit = state
        .llm_audit
        .read()
        .expect("llm audit config lock poisoned")
        .clone();
    LlmAuditStatusResponse {
        enabled: audit.enabled,
        redacted: audit.redact,
        max_message_chars: audit.max_message_chars,
        max_total_chars: audit.max_total_chars,
        profile: state.config.profile.to_string(),
        config_path: llm_audit_config_path(&state.config).map(|path| path.display().to_string()),
    }
}

pub(crate) async fn persist_llm_audit_config(
    state: &AppState,
    audit: &LlmAuditConfig,
) -> Result<(), ApiError> {
    let Some(config_path) = llm_audit_config_path(&state.config) else {
        return Err(ApiError::status_message(
            StatusCode::INTERNAL_SERVER_ERROR,
            "cannot persist LLM audit setting: no TOML config file is resolved",
        ));
    };
    let existing = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("read {}: {err}", config_path.display())))?;
    let rendered = set_llm_audit_enabled_in_toml(&existing, audit.enabled)
        .map_err(|err| ApiError::io(anyhow::anyhow!("update {}: {err}", config_path.display())))?;
    let tmp_path = config_path.with_extension("toml.tmp");
    tokio::fs::write(&tmp_path, rendered.as_bytes())
        .await
        .map_err(|err| ApiError::io(anyhow::anyhow!("write {}: {err}", tmp_path.display())))?;
    tokio::fs::rename(&tmp_path, &config_path)
        .await
        .map_err(|err| {
            ApiError::io(anyhow::anyhow!(
                "rename {} -> {}: {err}",
                tmp_path.display(),
                config_path.display()
            ))
        })?;
    Ok(())
}

pub(crate) fn llm_audit_config_path(config: &AppConfig) -> Option<PathBuf> {
    config
        .resolved_dev_overlay_path
        .clone()
        .or_else(|| config.resolved_config_path.clone())
}

pub(crate) fn set_llm_audit_enabled_in_toml(
    existing: &str,
    enabled: bool,
) -> anyhow::Result<String> {
    let mut doc = existing.parse::<toml_edit::DocumentMut>()?;
    if !doc.contains_key("llm_audit") {
        doc["llm_audit"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let llm_audit = doc["llm_audit"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[llm_audit] is not a table in config"))?;
    llm_audit["enabled"] = toml_edit::value(enabled);
    if !llm_audit.contains_key("redact") {
        llm_audit["redact"] = toml_edit::value(true);
    }
    if !llm_audit.contains_key("max_message_chars") {
        llm_audit["max_message_chars"] = toml_edit::value(8_000);
    }
    if !llm_audit.contains_key("max_total_chars") {
        llm_audit["max_total_chars"] = toml_edit::value(32_000);
    }
    Ok(doc.to_string())
}

pub(crate) fn set_active_embedding_backend_in_toml(
    existing: &str,
    active_name: Option<&str>,
) -> anyhow::Result<String> {
    let mut doc = existing.parse::<toml_edit::DocumentMut>()?;
    // Ensure [embeddings] table exists.
    if !doc.contains_key("embeddings") {
        doc["embeddings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let embeddings = doc["embeddings"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[embeddings] is not a table in config"))?;
    match active_name {
        Some(name) => {
            embeddings["enabled"] = toml_edit::value(true);
            embeddings["active"] = toml_edit::value(name);
        }
        None => {
            embeddings["enabled"] = toml_edit::value(false);
        }
    }
    Ok(doc.to_string())
}

pub(crate) fn set_embedding_creation_enabled_in_toml(
    existing: &str,
    name: &str,
    enabled: bool,
) -> anyhow::Result<String> {
    let mut doc = existing.parse::<toml_edit::DocumentMut>()?;
    if !doc.contains_key("embeddings") {
        doc["embeddings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let embeddings = doc["embeddings"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[embeddings] is not a table in config"))?;
    embeddings["create_enabled"] = toml_edit::value(true);
    if let Some(backends) = embeddings
        .get_mut("backends")
        .and_then(|item| item.as_array_of_tables_mut())
    {
        let mut updated = false;
        for backend in backends.iter_mut() {
            if backend
                .get("name")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value == name)
            {
                backend["create_enabled"] = toml_edit::value(enabled);
                updated = true;
                break;
            }
        }
        if updated {
            return Ok(doc.to_string());
        }
    }
    embeddings["create_enabled"] = toml_edit::value(enabled);
    Ok(doc.to_string())
}
