use crate::prelude::*;
use crate::*;

pub(crate) async fn curate_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CurateRequest>,
) -> Result<Json<mem_api::CurateResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/curate", &request, true).await?,
        ));
    }
    let response = if request.dry_run {
        preview_curate(&state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    } else {
        curate(&state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    };
    if request.dry_run {
        return Ok(Json(response));
    }
    // The post-curate pipeline (embedding rebuild, semantic dedup, validation
    // and consolidation due checks) can outlive the client's timeout on large
    // projects. Run it in a detached task so a client disconnect — axum drops
    // the handler future — cannot cancel it mid-flight; awaiting the handle
    // keeps the response complete for clients that do wait.
    let task = tokio::spawn(finish_curate(state, request, response));
    match task.await {
        Ok(result) => result.map(Json),
        Err(error) => Err(ApiError::io(anyhow::anyhow!(
            "post-curate pipeline task failed: {error}"
        ))),
    }
}

/// Everything that happens after the curate transaction commits: advisory
/// passes that enrich the response but must survive client disconnects.
async fn finish_curate(
    state: AppState,
    request: CurateRequest,
    mut response: mem_api::CurateResponse,
) -> Result<mem_api::CurateResponse, ApiError> {
    let project = request.project.clone();
    let embedders = state.embedders.read().await;
    if !embedders.is_empty() {
        let rebuild_result = if request.raw_capture_id.is_some() {
            rebuild_memory_chunks_for_automatic_creation(
                &state.pool()?,
                &request.project,
                &response.memory_ids,
                &embedders,
                state
                    .automated_embedding_creation_enabled
                    .load(Ordering::Relaxed),
            )
            .await
        } else {
            rebuild_chunks_for_automatic_creation(
                &state.pool()?,
                &request.project,
                &embedders,
                state
                    .automated_embedding_creation_enabled
                    .load(Ordering::Relaxed),
            )
            .await
        };
        match rebuild_result {
            Err(error) => {
                let diagnostic = classify_anyhow_diagnostic(
                    &error,
                    "embeddings",
                    "automatic_embedding_creation",
                    DiagnosticSeverity::Warning,
                );
                notify_project_diagnostic(&state, request.project.clone(), diagnostic.clone());
                response.warnings.push(diagnostic);
            }
            Ok(_) => {
                // Embeddings for the new memories exist now, so the semantic
                // dedup pass can catch paraphrased duplicates the lexical
                // curation pass missed. Advisory — never fails curation.
                let curation_config = &state.config.curation;
                if curation_config.semantic_dedup_enabled && !response.memory_ids.is_empty() {
                    match mem_curate::refresh_semantic_relations(
                        &state.pool()?,
                        &request.project,
                        &response.memory_ids,
                        curation_config.semantic_duplicate_threshold,
                    )
                    .await
                    {
                        Ok(duplicates) if !duplicates.is_empty() => {
                            match crate::repository::handlers::loops::queue_semantic_dedup_proposals(
                                &state.pool()?,
                                &request.project,
                                &duplicates,
                            )
                            .await
                            {
                                Ok(queued) if queued > 0 => {
                                    response.proposal_count += queued as i64;
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    tracing::warn!(error = %error, "semantic dedup proposal queueing failed");
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            tracing::warn!(error = %error, "semantic dedup pass failed");
                        }
                    }
                }
            }
        }
    }
    // Curator-side threshold check: report memories due for validation and
    // nudge the background scheduler so they validate promptly. Advisory —
    // never fails curation.
    match crate::repository::handlers::reinforcement::due_validation_infos(
        &state,
        response.project_id,
    )
    .await
    {
        Ok(due) => {
            if !due.is_empty()
                && let Some(runtime) = &state.reinforcement
            {
                runtime.notify.notify_one();
            }
            response.validation_due = due;
        }
        Err(error) => {
            tracing::warn!(error = %error, "reinforcement due-validation check failed");
        }
    }
    // Consolidation accumulator: when enabled, report clusters worth
    // consolidating and, if auto-trigger is on, wake the LLM synthesis in the
    // background. Deterministic scan only; advisory and never fails curation.
    if state.config.consolidation.enabled {
        let cfg = &state.config.consolidation;
        let half_life_secs = state.config.reinforcement.half_life.as_secs_f64().max(1.0);
        match crate::repository::handlers::consolidation::run_memory_consolidation(
            &state.pool()?,
            &request.project,
            cfg,
            half_life_secs,
        )
        .await
        {
            Ok(report) => {
                let due = report.due_infos();
                if !due.is_empty() && cfg.auto_trigger && !cfg.dry_run {
                    // Opt-in utility floor: suppress the auto-fire (never the
                    // manual run, never a mode/gate) when the loop's learned
                    // utility has collapsed. Advisory learning acting only on
                    // automation cadence.
                    let suppressed = auto_trigger_suppressed_by_utility(
                        &state,
                        response.project_id,
                        mem_loops::LOOP_MEMORY_CONSOLIDATION,
                    )
                    .await;
                    if suppressed {
                        tracing::info!(
                            loop_id = mem_loops::LOOP_MEMORY_CONSOLIDATION,
                            "auto-trigger suppressed: learned utility below floor; review or snooze the loop"
                        );
                    } else {
                        let state = state.clone();
                        let project = request.project.clone();
                        tokio::spawn(async move {
                            if let Err(error) = crate::consolidate::emit_consolidation_proposals(
                                &state, &project, None,
                            )
                            .await
                            {
                                tracing::warn!(error = %error, "auto-triggered consolidation failed");
                            }
                        });
                    }
                }
                response.consolidation_due = due;
            }
            Err(error) => {
                tracing::warn!(error = %error.message, "consolidation due check failed");
            }
        }
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::Curate,
        format!(
            "Curated {} capture(s) into {} memory entry/entries with {} replacement(s) and {} queued update proposal(s).",
            response.input_count,
            response.output_count,
            response.replaced_count,
            response.proposal_count
        ),
        Some(ActivityDetails::Curate {
            run_id: response.run_id,
            input_count: response.input_count,
            output_count: response.output_count,
            replaced_count: response.replaced_count,
            proposal_count: response.proposal_count,
        }),
    );
    for replacement in &response.replacements {
        notify_project_changed(
            &state,
            request.project.clone(),
            Some(replacement.new_memory_id),
            ActivityKind::MemoryReplacement,
            format!(
                "Replaced memory \"{}\" with \"{}\".",
                replacement.old_summary, replacement.new_summary
            ),
            Some(ActivityDetails::MemoryReplacement {
                old_memory_id: replacement.old_memory_id,
                old_summary: replacement.old_summary.clone(),
                new_memory_id: replacement.new_memory_id,
                new_summary: replacement.new_summary.clone(),
                automatic: replacement.automatic,
                policy: replacement.policy,
            }),
        );
    }
    Ok(response)
}

pub(crate) async fn project_replacement_proposals(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ReplacementProposalListResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-proposals"),
            )
            .await?,
        ));
    }
    Ok(Json(
        list_replacement_proposals(&state.pool()?, &slug)
            .await
            .map_err(ApiError::sql)?,
    ))
}

pub(crate) async fn project_replacement_proposal_approve(
    State(state): State<AppState>,
    Path((slug, proposal_id)): Path<(String, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<ReplacementProposalResolutionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-proposals/{proposal_id}/approve"),
                &serde_json::json!({}),
                true,
            )
            .await?,
        ));
    }
    let response = approve_replacement_proposal(&state.pool()?, &slug, proposal_id)
        .await
        .map_err(ApiError::sql)?;
    if let Some(new_memory_id) = response.new_memory_id {
        notify_project_changed(
            &state,
            slug.clone(),
            Some(new_memory_id),
            ActivityKind::MemoryReplacement,
            format!(
                "Replaced memory \"{}\" with \"{}\" after review.",
                response.target_summary, response.candidate_summary
            ),
            Some(ActivityDetails::MemoryReplacement {
                old_memory_id: response.target_memory_id,
                old_summary: response.target_summary.clone(),
                new_memory_id,
                new_summary: response.candidate_summary.clone(),
                automatic: false,
                policy: response.policy,
            }),
        );
    }
    notify_project_refreshed(&state, slug.clone());
    Ok(Json(response))
}

pub(crate) async fn project_replacement_proposal_reject(
    State(state): State<AppState>,
    Path((slug, proposal_id)): Path<(String, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<ReplacementProposalResolutionResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-proposals/{proposal_id}/reject"),
                &serde_json::json!({}),
                true,
            )
            .await?,
        ));
    }
    let response = reject_replacement_proposal(&state.pool()?, &slug, proposal_id)
        .await
        .map_err(ApiError::sql)?;
    notify_project_refreshed(&state, slug.clone());
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReplacementPolicyQuery {
    repo_root: Option<String>,
}

pub(crate) async fn project_replacement_policy(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ReplacementPolicyQuery>,
) -> Result<Json<ReplacementPolicyResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/projects/{slug}/replacement-policy")).await?,
        ));
    }
    let repo_root = resolve_project_repo_root(&state, &slug, params.repo_root.as_deref());
    let replacement_policy = repo_root
        .as_deref()
        .and_then(|root| load_repo_replacement_policy(FsPath::new(root)).ok())
        .unwrap_or_default();
    Ok(Json(ReplacementPolicyResponse {
        project: slug,
        writable: repo_root.is_some(),
        repo_root,
        replacement_policy,
    }))
}

pub(crate) async fn project_replacement_policy_update(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ReplacementPolicyRequest>,
) -> Result<Json<ReplacementPolicyResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/replacement-policy"),
                &request,
                true,
            )
            .await?,
        ));
    }
    let repo_root = request
        .repo_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::validation(ValidationError::new("repo_root must be non-empty")))?;
    write_replacement_policy(FsPath::new(repo_root), request.replacement_policy)
        .map_err(ApiError::io)?;
    notify_project_refreshed(&state, slug.clone());
    Ok(Json(ReplacementPolicyResponse {
        project: slug,
        repo_root: Some(repo_root.to_string()),
        replacement_policy: request.replacement_policy,
        writable: true,
    }))
}

/// True when the opt-in procedural utility floor says this loop's auto-fire
/// should be skipped: floor enabled, enough decisions observed, and learned
/// utility below the floor. Errors and missing rows never suppress.
pub(crate) async fn auto_trigger_suppressed_by_utility(
    state: &AppState,
    project_id: Uuid,
    loop_id: &str,
) -> bool {
    let procedural = &state.config.procedural;
    if !procedural.enabled || !procedural.utility_floor_enabled {
        return false;
    }
    let Ok(pool) = state.pool() else {
        return false;
    };
    match mem_reinforce::repository::list_procedural_utility(&pool, project_id, "loop").await {
        Ok(snapshots) => snapshots.iter().any(|snapshot| {
            snapshot.producer_id == loop_id
                && snapshot.update_count >= procedural.min_samples
                && snapshot.utility < procedural.utility_floor
        }),
        Err(error) => {
            tracing::warn!(error = %error, "utility floor lookup failed; not suppressing");
            false
        }
    }
}

pub(crate) fn resolve_project_repo_root(
    state: &AppState,
    project: &str,
    requested: Option<&str>,
) -> Option<String> {
    if let Some(repo_root) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(repo_root.to_string());
    }
    if let Some(repo_root) = state
        .config
        .automation
        .repo_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(repo_root.to_string());
    }

    let mut repo_roots = state
        .watchers
        .lock()
        .expect("watcher registry mutex poisoned")
        .values()
        .filter(|watcher| watcher.project == project)
        .map(|watcher| watcher.repo_root.clone())
        .collect::<Vec<_>>();
    repo_roots.sort();
    repo_roots.dedup();
    if repo_roots.len() == 1 {
        repo_roots.pop()
    } else {
        None
    }
}

pub(crate) fn write_replacement_policy(
    repo_root: &FsPath,
    policy: ReplacementPolicy,
) -> Result<()> {
    let path = repo_agent_settings_path(repo_root);
    let mut document = if path.exists() {
        std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        toml_edit::DocumentMut::new()
    };
    document["curation"]["replacement_policy"] = toml_edit::value(policy.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&path, document.to_string())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
