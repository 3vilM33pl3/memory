use crate::prelude::*;
use crate::*;

pub(crate) async fn sync_commits(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CommitSyncRequest>,
) -> Result<Json<CommitSyncResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    request.validate().map_err(ApiError::validation)?;
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(&state, "/v1/commits/sync", &request, true).await?,
        ));
    }
    let project = request.project.clone();
    let response = if request.dry_run {
        preview_project_commit_sync(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    } else {
        sync_project_commits(state.pool()?, &request)
            .await
            .map_err(ApiError::sql)?
    };
    if request.dry_run {
        return Ok(Json(response));
    }
    notify_project_changed(
        &state,
        project,
        None,
        ActivityKind::CommitSync,
        format!(
            "Synced {} commit(s): {} imported, {} updated.",
            response.total_received, response.imported_count, response.updated_count
        ),
        Some(ActivityDetails::CommitSync {
            imported_count: response.imported_count,
            updated_count: response.updated_count,
            total_received: response.total_received,
            newest_commit: response.newest_commit.clone(),
            oldest_commit: response.oldest_commit.clone(),
        }),
    );
    Ok(Json(response))
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProjectMemoriesParams {
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProjectCommitsParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

pub(crate) async fn project_memories(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ProjectMemoriesParams>,
) -> Result<Json<ProjectMemoriesResponse>, ApiError> {
    if !state.is_primary() {
        let suffix = format!(
            "?limit={}&offset={}",
            params.limit.unwrap_or(200).clamp(1, 500),
            params.offset.unwrap_or(0).max(0)
        );
        let mut path = format!("/v1/projects/{slug}/memories{suffix}");
        if let Some(status) = &params.status {
            path.push_str("&status=");
            path.push_str(status);
        }
        return Ok(Json(proxy_get_json(&state, &path).await?));
    }
    let limit = params.limit.unwrap_or(200).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    let status_filter = params
        .status
        .as_deref()
        .map(parse_status_filter)
        .transpose()
        .map_err(ApiError::validation)?;

    Ok(Json(
        fetch_project_memories(
            state.pool()?,
            &slug,
            status_filter.as_deref(),
            limit,
            offset,
        )
        .await
        .map_err(ApiError::sql)?,
    ))
}

pub(crate) async fn project_overview(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ProjectOverviewResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/projects/{slug}/overview")).await?,
        ));
    }
    Ok(Json(
        fetch_project_overview_with_watchers(&state, &slug)
            .await
            .map_err(ApiError::sql)?,
    ))
}

pub(crate) async fn project_commits(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ProjectCommitsParams>,
) -> Result<Json<ProjectCommitsResponse>, ApiError> {
    if !state.is_primary() {
        let path = format!(
            "/v1/projects/{slug}/commits?limit={}&offset={}",
            params.limit.unwrap_or(50).clamp(1, 500),
            params.offset.unwrap_or(0).max(0)
        );
        return Ok(Json(proxy_get_json(&state, &path).await?));
    }
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    Ok(Json(
        fetch_project_commits(state.pool()?, &slug, limit, offset)
            .await
            .map_err(ApiError::sql)?,
    ))
}

pub(crate) async fn project_commit_detail(
    State(state): State<AppState>,
    Path((slug, hash)): Path<(String, String)>,
) -> Result<Json<CommitDetailResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/projects/{slug}/commits/{hash}")).await?,
        ));
    }
    let commit = fetch_project_commit(state.pool()?, &slug, &hash)
        .await
        .map_err(ApiError::sql)?
        .ok_or_else(|| ApiError::not_found("project commit not found"))?;
    Ok(Json(CommitDetailResponse {
        project: slug,
        commit,
    }))
}

pub(crate) async fn project_resume(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(mut request): Json<ResumeRequest>,
) -> Result<Json<ResumeResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if request.project != slug {
        return Err(ApiError::validation(ValidationError::new(
            "request project must match path slug",
        )));
    }
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/resume"),
                &request,
                false,
            )
            .await?,
        ));
    }

    if request.checkpoint.is_none() {
        request.checkpoint = request.repo_root.as_deref().and_then(|root| {
            load_resume_checkpoint(&slug, FsPath::new(root))
                .ok()
                .flatten()
        });
    }

    let pool = state.pool()?;
    let since = request
        .checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.marked_at)
        .or(request.since);
    let overview_fut = fetch_project_overview_with_watchers(&state, &slug);
    let timeline_fut = fetch_project_timeline(pool, &slug, since, request.limit);
    let commits_fut = fetch_project_commits_since(pool, &slug, since, request.limit);
    let changed_memories_fut = fetch_recent_project_memories(pool, &slug, since, request.limit);
    let durable_context_fut = fetch_durable_resume_context(pool, &slug, request.limit.min(8));
    let active_plan_fut = fetch_latest_active_plan_memory(pool, &slug);
    let (overview, timeline, commits, changed_memories, durable_context, active_plan) =
        tokio::try_join!(
            overview_fut,
            timeline_fut,
            commits_fut,
            changed_memories_fut,
            durable_context_fut,
            active_plan_fut,
        )
        .map_err(ApiError::sql)?;
    let warnings = resume_warnings(&overview);
    let actions = resume_actions(
        &slug,
        request.checkpoint.as_ref(),
        &overview,
        &timeline,
        &changed_memories,
    );
    let current_thread = infer_current_thread(
        request.checkpoint.as_ref(),
        &overview,
        &timeline,
        &commits,
        &changed_memories,
        active_plan.as_ref(),
    );
    let change_summary = build_change_summary(&timeline, &commits, &changed_memories);
    let attention_items = build_attention_items(&overview, &timeline);
    let context_items =
        select_resume_context(&changed_memories, &durable_context, active_plan.as_ref());
    let primary_next_step = actions.first().cloned();
    let secondary_next_steps = actions.iter().skip(1).take(2).cloned().collect::<Vec<_>>();
    let deterministic = build_resume_briefing(
        &slug,
        request.checkpoint.as_ref(),
        current_thread.as_deref(),
        &change_summary,
        &attention_items,
        primary_next_step.as_ref(),
        &secondary_next_steps,
        &context_items,
    );
    let briefing = if request.include_llm_summary {
        summarize_resume_with_llm(&state, &slug, "resume_summary", &deterministic)
            .await
            .unwrap_or(deterministic)
    } else {
        deterministic
    };

    Ok(Json(ResumeResponse {
        project: slug,
        generated_at: chrono::Utc::now(),
        checkpoint: request.checkpoint,
        briefing,
        current_thread,
        change_summary,
        attention_items,
        primary_next_step,
        secondary_next_steps,
        context_items,
        timeline,
        commits,
        changed_memories,
        durable_context,
        warnings,
        actions,
        overview,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ActivityListQuery {
    limit: Option<usize>,
    kind: Option<String>,
    since: Option<chrono::DateTime<chrono::Utc>>,
    before: Option<chrono::DateTime<chrono::Utc>>,
    include_details: Option<bool>,
}

pub(crate) async fn project_activities(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<ActivityListQuery>,
) -> Result<Json<ActivityListResponse>, ApiError> {
    if !state.is_primary() {
        let mut path = format!("/v1/projects/{slug}/activities");
        let mut params = Vec::new();
        if let Some(limit) = query.limit {
            params.push(format!("limit={limit}"));
        }
        if let Some(kind) = &query.kind {
            params.push(format!("kind={kind}"));
        }
        if let Some(since) = query.since {
            params.push(format!("since={}", since.to_rfc3339()));
        }
        if let Some(before) = query.before {
            params.push(format!("before={}", before.to_rfc3339()));
        }
        if let Some(include_details) = query.include_details {
            params.push(format!("include_details={include_details}"));
        }
        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }
        return Ok(Json(proxy_get_json(&state, &path).await?));
    }
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut items = fetch_project_activities(
        state.pool()?,
        &slug,
        query.since,
        query.before,
        query.kind.as_deref(),
        limit,
        query.include_details.unwrap_or(true),
    )
    .await
    .map_err(ApiError::sql)?;
    if !query.include_details.unwrap_or(true) {
        for item in &mut items {
            item.details = None;
        }
    }
    Ok(Json(ActivityListResponse {
        project: slug,
        total_returned: items.len(),
        items,
    }))
}

pub(crate) async fn project_up_to_speed(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<UpToSpeedRequest>,
) -> Result<Json<UpToSpeedResponse>, ApiError> {
    request.validate().map_err(ApiError::validation)?;
    if request.project != slug {
        return Err(ApiError::validation(ValidationError::new(
            "request project must match path slug",
        )));
    }
    if !state.is_primary() {
        return Ok(Json(
            proxy_post_json(
                &state,
                &format!("/v1/projects/{slug}/up-to-speed"),
                &request,
                false,
            )
            .await?,
        ));
    }
    let response = build_up_to_speed_response(&state, &slug, &request).await?;
    notify_project_changed(
        &state,
        slug,
        None,
        ActivityKind::Briefing,
        "Generated get-up-to-speed briefing.".to_string(),
        None,
    );
    Ok(Json(response))
}

pub(crate) async fn build_up_to_speed_response(
    state: &AppState,
    slug: &str,
    request: &UpToSpeedRequest,
) -> Result<UpToSpeedResponse, ApiError> {
    let pool = state.pool()?;
    let limit = request.limit.clamp(1, 50);
    let overview_fut = fetch_project_overview_with_watchers(state, slug);
    let activities_fut = fetch_project_activities(pool, slug, None, None, None, limit, true);
    let commits_fut = fetch_project_commits_since(pool, slug, None, 8);
    let durable_context_fut = fetch_durable_resume_context(pool, slug, 8);
    let active_plan_fut = fetch_latest_active_plan_memory(pool, slug);
    let (overview, all_activities, commits, durable_context, active_plan) = tokio::try_join!(
        overview_fut,
        activities_fut,
        commits_fut,
        durable_context_fut,
        active_plan_fut,
    )
    .map_err(ApiError::sql)?;
    let recent_activities = all_activities
        .into_iter()
        .filter(|event| !matches!(event.kind, ActivityKind::Briefing))
        .collect::<Vec<_>>();
    let changed_memories = fetch_recent_project_memories(pool, slug, None, 8)
        .await
        .map_err(ApiError::sql)?;
    let warnings = resume_warnings(&overview);
    let next_actions = resume_actions(slug, None, &overview, &recent_activities, &changed_memories);
    let recent_work = build_change_summary(&recent_activities, &commits, &changed_memories);
    let blockers = build_attention_items(&overview, &recent_activities);
    let useful_memories =
        select_resume_context(&changed_memories, &durable_context, active_plan.as_ref());
    let current_focus = infer_current_thread(
        None,
        &overview,
        &recent_activities,
        &commits,
        &changed_memories,
        active_plan.as_ref(),
    )
    .into_iter()
    .collect::<Vec<_>>();
    let token_usage = summarize_activity_tokens(&recent_activities);
    let deterministic = build_up_to_speed_briefing(
        slug,
        &current_focus,
        &recent_work,
        &blockers,
        &next_actions,
        &useful_memories,
        &token_usage,
    );
    let briefing = if request.include_llm_summary {
        summarize_resume_with_llm(state, slug, "up_to_speed_summary", &deterministic)
            .await
            .unwrap_or(deterministic)
    } else {
        deterministic
    };
    Ok(UpToSpeedResponse {
        project: slug.to_string(),
        generated_at: chrono::Utc::now(),
        briefing,
        current_focus,
        recent_work,
        blockers,
        next_actions,
        useful_memories,
        recent_activities,
        token_usage,
        warnings,
    })
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct StoredResumeCheckpoints {
    #[serde(default)]
    checkpoints: BTreeMap<String, ResumeCheckpoint>,
}

pub(crate) fn load_resume_checkpoint(
    project: &str,
    repo_root: &FsPath,
) -> Result<Option<ResumeCheckpoint>> {
    let state_dir = mem_platform::preferred_user_state_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine user state directory"))?;
    let path = state_dir.join("resume-checkpoints.json");
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let store: StoredResumeCheckpoints =
        serde_json::from_str(&contents).context("parse checkpoint store")?;
    Ok(store
        .checkpoints
        .get(&format!("{}::{}", project, repo_root.display()))
        .cloned())
}

pub(crate) async fn fetch_project_timeline(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: usize,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT te.id, te.recorded_at, p.slug AS project, te.kind, te.memory_id, te.summary, te.details_json,
               te.actor_id, te.actor_name, te.source, te.operation_id, te.duration_ms, te.provider, te.model,
               te.input_tokens, te.output_tokens, te.cache_read_tokens, te.cache_write_tokens, te.total_tokens
        FROM project_timeline_events te
        JOIN projects p ON p.id = te.project_id
        WHERE p.slug = $1
          AND ($2::timestamptz IS NULL OR te.recorded_at >= $2)
        ORDER BY te.recorded_at DESC
        LIMIT $3
        "#,
    )
    .bind(slug)
    .bind(since)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: String = row.try_get("kind")?;
        let details = row
            .try_get::<Option<sqlx::types::Json<ActivityDetails>>, _>("details_json")?
            .map(|payload| payload.0);
        items.push(ActivityEvent {
            id: row.try_get("id")?,
            recorded_at: row.try_get("recorded_at")?,
            project: row.try_get("project")?,
            kind: parse_activity_kind(&kind),
            memory_id: row.try_get("memory_id")?,
            summary: row.try_get("summary")?,
            details,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            source: row.try_get("source")?,
            operation_id: row.try_get("operation_id")?,
            duration_ms: row
                .try_get::<Option<i64>, _>("duration_ms")?
                .map(|value| value as u64),
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            token_usage: token_usage_from_row(&row)?,
        });
    }
    Ok(items)
}

pub(crate) async fn fetch_project_activities(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    before: Option<chrono::DateTime<chrono::Utc>>,
    kind: Option<&str>,
    limit: usize,
    include_details: bool,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT te.id, te.recorded_at, p.slug AS project, te.kind, te.memory_id, te.summary,
               CASE WHEN $6 THEN te.details_json ELSE NULL END AS details_json,
               te.actor_id, te.actor_name, te.source, te.operation_id, te.duration_ms, te.provider, te.model,
               te.input_tokens, te.output_tokens, te.cache_read_tokens, te.cache_write_tokens, te.total_tokens
        FROM project_timeline_events te
        JOIN projects p ON p.id = te.project_id
        WHERE p.slug = $1
          AND ($2::timestamptz IS NULL OR te.recorded_at >= $2)
          AND ($3::timestamptz IS NULL OR te.recorded_at < $3)
          AND ($4::text IS NULL OR te.kind = $4)
        ORDER BY te.recorded_at DESC
        LIMIT $5
        "#,
    )
    .bind(slug)
    .bind(since)
    .bind(before)
    .bind(kind)
    .bind(limit as i64)
    .bind(include_details)
    .fetch_all(pool)
    .await?;
    activity_events_from_rows(rows)
}

pub(crate) fn activity_events_from_rows(
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: String = row.try_get("kind")?;
        let details = row
            .try_get::<Option<sqlx::types::Json<ActivityDetails>>, _>("details_json")?
            .map(|payload| payload.0);
        items.push(ActivityEvent {
            id: row.try_get("id")?,
            recorded_at: row.try_get("recorded_at")?,
            project: row.try_get("project")?,
            kind: parse_activity_kind(&kind),
            memory_id: row.try_get("memory_id")?,
            summary: row.try_get("summary")?,
            details,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            source: row.try_get("source")?,
            operation_id: row.try_get("operation_id")?,
            duration_ms: row
                .try_get::<Option<i64>, _>("duration_ms")?
                .map(|value| value as u64),
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            token_usage: token_usage_from_row(&row)?,
        });
    }
    Ok(items)
}

pub(crate) fn token_usage_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<TokenUsage>, sqlx::Error> {
    let input_tokens = row
        .try_get::<Option<i64>, _>("input_tokens")?
        .unwrap_or_default() as u64;
    let output_tokens = row
        .try_get::<Option<i64>, _>("output_tokens")?
        .unwrap_or_default() as u64;
    let cache_read_tokens = row
        .try_get::<Option<i64>, _>("cache_read_tokens")?
        .unwrap_or_default() as u64;
    let cache_write_tokens = row
        .try_get::<Option<i64>, _>("cache_write_tokens")?
        .unwrap_or_default() as u64;
    let total_tokens = row
        .try_get::<Option<i64>, _>("total_tokens")?
        .unwrap_or_default() as u64;
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
        && total_tokens == 0
    {
        return Ok(None);
    }
    Ok(Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
    }))
}

pub(crate) async fn fetch_project_commits_since(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: usize,
) -> Result<Vec<mem_api::CommitRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT pc.commit_hash, pc.short_hash, pc.subject, pc.body, pc.author_name, pc.author_email,
               pc.committed_at, pc.parent_hashes, pc.changed_paths, pc.imported_at
        FROM project_commits pc
        JOIN projects p ON p.id = pc.project_id
        WHERE p.slug = $1
          AND ($2::timestamptz IS NULL OR pc.imported_at >= $2 OR pc.committed_at >= $2)
        ORDER BY pc.committed_at DESC
        LIMIT $3
        "#,
    )
    .bind(slug)
    .bind(since)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(crate::repository::row_to_commit_record)
        .collect()
}

pub(crate) async fn fetch_recent_project_memories(
    pool: &PgPool,
    slug: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: usize,
) -> Result<Vec<mem_api::ProjectMemoryListItem>, sqlx::Error> {
    let response = fetch_project_memories(pool, slug, None, limit as i64, 0).await?;
    Ok(response
        .items
        .into_iter()
        .filter(|item| since.is_none_or(|cutoff| item.updated_at >= cutoff))
        .collect())
}

pub(crate) async fn fetch_durable_resume_context(
    pool: &PgPool,
    slug: &str,
    limit: usize,
) -> Result<Vec<mem_api::ProjectMemoryListItem>, sqlx::Error> {
    let response = fetch_project_memories(pool, slug, Some("active"), 200, 0).await?;
    let mut items = response.items;
    items.sort_by(|left, right| {
        right
            .importance
            .cmp(&left.importance)
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    items.retain(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Architecture
                | mem_api::MemoryType::Convention
                | mem_api::MemoryType::Documentation
                | mem_api::MemoryType::Environment
                | mem_api::MemoryType::Refactor
        )
    });
    items.truncate(limit);
    Ok(items)
}

pub(crate) async fn fetch_latest_active_plan_memory(
    pool: &PgPool,
    slug: &str,
) -> Result<Option<mem_api::ProjectMemoryListItem>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            m.id,
            m.summary,
            left(m.canonical_text, 240) AS preview,
            m.memory_type,
            m.status,
            m.confidence,
            m.importance,
            m.updated_at,
            m.canonical_id,
            m.version_no,
            m.is_tombstone,
            COALESCE((
                SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), ARRAY[]::text[]) AS tags,
            COALESCE((
                SELECT COUNT(*)
                FROM memory_tags mt
                WHERE mt.memory_entry_id = m.id
            ), 0) AS tag_count,
            COALESCE((
                SELECT COUNT(*)
                FROM memory_sources ms
                WHERE ms.memory_entry_id = m.id
            ), 0) AS source_count
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
          AND m.status = 'active'
          AND m.memory_type = 'plan'
          AND m.is_tombstone = FALSE
          AND m.version_no = (
              SELECT MAX(m2.version_no)
              FROM memory_entries m2
              WHERE m2.canonical_id = m.canonical_id
          )
        ORDER BY m.updated_at DESC, m.id DESC
        LIMIT 1
        "#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        Ok(mem_api::ProjectMemoryListItem {
            id: row.try_get("id")?,
            summary: row.try_get("summary")?,
            preview: row.try_get("preview")?,
            memory_type: mem_search::parse_memory_type(&row.try_get::<String, _>("memory_type")?),
            status: match row.try_get::<String, _>("status")?.as_str() {
                "archived" => mem_api::MemoryStatus::Archived,
                _ => mem_api::MemoryStatus::Active,
            },
            confidence: row.try_get("confidence")?,
            importance: row.try_get("importance")?,
            updated_at: row.try_get("updated_at")?,
            tags: row.try_get("tags")?,
            tag_count: row.try_get("tag_count")?,
            source_count: row.try_get("source_count")?,
            canonical_id: row.try_get("canonical_id")?,
            version_no: row.try_get("version_no")?,
            is_tombstone: row.try_get("is_tombstone")?,
        })
    })
    .transpose()
}

pub(crate) fn resume_warnings(overview: &ProjectOverviewResponse) -> Vec<String> {
    let mut warnings = Vec::new();
    if overview.uncurated_raw_captures > 0 {
        warnings.push(format!(
            "{} raw capture(s) still need curation.",
            overview.uncurated_raw_captures
        ));
    }
    if overview
        .watchers
        .as_ref()
        .is_some_and(|watchers| watchers.unhealthy_count > 0)
    {
        let unhealthy = overview
            .watchers
            .as_ref()
            .map(|w| w.unhealthy_count)
            .unwrap_or(0);
        warnings.push(format!("{unhealthy} watcher(s) are unhealthy."));
    }
    if overview.missing_embedding_chunks > 0 {
        warnings.push(format!(
            "{} chunk(s) are missing active-space embeddings.",
            overview.missing_embedding_chunks
        ));
    }
    if overview.pending_replacement_proposals > 0 {
        warnings.push(format!(
            "{} memory update proposal(s) are waiting for review.",
            overview.pending_replacement_proposals
        ));
    }
    warnings
}

pub(crate) fn resume_actions(
    project: &str,
    checkpoint: Option<&mem_api::ResumeCheckpoint>,
    overview: &ProjectOverviewResponse,
    timeline: &[ActivityEvent],
    changed_memories: &[mem_api::ProjectMemoryListItem],
) -> Vec<ResumeAction> {
    let mut actions = Vec::new();
    let active_task_title = latest_capture_task_title(timeline);
    if overview.pending_replacement_proposals > 0 {
        actions.push(ResumeAction {
            title: "Review queued memory updates".to_string(),
            rationale: active_task_title
                .as_ref()
                .map(|task_title| {
                    format!(
                        "{} memory update proposal(s) from \"{}\" are waiting for review before outdated memories can be replaced.",
                        overview.pending_replacement_proposals, task_title
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "{} memory update proposal(s) are waiting for review before outdated memories can be replaced.",
                        overview.pending_replacement_proposals
                    )
                }),
            command_hint: Some(format!("memory tui --project {project}")),
        });
    }
    if overview.uncurated_raw_captures > 0 {
        actions.push(ResumeAction {
            title: "Curate pending captures".to_string(),
            rationale: format!(
                "{} raw capture(s) are waiting to be curated into canonical memory.",
                overview.uncurated_raw_captures
            ),
            command_hint: Some(format!("memory curate --project {project}")),
        });
    }
    if overview
        .watchers
        .as_ref()
        .is_some_and(|watchers| watchers.unhealthy_count > 0)
    {
        actions.push(ResumeAction {
            title: "Inspect watcher health".to_string(),
            rationale: "At least one watcher is unhealthy or restarting.".to_string(),
            command_hint: Some(format!("memory watcher status --project {project}")),
        });
    }
    if timeline
        .iter()
        .any(|event| matches!(event.kind, ActivityKind::QueryError))
    {
        actions.push(ResumeAction {
            title: "Review recent failed queries".to_string(),
            rationale: "Recent agent or user queries failed and may indicate blockers.".to_string(),
            command_hint: Some(format!("memory tui --project {project}")),
        });
    }
    if !changed_memories.is_empty() {
        actions.push(ResumeAction {
            title: "Review changed memories".to_string(),
            rationale: format!(
                "{} memory entry/entries changed since the last checkpoint.",
                changed_memories.len()
            ),
            command_hint: Some(format!("memory resume --project {project}")),
        });
    }
    if let Some(note) = checkpoint.and_then(|checkpoint| checkpoint.note.as_deref()) {
        actions.push(ResumeAction {
            title: "Resume the last approved thread".to_string(),
            rationale: format!("Your last checkpoint note was: {note}"),
            command_hint: Some(format!("memory resume --project {project}")),
        });
    }
    if actions.is_empty() {
        actions.push(ResumeAction {
            title: "Ask the next scoped question".to_string(),
            rationale: "The project looks stable; use the resume pack as the launch point for your next task.".to_string(),
            command_hint: Some(format!("memory query --project {project} --question \"What should I work on next?\"")),
        });
    }
    actions
}

pub(crate) fn infer_current_thread(
    checkpoint: Option<&mem_api::ResumeCheckpoint>,
    overview: &ProjectOverviewResponse,
    timeline: &[ActivityEvent],
    commits: &[mem_api::CommitRecord],
    changed_memories: &[mem_api::ProjectMemoryListItem],
    active_plan: Option<&mem_api::ProjectMemoryListItem>,
) -> Option<String> {
    if let Some(plan) = active_plan {
        if overview.pending_replacement_proposals > 0 {
            return Some(format!(
                "Approved plan in execution: {}. Curation left {} queued memory update proposal(s) to review.",
                plan.summary, overview.pending_replacement_proposals
            ));
        }
        if overview.uncurated_raw_captures > 0 {
            return Some(format!(
                "Approved plan in execution: {}. {} raw capture(s) are still waiting to be curated.",
                plan.summary, overview.uncurated_raw_captures
            ));
        }
        return Some(format!("Approved plan in execution: {}.", plan.summary));
    }

    let active_task_title = latest_capture_task_title(timeline);
    if overview.pending_replacement_proposals > 0 {
        return Some(
            active_task_title
                .as_ref()
                .map(|task_title| {
                    format!(
                        "Recent work focused on {}. Curation left {} queued memory update proposal(s) to review.",
                        task_title, overview.pending_replacement_proposals
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "Recent curation surfaced {} queued memory update proposal(s) that still need review.",
                        overview.pending_replacement_proposals
                    )
                }),
        );
    }
    if overview.uncurated_raw_captures > 0 {
        return Some(
            active_task_title
                .as_ref()
                .map(|task_title| {
                    format!(
                        "Recent work focused on {}. {} raw capture(s) are still waiting to be curated.",
                        task_title, overview.uncurated_raw_captures
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "{} raw capture(s) are waiting to be curated into canonical memory.",
                        overview.uncurated_raw_captures
                    )
                }),
        );
    }
    if let Some(task_title) = active_task_title {
        return Some(format!("Recent work focused on {}.", task_title));
    }
    if let Some(event) = timeline
        .iter()
        .find(|event| !matches!(event.kind, ActivityKind::Checkpoint))
    {
        let thread = match event.kind {
            ActivityKind::Scan => {
                "Recent work focused on refreshing project memory from a repo scan."
            }
            ActivityKind::Plan => {
                "Recent work focused on an approved execution plan for the current task."
            }
            ActivityKind::Curate => {
                "Recent work focused on curating new captures into canonical memory."
            }
            ActivityKind::CaptureTask => {
                "Recent work captured fresh project evidence that may need follow-up."
            }
            ActivityKind::MemoryReplacement => {
                "Recent work replaced outdated memory with a newer canonical version."
            }
            ActivityKind::Reindex => "Recent work rebuilt the project's searchable chunk index.",
            ActivityKind::Reembed => {
                "Recent work refreshed the active embedding space for semantic retrieval."
            }
            ActivityKind::GraphExtract => {
                "Recent work refreshed the parser-backed code graph for graph-aware retrieval."
            }
            ActivityKind::CommitSync => "Recent work synced stored commit history for the project.",
            ActivityKind::Query | ActivityKind::QueryError => {
                "Recent work centered on answering or debugging project questions."
            }
            ActivityKind::WatcherHealth => {
                "Recent work involved watcher health and background automation recovery."
            }
            ActivityKind::BundleImport | ActivityKind::BundleExport => {
                "Recent work focused on importing or exporting shareable memory bundles."
            }
            ActivityKind::Archive | ActivityKind::DeleteMemory => {
                "Recent work changed the active memory set for the project."
            }
            ActivityKind::Briefing => "Recent work generated a get-up-to-speed briefing.",
            ActivityKind::Diagnostic => {
                "Recent work recorded an operational diagnostic that may need attention."
            }
            ActivityKind::LlmAudit => {
                "Recent work recorded LLM audit/debug activity for service-side prompts."
            }
            ActivityKind::Checkpoint => "",
        };
        if !thread.is_empty() {
            return Some(format!(
                "{thread} Latest event: {}",
                event.summary.trim_end_matches('.')
            ));
        }
    }
    if let Some(commit) = commits.first() {
        return Some(format!(
            "Recent work landed in git, most recently `{}` ({})",
            commit.subject, commit.short_hash
        ));
    }
    if let Some(memory) = changed_memories.first() {
        return Some(format!(
            "Recent work changed project memory, including: {}",
            memory.summary
        ));
    }
    checkpoint
        .and_then(|checkpoint| checkpoint.note.as_ref())
        .map(|note| format!("The last explicit work checkpoint was: {note}"))
}

pub(crate) fn build_change_summary(
    timeline: &[ActivityEvent],
    commits: &[mem_api::CommitRecord],
    changed_memories: &[mem_api::ProjectMemoryListItem],
) -> Vec<String> {
    let mut items = Vec::new();
    let mut seen_titles = Vec::new();
    for event in timeline.iter().take(6) {
        if let Some(task_title) = extract_capture_task_title(event)
            && !seen_titles.contains(&task_title)
        {
            items.push(format!(
                "{} Worked on: {}",
                event.recorded_at.format("%m-%d %H:%M"),
                task_title
            ));
            seen_titles.push(task_title);
        }
    }
    if let Some(commit) = commits.first() {
        let changed_paths = if commit.changed_paths.is_empty() {
            "no path summary".to_string()
        } else {
            commit
                .changed_paths
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        };
        items.push(format!(
            "Latest stored commit: {} ({}) touching {}",
            commit.subject, commit.short_hash, changed_paths
        ));
    }
    if items.is_empty() {
        for event in timeline
            .iter()
            .filter(|event| !matches!(event.kind, ActivityKind::Checkpoint | ActivityKind::Curate))
            .take(3)
        {
            let entry = format!(
                "{} {}",
                event.recorded_at.format("%m-%d %H:%M"),
                format_resume_event_summary(event)
            );
            if !items.contains(&entry) {
                items.push(entry);
            }
        }
    }
    if !changed_memories.is_empty() && items.is_empty() {
        let examples = changed_memories
            .iter()
            .take(2)
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
            .join(" | ");
        items.push(format!(
            "{} memory update(s) landed, including: {}",
            changed_memories.len(),
            examples
        ));
    }
    items.truncate(5);
    items
}

pub(crate) fn latest_capture_task_title(timeline: &[ActivityEvent]) -> Option<String> {
    timeline.iter().find_map(extract_capture_task_title)
}

pub(crate) fn extract_capture_task_title(event: &ActivityEvent) -> Option<String> {
    match &event.details {
        Some(ActivityDetails::CaptureTask { task_title, .. }) => task_title
            .as_ref()
            .map(|title| title.trim().trim_end_matches('.').to_string())
            .filter(|title| !title.is_empty())
            .or_else(|| {
                event
                    .summary
                    .strip_prefix("Captured task: ")
                    .map(|title| title.trim().trim_end_matches('.').to_string())
                    .filter(|title| !title.is_empty())
            }),
        _ => event
            .summary
            .strip_prefix("Captured task: ")
            .map(|title| title.trim().trim_end_matches('.').to_string())
            .filter(|title| !title.is_empty()),
    }
}

pub(crate) fn format_resume_event_summary(event: &ActivityEvent) -> String {
    let base = match &event.details {
        Some(ActivityDetails::Plan { action, title, .. }) => {
            let prefix = match action {
                PlanActivityAction::Started => "Approved plan recorded",
                PlanActivityAction::Synced => "Approved plan synced",
                PlanActivityAction::FinishBlocked => "Plan completion blocked",
                PlanActivityAction::FinishVerified => "Plan completion verified",
            };
            format!("{prefix}: {}", title.trim())
        }
        Some(ActivityDetails::Query { query, .. }) => {
            format!("Query explored: {}", query.trim())
        }
        Some(ActivityDetails::Checkpoint { note, .. }) => note
            .as_ref()
            .map(|note| format!("Saved checkpoint: {note}"))
            .unwrap_or_else(|| event.summary.trim().to_string()),
        _ => match event.kind {
            ActivityKind::Query | ActivityKind::QueryError => {
                let query = event
                    .summary
                    .strip_prefix("Query: ")
                    .or_else(|| event.summary.strip_prefix("Query failed: "))
                    .unwrap_or(event.summary.as_str())
                    .trim();
                format!("Query explored: {query}")
            }
            ActivityKind::Briefing => "Generated a get-up-to-speed briefing".to_string(),
            _ => event.summary.trim().to_string(),
        },
    };
    clamp_resume_line(base.trim_end_matches('.'), 110)
}

pub(crate) fn clamp_resume_line(value: &str, limit: usize) -> String {
    let value = value.trim();
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut truncated = String::new();
    for ch in value.chars().take(limit.saturating_sub(1)) {
        truncated.push(ch);
    }
    truncated.push('…');
    truncated
}

pub(crate) fn build_attention_items(
    overview: &ProjectOverviewResponse,
    timeline: &[ActivityEvent],
) -> Vec<String> {
    let mut items = Vec::new();
    if overview.pending_replacement_proposals > 0 {
        items.push(format!(
            "{} memory update proposal(s) are waiting for review.",
            overview.pending_replacement_proposals
        ));
    }
    if overview.uncurated_raw_captures > 0 {
        items.push(format!(
            "{} raw capture(s) still need curation.",
            overview.uncurated_raw_captures
        ));
    }
    if overview
        .watchers
        .as_ref()
        .is_some_and(|watchers| watchers.unhealthy_count > 0)
    {
        let unhealthy = overview
            .watchers
            .as_ref()
            .map(|watchers| watchers.unhealthy_count)
            .unwrap_or(0);
        items.push(format!(
            "{unhealthy} watcher(s) are unhealthy or restarting."
        ));
    }
    if timeline
        .iter()
        .any(|event| matches!(event.kind, ActivityKind::QueryError))
    {
        items.push("Recent query errors may indicate an unresolved blocker.".to_string());
    }
    let embedding_work_active = timeline.iter().any(|event| {
        matches!(
            event.kind,
            ActivityKind::Scan
                | ActivityKind::GraphExtract
                | ActivityKind::Reembed
                | ActivityKind::Reindex
        )
    });
    if overview.missing_embedding_chunks > 0 && (embedding_work_active || items.is_empty()) {
        items.push(format!(
            "{} chunk(s) are missing active-space embeddings.",
            overview.missing_embedding_chunks
        ));
    }
    items
}

pub(crate) fn select_resume_context(
    changed_memories: &[mem_api::ProjectMemoryListItem],
    durable_context: &[mem_api::ProjectMemoryListItem],
    active_plan: Option<&mem_api::ProjectMemoryListItem>,
) -> Vec<mem_api::ProjectMemoryListItem> {
    let mut selected = Vec::new();

    if let Some(plan) = active_plan {
        selected.push(plan.clone());
    }

    if let Some(item) = changed_memories.iter().find(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Task
                | mem_api::MemoryType::Plan
                | mem_api::MemoryType::Decision
                | mem_api::MemoryType::Architecture
                | mem_api::MemoryType::Convention
                | mem_api::MemoryType::Documentation
                | mem_api::MemoryType::Debugging
                | mem_api::MemoryType::Refactor
        ) && !selected.iter().any(|existing| existing.id == item.id)
    }) {
        selected.push(item.clone());
    } else if let Some(item) = changed_memories
        .iter()
        .find(|item| !selected.iter().any(|existing| existing.id == item.id))
    {
        selected.push(item.clone());
    }

    if let Some(item) = durable_context.iter().find(|item| {
        matches!(
            item.memory_type,
            mem_api::MemoryType::Decision
                | mem_api::MemoryType::Architecture
                | mem_api::MemoryType::Convention
                | mem_api::MemoryType::Documentation
                | mem_api::MemoryType::Environment
                | mem_api::MemoryType::Refactor
        ) && !selected.iter().any(|existing| existing.id == item.id)
    }) {
        selected.push(item.clone());
    }

    if let Some(item) = durable_context
        .iter()
        .find(|item| !selected.iter().any(|existing| existing.id == item.id))
    {
        selected.push(item.clone());
    }

    selected.truncate(3);
    selected
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_resume_briefing(
    project: &str,
    checkpoint: Option<&mem_api::ResumeCheckpoint>,
    current_thread: Option<&str>,
    change_summary: &[String],
    attention_items: &[String],
    primary_next_step: Option<&ResumeAction>,
    secondary_next_steps: &[ResumeAction],
    context_items: &[mem_api::ProjectMemoryListItem],
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Resume briefing for project `{project}`."));
    if let Some(checkpoint) = checkpoint {
        lines.push(format!(
            "Last checkpoint: {}.",
            checkpoint.marked_at.format("%Y-%m-%d %H:%M UTC")
        ));
        if let Some(note) = &checkpoint.note {
            lines.push(format!("Checkpoint note: {note}"));
        }
    } else {
        lines.push("No checkpoint is stored yet. This is a current-state briefing.".to_string());
    }
    if let Some(current_thread) = current_thread {
        lines.push(String::new());
        lines.push("Current thread:".to_string());
        lines.push(format!("- {current_thread}"));
    }
    if let Some(action) = primary_next_step {
        lines.push(String::new());
        lines.push("Next step:".to_string());
        lines.push(format!("- {}: {}", action.title, action.rationale));
        if let Some(command_hint) = &action.command_hint {
            lines.push(format!("  {command_hint}"));
        }
    }
    if !change_summary.is_empty() {
        lines.push(String::new());
        lines.push("What changed:".to_string());
        for item in change_summary.iter().take(5) {
            lines.push(format!("- {item}"));
        }
    }
    if !attention_items.is_empty() {
        lines.push(String::new());
        lines.push("Needs attention:".to_string());
        for item in attention_items.iter().take(4) {
            lines.push(format!("- {item}"));
        }
    }
    if !context_items.is_empty() {
        lines.push(String::new());
        lines.push("Keep in mind:".to_string());
        for item in context_items.iter().take(3) {
            lines.push(format!("- [{}] {}", item.memory_type, item.summary));
        }
    }
    if !secondary_next_steps.is_empty() {
        lines.push(String::new());
        lines.push("Other useful follow-ups:".to_string());
        for action in secondary_next_steps.iter().take(2) {
            lines.push(format!("- {}: {}", action.title, action.rationale));
        }
    }
    lines.join("\n")
}

pub(crate) fn summarize_activity_tokens(events: &[ActivityEvent]) -> TokenUsageSummary {
    let mut summary = TokenUsageSummary::default();
    for usage in events.iter().filter_map(|event| event.token_usage.as_ref()) {
        summary.action_count += 1;
        summary.total_input_tokens += usage.input_tokens;
        summary.total_output_tokens += usage.output_tokens;
        summary.total_cache_read_tokens += usage.cache_read_tokens;
        summary.total_cache_write_tokens += usage.cache_write_tokens;
        summary.total_tokens += usage.total_tokens;
    }
    summary
}

pub(crate) fn build_up_to_speed_briefing(
    project: &str,
    current_focus: &[String],
    recent_work: &[String],
    blockers: &[String],
    next_actions: &[ResumeAction],
    useful_memories: &[ProjectMemoryListItem],
    token_usage: &TokenUsageSummary,
) -> String {
    let mut lines = vec![format!("Get up to speed for `{project}`.")];
    if !current_focus.is_empty() {
        lines.push(String::new());
        lines.push("Current focus:".to_string());
        for item in current_focus {
            lines.push(format!("- {item}"));
        }
    }
    if !recent_work.is_empty() {
        lines.push(String::new());
        lines.push("Recent work:".to_string());
        for item in recent_work.iter().take(6) {
            lines.push(format!("- {item}"));
        }
    }
    if !blockers.is_empty() {
        lines.push(String::new());
        lines.push("Needs attention:".to_string());
        for item in blockers.iter().take(6) {
            lines.push(format!("- {item}"));
        }
    }
    if !useful_memories.is_empty() {
        lines.push(String::new());
        lines.push("Useful memories:".to_string());
        for item in useful_memories.iter().take(6) {
            lines.push(format!("- [{}] {}", item.memory_type, item.summary));
        }
    }
    if token_usage.action_count > 0 {
        lines.push(String::new());
        lines.push(format!(
            "Token usage across {} recent action(s): {} total ({} input, {} output, {} cache read, {} cache write).",
            token_usage.action_count,
            token_usage.total_tokens,
            token_usage.total_input_tokens,
            token_usage.total_output_tokens,
            token_usage.total_cache_read_tokens,
            token_usage.total_cache_write_tokens,
        ));
    }
    if !next_actions.is_empty() {
        lines.push(String::new());
        lines.push("Recommended next actions:".to_string());
        for action in next_actions.iter().take(3) {
            lines.push(format!("- {}: {}", action.title, action.rationale));
            if let Some(command_hint) = &action.command_hint {
                lines.push(format!("  {command_hint}"));
            }
        }
    }
    if lines.len() == 1 {
        lines.push(
            "No recent activity was found. Start with `memory query` or inspect the TUI."
                .to_string(),
        );
    }
    lines.join("\n")
}

pub(crate) async fn summarize_resume_with_llm(
    state: &AppState,
    project: &str,
    operation: &str,
    deterministic: &str,
) -> Result<String> {
    if !is_supported_llm_provider(&state.config.llm.provider)
        || state.config.llm.model.trim().is_empty()
    {
        anyhow::bail!("llm summary is not configured");
    }
    let api_key = resolve_llm_api_key(&state.config.llm);
    if llm_requires_api_key(&state.config.llm) && api_key.is_none() {
        anyhow::bail!(
            "read llm api key {} for resume summary",
            state.config.llm.api_key_env
        );
    }
    let url = format!(
        "{}/chat/completions",
        effective_llm_base_url(&state.config.llm)
    );
    let mut request = serde_json::json!({
        "model": state.config.llm.model,
        "temperature": 0.0,
        "messages": [
            {
                "role": "system",
                "content": "You write concise project resume briefings for returning developers. Summarize what changed, what still matters, and what to do next. Keep it factual and grounded in the provided project resume pack."
            },
            {
                "role": "user",
                "content": format!("Project: {project}\n\nResume pack:\n{deterministic}")
            }
        ]
    });
    request[llm_max_output_tokens_field(&state.config.llm.provider)] = serde_json::json!(600);
    let started = std::time::Instant::now();
    let mut builder = state.http_client.post(url);
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key);
    }
    let response = match builder.json(&request).send().await {
        Ok(response) => response,
        Err(error) => {
            emit_llm_audit_activity(
                state,
                project,
                operation,
                format!("Project: {project}"),
                &request,
                "error",
                Some(&format!("send llm resume summary request: {error}")),
                Some(started.elapsed().as_millis() as u64),
                None,
            );
            return Err(error).context("send llm resume summary request");
        }
    };
    let status = response.status();
    let body = match response.text().await {
        Ok(body) => body,
        Err(error) => {
            emit_llm_audit_activity(
                state,
                project,
                operation,
                format!("Project: {project}"),
                &request,
                "error",
                Some(&format!("read llm resume summary body: {error}")),
                Some(started.elapsed().as_millis() as u64),
                None,
            );
            return Err(error).context("read llm resume summary body");
        }
    };
    let token_usage = token_usage_from_chat_body(&body);
    if !status.is_success() {
        let error = format!("llm resume summary failed: {status} {body}");
        emit_llm_audit_activity(
            state,
            project,
            operation,
            format!("Project: {project}"),
            &request,
            "error",
            Some(&error),
            Some(started.elapsed().as_millis() as u64),
            token_usage,
        );
        anyhow::bail!("llm resume summary failed: {status} {body}");
    }
    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(error) => {
            emit_llm_audit_activity(
                state,
                project,
                operation,
                format!("Project: {project}"),
                &request,
                "error",
                Some(&format!("parse llm resume summary response: {error}")),
                Some(started.elapsed().as_millis() as u64),
                token_usage,
            );
            return Err(error).context("parse llm resume summary response");
        }
    };
    let content = match payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
    {
        Some(content) => content,
        None => {
            emit_llm_audit_activity(
                state,
                project,
                operation,
                format!("Project: {project}"),
                &request,
                "error",
                Some("llm resume summary missing content"),
                Some(started.elapsed().as_millis() as u64),
                token_usage,
            );
            anyhow::bail!("llm resume summary missing content");
        }
    };
    emit_llm_audit_activity(
        state,
        project,
        operation,
        format!("Project: {project}"),
        &request,
        "success",
        None,
        Some(started.elapsed().as_millis() as u64),
        token_usage,
    );
    Ok(content.to_string())
}
