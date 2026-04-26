use std::{collections::BTreeMap, future::Future, pin::Pin};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_analyze::{ResolutionStatus, ResolvedAnalysisReport, resolve_analysis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphExtractionRequest {
    pub project: String,
    pub repo_root: String,
    pub git_head: Option<String>,
    pub since: Option<String>,
    pub force: bool,
    pub dry_run: bool,
    pub index_reused: bool,
    pub analysis: mem_analyze::AnalysisReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphExtractionReport {
    pub project: String,
    pub repo_root: String,
    pub git_head: Option<String>,
    pub since: Option<String>,
    pub analyzer_version: String,
    pub strategy_version: String,
    pub extraction_run_id: Option<Uuid>,
    pub reused_existing_run: bool,
    pub dry_run: bool,
    pub index_reused: bool,
    pub symbol_count: usize,
    pub reference_count: usize,
    pub resolved_reference_count: usize,
    pub unresolved_reference_count: usize,
    pub ambiguous_reference_count: usize,
    pub graph_node_count: usize,
    pub graph_edge_count: usize,
    pub evidence_count: usize,
    pub sample_unresolved_references: Vec<GraphReferencePreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphReferencePreview {
    pub kind: String,
    pub file_path: String,
    pub target_text: String,
    pub resolution_status: String,
    pub start_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStatusReport {
    pub project: String,
    pub repo_root: String,
    pub git_head: Option<String>,
    pub since: Option<String>,
    pub analyzer_version: String,
    pub strategy_version: String,
    pub extraction_run_id: Uuid,
    pub status: String,
    pub completed_at: Option<DateTime<Utc>>,
    pub symbol_count: i64,
    pub reference_count: i64,
    pub resolved_reference_count: i64,
    pub unresolved_reference_count: i64,
    pub ambiguous_reference_count: i64,
    pub graph_node_count: i64,
    pub graph_edge_count: i64,
    pub evidence_count: i64,
}

pub fn build_extraction_preview(request: &GraphExtractionRequest) -> GraphExtractionReport {
    let resolved = resolve_analysis(&request.analysis);
    report_from_resolved(request, &resolved, None, false)
}

pub trait GraphRepository {
    fn extract(
        &self,
        request: GraphExtractionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<GraphExtractionReport>> + Send + '_>>;

    fn latest_status<'a>(
        &'a self,
        project: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<GraphStatusReport>>> + Send + 'a>>;
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .context("run graph migrations")
}

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

fn report_from_resolved(
    request: &GraphExtractionRequest,
    resolved: &ResolvedAnalysisReport,
    run_id: Option<Uuid>,
    reused_existing_run: bool,
) -> GraphExtractionReport {
    let resolved_reference_count = resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status == ResolutionStatus::Resolved)
        .count();
    let unresolved_reference_count = resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status == ResolutionStatus::Unresolved)
        .count();
    let ambiguous_reference_count = resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status == ResolutionStatus::Ambiguous)
        .count();
    let graph_edge_count = resolved
        .references
        .iter()
        .filter(|reference| {
            reference.resolution_status == ResolutionStatus::Resolved
                && reference.source_symbol_identity.is_some()
                && reference.target_symbol_identity.is_some()
        })
        .count();
    GraphExtractionReport {
        project: request.project.clone(),
        repo_root: request.repo_root.clone(),
        git_head: request.git_head.clone(),
        since: request.since.clone(),
        analyzer_version: resolved.analyzer_version.clone(),
        strategy_version: resolved.resolution_strategy_version.clone(),
        extraction_run_id: run_id,
        reused_existing_run,
        dry_run: request.dry_run,
        index_reused: request.index_reused,
        symbol_count: resolved.symbols.len(),
        reference_count: resolved.references.len(),
        resolved_reference_count,
        unresolved_reference_count,
        ambiguous_reference_count,
        graph_node_count: resolved.symbols.len(),
        graph_edge_count,
        evidence_count: resolved.symbols.len() + graph_edge_count,
        sample_unresolved_references: unresolved_previews(resolved),
    }
}

fn unresolved_previews(resolved: &ResolvedAnalysisReport) -> Vec<GraphReferencePreview> {
    resolved
        .references
        .iter()
        .filter(|reference| reference.resolution_status != ResolutionStatus::Resolved)
        .take(10)
        .map(|reference| GraphReferencePreview {
            kind: reference.graph_edge_kind.clone(),
            file_path: reference.file_path.clone(),
            target_text: reference.target_text.clone(),
            resolution_status: reference.resolution_status.as_str().to_string(),
            start_line: reference.span.start_line,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_preview_reports_resolution_counts() {
        let report = mem_analyze::AnalysisReport {
            analyzer_version: mem_analyze::ANALYZER_VERSION.to_string(),
            symbols: vec![mem_analyze::SymbolFact {
                id: "helper".to_string(),
                stable_identity: "rust:src/lib.rs:function:helper:1-1".to_string(),
                language: mem_analyze::AnalyzerLanguage::Rust,
                file_path: "src/lib.rs".to_string(),
                kind: mem_analyze::SymbolKind::Function,
                name: "helper".to_string(),
                qualified_name: Some("helper".to_string()),
                span: mem_analyze::Span {
                    start_byte: 0,
                    end_byte: 1,
                    start_line: 1,
                    end_line: 1,
                },
                display: "helper".to_string(),
                source_hash: None,
            }],
            calls: vec![mem_analyze::CallFact {
                id: "call".to_string(),
                language: mem_analyze::AnalyzerLanguage::Rust,
                file_path: "src/lib.rs".to_string(),
                callee_text: "helper".to_string(),
                caller_symbol: None,
                span: mem_analyze::Span {
                    start_byte: 2,
                    end_byte: 3,
                    start_line: 2,
                    end_line: 2,
                },
            }],
            ..mem_analyze::AnalysisReport::default()
        };
        let request = GraphExtractionRequest {
            project: "memory".to_string(),
            repo_root: "/repo".to_string(),
            git_head: Some("abc".to_string()),
            since: None,
            force: false,
            dry_run: true,
            index_reused: false,
            analysis: report,
        };
        let preview = build_extraction_preview(&request);
        assert_eq!(preview.symbol_count, 1);
        assert_eq!(preview.reference_count, 1);
        assert_eq!(preview.resolved_reference_count, 1);
        assert_eq!(preview.graph_edge_count, 0);
        assert_eq!(
            preview.strategy_version,
            mem_analyze::RESOLUTION_STRATEGY_VERSION
        );
    }
}
