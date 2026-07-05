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
