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
    hasher.update(request.task_title.as_bytes());
    hasher.update(request.user_prompt.as_bytes());
    hasher.update(request.agent_summary.as_bytes());
    for file in &request.files_changed {
        hasher.update(file.as_bytes());
    }
    for note in &request.notes {
        hasher.update(note.as_bytes());
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
    } else {
        MemoryType::Convention
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
    for test in &request.tests {
        sources.push(CandidateSource {
            file_path: None,
            source_kind: SourceKind::Test,
            excerpt: Some(format!("{}: {}", test.command, test.status)),
        });
    }
    sources
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> CaptureTaskRequest {
        CaptureTaskRequest {
            project: "memory".to_string(),
            task_title: "Add memory service".to_string(),
            user_prompt: "Implement the backend service".to_string(),
            agent_summary: "Added the backend service and migrations".to_string(),
            files_changed: vec!["crates/mem-service/src/main.rs".to_string()],
            git_diff_summary: None,
            tests: Vec::new(),
            notes: vec![
                "The backend service owns capture, curation, and query endpoints".to_string(),
                "Project memory is stored in PostgreSQL".to_string(),
            ],
            command_output: None,
            idempotency_key: None,
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
}
