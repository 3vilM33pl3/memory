use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
};

use super::*;
use mem_api::{
    CodeGraphEdge, CodeGraphNode, CodeGraphResponse, CodeGraphStats, CodeGraphStatusResponse,
    CodeGraphViewFilters,
};
use serde_json::json;
use sqlx::Row;

pub struct PostgresGraphRepository {
    pool: PgPool,
}

impl PostgresGraphRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn extract(&self, request: GraphExtractionRequest) -> Result<GraphExtractionReport> {
        let resolved = resolve_analysis(&request.analysis);
        if request.dry_run {
            return Ok(report_from_resolved(&request, &resolved, None, false));
        }

        let project_id = upsert_project(&self.pool, &request.project, &request.repo_root).await?;
        if !request.force
            && let Some(existing_run_id) = find_existing_completed_run(
                &self.pool,
                project_id,
                &request,
                &resolved.resolution_strategy_version,
            )
            .await?
        {
            let mut report = self
                .status_for_run(&request.project, existing_run_id)
                .await?
                .map(|status| GraphExtractionReport {
                    project: status.project,
                    repo_root: status.repo_root,
                    git_head: status.git_head,
                    since: status.since,
                    analyzer_version: status.analyzer_version,
                    strategy_version: status.strategy_version,
                    extraction_run_id: Some(status.extraction_run_id),
                    reused_existing_run: true,
                    dry_run: false,
                    index_reused: request.index_reused,
                    symbol_count: status.symbol_count as usize,
                    reference_count: status.reference_count as usize,
                    resolved_reference_count: status.resolved_reference_count as usize,
                    unresolved_reference_count: status.unresolved_reference_count as usize,
                    ambiguous_reference_count: status.ambiguous_reference_count as usize,
                    graph_node_count: status.graph_node_count as usize,
                    graph_edge_count: status.graph_edge_count as usize,
                    evidence_count: status.evidence_count as usize,
                    sample_unresolved_references: Vec::new(),
                })
                .context("existing graph extraction run disappeared")?;
            report.sample_unresolved_references = unresolved_previews(&resolved);
            return Ok(report);
        }

        let run_id = Uuid::new_v4();
        let mut tx = self.pool.begin().await.context("begin graph transaction")?;
        sqlx::query(
            r#"
            INSERT INTO graph_extraction_runs
                (id, project_id, repo_root, git_head, since_marker, analyzer_version,
                 strategy_version, status, started_at, summary_json)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'running', now(), '{}'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(project_id)
        .bind(&request.repo_root)
        .bind(&request.git_head)
        .bind(&request.since)
        .bind(&resolved.analyzer_version)
        .bind(&resolved.resolution_strategy_version)
        .execute(&mut *tx)
        .await
        .context("insert graph extraction run")?;

        let mut node_ids = BTreeMap::new();
        for symbol in &resolved.symbols {
            let node_id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO graph_nodes
                    (id, project_id, extraction_run_id, node_kind, stable_identity,
                     display_name, metadata_json, created_at)
                VALUES ($1, $2, $3, 'code_symbol', $4, $5, $6, now())
                "#,
            )
            .bind(node_id)
            .bind(project_id)
            .bind(run_id)
            .bind(&symbol.stable_identity)
            .bind(&symbol.display)
            .bind(json!({
                "language": symbol.language.as_str(),
                "file_path": symbol.file_path,
                "symbol_kind": symbol.kind.as_str(),
                "qualified_name": symbol.qualified_name,
                "fact_id": symbol.fact_id,
            }))
            .execute(&mut *tx)
            .await
            .context("insert graph node")?;

            sqlx::query(
                r#"
                INSERT INTO code_symbols
                    (id, project_id, extraction_run_id, graph_node_id, fact_id,
                     stable_identity, language, file_path, symbol_kind, name,
                     qualified_name, start_byte, end_byte, start_line, end_line,
                     display_name, source_hash, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                        $12, $13, $14, $15, $16, $17, now())
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(project_id)
            .bind(run_id)
            .bind(node_id)
            .bind(&symbol.fact_id)
            .bind(&symbol.stable_identity)
            .bind(symbol.language.as_str())
            .bind(&symbol.file_path)
            .bind(symbol.kind.as_str())
            .bind(&symbol.name)
            .bind(&symbol.qualified_name)
            .bind(symbol.span.start_byte as i64)
            .bind(symbol.span.end_byte as i64)
            .bind(symbol.span.start_line as i64)
            .bind(symbol.span.end_line as i64)
            .bind(&symbol.display)
            .bind(&symbol.source_hash)
            .execute(&mut *tx)
            .await
            .context("insert code symbol")?;

            insert_evidence(
                &mut tx,
                EvidenceInput {
                    project_id,
                    run_id,
                    node_id: Some(node_id),
                    edge_id: None,
                    file_path: &symbol.file_path,
                    start_line: symbol.span.start_line as i64,
                    end_line: symbol.span.end_line as i64,
                    confidence: 1.0,
                    strategy_version: &resolved.resolution_strategy_version,
                },
            )
            .await?;
            node_ids.insert(symbol.stable_identity.clone(), node_id);
        }

        let mut edge_count = 0usize;
        for reference in &resolved.references {
            let source_node_id = reference
                .source_symbol_identity
                .as_ref()
                .and_then(|identity| node_ids.get(identity))
                .copied();
            let target_node_id = reference
                .target_symbol_identity
                .as_ref()
                .and_then(|identity| node_ids.get(identity))
                .copied();
            let edge_id = if reference.resolution_status == ResolutionStatus::Resolved {
                match (source_node_id, target_node_id) {
                    (Some(source), Some(target)) => {
                        let edge_id = Uuid::new_v4();
                        sqlx::query(
                            r#"
                            INSERT INTO graph_edges
                                (id, project_id, extraction_run_id, source_node_id, target_node_id,
                                 edge_kind, confidence, metadata_json, created_at)
                            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
                            "#,
                        )
                        .bind(edge_id)
                        .bind(project_id)
                        .bind(run_id)
                        .bind(source)
                        .bind(target)
                        .bind(&reference.graph_edge_kind)
                        .bind(reference.confidence)
                        .bind(json!({
                            "reference_kind": reference.kind.as_str(),
                            "fact_id": reference.fact_id,
                            "target_text": reference.target_text,
                        }))
                        .execute(&mut *tx)
                        .await
                        .context("insert graph edge")?;
                        insert_evidence(
                            &mut tx,
                            EvidenceInput {
                                project_id,
                                run_id,
                                node_id: None,
                                edge_id: Some(edge_id),
                                file_path: &reference.file_path,
                                start_line: reference.span.start_line as i64,
                                end_line: reference.span.end_line as i64,
                                confidence: reference.confidence,
                                strategy_version: &resolved.resolution_strategy_version,
                            },
                        )
                        .await?;
                        edge_count += 1;
                        Some(edge_id)
                    }
                    _ => None,
                }
            } else {
                None
            };

            sqlx::query(
                r#"
                INSERT INTO code_references
                    (id, project_id, extraction_run_id, graph_edge_id, fact_id, reference_kind,
                     language, file_path, source_symbol_identity, target_symbol_identity,
                     source_text, target_text, resolution_status, confidence,
                     start_byte, end_byte, start_line, end_line, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                        $11, $12, $13, $14, $15, $16, $17, $18, now())
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(project_id)
            .bind(run_id)
            .bind(edge_id)
            .bind(&reference.fact_id)
            .bind(reference.kind.as_str())
            .bind(reference.language.as_str())
            .bind(&reference.file_path)
            .bind(&reference.source_symbol_identity)
            .bind(&reference.target_symbol_identity)
            .bind(&reference.source_text)
            .bind(&reference.target_text)
            .bind(reference.resolution_status.as_str())
            .bind(reference.confidence)
            .bind(reference.span.start_byte as i64)
            .bind(reference.span.end_byte as i64)
            .bind(reference.span.start_line as i64)
            .bind(reference.span.end_line as i64)
            .execute(&mut *tx)
            .await
            .context("insert code reference")?;
        }

        let report = report_from_resolved(&request, &resolved, Some(run_id), false);
        sqlx::query(
            r#"
            UPDATE graph_extraction_runs
            SET status = 'completed', completed_at = now(), summary_json = $2
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(json!({
            "symbol_count": report.symbol_count,
            "reference_count": report.reference_count,
            "graph_node_count": report.graph_node_count,
            "graph_edge_count": edge_count,
            "evidence_count": report.graph_node_count + edge_count,
        }))
        .execute(&mut *tx)
        .await
        .context("complete graph extraction run")?;
        tx.commit().await.context("commit graph transaction")?;

        Ok(GraphExtractionReport {
            graph_edge_count: edge_count,
            evidence_count: report.graph_node_count + edge_count,
            ..report
        })
    }

    pub async fn latest_status(&self, project: &str) -> Result<Option<GraphStatusReport>> {
        let row = sqlx::query(
            r#"
            SELECT ger.id
            FROM graph_extraction_runs ger
            JOIN projects p ON p.id = ger.project_id
            WHERE p.slug = $1 AND ger.status = 'completed'
            ORDER BY ger.completed_at DESC NULLS LAST, ger.started_at DESC
            LIMIT 1
            "#,
        )
        .bind(project)
        .fetch_optional(&self.pool)
        .await
        .context("read latest graph extraction run")?;
        let Some(row) = row else {
            return Ok(None);
        };
        let run_id: Uuid = row.try_get("id")?;
        self.status_for_run(project, run_id).await
    }

    pub async fn status_response(&self, project: &str) -> Result<CodeGraphStatusResponse> {
        Ok(self
            .latest_status(project)
            .await?
            .map(|status| status_to_response(&status))
            .unwrap_or_else(|| CodeGraphStatusResponse::empty(project)))
    }

    pub async fn visualization_graph(
        &self,
        project: &str,
        mut filters: CodeGraphViewFilters,
    ) -> Result<CodeGraphResponse> {
        let status = if let Some(run_id) = filters.run_id {
            self.status_for_run(project, run_id).await?
        } else {
            self.latest_status(project).await?
        };
        let Some(status) = status else {
            return Ok(empty_graph_response(project, filters));
        };

        filters.run_id = Some(status.extraction_run_id);
        let status_response = status_to_response(&status);
        let mut truncation_reasons = Vec::new();
        let mut seed_ids = self
            .fetch_seed_node_ids(status.extraction_run_id, &filters)
            .await?;
        if seed_ids.len() > filters.limit_nodes {
            seed_ids.truncate(filters.limit_nodes);
            truncation_reasons.push(format!("seed nodes were capped at {}", filters.limit_nodes));
        }

        let seed_set: BTreeSet<Uuid> = seed_ids.iter().copied().collect();
        let mut node_ids = seed_set.clone();
        let mut frontier = seed_ids;
        let mut edges_by_id = BTreeMap::new();

        for _ in 0..filters.depth {
            if frontier.is_empty() || edges_by_id.len() >= filters.limit_edges {
                break;
            }
            let remaining_edges = filters.limit_edges.saturating_sub(edges_by_id.len());
            let edge_rows = self
                .fetch_edges_for_frontier(
                    status.extraction_run_id,
                    &frontier,
                    filters.edge_kind.as_deref(),
                    remaining_edges + 1,
                )
                .await?;
            if edge_rows.len() > remaining_edges {
                truncation_reasons.push(format!("edges were capped at {}", filters.limit_edges));
            }

            let mut next_frontier = BTreeSet::new();
            for edge in edge_rows.into_iter().take(remaining_edges) {
                if node_ids.insert(edge.source) {
                    next_frontier.insert(edge.source);
                }
                if node_ids.insert(edge.target) {
                    next_frontier.insert(edge.target);
                }
                edges_by_id.entry(edge.id).or_insert(edge);
            }
            frontier = next_frontier.into_iter().collect();
        }

        let mut nodes = self
            .fetch_graph_nodes(status.extraction_run_id, &node_ids, &seed_set)
            .await?;
        nodes.sort_by(|left, right| {
            right
                .seed
                .cmp(&left.seed)
                .then_with(|| right.degree.cmp(&left.degree))
                .then_with(|| left.file_path.cmp(&right.file_path))
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.label.cmp(&right.label))
        });
        if nodes.len() > filters.limit_nodes {
            nodes.truncate(filters.limit_nodes);
            truncation_reasons.push(format!("nodes were capped at {}", filters.limit_nodes));
        }

        let returned_node_ids: BTreeSet<Uuid> = nodes.iter().map(|node| node.id).collect();
        let mut edges: Vec<CodeGraphEdge> = edges_by_id
            .into_values()
            .filter(|edge| {
                returned_node_ids.contains(&edge.source) && returned_node_ids.contains(&edge.target)
            })
            .collect();
        edges.sort_by(|left, right| {
            left.edge_kind
                .cmp(&right.edge_kind)
                .then_with(|| left.file_path.cmp(&right.file_path))
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.id.cmp(&right.id))
        });
        if edges.len() > filters.limit_edges {
            edges.truncate(filters.limit_edges);
            truncation_reasons.push(format!("edges were capped at {}", filters.limit_edges));
        }

        let stats = CodeGraphStats {
            total_nodes: status.graph_node_count,
            total_edges: status.graph_edge_count,
            total_symbols: status.symbol_count,
            total_references: status.reference_count,
            unresolved_references: status.unresolved_reference_count,
            returned_nodes: nodes.len(),
            returned_edges: edges.len(),
            seed_nodes: seed_set.len(),
        };
        Ok(CodeGraphResponse {
            project: project.to_string(),
            status: status_response,
            filters,
            stats,
            truncated: !truncation_reasons.is_empty(),
            truncation_reason: if truncation_reasons.is_empty() {
                None
            } else {
                Some(truncation_reasons.join("; "))
            },
            nodes,
            edges,
        })
    }

    async fn status_for_run(
        &self,
        project: &str,
        run_id: Uuid,
    ) -> Result<Option<GraphStatusReport>> {
        let row = sqlx::query(
            r#"
            SELECT p.slug AS project, ger.repo_root, ger.git_head, ger.since_marker,
                   ger.analyzer_version, ger.strategy_version, ger.id, ger.status,
                   ger.completed_at,
                   (SELECT COUNT(*) FROM code_symbols cs WHERE cs.extraction_run_id = ger.id) AS symbol_count,
                   (SELECT COUNT(*) FROM code_references cr WHERE cr.extraction_run_id = ger.id) AS reference_count,
                   (SELECT COUNT(*) FROM code_references cr WHERE cr.extraction_run_id = ger.id AND cr.resolution_status = 'resolved') AS resolved_reference_count,
                   (SELECT COUNT(*) FROM code_references cr WHERE cr.extraction_run_id = ger.id AND cr.resolution_status = 'unresolved') AS unresolved_reference_count,
                   (SELECT COUNT(*) FROM code_references cr WHERE cr.extraction_run_id = ger.id AND cr.resolution_status = 'ambiguous') AS ambiguous_reference_count,
                   (SELECT COUNT(*) FROM graph_nodes gn WHERE gn.extraction_run_id = ger.id) AS graph_node_count,
                   (SELECT COUNT(*) FROM graph_edges ge WHERE ge.extraction_run_id = ger.id) AS graph_edge_count,
                   (SELECT COUNT(*) FROM graph_evidence ev WHERE ev.extraction_run_id = ger.id) AS evidence_count
            FROM graph_extraction_runs ger
            JOIN projects p ON p.id = ger.project_id
            WHERE p.slug = $1 AND ger.id = $2
            LIMIT 1
            "#,
        )
        .bind(project)
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await
        .context("read graph extraction status")?;
        row.map(row_to_status).transpose()
    }

    async fn fetch_seed_node_ids(
        &self,
        run_id: Uuid,
        filters: &CodeGraphViewFilters,
    ) -> Result<Vec<Uuid>> {
        let limit = filters.limit_nodes + 1;
        let rows = if filters.has_seed_filter() {
            sqlx::query(
                r#"
                SELECT cs.graph_node_id AS id
                FROM code_symbols cs
                WHERE cs.extraction_run_id = $1
                  AND ($2::text IS NULL OR cs.file_path ILIKE '%' || $2 || '%')
                  AND (
                    $3::text IS NULL
                    OR cs.name ILIKE '%' || $3 || '%'
                    OR COALESCE(cs.qualified_name, '') ILIKE '%' || $3 || '%'
                    OR cs.display_name ILIKE '%' || $3 || '%'
                    OR cs.stable_identity ILIKE '%' || $3 || '%'
                  )
                  AND (
                    $4::text IS NULL
                    OR cs.file_path ILIKE '%' || $4 || '%'
                    OR cs.name ILIKE '%' || $4 || '%'
                    OR COALESCE(cs.qualified_name, '') ILIKE '%' || $4 || '%'
                    OR cs.display_name ILIKE '%' || $4 || '%'
                    OR cs.stable_identity ILIKE '%' || $4 || '%'
                  )
                ORDER BY cs.file_path, cs.start_line, cs.display_name
                LIMIT $5
                "#,
            )
            .bind(run_id)
            .bind(filters.file_path.as_deref())
            .bind(filters.symbol.as_deref())
            .bind(filters.q.as_deref())
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .context("read seeded code graph nodes")?
        } else {
            sqlx::query(
                r#"
                WITH degree_counts AS (
                    SELECT source_node_id AS node_id, COUNT(*)::bigint AS degree
                    FROM graph_edges
                    WHERE extraction_run_id = $1
                    GROUP BY source_node_id
                    UNION ALL
                    SELECT target_node_id AS node_id, COUNT(*)::bigint AS degree
                    FROM graph_edges
                    WHERE extraction_run_id = $1
                    GROUP BY target_node_id
                ),
                degrees AS (
                    SELECT node_id, SUM(degree)::bigint AS degree
                    FROM degree_counts
                    GROUP BY node_id
                )
                SELECT gn.id AS id
                FROM graph_nodes gn
                LEFT JOIN degrees d ON d.node_id = gn.id
                WHERE gn.extraction_run_id = $1
                ORDER BY COALESCE(d.degree, 0) DESC, gn.display_name
                LIMIT $2
                "#,
            )
            .bind(run_id)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .context("read overview code graph nodes")?
        };

        let mut seen = BTreeSet::new();
        let mut ids = Vec::new();
        for row in rows {
            let id: Uuid = row.try_get("id")?;
            if seen.insert(id) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    async fn fetch_edges_for_frontier(
        &self,
        run_id: Uuid,
        frontier: &[Uuid],
        edge_kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CodeGraphEdge>> {
        if frontier.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            r#"
            SELECT ge.id, ge.source_node_id, ge.target_node_id, ge.edge_kind,
                   ge.confidence, ge.metadata_json->>'reference_kind' AS reference_kind,
                   ev.file_path, ev.start_line, ev.end_line
            FROM graph_edges ge
            LEFT JOIN graph_evidence ev ON ev.edge_id = ge.id
            WHERE ge.extraction_run_id = $1
              AND (ge.source_node_id = ANY($2) OR ge.target_node_id = ANY($2))
              AND ($3::text IS NULL OR ge.edge_kind = $3)
            ORDER BY ge.edge_kind, ge.id
            LIMIT $4
            "#,
        )
        .bind(run_id)
        .bind(frontier)
        .bind(edge_kind)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .context("read code graph edges")?;

        rows.into_iter()
            .map(|row| {
                Ok(CodeGraphEdge {
                    id: row.try_get("id")?,
                    source: row.try_get("source_node_id")?,
                    target: row.try_get("target_node_id")?,
                    edge_kind: row.try_get("edge_kind")?,
                    reference_kind: row.try_get("reference_kind")?,
                    confidence: row.try_get("confidence")?,
                    file_path: row.try_get("file_path")?,
                    start_line: row.try_get("start_line")?,
                    end_line: row.try_get("end_line")?,
                    resolution_status: "resolved".to_string(),
                })
            })
            .collect()
    }

    async fn fetch_graph_nodes(
        &self,
        run_id: Uuid,
        node_ids: &BTreeSet<Uuid>,
        seed_ids: &BTreeSet<Uuid>,
    ) -> Result<Vec<CodeGraphNode>> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = node_ids.iter().copied().collect();
        let rows = sqlx::query(
            r#"
            WITH degree_counts AS (
                SELECT source_node_id AS node_id, COUNT(*)::bigint AS degree
                FROM graph_edges
                WHERE extraction_run_id = $1
                GROUP BY source_node_id
                UNION ALL
                SELECT target_node_id AS node_id, COUNT(*)::bigint AS degree
                FROM graph_edges
                WHERE extraction_run_id = $1
                GROUP BY target_node_id
            ),
            degrees AS (
                SELECT node_id, SUM(degree)::bigint AS degree
                FROM degree_counts
                GROUP BY node_id
            )
            SELECT gn.id, gn.stable_identity, gn.display_name, gn.node_kind,
                   cs.language, cs.file_path, cs.symbol_kind, cs.name,
                   cs.qualified_name, cs.start_line, cs.end_line,
                   COALESCE(d.degree, 0)::bigint AS degree
            FROM graph_nodes gn
            LEFT JOIN code_symbols cs ON cs.graph_node_id = gn.id
            LEFT JOIN degrees d ON d.node_id = gn.id
            WHERE gn.extraction_run_id = $1
              AND gn.id = ANY($2)
            ORDER BY cs.file_path, cs.start_line, gn.display_name
            "#,
        )
        .bind(run_id)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .context("read code graph nodes")?;

        rows.into_iter()
            .map(|row| {
                let id: Uuid = row.try_get("id")?;
                let language: Option<String> = row.try_get("language")?;
                let symbol_kind: Option<String> = row.try_get("symbol_kind")?;
                let node_kind: String = row.try_get("node_kind")?;
                Ok(CodeGraphNode {
                    id,
                    stable_identity: row.try_get("stable_identity")?,
                    label: row.try_get("display_name")?,
                    node_kind: node_kind.clone(),
                    language: language.clone(),
                    symbol_kind: symbol_kind.clone(),
                    file_path: row.try_get("file_path")?,
                    name: row.try_get("name")?,
                    qualified_name: row.try_get("qualified_name")?,
                    start_line: row.try_get("start_line")?,
                    end_line: row.try_get("end_line")?,
                    degree: row.try_get("degree")?,
                    seed: seed_ids.contains(&id),
                    group: language.or(symbol_kind).unwrap_or(node_kind),
                })
            })
            .collect()
    }
}

impl GraphRepository for PostgresGraphRepository {
    fn extract(
        &self,
        request: GraphExtractionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<GraphExtractionReport>> + Send + '_>> {
        Box::pin(async move { PostgresGraphRepository::extract(self, request).await })
    }

    fn latest_status<'a>(
        &'a self,
        project: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<GraphStatusReport>>> + Send + 'a>> {
        Box::pin(async move { PostgresGraphRepository::latest_status(self, project).await })
    }

    fn status_response<'a>(
        &'a self,
        project: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<CodeGraphStatusResponse>> + Send + 'a>> {
        Box::pin(async move { PostgresGraphRepository::status_response(self, project).await })
    }

    fn visualization_graph<'a>(
        &'a self,
        project: &'a str,
        filters: CodeGraphViewFilters,
    ) -> Pin<Box<dyn Future<Output = Result<CodeGraphResponse>> + Send + 'a>> {
        Box::pin(async move {
            PostgresGraphRepository::visualization_graph(self, project, filters).await
        })
    }
}

async fn upsert_project(pool: &PgPool, slug: &str, repo_root: &str) -> Result<Uuid> {
    let row = sqlx::query(
        r#"
        INSERT INTO projects (id, slug, name, root_path)
        VALUES ($1, $2, $2, $3)
        ON CONFLICT (slug) DO UPDATE
            SET root_path = COALESCE(NULLIF(projects.root_path, ''), EXCLUDED.root_path)
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(slug)
    .bind(repo_root)
    .fetch_one(pool)
    .await
    .context("upsert graph project")?;
    row.try_get("id").context("read project id")
}

async fn find_existing_completed_run(
    pool: &PgPool,
    project_id: Uuid,
    request: &GraphExtractionRequest,
    strategy_version: &str,
) -> Result<Option<Uuid>> {
    let row = sqlx::query(
        r#"
        SELECT id
        FROM graph_extraction_runs
        WHERE project_id = $1
          AND repo_root = $2
          AND COALESCE(git_head, '') = COALESCE($3, '')
          AND COALESCE(since_marker, '') = COALESCE($4, '')
          AND analyzer_version = $5
          AND strategy_version = $6
          AND status = 'completed'
        ORDER BY completed_at DESC NULLS LAST, started_at DESC
        LIMIT 1
        "#,
    )
    .bind(project_id)
    .bind(&request.repo_root)
    .bind(&request.git_head)
    .bind(&request.since)
    .bind(&request.analysis.analyzer_version)
    .bind(strategy_version)
    .fetch_optional(pool)
    .await
    .context("find existing graph extraction run")?;
    row.map(|row| row.try_get("id"))
        .transpose()
        .map_err(Into::into)
}

struct EvidenceInput<'a> {
    project_id: Uuid,
    run_id: Uuid,
    node_id: Option<Uuid>,
    edge_id: Option<Uuid>,
    file_path: &'a str,
    start_line: i64,
    end_line: i64,
    confidence: f32,
    strategy_version: &'a str,
}

async fn insert_evidence(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    evidence: EvidenceInput<'_>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO graph_evidence
            (id, project_id, extraction_run_id, node_id, edge_id, evidence_kind,
             file_path, start_line, end_line, confidence, strategy_version, created_at)
        VALUES ($1, $2, $3, $4, $5, 'file_span', $6, $7, $8, $9, $10, now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(evidence.project_id)
    .bind(evidence.run_id)
    .bind(evidence.node_id)
    .bind(evidence.edge_id)
    .bind(evidence.file_path)
    .bind(evidence.start_line)
    .bind(evidence.end_line)
    .bind(evidence.confidence)
    .bind(evidence.strategy_version)
    .execute(&mut **tx)
    .await
    .context("insert graph evidence")?;
    Ok(())
}

fn row_to_status(row: sqlx::postgres::PgRow) -> Result<GraphStatusReport> {
    Ok(GraphStatusReport {
        project: row.try_get("project")?,
        repo_root: row.try_get("repo_root")?,
        git_head: row.try_get("git_head")?,
        since: row.try_get("since_marker")?,
        analyzer_version: row.try_get("analyzer_version")?,
        strategy_version: row.try_get("strategy_version")?,
        extraction_run_id: row.try_get("id")?,
        status: row.try_get("status")?,
        completed_at: row.try_get("completed_at")?,
        symbol_count: row.try_get("symbol_count")?,
        reference_count: row.try_get("reference_count")?,
        resolved_reference_count: row.try_get("resolved_reference_count")?,
        unresolved_reference_count: row.try_get("unresolved_reference_count")?,
        ambiguous_reference_count: row.try_get("ambiguous_reference_count")?,
        graph_node_count: row.try_get("graph_node_count")?,
        graph_edge_count: row.try_get("graph_edge_count")?,
        evidence_count: row.try_get("evidence_count")?,
    })
}

fn status_to_response(status: &GraphStatusReport) -> CodeGraphStatusResponse {
    CodeGraphStatusResponse {
        project: status.project.clone(),
        has_graph: true,
        latest_run_id: Some(status.extraction_run_id),
        repo_root: Some(status.repo_root.clone()),
        git_head: status.git_head.clone(),
        since: status.since.clone(),
        analyzer_version: Some(status.analyzer_version.clone()),
        strategy_version: Some(status.strategy_version.clone()),
        status: Some(status.status.clone()),
        completed_at: status.completed_at,
        symbol_count: status.symbol_count,
        reference_count: status.reference_count,
        resolved_reference_count: status.resolved_reference_count,
        unresolved_reference_count: status.unresolved_reference_count,
        ambiguous_reference_count: status.ambiguous_reference_count,
        graph_node_count: status.graph_node_count,
        graph_edge_count: status.graph_edge_count,
        evidence_count: status.evidence_count,
    }
}

fn empty_graph_response(project: &str, filters: CodeGraphViewFilters) -> CodeGraphResponse {
    CodeGraphResponse {
        project: project.to_string(),
        status: CodeGraphStatusResponse::empty(project),
        filters,
        stats: CodeGraphStats::default(),
        truncated: false,
        truncation_reason: None,
        nodes: Vec::new(),
        edges: Vec::new(),
    }
}
