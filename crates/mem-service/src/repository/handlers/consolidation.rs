//! Repository layer for memory consolidation: pulls the three edge channels
//! and per-memory salience out of PostgreSQL, runs the deterministic
//! clustering + value gate from `mem_consolidate`, and returns the accepted
//! clusters. LLM synthesis and proposal emission live in `llm/consolidation.rs`
//! and the proposal apply path; this module is deterministic and pool-only so
//! the control-plane loop can call it without an `AppState`.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use mem_api::ConsolidationConfig;
use mem_consolidate::{
    DetectParams, FuseWeights, GateOutcome, MemberStat, TriggerReason, ValueGateConfig,
    detect_communities, evaluate_cluster, fuse_edges,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::ApiError;

/// Fallback activation half-life (30 days) when no reinforcement config is
/// supplied by the caller.
const DEFAULT_HALF_LIFE_SECS: f64 = 30.0 * 86_400.0;

/// One active memory considered for clustering.
#[derive(Debug, Clone)]
struct MemoryRow {
    canonical_id: Uuid,
    summary: String,
    canonical_text: String,
    activation: f64,
}

/// Bundle of everything clustering needs for one project.
struct ConsolidationInputs {
    memories: Vec<MemoryRow>,
    relations: Vec<(Uuid, Uuid)>,
    similarities: Vec<(Uuid, Uuid, f64)>,
    coaccess: Vec<(Uuid, Uuid, u32)>,
    /// canonical id of each existing insight -> the member canonical ids it summarizes.
    insight_coverage: Vec<BTreeSet<Uuid>>,
}

/// A member of an accepted cluster, carried into synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClusterMember {
    pub canonical_id: Uuid,
    pub summary: String,
    pub canonical_text: String,
    pub activation: f64,
}

/// One cluster that passed the value gate and novelty check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AcceptedCluster {
    pub members: Vec<ClusterMember>,
    pub size: usize,
    pub intra_density: f64,
    pub coaccess_mass: f64,
    pub activation_mass: f64,
    pub trigger: String,
}

/// The deterministic clustering result stored in the loop run and surfaced to
/// the accumulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConsolidationReport {
    pub project: String,
    pub candidate_count: usize,
    pub accepted: Vec<AcceptedCluster>,
    pub rejected_count: usize,
    pub covered_skipped: usize,
}

impl ConsolidationReport {
    pub(crate) fn summary(&self) -> String {
        format!(
            "Consolidation scanned {} candidate cluster(s): {} accepted, {} rejected, {} already covered.",
            self.candidate_count,
            self.accepted.len(),
            self.rejected_count,
            self.covered_skipped
        )
    }
}

/// Runs the full deterministic consolidation scan for one project.
pub(crate) async fn run_memory_consolidation(
    pool: &PgPool,
    project: &str,
    cfg: &ConsolidationConfig,
    half_life_secs: f64,
) -> Result<ConsolidationReport, ApiError> {
    let project_id = match fetch_project_id(pool, project).await? {
        Some(id) => id,
        None => {
            return Ok(ConsolidationReport {
                project: project.to_string(),
                candidate_count: 0,
                accepted: Vec::new(),
                rejected_count: 0,
                covered_skipped: 0,
            });
        }
    };

    let inputs = fetch_consolidation_inputs(pool, project, project_id, cfg, half_life_secs).await?;

    // Per-canonical lookup for member enrichment and salience.
    let by_id: BTreeMap<Uuid, MemoryRow> = inputs
        .memories
        .iter()
        .map(|m| (m.canonical_id, m.clone()))
        .collect();

    // Co-access mass incident on each canonical, for the value gate.
    let mut coaccess_mass: BTreeMap<Uuid, f64> = BTreeMap::new();
    for &(a, b, count) in &inputs.coaccess {
        *coaccess_mass.entry(a).or_default() += count as f64;
        *coaccess_mass.entry(b).or_default() += count as f64;
    }

    let weights = FuseWeights {
        relation: cfg.relation_weight,
        similarity: cfg.similarity_weight,
        coaccess: cfg.coaccess_weight,
        sim_floor: cfg.sim_floor,
        coaccess_norm: cfg.coaccess_norm,
    };
    let edges = fuse_edges(&inputs.relations, &inputs.similarities, &inputs.coaccess, &weights);
    let graph = mem_consolidate::FusedGraph::from_edges(edges);
    let communities = detect_communities(&graph, &DetectParams::default());

    let gate = ValueGateConfig {
        min_size: cfg.min_size,
        max_size: cfg.max_size,
        min_cohesion: cfg.min_cohesion,
        min_salience: cfg.min_salience,
        cold_activation_max: cfg.cold_activation_max,
    };

    let mut accepted = Vec::new();
    let mut rejected_count = 0usize;
    let mut covered_skipped = 0usize;
    let candidate_count = communities.len();

    for community in communities {
        let stats: Vec<MemberStat> = community
            .members
            .iter()
            .map(|id| MemberStat {
                canonical_id: *id,
                activation: by_id.get(id).map(|m| m.activation).unwrap_or(0.0),
                coaccess_mass: coaccess_mass.get(id).copied().unwrap_or(0.0),
            })
            .collect();
        let (metrics, outcome) = evaluate_cluster(&stats, &graph, &gate);
        let trigger = match outcome {
            GateOutcome::Accept(reason) => reason,
            GateOutcome::Reject(_) => {
                rejected_count += 1;
                continue;
            }
        };

        let member_set: BTreeSet<Uuid> = community.members.iter().copied().collect();
        if is_covered(&member_set, &inputs.insight_coverage, cfg.novelty_overlap_max) {
            covered_skipped += 1;
            continue;
        }

        let members: Vec<ClusterMember> = community
            .members
            .iter()
            .filter_map(|id| by_id.get(id))
            .map(|m| ClusterMember {
                canonical_id: m.canonical_id,
                summary: m.summary.clone(),
                canonical_text: m.canonical_text.clone(),
                activation: m.activation,
            })
            .collect();

        accepted.push(AcceptedCluster {
            members,
            size: metrics.size,
            intra_density: metrics.intra_density,
            coaccess_mass: metrics.coaccess_mass,
            activation_mass: metrics.activation_mass,
            trigger: match trigger {
                TriggerReason::Salient => "salient".to_string(),
                TriggerReason::ColdDense => "cold_dense".to_string(),
            },
        });
    }

    Ok(ConsolidationReport {
        project: project.to_string(),
        candidate_count,
        accepted,
        rejected_count,
        covered_skipped,
    })
}

/// True when an existing insight already summarizes at least `overlap_max` of
/// this cluster's members (novelty gate).
fn is_covered(members: &BTreeSet<Uuid>, coverage: &[BTreeSet<Uuid>], overlap_max: f64) -> bool {
    if members.is_empty() {
        return true;
    }
    coverage.iter().any(|covered| {
        let shared = members.intersection(covered).count();
        shared as f64 / members.len() as f64 >= overlap_max
    })
}

async fn fetch_project_id(pool: &PgPool, project: &str) -> Result<Option<Uuid>, ApiError> {
    let row = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(project)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?;
    row.map(|row| row.try_get::<Uuid, _>("id"))
        .transpose()
        .map_err(ApiError::sql)
}

async fn fetch_consolidation_inputs(
    pool: &PgPool,
    project: &str,
    project_id: Uuid,
    cfg: &ConsolidationConfig,
    half_life_secs: f64,
) -> Result<ConsolidationInputs, ApiError> {
    let memories = fetch_active_memories(pool, project_id, half_life_secs).await?;
    let ids: Vec<Uuid> = memories.iter().map(|m| m.canonical_id).collect();

    let relations = mem_reinforce::repository::fetch_canonical_edges(pool, &ids, 1)
        .await
        .map_err(ApiError::io)?
        .into_iter()
        .map(|edge| (edge.a, edge.b))
        .collect();

    let similarities = fetch_similarity_edges(pool, project_id, cfg.knn_k, cfg.sim_floor).await?;
    let since = Utc::now() - Duration::days(cfg.coaccess_window_days.max(0));
    let coaccess = fetch_coaccess_edges(pool, project_id, since, cfg.min_coaccess_count).await?;
    let insight_coverage = fetch_insight_coverage(pool, project).await?;

    Ok(ConsolidationInputs { memories, relations, similarities, coaccess, insight_coverage })
}

/// Latest active version of every canonical memory, with decay-adjusted
/// activation (0 for never-accessed memories via LEFT JOIN).
async fn fetch_active_memories(
    pool: &PgPool,
    project_id: Uuid,
    half_life_secs: f64,
) -> Result<Vec<MemoryRow>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT
            m.canonical_id,
            m.summary,
            m.canonical_text,
            COALESCE(
                s.activation * power(
                    0.5,
                    GREATEST(EXTRACT(EPOCH FROM (now() - s.last_decay_at)), 0) / $2
                ),
                0
            )::float8 AS activation
        FROM memory_entries m
        JOIN LATERAL (
            SELECT id FROM memory_entries e
            WHERE e.canonical_id = m.canonical_id
              AND e.status = 'active'
              AND COALESCE(e.is_tombstone, false) = false
            ORDER BY e.version_no DESC
            LIMIT 1
        ) latest ON latest.id = m.id
        LEFT JOIN memory_scores s ON s.canonical_id = m.canonical_id
        WHERE m.project_id = $1
          AND m.status = 'active'
          AND COALESCE(m.is_tombstone, false) = false
        "#,
    )
    .bind(project_id)
    .bind(half_life_secs.max(1.0))
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;

    rows.into_iter()
        .map(|row| {
            Ok(MemoryRow {
                canonical_id: row.try_get("canonical_id")?,
                summary: row.try_get("summary")?,
                canonical_text: row.try_get("canonical_text")?,
                activation: row.try_get("activation")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(ApiError::sql)
}

/// kNN semantic-similarity edges over chunk embeddings, aggregated to canonical
/// pairs by max cosine. Vectors never leave PostgreSQL (all math is `<=>`).
pub(crate) async fn fetch_similarity_edges(
    pool: &PgPool,
    project_id: Uuid,
    k: i64,
    min_cosine: f64,
) -> Result<Vec<(Uuid, Uuid, f64)>, ApiError> {
    let rows = sqlx::query(
        r#"
        WITH nn AS (
            SELECT
                me.canonical_id AS a,
                other.canonical_id AS b,
                1 - (ce.embedding <=> oe.embedding) AS cosine
            FROM memory_chunks mc
            JOIN memory_chunk_embeddings ce ON ce.chunk_id = mc.id
            JOIN memory_entries me ON me.id = mc.memory_entry_id
            JOIN LATERAL (
                SELECT oc.memory_entry_id, oe.embedding
                FROM memory_chunk_embeddings oe
                JOIN memory_chunks oc ON oc.id = oe.chunk_id
                JOIN memory_entries oe_me ON oe_me.id = oc.memory_entry_id
                WHERE oe.embedding_space = ce.embedding_space
                  AND COALESCE(oe.embedding_dimension, 0) = COALESCE(ce.embedding_dimension, 0)
                  AND oe_me.project_id = $1
                  AND oe_me.status = 'active'
                  AND oe_me.canonical_id <> me.canonical_id
                ORDER BY oe.embedding <=> ce.embedding
                LIMIT $2
            ) oe ON true
            JOIN memory_entries other ON other.id = oe.memory_entry_id
            WHERE me.project_id = $1
              AND me.status = 'active'
        )
        SELECT
            LEAST(a, b) AS lo,
            GREATEST(a, b) AS hi,
            MAX(cosine) AS cosine
        FROM nn
        GROUP BY LEAST(a, b), GREATEST(a, b)
        HAVING MAX(cosine) >= $3
        "#,
    )
    .bind(project_id)
    .bind(k.max(1))
    .bind(min_cosine)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;

    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get::<Uuid, _>("lo")?,
                row.try_get::<Uuid, _>("hi")?,
                row.try_get::<f64, _>("cosine")?,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(ApiError::sql)
}

/// Co-access edges: memories that appeared in the same real query operation.
/// Excludes the legacy constant operation ids and single-memory direct reads.
pub(crate) async fn fetch_coaccess_edges(
    pool: &PgPool,
    project_id: Uuid,
    since: DateTime<Utc>,
    min_count: i64,
) -> Result<Vec<(Uuid, Uuid, u32)>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT
            a.canonical_id AS lo,
            b.canonical_id AS hi,
            COUNT(DISTINCT a.operation_id) AS pair_count
        FROM memory_access_events a
        JOIN memory_access_events b
          ON a.operation_id = b.operation_id
         AND a.canonical_id < b.canonical_id
        WHERE a.project_id = $1
          AND b.project_id = $1
          AND a.accessed_at >= $2
          AND a.operation_id IS NOT NULL
          AND a.operation_id NOT IN ('query', 'direct_read')
          AND a.operation_id NOT LIKE 'direct_read:%'
        GROUP BY a.canonical_id, b.canonical_id
        HAVING COUNT(DISTINCT a.operation_id) >= $3
        "#,
    )
    .bind(project_id)
    .bind(since)
    .bind(min_count.max(1))
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;

    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get::<Uuid, _>("lo")?,
                row.try_get::<Uuid, _>("hi")?,
                row.try_get::<i64, _>("pair_count")? as u32,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(ApiError::sql)
}

/// For each existing `insight` memory, the set of member canonical ids it
/// `summarizes` (used for the novelty gate). Relations key on version ids, so
/// both ends are resolved to canonical ids.
async fn fetch_insight_coverage(
    pool: &PgPool,
    project: &str,
) -> Result<Vec<BTreeSet<Uuid>>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT src.canonical_id AS insight_id, dst.canonical_id AS member_id
        FROM memory_relations rel
        JOIN memory_entries src ON src.id = rel.src_memory_id
        JOIN memory_entries dst ON dst.id = rel.dst_memory_id
        JOIN projects p ON p.id = src.project_id
        WHERE rel.relation_type = 'summarizes'
          AND p.slug = $1
          AND src.memory_type = 'insight'
          AND src.status = 'active'
        "#,
    )
    .bind(project)
    .fetch_all(pool)
    .await
    .map_err(ApiError::sql)?;

    let mut coverage: BTreeMap<Uuid, BTreeSet<Uuid>> = BTreeMap::new();
    for row in rows {
        let insight_id: Uuid = row.try_get("insight_id").map_err(ApiError::sql)?;
        let member_id: Uuid = row.try_get("member_id").map_err(ApiError::sql)?;
        coverage.entry(insight_id).or_default().insert(member_id);
    }
    Ok(coverage.into_values().collect())
}

/// Convenience wrapper that runs consolidation with the default half-life when
/// the caller has no reinforcement config in scope.
pub(crate) async fn run_memory_consolidation_default(
    pool: &PgPool,
    project: &str,
    cfg: &ConsolidationConfig,
) -> Result<ConsolidationReport, ApiError> {
    run_memory_consolidation(pool, project, cfg, DEFAULT_HALF_LIFE_SECS).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seed_project(pool: &PgPool, slug: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, slug, name, root_path, created_at) VALUES ($1, $2, $2, $2, now())",
        )
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert project");
        id
    }

    async fn seed_memory(pool: &PgPool, project_id: Uuid, summary: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO memory_entries
                (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
                 summary, memory_type, scope, importance, confidence, status,
                 created_at, updated_at, search_document)
            VALUES ($1, $2, $1, 1, false, $3, $3, 'reference', 'project', 3, 0.9,
                    'active', now(), now(), to_tsvector('english', $3))
            "#,
        )
        .bind(id)
        .bind(project_id)
        .bind(summary)
        .execute(pool)
        .await
        .expect("insert memory");
        id
    }

    async fn seed_relation(pool: &PgPool, src: Uuid, dst: Uuid) {
        sqlx::query(
            "INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id) VALUES (gen_random_uuid(), $1, 'related_to', $2) ON CONFLICT DO NOTHING",
        )
        .bind(src)
        .bind(dst)
        .execute(pool)
        .await
        .expect("insert relation");
    }

    async fn seed_coaccess(pool: &PgPool, project_id: Uuid, members: &[Uuid], operation_id: &str) {
        for &member in members {
            sqlx::query(
                r#"
                INSERT INTO memory_access_events
                    (canonical_id, project_id, accessed_at, kind, boost, hop_distance, operation_id)
                VALUES ($1, $2, now(), 'retrieval', 1.0, 0, $3)
                "#,
            )
            .bind(member)
            .bind(project_id)
            .bind(operation_id)
            .execute(pool)
            .await
            .expect("insert access event");
        }
    }

    #[tokio::test]
    async fn discovers_two_clusters_from_relations_and_coaccess() {
        let Some(pool) = mem_test_support::migrated_pool().await else {
            return;
        };
        let slug = mem_test_support::unique_project_slug("consolidate-db");
        mem_test_support::cleanup_project(&pool, &slug)
            .await
            .expect("cleanup old project");
        let project_id = seed_project(&pool, &slug).await;

        // Two disjoint triangles.
        let mut a = Vec::new();
        for i in 0..3 {
            a.push(seed_memory(&pool, project_id, &format!("cluster A fact {i}")).await);
        }
        let mut b = Vec::new();
        for i in 0..3 {
            b.push(seed_memory(&pool, project_id, &format!("cluster B fact {i}")).await);
        }
        for cluster in [&a, &b] {
            seed_relation(&pool, cluster[0], cluster[1]).await;
            seed_relation(&pool, cluster[1], cluster[2]).await;
            seed_relation(&pool, cluster[0], cluster[2]).await;
        }
        // Two distinct co-access operations per cluster make each cluster salient
        // and satisfy the min-distinct-operations threshold.
        seed_coaccess(&pool, project_id, &a, "op-a-1").await;
        seed_coaccess(&pool, project_id, &a, "op-a-2").await;
        seed_coaccess(&pool, project_id, &b, "op-b-1").await;
        seed_coaccess(&pool, project_id, &b, "op-b-2").await;

        let cfg = ConsolidationConfig::default();
        let report = run_memory_consolidation_default(&pool, &slug, &cfg)
            .await
            .expect("run consolidation");

        assert_eq!(report.accepted.len(), 2, "expected two accepted clusters");
        for cluster in &report.accepted {
            assert_eq!(cluster.size, 3);
            assert_eq!(cluster.trigger, "salient");
            assert!(cluster.intra_density >= 0.99);
        }

        mem_test_support::cleanup_project(&pool, &slug)
            .await
            .expect("cleanup project");
    }
}
