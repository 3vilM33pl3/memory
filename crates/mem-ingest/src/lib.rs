use mem_api::{CaptureTaskRequest, MemoryType, SourceKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateAssertion {
    pub canonical_text: String,
    pub summary: String,
    pub memory_type: MemoryType,
    pub confidence: f32,
    pub importance: i32,
    pub tags: Vec<String>,
    pub sources: Vec<CandidateSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSource {
    pub file_path: Option<String>,
    pub source_kind: SourceKind,
    pub excerpt: Option<String>,
}

pub fn idempotency_key(request: &CaptureTaskRequest) -> String {
    if let Some(existing) = &request.idempotency_key {
        return existing.clone();
    }

    let mut hasher = Sha256::new();
    hasher.update(request.project.as_bytes());
    hasher.update(request.writer_id.as_bytes());
    hasher.update(request.task_title.as_bytes());
    hasher.update(request.user_prompt.as_bytes());
    hasher.update(request.agent_summary.as_bytes());
    for file in &request.files_changed {
        hasher.update(file.as_bytes());
    }
    for note in &request.notes {
        hasher.update(note.as_bytes());
    }
    for candidate in &request.structured_candidates {
        hasher.update(candidate.canonical_text.as_bytes());
        hasher.update(candidate.summary.as_bytes());
        hasher.update(candidate.memory_type.to_string().as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub fn extract_candidates(request: &CaptureTaskRequest) -> Vec<CandidateAssertion> {
    let mut items = Vec::new();
    let files = request
        .files_changed
        .iter()
        .map(|path| CandidateSource {
            file_path: Some(path.clone()),
            source_kind: SourceKind::File,
            excerpt: Some(format!("Changed file during task: {path}")),
        })
        .collect::<Vec<_>>();

    let confidence = infer_confidence(request);
    let importance = infer_importance(request);
    let tags = infer_tags(request);

    if !request.structured_candidates.is_empty() {
        items.extend(request.structured_candidates.iter().map(|candidate| {
            CandidateAssertion {
                canonical_text: normalize_candidate_canonical_text(
                    &candidate.memory_type,
                    &candidate.canonical_text,
                ),
                summary: normalize_summary(&candidate.summary, request),
                memory_type: candidate.memory_type.clone(),
                confidence: candidate.confidence,
                importance: candidate.importance,
                tags: normalize_tags(&candidate.tags),
                sources: candidate
                    .sources
                    .iter()
                    .map(|source| CandidateSource {
                        file_path: source.file_path.clone(),
                        source_kind: source.source_kind.clone(),
                        excerpt: source.excerpt.clone(),
                    })
                    .collect(),
            }
        }));
    }

    for note in &request.notes {
        items.push(CandidateAssertion {
            canonical_text: normalize_sentence(note),
            summary: summarize(request),
            memory_type: classify_type(request, note),
            confidence,
            importance,
            tags: tags.clone(),
            sources: build_sources(request, note, &files),
        });
    }

    if items.is_empty() {
        let summary_text = request.agent_summary.trim().to_string();
        items.push(CandidateAssertion {
            canonical_text: normalize_sentence(&summary_text),
            summary: summarize(request),
            memory_type: classify_type(request, &summary_text),
            confidence,
            importance,
            tags,
            sources: build_sources(request, &summary_text, &files),
        });
    }
    items
}

fn classify_type(request: &CaptureTaskRequest, text: &str) -> MemoryType {
    let haystack = format!(
        "{} {} {} {}",
        request.task_title.to_lowercase(),
        request.user_prompt.to_lowercase(),
        request.agent_summary.to_lowercase(),
        text.to_lowercase()
    );

    if haystack.contains("debug") || haystack.contains("fix") || haystack.contains("bug") {
        MemoryType::Debugging
    } else if haystack.contains("decision") || haystack.contains("choose") {
        MemoryType::Decision
    } else if haystack.contains("architecture") || haystack.contains("service") {
        MemoryType::Architecture
    } else if haystack.contains("environment")
        || haystack.contains("setup")
        || haystack.contains("config")
    {
        MemoryType::Environment
    } else if haystack.contains("convention")
        || haystack.contains("workflow")
        || haystack.contains("rule")
        || haystack.contains("policy")
    {
        MemoryType::Convention
    } else if haystack.contains("domain")
        || haystack.contains("protocol")
        || haystack.contains("business")
    {
        MemoryType::DomainFact
    } else {
        MemoryType::Implementation
    }
}

fn summarize(request: &CaptureTaskRequest) -> String {
    request
        .task_title
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
}

fn infer_confidence(request: &CaptureTaskRequest) -> f32 {
    if request.tests.iter().any(|test| test.status == "passed") {
        0.85
    } else if !request.files_changed.is_empty() {
        0.7
    } else {
        0.55
    }
}

fn infer_importance(request: &CaptureTaskRequest) -> i32 {
    if request.files_changed.len() >= 3 || !request.tests.is_empty() {
        3
    } else if !request.notes.is_empty() {
        2
    } else {
        1
    }
}

fn infer_tags(request: &CaptureTaskRequest) -> Vec<String> {
    let mut tags = Vec::new();
    for file in &request.files_changed {
        if let Some(prefix) = file.split('/').next() {
            if !prefix.is_empty() {
                tags.push(prefix.to_string());
            }
        }
    }
    tags.sort();
    tags.dedup();
    tags
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut tags = tags
        .iter()
        .map(|tag| tag.trim().to_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    tags.sort();
    tags.dedup();
    tags
}

fn build_sources(
    request: &CaptureTaskRequest,
    note: &str,
    files: &[CandidateSource],
) -> Vec<CandidateSource> {
    let mut sources = files.to_vec();
    sources.push(CandidateSource {
        file_path: None,
        source_kind: SourceKind::TaskPrompt,
        excerpt: Some(request.user_prompt.clone()),
    });
    sources.push(CandidateSource {
        file_path: None,
        source_kind: SourceKind::Note,
        excerpt: Some(note.to_string()),
    });
    if let Some(summary) = &request.git_diff_summary {
        sources.push(CandidateSource {
            file_path: None,
            source_kind: SourceKind::GitCommit,
            excerpt: Some(summary.clone()),
        });
    }
    if let Some(output) = &request.command_output {
        sources.push(CandidateSource {
            file_path: None,
            source_kind: SourceKind::CommandOutput,
            excerpt: Some(output.clone()),
        });
    }
    for test in &request.tests {
        sources.push(CandidateSource {
            file_path: None,
            source_kind: SourceKind::Test,
            excerpt: Some(format!("{}: {}", test.command, test.status)),
        });
    }
    sources
}

fn normalize_candidate_canonical_text(memory_type: &MemoryType, input: &str) -> String {
    match memory_type {
        MemoryType::Plan | MemoryType::Implementation => normalize_markdown_block(input),
        _ => normalize_sentence(input),
    }
}

fn normalize_markdown_block(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn normalize_sentence(input: &str) -> String {
    let mut value = input.trim().replace('\n', " ");
    while value.contains("  ") {
        value = value.replace("  ", " ");
    }
    if !value.ends_with('.') {
        value.push('.');
    }
    value
}

fn normalize_summary(input: &str, request: &CaptureTaskRequest) -> String {
    let value = input.trim();
    if value.is_empty() {
        summarize(request)
    } else {
        value
            .split_whitespace()
            .take(12)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> CaptureTaskRequest {
        CaptureTaskRequest {
            project: "memory".to_string(),
            task_title: "Add memory service".to_string(),
            user_prompt: "Implement the backend service".to_string(),
            writer_id: "codex-writer".to_string(),
            writer_name: Some("Codex".to_string()),
            agent_summary: "Added the backend service and migrations".to_string(),
            files_changed: vec!["crates/mem-service/src/main.rs".to_string()],
            git_diff_summary: None,
            tests: Vec::new(),
            notes: vec![
                "The backend service owns capture, curation, and query endpoints".to_string(),
                "Project memory is stored in PostgreSQL".to_string(),
            ],
            structured_candidates: Vec::new(),
            command_output: None,
            idempotency_key: None,
            dry_run: false,
        }
    }

    #[test]
    fn idempotency_is_stable() {
        let request = sample_request();
        assert_eq!(idempotency_key(&request), idempotency_key(&request));
    }

    #[test]
    fn extract_candidates_uses_each_note_as_canonical_text() {
        let request = sample_request();
        let candidates = extract_candidates(&request);
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates[0]
                .canonical_text
                .contains("backend service owns capture")
        );
        assert!(
            candidates[1]
                .canonical_text
                .contains("Project memory is stored")
        );
    }

    #[test]
    fn structured_plan_candidates_preserve_multiline_markdown() {
        let mut request = sample_request();
        request.notes.clear();
        request.structured_candidates = vec![mem_api::CaptureCandidateInput {
            canonical_text: "# Plan\n\n- Step one\n- Step two\n".to_string(),
            summary: "Approved plan".to_string(),
            memory_type: MemoryType::Plan,
            confidence: 0.95,
            importance: 4,
            tags: vec!["plan".to_string()],
            sources: Vec::new(),
        }];

        let candidates = extract_candidates(&request);
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].canonical_text,
            "# Plan\n\n- Step one\n- Step two"
        );
        assert_eq!(candidates[0].memory_type, MemoryType::Plan);
    }

    #[test]
    fn structured_candidates_can_coexist_with_inferred_debugging_notes() {
        let mut request = sample_request();
        request.task_title = "Finish implementation".to_string();
        request.agent_summary = "Implemented the manager status footer".to_string();
        request.notes = vec!["Fixed stale manager footer status bug".to_string()];
        request.structured_candidates = vec![mem_api::CaptureCandidateInput {
            canonical_text: "Implemented the manager status footer.".to_string(),
            summary: "Implemented manager footer".to_string(),
            memory_type: MemoryType::Implementation,
            confidence: 0.9,
            importance: 3,
            tags: vec!["implementation".to_string()],
            sources: Vec::new(),
        }];

        let candidates = extract_candidates(&request);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].memory_type, MemoryType::Implementation);
        assert_eq!(candidates[1].memory_type, MemoryType::Debugging);
    }

    #[test]
    fn generic_completed_work_defaults_to_implementation() {
        let mut request = sample_request();
        request.task_title = "Ship watcher manager status".to_string();
        request.user_prompt = "Implement the new manager status footer".to_string();
        request.agent_summary = "Added manager session counts to the footer".to_string();
        request.notes = vec!["Added manager session counts to the footer".to_string()];

        let candidates = extract_candidates(&request);
        assert_eq!(candidates[0].memory_type, MemoryType::Implementation);
    }
}
