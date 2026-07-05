//! Database access for reinforcement scoring. All sqlx queries in this
//! crate live here, per workspace convention.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::propagation::CanonicalEdge;
use crate::scoring::ScoreParams;

/// Cap on edges fetched per propagation batch so a densely linked project
/// cannot make access recording expensive.
pub const EDGE_FETCH_LIMIT: i64 = 500;

/// What a score update did to one canonical memory.
#[derive(Debug, Clone, Copy)]
pub struct ScoreUpdate {
    pub canonical_id: Uuid,
    pub project_id: Uuid,
    pub old_activation: f64,
    pub new_activation: f64,
}

impl ScoreUpdate {
    pub fn crossed(&self, threshold: f64) -> bool {
        self.old_activation < threshold && self.new_activation >= threshold
    }
}

/// Counter increments applied together with an activation boost.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreCounters {
    pub access: i64,
    pub citation: i64,
    pub propagated: i64,
}

/// Maps memory version ids to `(canonical_id, project_id)`.
pub async fn resolve_canonicals(
    pool: &PgPool,
    memory_ids: &[Uuid],
) -> Result<HashMap<Uuid, (Uuid, Uuid)>> {
    if memory_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        r#"
        SELECT id, canonical_id, project_id
        FROM memory_entries
        WHERE id = ANY($1)
        "#,
    )
    .bind(memory_ids)
    .fetch_all(pool)
    .await
    .context("resolve canonical ids for accessed memories")?;
    Ok(rows
        .into_iter()
        .map(|(id, canonical_id, project_id)| (id, (canonical_id, project_id)))
        .collect())
}

/// Fetches undirected canonical-level relation edges reachable from `seeds`
/// within `max_hops`, excluding `supersedes` (version lineage, not semantic
/// association). Bounded by [`EDGE_FETCH_LIMIT`].
pub async fn fetch_canonical_edges(
    pool: &PgPool,
    seeds: &[Uuid],
    max_hops: u8,
) -> Result<Vec<CanonicalEdge>> {
    if seeds.is_empty() || max_hops == 0 {
        return Ok(Vec::new());
    }
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        r#"
        WITH RECURSIVE reachable(canonical_id, hop) AS (
            SELECT DISTINCT m.canonical_id, 0
            FROM memory_entries m
            WHERE m.canonical_id = ANY($1)
            UNION
            SELECT other.canonical_id, r.hop + 1
            FROM reachable r
            JOIN memory_entries me ON me.canonical_id = r.canonical_id
            JOIN memory_relations rel
              ON rel.src_memory_id = me.id OR rel.dst_memory_id = me.id
            JOIN memory_entries other ON other.id = CASE
                WHEN rel.src_memory_id = me.id THEN rel.dst_memory_id
                ELSE rel.src_memory_id
            END
            WHERE rel.relation_type <> 'supersedes' AND r.hop < $2
        )
        SELECT DISTINCT
            LEAST(ma.canonical_id, mb.canonical_id),
            GREATEST(ma.canonical_id, mb.canonical_id)
        FROM memory_relations rel
        JOIN memory_entries ma ON ma.id = rel.src_memory_id
        JOIN memory_entries mb ON mb.id = rel.dst_memory_id
        JOIN reachable ra ON ra.canonical_id = ma.canonical_id
        JOIN reachable rb ON rb.canonical_id = mb.canonical_id
        WHERE rel.relation_type <> 'supersedes'
          AND ma.canonical_id <> mb.canonical_id
        LIMIT $3
        "#,
    )
    .bind(seeds)
    .bind(i32::from(max_hops))
    .bind(EDGE_FETCH_LIMIT)
    .fetch_all(pool)
    .await
    .context("fetch canonical relation edges")?;
    Ok(rows
        .into_iter()
        .map(|(a, b)| CanonicalEdge { a, b })
        .collect())
}

/// Maps canonical ids to their project id.
pub async fn resolve_project_ids(
    pool: &PgPool,
    canonical_ids: &[Uuid],
) -> Result<HashMap<Uuid, Uuid>> {
    if canonical_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        r#"
        SELECT DISTINCT canonical_id, project_id
        FROM memory_entries
        WHERE canonical_id = ANY($1)
        "#,
    )
    .bind(canonical_ids)
    .fetch_all(pool)
    .await
    .context("resolve project ids for canonical memories")?;
    Ok(rows.into_iter().collect())
}

/// Atomic decay-then-boost upsert. Decay is computed inside the statement
/// from `last_decay_at`, so concurrent writers never race a
/// read-modify-write cycle. Returns the pre- and post-update activation.
pub async fn apply_score_boost(
    pool: &PgPool,
    canonical_id: Uuid,
    project_id: Uuid,
    boost: f64,
    counters: ScoreCounters,
    params: &ScoreParams,
) -> Result<ScoreUpdate> {
    let half_life_secs = (params.half_life.num_milliseconds() as f64 / 1000.0).max(1.0);
    let is_direct = counters.access > 0;
    let (old_activation, new_activation): (f64, f64) = sqlx::query_as(
        r#"
        WITH existing AS (
            SELECT activation FROM memory_scores WHERE canonical_id = $1
        ), upsert AS (
            INSERT INTO memory_scores (
                canonical_id, project_id, activation, last_decay_at,
                last_access_at, access_count, citation_count,
                propagated_count, updated_at
            ) VALUES (
                $1, $2, LEAST($3, $4), now(),
                CASE WHEN $5 THEN now() END, $6, $7, $8, now()
            )
            ON CONFLICT (canonical_id) DO UPDATE SET
                activation = LEAST($4,
                    memory_scores.activation * power(
                        0.5,
                        GREATEST(EXTRACT(EPOCH FROM (now() - memory_scores.last_decay_at)), 0) / $9
                    ) + $3),
                last_decay_at = now(),
                last_access_at = CASE
                    WHEN $5 THEN now()
                    ELSE memory_scores.last_access_at
                END,
                access_count = memory_scores.access_count + $6,
                citation_count = memory_scores.citation_count + $7,
                propagated_count = memory_scores.propagated_count + $8,
                updated_at = now()
            RETURNING activation
        )
        SELECT
            COALESCE((SELECT activation FROM existing), 0)::float8,
            (SELECT activation FROM upsert)::float8
        "#,
    )
    .bind(canonical_id)
    .bind(project_id)
    .bind(boost)
    .bind(params.max_activation)
    .bind(is_direct)
    .bind(counters.access)
    .bind(counters.citation)
    .bind(counters.propagated)
    .bind(half_life_secs)
    .fetch_one(pool)
    .await
    .context("apply reinforcement score boost")?;
    Ok(ScoreUpdate {
        canonical_id,
        project_id,
        old_activation,
        new_activation,
    })
}

/// One row for the compact append-only access log.
#[derive(Debug, Clone)]
pub struct AccessEventRow {
    pub canonical_id: Uuid,
    pub project_id: Uuid,
    pub kind: &'static str,
    pub boost: f64,
    pub hop_distance: i16,
    pub operation_id: Option<String>,
}

pub async fn insert_access_events(pool: &PgPool, rows: &[AccessEventRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let canonical_ids: Vec<Uuid> = rows.iter().map(|r| r.canonical_id).collect();
    let project_ids: Vec<Uuid> = rows.iter().map(|r| r.project_id).collect();
    let kinds: Vec<String> = rows.iter().map(|r| r.kind.to_string()).collect();
    let boosts: Vec<f32> = rows.iter().map(|r| r.boost as f32).collect();
    let hops: Vec<i16> = rows.iter().map(|r| r.hop_distance).collect();
    let operation_ids: Vec<Option<String>> = rows.iter().map(|r| r.operation_id.clone()).collect();
    sqlx::query(
        r#"
        INSERT INTO memory_access_events (
            canonical_id, project_id, kind, boost, hop_distance, operation_id
        )
        SELECT * FROM UNNEST(
            $1::uuid[], $2::uuid[], $3::text[], $4::real[], $5::smallint[], $6::text[]
        )
        "#,
    )
    .bind(&canonical_ids)
    .bind(&project_ids)
    .bind(&kinds)
    .bind(&boosts)
    .bind(&hops)
    .bind(&operation_ids)
    .execute(pool)
    .await
    .context("insert memory access events")?;
    Ok(())
}

pub async fn insert_score_audit(
    pool: &PgPool,
    canonical_id: Uuid,
    project_id: Uuid,
    reason: &str,
    old_activation: Option<f64>,
    new_activation: Option<f64>,
    details: serde_json::Value,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO memory_score_audit (
            id, canonical_id, project_id, reason,
            old_activation, new_activation, details_json
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(canonical_id)
    .bind(project_id)
    .bind(reason)
    .bind(old_activation)
    .bind(new_activation)
    .bind(details)
    .execute(pool)
    .await
    .context("insert memory score audit row")?;
    Ok(())
}

pub async fn prune_access_events(pool: &PgPool, older_than: DateTime<Utc>) -> Result<u64> {
    let result = sqlx::query("DELETE FROM memory_access_events WHERE accessed_at < $1")
        .bind(older_than)
        .execute(pool)
        .await
        .context("prune memory access events")?;
    Ok(result.rows_affected())
}

/// Parameters for the due-for-validation scan, mirrored from
/// `ReinforcementConfig`.
#[derive(Debug, Clone)]
pub struct SelectionParams {
    pub threshold: f64,
    pub half_life_secs: f64,
    pub min_revalidation_secs: f64,
    pub volatility_factor: f64,
}

impl From<&mem_api::ReinforcementConfig> for SelectionParams {
    fn from(config: &mem_api::ReinforcementConfig) -> Self {
        Self {
            threshold: config.validation_threshold,
            half_life_secs: config.half_life.as_secs_f64().max(1.0),
            min_revalidation_secs: config.min_revalidation_interval.as_secs_f64(),
            volatility_factor: config.volatility_revalidation_factor.max(0.0),
        }
    }
}

/// Row shape returned by the due-for-validation scan.
type DueCandidateRow = (
    Uuid,
    Uuid,
    Uuid,
    f64,
    f32,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

/// Scan-based selection of memories due for validation: decay-corrected
/// activation over threshold, not flagged for review, past cooldown, and
/// (never validated OR past the volatility-shortened revalidation
/// interval). Ordered by activation so the hottest memories validate first.
pub async fn fetch_due_candidates(
    pool: &PgPool,
    project_id: Option<Uuid>,
    params: &SelectionParams,
    limit: i64,
) -> Result<Vec<crate::selection::ValidationCandidate>> {
    let rows: Vec<DueCandidateRow> = sqlx::query_as(
        r#"
            SELECT s.canonical_id, m.id, s.project_id,
                   (s.activation * power(
                       0.5,
                       GREATEST(EXTRACT(EPOCH FROM (now() - s.last_decay_at)), 0) / $2
                   ))::float8 AS activation,
                   s.volatility, s.validated_at, s.validation_cooldown_until
            FROM memory_scores s
            JOIN LATERAL (
                SELECT id FROM memory_entries
                WHERE canonical_id = s.canonical_id
                  AND COALESCE(is_tombstone, false) = false
                  AND status = 'active'
                ORDER BY version_no DESC
                LIMIT 1
            ) m ON true
            WHERE ($1::uuid IS NULL OR s.project_id = $1)
              AND NOT s.needs_review
              AND (s.validation_cooldown_until IS NULL OR s.validation_cooldown_until < now())
              AND (s.activation * power(
                       0.5,
                       GREATEST(EXTRACT(EPOCH FROM (now() - s.last_decay_at)), 0) / $2
                   )) >= $3
              AND (s.validated_at IS NULL
                   OR s.validated_at < now() - ($4 / (1.0 + GREATEST(s.volatility, 0) * $5))
                       * interval '1 second')
            ORDER BY s.activation DESC
            LIMIT $6
            "#,
    )
    .bind(project_id)
    .bind(params.half_life_secs)
    .bind(params.threshold)
    .bind(params.min_revalidation_secs)
    .bind(params.volatility_factor)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("scan memories due for validation")?;
    Ok(rows
        .into_iter()
        .map(
            |(
                canonical_id,
                memory_id,
                project_id,
                activation,
                volatility,
                validated_at,
                cooldown_until,
            )| {
                crate::selection::ValidationCandidate {
                    canonical_id,
                    memory_id,
                    project_id,
                    activation,
                    volatility,
                    validated_at,
                    needs_review: false,
                    cooldown_until,
                }
            },
        )
        .collect())
}

/// Counts non-dry-run validation runs started since `since` (daily budget).
pub async fn count_validation_runs_since(pool: &PgPool, since: DateTime<Utc>) -> Result<i64> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM memory_validation_runs WHERE started_at > $1 AND NOT dry_run",
    )
    .bind(since)
    .fetch_one(pool)
    .await
    .context("count recent validation runs")?;
    Ok(count)
}

/// Timestamp of the last compaction sweep, from its audit trail.
pub async fn last_compaction_at(pool: &PgPool) -> Result<Option<DateTime<Utc>>> {
    let (at,): (Option<DateTime<Utc>>,) = sqlx::query_as(
        "SELECT MAX(created_at) FROM memory_score_audit WHERE reason = 'decay_compaction'",
    )
    .fetch_one(pool)
    .await
    .context("read last compaction timestamp")?;
    Ok(at)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CompactionSummary {
    pub cold_rows_deleted: u64,
    pub orphan_rows_deleted: u64,
}

/// Removes score rows that decayed to noise (activation < 0.01 after decay
/// and no access in 90 days) and rows whose canonical memory no longer
/// exists. Writes one `decay_compaction` audit row when anything was
/// removed (canonical_id is nil for sweep-level entries).
pub async fn compact_scores(pool: &PgPool, half_life_secs: f64) -> Result<CompactionSummary> {
    let cold = sqlx::query(
        r#"
        DELETE FROM memory_scores
        WHERE (activation * power(
                  0.5,
                  GREATEST(EXTRACT(EPOCH FROM (now() - last_decay_at)), 0) / $1
              )) < 0.01
          AND (last_access_at IS NULL OR last_access_at < now() - interval '90 days')
          AND created_at < now() - interval '90 days'
        "#,
    )
    .bind(half_life_secs.max(1.0))
    .execute(pool)
    .await
    .context("compact cold score rows")?;
    let orphans = sqlx::query(
        r#"
        DELETE FROM memory_scores s
        WHERE NOT EXISTS (
            SELECT 1 FROM memory_entries m WHERE m.canonical_id = s.canonical_id
        )
        "#,
    )
    .execute(pool)
    .await
    .context("compact orphaned score rows")?;
    let summary = CompactionSummary {
        cold_rows_deleted: cold.rows_affected(),
        orphan_rows_deleted: orphans.rows_affected(),
    };
    if summary.cold_rows_deleted > 0 || summary.orphan_rows_deleted > 0 {
        sqlx::query(
            r#"
            INSERT INTO memory_score_audit
                (id, canonical_id, project_id, reason, details_json)
            VALUES ($1, $2, $2, 'decay_compaction', $3)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(Uuid::nil())
        .bind(serde_json::json!({
            "cold_rows_deleted": summary.cold_rows_deleted,
            "orphan_rows_deleted": summary.orphan_rows_deleted,
        }))
        .execute(pool)
        .await
        .context("insert compaction audit row")?;
    }
    Ok(summary)
}

/// A volatility change produced by [`fold_volatility`].
#[derive(Debug, Clone, Copy)]
pub struct VolatilityShift {
    pub canonical_id: Uuid,
    pub project_id: Uuid,
    pub old_volatility: f32,
    pub new_volatility: f32,
}

/// Folds observed provenance change events into the per-memory volatility
/// EWMA (events per day, elapsed measured from the previous fold). Only
/// memories that already have a score row are updated: volatility exists to
/// steer revalidation, which only applies to scored memories.
pub async fn fold_volatility(
    pool: &PgPool,
    change_counts: &HashMap<Uuid, u32>,
    alpha: f64,
) -> Result<Vec<VolatilityShift>> {
    if change_counts.is_empty() {
        return Ok(Vec::new());
    }
    let canonical_ids: Vec<Uuid> = change_counts.keys().copied().collect();
    let counts: Vec<i32> = canonical_ids
        .iter()
        .map(|id| change_counts[id] as i32)
        .collect();
    let rows: Vec<(Uuid, Uuid, f32, f32)> = sqlx::query_as(
        r#"
        WITH previous AS (
            SELECT canonical_id, volatility
            FROM memory_scores
            WHERE canonical_id = ANY($1)
        ), updated AS (
            UPDATE memory_scores s SET
                volatility = (
                    $3 * (c.changes::float8 / GREATEST(
                        EXTRACT(EPOCH FROM (now() - COALESCE(s.volatility_updated_at, s.created_at))) / 86400.0,
                        0.04))
                    + (1.0 - $3) * s.volatility
                )::real,
                volatility_updated_at = now(),
                updated_at = now()
            FROM (SELECT UNNEST($1::uuid[]) AS canonical_id, UNNEST($2::int[]) AS changes) c
            WHERE s.canonical_id = c.canonical_id
            RETURNING s.canonical_id, s.project_id, s.volatility
        )
        SELECT u.canonical_id, u.project_id, p.volatility, u.volatility
        FROM updated u
        JOIN previous p ON p.canonical_id = u.canonical_id
        "#,
    )
    .bind(&canonical_ids)
    .bind(&counts)
    .bind(alpha.clamp(0.0, 1.0))
    .fetch_all(pool)
    .await
    .context("fold volatility EWMA")?;
    Ok(rows
        .into_iter()
        .map(
            |(canonical_id, project_id, old_volatility, new_volatility)| VolatilityShift {
                canonical_id,
                project_id,
                old_volatility,
                new_volatility,
            },
        )
        .collect())
}

/// Full snapshot of the memory version being validated.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemorySnapshot {
    pub memory_id: Uuid,
    pub canonical_id: Uuid,
    pub project_id: Uuid,
    pub project_slug: String,
    pub repo_root: String,
    pub summary: String,
    pub canonical_text: String,
    pub memory_type: String,
    pub importance: i32,
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn fetch_memory_snapshot(
    pool: &PgPool,
    memory_id: Uuid,
) -> Result<Option<MemorySnapshot>> {
    sqlx::query_as::<_, MemorySnapshot>(
        r#"
        SELECT m.id AS memory_id, m.canonical_id, m.project_id,
               p.slug AS project_slug, COALESCE(p.root_path, '') AS repo_root,
               m.summary, m.canonical_text, m.memory_type, m.importance,
               m.confidence, m.created_at, m.updated_at
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE m.id = $1 AND COALESCE(m.is_tombstone, false) = false
        "#,
    )
    .bind(memory_id)
    .fetch_optional(pool)
    .await
    .context("fetch memory snapshot for validation")
}

pub async fn fetch_memory_tags(pool: &PgPool, memory_id: Uuid) -> Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT tag FROM memory_tags WHERE memory_entry_id = $1 ORDER BY tag")
            .bind(memory_id)
            .fetch_all(pool)
            .await
            .context("fetch memory tags for validation")?;
    Ok(rows.into_iter().map(|(tag,)| tag).collect())
}

/// A memory source plus its latest provenance verification, if any.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SourceEvidence {
    pub file_path: Option<String>,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub source_kind: String,
    pub excerpt: Option<String>,
    pub git_commit: Option<String>,
    pub provenance_status: Option<String>,
    pub provenance_checked_at: Option<DateTime<Utc>>,
}

pub async fn fetch_source_evidence(pool: &PgPool, memory_id: Uuid) -> Result<Vec<SourceEvidence>> {
    sqlx::query_as::<_, SourceEvidence>(
        r#"
        SELECT ms.file_path, ms.symbol_name, ms.symbol_kind, ms.source_kind,
               ms.excerpt, ms.git_commit,
               v.status AS provenance_status, v.checked_at AS provenance_checked_at
        FROM memory_sources ms
        LEFT JOIN memory_source_verifications v ON v.source_id = ms.id
        WHERE ms.memory_entry_id = $1
        ORDER BY ms.created_at
        "#,
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .context("fetch source evidence for validation")
}

/// A memory related to the one under validation (1 hop, any direction).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RelatedMemorySnapshot {
    pub memory_id: Uuid,
    pub relation_type: String,
    pub summary: String,
}

pub async fn fetch_related_snapshots(
    pool: &PgPool,
    memory_id: Uuid,
) -> Result<Vec<RelatedMemorySnapshot>> {
    sqlx::query_as::<_, RelatedMemorySnapshot>(
        r#"
        SELECT other.id AS memory_id, r.relation_type, other.summary
        FROM memory_relations r
        JOIN memory_entries other ON other.id = CASE
            WHEN r.src_memory_id = $1 THEN r.dst_memory_id
            ELSE r.src_memory_id
        END
        WHERE (r.src_memory_id = $1 OR r.dst_memory_id = $1)
        ORDER BY r.relation_type, other.summary
        LIMIT 20
        "#,
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .context("fetch related memories for validation")
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PriorValidationRun {
    pub verdict: Option<String>,
    pub confidence: Option<f32>,
    pub action: Option<String>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub async fn fetch_prior_validation_runs(
    pool: &PgPool,
    canonical_id: Uuid,
    limit: i64,
) -> Result<Vec<PriorValidationRun>> {
    sqlx::query_as::<_, PriorValidationRun>(
        r#"
        SELECT verdict, confidence, action, finished_at
        FROM memory_validation_runs
        WHERE canonical_id = $1 AND status = 'completed'
        ORDER BY started_at DESC
        LIMIT $2
        "#,
    )
    .bind(canonical_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("fetch prior validation runs")
}

/// Inserts a validation run in `running` state and returns its id.
pub async fn insert_validation_run(
    pool: &PgPool,
    canonical_id: Uuid,
    memory_id: Uuid,
    project_id: Uuid,
    trigger: &str,
    dry_run: bool,
) -> Result<Uuid> {
    let run_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_validation_runs
            (id, canonical_id, memory_id, project_id, trigger_kind, status, dry_run)
        VALUES ($1, $2, $3, $4, $5, 'running', $6)
        "#,
    )
    .bind(run_id)
    .bind(canonical_id)
    .bind(memory_id)
    .bind(project_id)
    .bind(trigger)
    .bind(dry_run)
    .execute(pool)
    .await
    .context("insert validation run")?;
    Ok(run_id)
}

/// Terminal update for a completed run.
#[derive(Debug, Clone)]
pub struct ValidationRunCompletion {
    pub verdict: &'static str,
    pub confidence: f32,
    pub action: String,
    pub reasons: serde_json::Value,
    pub proposed_candidate_json: Option<serde_json::Value>,
    pub review_status: Option<&'static str>,
    pub model: Option<String>,
    pub details: serde_json::Value,
}

pub async fn complete_validation_run(
    pool: &PgPool,
    run_id: Uuid,
    completion: &ValidationRunCompletion,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE memory_validation_runs SET
            status = 'completed', verdict = $2, confidence = $3, action = $4,
            reasons_json = $5, proposed_candidate_json = $6, review_status = $7,
            model = $8, details_json = $9, finished_at = now()
        WHERE id = $1
        "#,
    )
    .bind(run_id)
    .bind(completion.verdict)
    .bind(completion.confidence)
    .bind(&completion.action)
    .bind(&completion.reasons)
    .bind(&completion.proposed_candidate_json)
    .bind(completion.review_status)
    .bind(&completion.model)
    .bind(&completion.details)
    .execute(pool)
    .await
    .context("complete validation run")?;
    Ok(())
}

pub async fn fail_validation_run(pool: &PgPool, run_id: Uuid, error: &str) -> Result<()> {
    sqlx::query(
        "UPDATE memory_validation_runs SET status = 'failed', error = $2, finished_at = now() WHERE id = $1",
    )
    .bind(run_id)
    .bind(error)
    .execute(pool)
    .await
    .context("mark validation run failed")?;
    Ok(())
}

/// One stance-annotated evidence row for a validation run.
#[derive(Debug, Clone)]
pub struct EvidenceRow {
    pub kind: String,
    pub evidence_ref: String,
    pub stance: String,
    pub excerpt: Option<String>,
}

pub async fn insert_validation_evidence(
    pool: &PgPool,
    run_id: Uuid,
    rows: &[EvidenceRow],
) -> Result<()> {
    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO memory_validation_evidence
                (id, validation_run_id, kind, evidence_ref, stance, excerpt)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(&row.kind)
        .bind(&row.evidence_ref)
        .bind(&row.stance)
        .bind(&row.excerpt)
        .execute(pool)
        .await
        .context("insert validation evidence row")?;
    }
    Ok(())
}

/// Marks a memory revalidated: fresh validation metadata plus cooldown.
pub async fn mark_validated(
    pool: &PgPool,
    canonical_id: Uuid,
    confidence: f32,
    run_id: Uuid,
    cooldown_until: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE memory_scores SET
            validated_at = now(), validation_confidence = $2,
            last_validation_id = $3, validation_cooldown_until = $4,
            needs_review = FALSE, needs_review_reason = NULL, updated_at = now()
        WHERE canonical_id = $1
        "#,
    )
    .bind(canonical_id)
    .bind(confidence)
    .bind(run_id)
    .bind(cooldown_until)
    .execute(pool)
    .await
    .context("mark memory validated")?;
    Ok(())
}

/// Flags a memory for human review; content is never touched.
pub async fn mark_needs_review(
    pool: &PgPool,
    canonical_id: Uuid,
    reason: &str,
    run_id: Uuid,
    cooldown_until: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE memory_scores SET
            needs_review = TRUE, needs_review_reason = $2,
            last_validation_id = $3, validation_cooldown_until = $4,
            updated_at = now()
        WHERE canonical_id = $1
        "#,
    )
    .bind(canonical_id)
    .bind(reason)
    .bind(run_id)
    .bind(cooldown_until)
    .execute(pool)
    .await
    .context("mark memory needs_review")?;
    Ok(())
}

/// Records that a correction is pending review; optionally stamps
/// `last_invalidated_at` when the verdict found the content no longer
/// accurate.
pub async fn mark_correction_pending(
    pool: &PgPool,
    canonical_id: Uuid,
    run_id: Uuid,
    cooldown_until: DateTime<Utc>,
    invalidated: bool,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE memory_scores SET
            last_validation_id = $2, validation_cooldown_until = $3,
            last_invalidated_at = CASE WHEN $4 THEN now() ELSE last_invalidated_at END,
            updated_at = now()
        WHERE canonical_id = $1
        "#,
    )
    .bind(canonical_id)
    .bind(run_id)
    .bind(cooldown_until)
    .bind(invalidated)
    .execute(pool)
    .await
    .context("mark memory correction pending")?;
    Ok(())
}

/// Failure-path cooldown so a broken provider cannot retry-loop a memory.
pub async fn set_validation_cooldown(
    pool: &PgPool,
    canonical_id: Uuid,
    cooldown_until: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "UPDATE memory_scores SET validation_cooldown_until = $2, updated_at = now() WHERE canonical_id = $1",
    )
    .bind(canonical_id)
    .bind(cooldown_until)
    .execute(pool)
    .await
    .context("set validation cooldown")?;
    Ok(())
}

/// Reads current score state for a set of canonicals (test/inspection
/// helper and status surface).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScoreRow {
    pub canonical_id: Uuid,
    pub project_id: Uuid,
    pub activation: f64,
    pub last_decay_at: DateTime<Utc>,
    pub last_access_at: Option<DateTime<Utc>>,
    pub access_count: i64,
    pub citation_count: i64,
    pub propagated_count: i64,
    pub volatility: f32,
    pub validated_at: Option<DateTime<Utc>>,
    pub validation_confidence: Option<f32>,
    pub needs_review: bool,
    pub needs_review_reason: Option<String>,
    pub last_invalidated_at: Option<DateTime<Utc>>,
    pub validation_cooldown_until: Option<DateTime<Utc>>,
}

pub async fn fetch_scores(pool: &PgPool, canonical_ids: &[Uuid]) -> Result<Vec<ScoreRow>> {
    if canonical_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as::<_, ScoreRow>(
        r#"
        SELECT canonical_id, project_id, activation, last_decay_at,
               last_access_at, access_count, citation_count, propagated_count,
               volatility, validated_at, validation_confidence, needs_review,
               needs_review_reason, last_invalidated_at, validation_cooldown_until
        FROM memory_scores
        WHERE canonical_id = ANY($1)
        "#,
    )
    .bind(canonical_ids)
    .fetch_all(pool)
    .await
    .context("fetch memory score rows")
}
