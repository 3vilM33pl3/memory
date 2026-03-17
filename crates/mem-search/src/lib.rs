use std::collections::HashSet;

use chrono::{DateTime, Utc};
use mem_api::{MemoryType, QueryRequest, QueryResponse, QueryResult, QuerySource, SourceKind};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const MAX_CANDIDATES: i64 = 64;
const CHUNK_TARGET_SIZE: usize = 320;
const CHUNK_OVERLAP: usize = 80;

pub async fn query_memory(
    pool: &PgPool,
    request: &QueryRequest,
) -> Result<QueryResponse, sqlx::Error> {
    let normalized = QueryIntent::from_query(&request.query);
    let memory_type_filters = request
        .filters
        .types
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let lexical_like_terms = normalized
        .lexical_terms
        .iter()
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    let path_like_terms = normalized
        .path_terms
        .iter()
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    let tag_like_terms = normalized
        .lexical_terms
        .iter()
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    let candidate_limit = (request.top_k * 8).clamp(request.top_k, MAX_CANDIDATES);

    let rows = sqlx::query(
        r#"
        WITH input AS (
            SELECT websearch_to_tsquery('english', $2) AS query
        ),
        candidates AS (
            SELECT
                m.id,
                m.summary,
                m.memory_type,
                m.canonical_text,
                m.importance,
                m.confidence,
                m.updated_at,
                COALESCE(ts_rank_cd(m.search_document, input.query), 0) AS entry_fts,
                COALESCE(best_chunk.chunk_fts, 0) AS chunk_fts,
                COALESCE(best_chunk.chunk_text, left(m.canonical_text, 320)) AS best_chunk_text,
                COALESCE((
                    SELECT ARRAY_AGG(mt.tag ORDER BY mt.tag)
                    FROM memory_tags mt
                    WHERE mt.memory_entry_id = m.id
                ), ARRAY[]::text[]) AS tags,
                COALESCE((
                    SELECT ARRAY_AGG(ms.file_path ORDER BY ms.file_path)
                    FROM memory_sources ms
                    WHERE ms.memory_entry_id = m.id
                      AND ms.file_path IS NOT NULL
                ), ARRAY[]::text[]) AS source_paths
            FROM memory_entries m
            JOIN projects p ON p.id = m.project_id
            CROSS JOIN input
            LEFT JOIN LATERAL (
                SELECT
                    mc.chunk_text,
                    ts_rank_cd(mc.tsv, input.query) AS chunk_fts
                FROM memory_chunks mc
                WHERE mc.memory_entry_id = m.id
                  AND mc.tsv @@ input.query
                ORDER BY chunk_fts DESC, mc.id
                LIMIT 1
            ) best_chunk ON true
            WHERE p.slug = $1
              AND m.status = 'active'
              AND ($3::text[] IS NULL OR m.memory_type = ANY($3))
              AND (
                    cardinality($4::text[]) = 0
                    OR EXISTS (
                        SELECT 1
                        FROM memory_tags mt
                        WHERE mt.memory_entry_id = m.id
                          AND mt.tag = ANY($4)
                    )
              )
              AND (
                    m.search_document @@ input.query
                    OR best_chunk.chunk_text IS NOT NULL
                    OR (
                        cardinality($5::text[]) > 0
                        AND (
                            m.summary ILIKE ANY($5)
                            OR m.canonical_text ILIKE ANY($5)
                        )
                    )
                    OR (
                        cardinality($6::text[]) > 0
                        AND EXISTS (
                            SELECT 1
                            FROM memory_sources ms
                            WHERE ms.memory_entry_id = m.id
                              AND ms.file_path ILIKE ANY($6)
                        )
                    )
                    OR (
                        cardinality($7::text[]) > 0
                        AND EXISTS (
                            SELECT 1
                            FROM memory_tags mt
                            WHERE mt.memory_entry_id = m.id
                              AND mt.tag ILIKE ANY($7)
                        )
                    )
                )
        )
        SELECT *
        FROM candidates
        ORDER BY GREATEST(entry_fts, chunk_fts) DESC, updated_at DESC, id DESC
        LIMIT $8
        "#,
    )
    .bind(&request.project)
    .bind(&request.query)
    .bind(if memory_type_filters.is_empty() {
        None::<Vec<String>>
    } else {
        Some(memory_type_filters)
    })
    .bind(&request.filters.tags)
    .bind(&lexical_like_terms)
    .bind(&path_like_terms)
    .bind(&tag_like_terms)
    .bind(candidate_limit)
    .fetch_all(pool)
    .await?;

    let mut ranked = rows
        .into_iter()
        .map(|row| rank_candidate(row, &normalized))
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    ranked.sort_by(|left, right| {
        right
            .final_score
            .total_cmp(&left.final_score)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });

    let mut results = Vec::new();
    for candidate in ranked.into_iter().take(request.top_k as usize) {
        if request
            .min_confidence
            .is_some_and(|threshold| candidate.confidence < threshold)
        {
            continue;
        }

        let sources = fetch_sources(pool, candidate.memory_id).await?;
        results.push(QueryResult {
            memory_id: candidate.memory_id,
            summary: candidate.summary,
            memory_type: candidate.memory_type,
            score: candidate.final_score,
            snippet: candidate.snippet,
            score_explanation: candidate.score_explanation,
            tags: candidate.tags,
            sources,
        });
    }

    let (answer, confidence, insufficient_evidence) = synthesize_answer(&results);

    Ok(QueryResponse {
        answer,
        confidence,
        results,
        insufficient_evidence,
    })
}

pub async fn rebuild_chunks(pool: &PgPool, project: &str) -> Result<u64, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT m.id, m.canonical_text, m.summary
        FROM memory_entries m
        JOIN projects p ON p.id = m.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(project)
    .fetch_all(pool)
    .await?;

    let mut count = 0_u64;
    for row in rows {
        let memory_id: Uuid = row.try_get("id")?;
        let canonical_text: String = row.try_get("canonical_text")?;
        let summary: String = row.try_get("summary")?;
        sqlx::query("DELETE FROM memory_chunks WHERE memory_entry_id = $1")
            .bind(memory_id)
            .execute(pool)
            .await?;

        for chunk_text in split_search_chunks(&summary, &canonical_text) {
            sqlx::query(
                r#"
                INSERT INTO memory_chunks (id, memory_entry_id, chunk_text, search_text, tsv)
                VALUES ($1, $2, $3, $4, to_tsvector('english', $4))
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(memory_id)
            .bind(&chunk_text)
            .bind(format!("{summary}\n{chunk_text}"))
            .execute(pool)
            .await?;
        }
        count += 1;
    }
    Ok(count)
}

pub fn parse_memory_type(value: &str) -> MemoryType {
    match value {
        "architecture" => MemoryType::Architecture,
        "convention" => MemoryType::Convention,
        "decision" => MemoryType::Decision,
        "incident" => MemoryType::Incident,
        "debugging" => MemoryType::Debugging,
        "environment" => MemoryType::Environment,
        "domain_fact" => MemoryType::DomainFact,
        _ => MemoryType::Convention,
    }
}

async fn fetch_sources(pool: &PgPool, memory_id: Uuid) -> Result<Vec<QuerySource>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT task_id, file_path, source_kind, excerpt
        FROM memory_sources
        WHERE memory_entry_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let source_kind: String = row.try_get("source_kind")?;
        items.push(QuerySource {
            task_id: row.try_get("task_id")?,
            file_path: row.try_get("file_path")?,
            source_kind: parse_source_kind(&source_kind),
            excerpt: row.try_get("excerpt")?,
        });
    }
    Ok(items)
}

pub fn parse_source_kind(value: &str) -> SourceKind {
    match value {
        "task_prompt" => SourceKind::TaskPrompt,
        "file" => SourceKind::File,
        "git_commit" => SourceKind::GitCommit,
        "command_output" => SourceKind::CommandOutput,
        "test" => SourceKind::Test,
        "note" => SourceKind::Note,
        _ => SourceKind::Note,
    }
}

#[derive(Debug)]
struct QueryIntent {
    normalized_query: String,
    lexical_terms: Vec<String>,
    exact_phrases: Vec<String>,
    path_terms: Vec<String>,
}

impl QueryIntent {
    fn from_query(query: &str) -> Self {
        let normalized_query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        let exact_phrases = extract_quoted_phrases(&normalized_query);
        let lexical_terms = extract_lexical_terms(&normalized_query);
        let path_terms = lexical_terms
            .iter()
            .filter(|term| is_path_like(term))
            .cloned()
            .collect();
        Self {
            normalized_query,
            lexical_terms,
            exact_phrases,
            path_terms,
        }
    }
}

struct RankedCandidate {
    memory_id: Uuid,
    summary: String,
    memory_type: MemoryType,
    confidence: f32,
    updated_at: DateTime<Utc>,
    tags: Vec<String>,
    snippet: String,
    final_score: f64,
    score_explanation: Vec<String>,
}

fn rank_candidate(
    row: sqlx::postgres::PgRow,
    intent: &QueryIntent,
) -> Result<RankedCandidate, sqlx::Error> {
    let memory_id: Uuid = row.try_get("id")?;
    let summary: String = row.try_get("summary")?;
    let canonical_text: String = row.try_get("canonical_text")?;
    let memory_type = parse_memory_type(&row.try_get::<String, _>("memory_type")?);
    let importance: i32 = row.try_get("importance")?;
    let confidence: f32 = row.try_get("confidence")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let entry_fts: f64 = row.try_get("entry_fts")?;
    let chunk_fts: f64 = row.try_get("chunk_fts")?;
    let best_chunk_text: String = row.try_get("best_chunk_text")?;
    let tags: Vec<String> = row.try_get("tags")?;
    let source_paths: Vec<String> = row.try_get("source_paths")?;

    let query_lower = intent.normalized_query.to_lowercase();
    let summary_lower = summary.to_lowercase();
    let canonical_lower = canonical_text.to_lowercase();
    let snippet_lower = best_chunk_text.to_lowercase();
    let combined_text = format!("{summary_lower}\n{snippet_lower}\n{canonical_lower}");

    let exact_phrase_matches = if intent.exact_phrases.is_empty() {
        usize::from(
            !query_lower.is_empty()
                && (summary_lower.contains(&query_lower)
                    || canonical_lower.contains(&query_lower)
                    || snippet_lower.contains(&query_lower)),
        )
    } else {
        intent
            .exact_phrases
            .iter()
            .filter(|phrase| combined_text.contains(&phrase.to_lowercase()))
            .count()
    };

    let term_overlap = lexical_overlap_ratio(&combined_text, &intent.lexical_terms);
    let tag_match_count = tags
        .iter()
        .filter(|tag| lexical_match(tag, &intent.lexical_terms))
        .count();
    let path_match_count = source_paths
        .iter()
        .filter(|path| lexical_match(path, &intent.path_terms))
        .count();

    let age_days = (Utc::now() - updated_at).num_days().max(0) as f64;
    let recency_boost = 1.0 / (1.0 + (age_days / 14.0));

    let mut final_score = (chunk_fts * 4.0)
        + (entry_fts * 2.5)
        + (exact_phrase_matches as f64 * 1.4)
        + (term_overlap * 1.5)
        + (tag_match_count as f64 * 0.9)
        + (path_match_count as f64 * 1.1)
        + (importance as f64 * 0.35)
        + (confidence as f64 * 1.8)
        + (recency_boost * 0.6);

    if exact_phrase_matches == 0 && term_overlap < 0.15 && chunk_fts == 0.0 && entry_fts == 0.0 {
        final_score *= 0.65;
    }

    let snippet = summarize_snippet(
        &best_chunk_text,
        &intent.lexical_terms,
        &intent.exact_phrases,
    );
    let mut score_explanation = Vec::new();
    if chunk_fts > 0.0 {
        score_explanation.push(format!("strong chunk match {:.2}", chunk_fts));
    }
    if entry_fts > 0.0 {
        score_explanation.push(format!("entry search match {:.2}", entry_fts));
    }
    if exact_phrase_matches > 0 {
        score_explanation.push(format!("exact phrase match x{}", exact_phrase_matches));
    }
    if tag_match_count > 0 {
        score_explanation.push(format!("tag match x{}", tag_match_count));
    }
    if path_match_count > 0 {
        score_explanation.push(format!("source path match x{}", path_match_count));
    }
    score_explanation.push(format!("term overlap {:.0}%", term_overlap * 100.0));
    score_explanation.push(format!("importance {}", importance));
    score_explanation.push(format!("memory confidence {:.2}", confidence));
    score_explanation.push(format!("updated {}d ago", age_days as i64));

    Ok(RankedCandidate {
        memory_id,
        summary,
        memory_type,
        confidence,
        updated_at,
        tags,
        snippet,
        final_score,
        score_explanation,
    })
}

fn synthesize_answer(results: &[QueryResult]) -> (String, f32, bool) {
    let Some(top) = results.first() else {
        return (
            "I could not find enough project memory to answer confidently.".to_string(),
            0.0,
            true,
        );
    };

    let best_score = top.score;
    let normalized = (best_score / (best_score + 6.0)).clamp(0.0, 1.0) as f32;
    let strong_results = results
        .iter()
        .take(3)
        .filter(|result| result.score >= best_score * 0.72)
        .collect::<Vec<_>>();

    let insufficient = strong_results.is_empty()
        || normalized < 0.38
        || strong_results[0]
            .score_explanation
            .iter()
            .all(|item| item.starts_with("term overlap 0%"));

    if insufficient {
        return (
            "I could not find enough project memory to answer confidently.".to_string(),
            normalized.min(0.3),
            true,
        );
    }

    let mut summaries = Vec::new();
    let mut seen = HashSet::new();
    for result in strong_results {
        let normalized_summary = result.summary.to_lowercase();
        if seen.insert(normalized_summary) {
            summaries.push(result.summary.clone());
        }
    }

    let answer = match summaries.as_slice() {
        [] => "I could not find enough project memory to answer confidently.".to_string(),
        [only] => format!("{only}"),
        [first, second] => format!("{first} Also relevant: {second}."),
        [first, second, third, ..] => {
            format!("{first} Also relevant: {second}. Supporting detail: {third}.")
        }
    };

    let confidence = (normalized + ((summaries.len().saturating_sub(1) as f32) * 0.08)).min(0.95);
    (answer, confidence, false)
}

fn extract_quoted_phrases(query: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut current = String::new();
    let mut quote_char = None;
    let mut escaped = false;
    for ch in query.chars() {
        if escaped {
            if quote_char.is_some() {
                current.push(ch);
            }
            escaped = false;
            continue;
        }

        match (quote_char, ch) {
            (_, '\\') => {
                escaped = true;
            }
            (None, '"' | '\'') => {
                quote_char = Some(ch);
                current.clear();
            }
            (Some(active), ch) if ch == active => {
                let phrase = current.split_whitespace().collect::<Vec<_>>().join(" ");
                if !phrase.is_empty() {
                    phrases.push(phrase);
                }
                current.clear();
                quote_char = None;
            }
            (Some(_), ch) => current.push(ch),
            _ => {}
        }
    }
    phrases
}

fn extract_lexical_terms(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    query
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-')))
        .filter_map(|raw| {
            let term = raw.trim().to_lowercase();
            if term.len() < 2 || !seen.insert(term.clone()) {
                None
            } else {
                Some(term)
            }
        })
        .collect()
}

fn lexical_overlap_ratio(text: &str, terms: &[String]) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let matched = terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .count();
    matched as f64 / terms.len() as f64
}

fn lexical_match(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let lowered = text.to_lowercase();
    terms.iter().any(|term| lowered.contains(term))
}

fn is_path_like(term: &str) -> bool {
    term.contains('/') || term.contains('.') || term.contains('_') || term.contains('-')
}

fn summarize_snippet(text: &str, lexical_terms: &[String], phrases: &[String]) -> String {
    let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.len() <= 240 {
        return trimmed;
    }

    let lowered = trimmed.to_lowercase();
    let target = phrases
        .iter()
        .map(|value| value.to_lowercase())
        .chain(lexical_terms.iter().cloned())
        .find_map(|needle| lowered.find(&needle));

    if let Some(index) = target {
        let start = index.saturating_sub(80);
        let end = (start + 240).min(trimmed.len());
        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < trimmed.len() { "..." } else { "" };
        return format!("{prefix}{}{suffix}", &trimmed[start..end]);
    }

    format!("{}...", &trimmed[..240])
}

fn split_search_chunks(summary: &str, canonical_text: &str) -> Vec<String> {
    let normalized_summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized_text = canonical_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let text = if normalized_summary.is_empty() {
        normalized_text
    } else if normalized_text.is_empty() {
        normalized_summary
    } else {
        format!("{normalized_summary}\n{normalized_text}")
    };
    if text.len() <= CHUNK_TARGET_SIZE {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + CHUNK_TARGET_SIZE).min(text.len());
        while end < text.len() && !text.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(relative) = text[start..end].rfind(' ') {
            let candidate = start + relative;
            if candidate > start + (CHUNK_TARGET_SIZE / 2) {
                end = candidate;
            }
        }

        let chunk = text[start..end].trim();
        if !chunk.is_empty() {
            chunks.push(chunk.to_string());
        }

        if end >= text.len() {
            break;
        }

        start = end.saturating_sub(CHUNK_OVERLAP);
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
    }

    if chunks.is_empty() {
        vec![format!("{summary}\n{canonical_text}")]
    } else {
        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_quoted_phrases() {
        assert_eq!(
            extract_quoted_phrases(r#"why \"repo root\" uses "memory watch""#),
            vec!["memory watch".to_string()]
        );
    }

    #[test]
    fn extracts_lexical_terms_and_paths() {
        let intent = QueryIntent::from_query("memory watch .mem/config.toml project");
        assert!(intent.lexical_terms.contains(&"memory".to_string()));
        assert!(
            intent.path_terms.contains(&".mem".to_string())
                || intent
                    .path_terms
                    .iter()
                    .any(|term| term.contains("config.toml"))
        );
    }

    #[test]
    fn chunking_splits_long_text() {
        let text = "alpha ".repeat(200);
        let chunks = split_search_chunks("summary", &text);
        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.len() <= CHUNK_TARGET_SIZE + 16)
        );
        assert!(
            chunks
                .first()
                .is_some_and(|chunk| chunk.contains("summary"))
        );
    }

    #[test]
    fn snippet_prefers_matching_term() {
        let text = format!("{} needle {}", "alpha ".repeat(80), "beta ".repeat(80));
        let snippet = summarize_snippet(&text, &["needle".to_string()], &[]);
        assert!(snippet.contains("needle"));
    }

    #[test]
    fn synthesize_answer_prefers_multiple_strong_results() {
        let results = vec![
            QueryResult {
                memory_id: Uuid::new_v4(),
                summary: "Primary summary".to_string(),
                memory_type: MemoryType::Architecture,
                score: 7.0,
                snippet: "Primary snippet".to_string(),
                score_explanation: vec![
                    "strong chunk match 1.20".to_string(),
                    "term overlap 100%".to_string(),
                ],
                tags: vec![],
                sources: vec![],
            },
            QueryResult {
                memory_id: Uuid::new_v4(),
                summary: "Secondary summary".to_string(),
                memory_type: MemoryType::Convention,
                score: 5.5,
                snippet: "Secondary snippet".to_string(),
                score_explanation: vec![
                    "entry search match 0.90".to_string(),
                    "term overlap 67%".to_string(),
                ],
                tags: vec![],
                sources: vec![],
            },
        ];

        let (answer, confidence, insufficient) = synthesize_answer(&results);
        assert!(answer.contains("Primary summary"));
        assert!(answer.contains("Secondary summary"));
        assert!(confidence > 0.45);
        assert!(!insufficient);
    }
}
