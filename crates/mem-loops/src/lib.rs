use chrono::{DateTime, Utc};
use mem_api::{
    EffectiveLoopSettings, LoopActionKind, LoopContextExclusion, LoopContextInstructionRef,
    LoopContextMemory, LoopContextPack, LoopContextPackDiff, LoopContextSourceRef,
    LoopDefinitionRecord, LoopMode, LoopRiskLevel, LoopScopeType, LoopSettingRecord,
    LoopTriggerRouteDecision, MemoryEntryResponse, MemoryStatus, SourceProvenanceStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const LOOP_CONTEXT_PACK_REFRESH: &str = "context_pack_refresh";
pub const LOOP_MEMORY_HYGIENE: &str = "memory_hygiene";
pub const LOOP_CI_FAILURE_TRIAGE: &str = "ci_failure_triage";
pub const LOOP_AGENT_READY_ISSUE_TRIAGE: &str = "agent_ready_issue_triage";
pub const LOOP_DRAFT_PR: &str = "draft_pr";
pub const LOOP_REVIEWER_DRIFT_DETECTION: &str = "reviewer_drift_detection";
pub const LOOP_SKILL_MINING: &str = "skill_mining";
pub const LOOP_MEMORY_EVAL: &str = "memory_eval";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinLoopDefinition {
    pub loop_id: &'static str,
    pub version: i32,
    pub name: &'static str,
    pub description: &'static str,
    pub risk_level: LoopRiskLevel,
    pub default_mode: LoopMode,
    pub trigger_spec: Value,
    pub context_spec: Value,
    pub policy_spec: Value,
    pub output_spec: Value,
}

impl BuiltinLoopDefinition {
    pub fn stable_id(&self) -> Uuid {
        Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("memory-layer:loop:{}:{}", self.loop_id, self.version).as_bytes(),
        )
    }

    pub fn to_record(&self, created_at: DateTime<Utc>) -> LoopDefinitionRecord {
        LoopDefinitionRecord {
            id: self.stable_id(),
            loop_id: self.loop_id.to_string(),
            version: self.version,
            name: self.name.to_string(),
            description: self.description.to_string(),
            risk_level: self.risk_level.clone(),
            default_mode: self.default_mode.clone(),
            trigger_spec: self.trigger_spec.clone(),
            context_spec: self.context_spec.clone(),
            policy_spec: self.policy_spec.clone(),
            output_spec: self.output_spec.clone(),
            created_at,
        }
    }
}

pub fn builtin_loop_definitions() -> Vec<BuiltinLoopDefinition> {
    vec![
        builtin(
            LOOP_CONTEXT_PACK_REFRESH,
            "Context Pack Refresh",
            "Refreshes project/repo context packs when docs, structure, commands, or important memories change.",
            LoopRiskLevel::Low,
            LoopMode::SuggestOnly,
            vec!["manual", "repo_docs_changed", "memory_changed"],
            vec!["read_memory", "read_repo"],
            vec!["context_pack_diff", "memory_proposals"],
        ),
        builtin(
            LOOP_MEMORY_HYGIENE,
            "Memory Hygiene",
            "Finds duplicate, stale, contradictory, or low-confidence memories and proposes cleanup actions.",
            LoopRiskLevel::Medium,
            LoopMode::SuggestOnly,
            vec!["manual", "schedule", "memory_changed"],
            vec!["read_memory"],
            vec!["memory_proposals", "hygiene_report"],
        ),
        builtin(
            LOOP_CI_FAILURE_TRIAGE,
            "CI Failure Triage",
            "Reads failed workflow evidence, retrieves relevant memories, and produces a triage report.",
            LoopRiskLevel::Medium,
            LoopMode::Observe,
            vec!["manual", "ci_failed"],
            vec!["read_memory", "read_ci_logs"],
            vec!["triage_report", "follow_up_task"],
        ),
        builtin(
            LOOP_AGENT_READY_ISSUE_TRIAGE,
            "Agent-Ready Issue Triage",
            "Classifies issues by ambiguity and risk, then suggests agent workflow labels and task packs.",
            LoopRiskLevel::Low,
            LoopMode::SuggestOnly,
            vec!["manual", "issue_labeled", "issue_created"],
            vec!["read_memory", "read_issue"],
            vec!["issue_report", "task_pack"],
        ),
        builtin(
            LOOP_DRAFT_PR,
            "Draft PR",
            "Creates isolated draft implementation work for labelled low-risk issues after approval.",
            LoopRiskLevel::High,
            LoopMode::DraftOutput,
            vec!["manual", "agent_ready_issue"],
            vec!["read_memory", "read_repo", "write_repo", "run_command"],
            vec!["draft_pr", "checks", "memory_proposals"],
        ),
        builtin(
            LOOP_REVIEWER_DRIFT_DETECTION,
            "Reviewer / Drift Detection",
            "Reviews PR diffs against remembered architecture, conventions, and safety constraints.",
            LoopRiskLevel::Medium,
            LoopMode::SuggestOnly,
            vec!["manual", "pull_request_opened", "pull_request_updated"],
            vec!["read_memory", "read_repo", "read_diff"],
            vec!["review_report", "memory_proposals"],
        ),
        builtin(
            LOOP_SKILL_MINING,
            "Skill Mining",
            "Extracts reusable development recipes from successful runs and accepted PRs.",
            LoopRiskLevel::Medium,
            LoopMode::SuggestOnly,
            vec!["manual", "run_succeeded", "pull_request_merged"],
            vec!["read_memory", "read_run_trace"],
            vec!["learned_skill_proposal"],
        ),
        builtin(
            LOOP_MEMORY_EVAL,
            "Memory Eval",
            "Runs golden retrieval scenarios and tracks context quality metrics.",
            LoopRiskLevel::Low,
            LoopMode::Observe,
            vec!["manual", "schedule", "retriever_changed"],
            vec!["read_memory", "read_eval_fixtures"],
            vec!["eval_report", "metrics"],
        ),
    ]
}

fn builtin(
    loop_id: &'static str,
    name: &'static str,
    description: &'static str,
    risk_level: LoopRiskLevel,
    default_mode: LoopMode,
    triggers: Vec<&'static str>,
    capabilities: Vec<&'static str>,
    outputs: Vec<&'static str>,
) -> BuiltinLoopDefinition {
    BuiltinLoopDefinition {
        loop_id,
        version: 1,
        name,
        description,
        risk_level,
        default_mode,
        trigger_spec: json!({ "supported": triggers }),
        context_spec: json!({ "capabilities": capabilities }),
        policy_spec: json!({
            "default_read_only": true,
            "forbidden": [
                "push_main",
                "deploy",
                "access_secret",
                "destructive_migration",
                "enable_loop"
            ],
            "approval_required": [
                "mutate_memory",
                "write_repo",
                "invoke_runner"
            ]
        }),
        output_spec: json!({ "produces": outputs }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyDecision {
    pub action: LoopActionKind,
    pub allowed: bool,
    pub requires_approval: bool,
    pub reason: String,
}

pub fn validate_definition(definition: &BuiltinLoopDefinition) -> Result<(), String> {
    if definition.loop_id.trim().is_empty() {
        return Err("loop_id must be non-empty".to_string());
    }
    if definition.version <= 0 {
        return Err("version must be positive".to_string());
    }
    if definition.name.trim().is_empty() {
        return Err("name must be non-empty".to_string());
    }
    if !definition.trigger_spec.is_object()
        || !definition.context_spec.is_object()
        || !definition.policy_spec.is_object()
        || !definition.output_spec.is_object()
    {
        return Err("loop specs must be JSON objects".to_string());
    }
    Ok(())
}

pub fn resolve_effective_settings(
    definition: &LoopDefinitionRecord,
    settings: &[LoopSettingRecord],
    global_kill_switch: bool,
    manual_run: bool,
    now: DateTime<Utc>,
) -> EffectiveLoopSettings {
    let mut ordered = settings.to_vec();
    ordered.sort_by_key(|setting| scope_precedence(&setting.scope_type));

    let mut enabled = false;
    let mut mode = definition.default_mode.clone();
    let mut scope_type = LoopScopeType::User;
    let mut scope_id = "default".to_string();
    let mut budgets = None;
    let mut approval_overrides = None;
    let mut paused_until = None;
    let mut snoozed_until = None;

    for setting in ordered {
        scope_type = setting.scope_type.clone();
        scope_id = setting.scope_id.clone();
        if let Some(value) = setting.enabled {
            enabled = value;
        }
        if let Some(value) = setting.mode {
            mode = value;
        }
        if setting.budgets.is_some() {
            budgets = setting.budgets;
        }
        if setting.approval_overrides.is_some() {
            approval_overrides = setting.approval_overrides;
        }
        if setting.paused_until.is_some() {
            paused_until = setting.paused_until;
        }
        if setting.snoozed_until.is_some() {
            snoozed_until = setting.snoozed_until;
        }
    }

    let mut blocked_reasons = Vec::new();
    if global_kill_switch && !manual_run {
        blocked_reasons.push("global_kill_switch_enabled".to_string());
    }
    if !enabled {
        blocked_reasons.push("loop_not_enabled".to_string());
    }
    if matches!(mode, LoopMode::Off) {
        blocked_reasons.push("mode_off".to_string());
    }
    if let Some(until) = paused_until
        && until > now
    {
        blocked_reasons.push("paused".to_string());
        mode = LoopMode::Paused;
    }
    if let Some(until) = snoozed_until
        && until > now
    {
        blocked_reasons.push("snoozed".to_string());
        mode = LoopMode::Snoozed;
    }

    EffectiveLoopSettings {
        loop_id: definition.loop_id.clone(),
        enabled,
        mode,
        scope_type,
        scope_id,
        global_kill_switch,
        blocked_reasons,
        budgets,
        approval_overrides,
        paused_until,
        snoozed_until,
    }
}

pub fn evaluate_action(mode: &LoopMode, action: LoopActionKind) -> PolicyDecision {
    let forbidden = matches!(
        action,
        LoopActionKind::PushMain
            | LoopActionKind::Deploy
            | LoopActionKind::AccessSecret
            | LoopActionKind::DestructiveMigration
            | LoopActionKind::EnableLoop
    );
    if forbidden {
        return PolicyDecision {
            action,
            allowed: false,
            requires_approval: false,
            reason: "forbidden_action".to_string(),
        };
    }

    let read_only_allowed = matches!(
        action,
        LoopActionKind::ReadMemory | LoopActionKind::ReadRepo
    );
    let can_suggest = matches!(
        action,
        LoopActionKind::WriteMemoryProposal | LoopActionKind::SubmitFeedback
    );
    let can_draft = matches!(
        action,
        LoopActionKind::WriteRepo
            | LoopActionKind::RunCommand
            | LoopActionKind::CreateBranch
            | LoopActionKind::InvokeRunner
    );
    let allowed = match mode {
        LoopMode::Off | LoopMode::Paused | LoopMode::Snoozed => false,
        LoopMode::Observe => read_only_allowed,
        LoopMode::SuggestOnly => read_only_allowed || can_suggest,
        LoopMode::DraftOutput | LoopMode::AutonomousSafe => {
            read_only_allowed || can_suggest || can_draft
        }
    };
    let requires_approval = matches!(
        action,
        LoopActionKind::MutateMemory
            | LoopActionKind::WriteRepo
            | LoopActionKind::RunCommand
            | LoopActionKind::CreateBranch
            | LoopActionKind::InvokeRunner
    );
    PolicyDecision {
        action,
        allowed,
        requires_approval: allowed && requires_approval,
        reason: if allowed {
            "allowed_by_mode".to_string()
        } else {
            "blocked_by_mode".to_string()
        },
    }
}

pub fn budget_blocked(budgets: Option<&Value>) -> Option<String> {
    let budgets = budgets?;
    if budgets
        .get("remaining_runs")
        .and_then(Value::as_i64)
        .is_some_and(|remaining| remaining <= 0)
    {
        return Some("budget_remaining_runs_exhausted".to_string());
    }
    if budgets
        .get("remaining_cost_usd")
        .and_then(Value::as_f64)
        .is_some_and(|remaining| remaining <= 0.0)
    {
        return Some("budget_remaining_cost_exhausted".to_string());
    }
    None
}

#[derive(Debug, Clone)]
pub struct TriggerRouteCandidate {
    pub definition: LoopDefinitionRecord,
    pub effective_settings: EffectiveLoopSettings,
}

pub fn route_trigger_event(
    event_type: &str,
    candidates: impl IntoIterator<Item = TriggerRouteCandidate>,
) -> Vec<LoopTriggerRouteDecision> {
    candidates
        .into_iter()
        .map(|candidate| {
            let supported = definition_supports_trigger(&candidate.definition, event_type);
            let mut skipped_reasons = Vec::new();
            if !supported {
                skipped_reasons.push("unsupported_trigger".to_string());
            }
            skipped_reasons.extend(candidate.effective_settings.blocked_reasons.clone());
            let eligible = supported && skipped_reasons.is_empty();
            LoopTriggerRouteDecision {
                loop_id: candidate.definition.loop_id,
                supported,
                eligible,
                skipped_reasons,
                mode: Some(candidate.effective_settings.mode),
                scope_type: Some(candidate.effective_settings.scope_type),
                scope_id: Some(candidate.effective_settings.scope_id),
                run_id: None,
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ContextPackBuildInput {
    pub loop_id: String,
    pub project: String,
    pub repo_root: Option<String>,
    pub run_id: Option<Uuid>,
    pub generated_at: DateTime<Utc>,
    pub token_budget: usize,
    pub instructions: Vec<LoopContextInstructionRef>,
    pub memories: Vec<MemoryEntryResponse>,
    pub metadata: Value,
}

pub fn build_context_pack(input: ContextPackBuildInput) -> LoopContextPack {
    let mut candidates = input.memories;
    candidates.sort_by(|left, right| {
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

    let mut estimated_tokens = input
        .instructions
        .iter()
        .map(|instruction| instruction.estimated_tokens)
        .sum::<usize>();
    let mut memories = Vec::new();
    let mut exclusions = Vec::new();
    let mut warnings = Vec::new();

    for memory in candidates {
        let memory_tokens =
            estimate_tokens(&format!("{}\n{}", memory.summary, memory.canonical_text));
        if memory.status != MemoryStatus::Active || memory.is_tombstone {
            exclusions.push(LoopContextExclusion {
                memory_id: memory.id,
                summary: memory.summary,
                reason: "memory is not active".to_string(),
                estimated_tokens: memory_tokens,
            });
            continue;
        }
        if estimated_tokens + memory_tokens > input.token_budget {
            exclusions.push(LoopContextExclusion {
                memory_id: memory.id,
                summary: memory.summary,
                reason: "token budget exceeded".to_string(),
                estimated_tokens: memory_tokens,
            });
            continue;
        }

        let stale = memory_is_stale(&memory, input.generated_at);
        let contradictory = memory_is_contradictory(&memory);
        if stale {
            warnings.push(format!(
                "Memory {} has stale or missing provenance.",
                memory.id
            ));
        }
        if contradictory {
            warnings.push(format!(
                "Memory {} may contradict other project context.",
                memory.id
            ));
        }
        estimated_tokens += memory_tokens;
        memories.push(LoopContextMemory {
            memory_id: memory.id,
            canonical_id: memory.canonical_id,
            summary: memory.summary,
            preview: preview_text(&memory.canonical_text),
            memory_type: memory.memory_type,
            confidence: memory.confidence,
            importance: memory.importance,
            freshness: freshness_label(memory.updated_at, input.generated_at),
            updated_at: memory.updated_at,
            tags: memory.tags,
            source_refs: memory
                .sources
                .iter()
                .map(|source| LoopContextSourceRef {
                    source_kind: source.source_kind.clone(),
                    file_path: source.file_path.clone(),
                    git_commit: source.git_commit.clone(),
                    symbol_name: source.symbol_name.clone(),
                    provenance_status: source.provenance.as_ref().map(|item| item.status.clone()),
                })
                .collect(),
            estimated_tokens: memory_tokens,
            stale,
            contradictory,
            inclusion_reason: "ranked by importance, confidence, and recency".to_string(),
        });
    }

    LoopContextPack {
        id: Uuid::new_v4(),
        loop_id: input.loop_id,
        project: input.project,
        repo_root: input.repo_root,
        run_id: input.run_id,
        generated_at: input.generated_at,
        token_budget: input.token_budget,
        estimated_tokens,
        instructions: input.instructions,
        memories,
        exclusions,
        warnings,
        metadata: input.metadata,
    }
}

pub fn diff_context_packs(
    current: &LoopContextPack,
    previous: Option<&LoopContextPack>,
) -> Option<LoopContextPackDiff> {
    let previous = previous?;
    let current_by_id = current
        .memories
        .iter()
        .map(|memory| (memory.memory_id, memory))
        .collect::<BTreeMap<_, _>>();
    let previous_by_id = previous
        .memories
        .iter()
        .map(|memory| (memory.memory_id, memory))
        .collect::<BTreeMap<_, _>>();
    let current_ids = current_by_id.keys().copied().collect::<BTreeSet<_>>();
    let previous_ids = previous_by_id.keys().copied().collect::<BTreeSet<_>>();
    let changed_memory_ids = current_ids
        .intersection(&previous_ids)
        .filter(|id| {
            let Some(current_memory) = current_by_id.get(id) else {
                return false;
            };
            let Some(previous_memory) = previous_by_id.get(id) else {
                return false;
            };
            current_memory.updated_at != previous_memory.updated_at
                || current_memory.confidence != previous_memory.confidence
                || current_memory.stale != previous_memory.stale
                || current_memory.contradictory != previous_memory.contradictory
        })
        .copied()
        .collect();

    Some(LoopContextPackDiff {
        previous_run_id: previous.run_id,
        previous_pack_id: Some(previous.id),
        added_memory_ids: current_ids.difference(&previous_ids).copied().collect(),
        removed_memory_ids: previous_ids.difference(&current_ids).copied().collect(),
        changed_memory_ids,
        token_delta: current.estimated_tokens as isize - previous.estimated_tokens as isize,
        warning_delta: current
            .warnings
            .iter()
            .filter(|warning| !previous.warnings.contains(warning))
            .cloned()
            .collect(),
    })
}

pub fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() / 4).max(1)
}

fn preview_text(text: &str) -> String {
    const MAX_CHARS: usize = 600;
    let mut preview = text.chars().take(MAX_CHARS).collect::<String>();
    if text.chars().count() > MAX_CHARS {
        preview.push_str("...");
    }
    preview
}

fn memory_is_stale(memory: &MemoryEntryResponse, now: DateTime<Utc>) -> bool {
    if now.signed_duration_since(memory.updated_at).num_days() > 180 {
        return true;
    }
    memory.sources.iter().any(|source| {
        source.provenance.as_ref().is_some_and(|provenance| {
            matches!(
                provenance.status,
                SourceProvenanceStatus::MissingFile
                    | SourceProvenanceStatus::MissingSymbol
                    | SourceProvenanceStatus::Stale
            )
        })
    })
}

fn memory_is_contradictory(memory: &MemoryEntryResponse) -> bool {
    memory
        .tags
        .iter()
        .any(|tag| tag.contains("contradict") || tag.contains("conflict"))
        || memory
            .related_memories
            .iter()
            .any(|related| related.relation_type == mem_api::MemoryRelationType::Duplicates)
        || memory.summary.to_lowercase().contains("contradict")
}

fn freshness_label(updated_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let age_days = now.signed_duration_since(updated_at).num_days();
    if age_days <= 7 {
        "fresh".to_string()
    } else if age_days <= 90 {
        "recent".to_string()
    } else if age_days <= 180 {
        "aging".to_string()
    } else {
        "stale".to_string()
    }
}

pub fn definition_supports_trigger(definition: &LoopDefinitionRecord, event_type: &str) -> bool {
    definition
        .trigger_spec
        .get("supported")
        .and_then(Value::as_array)
        .is_some_and(|supported| {
            supported
                .iter()
                .filter_map(Value::as_str)
                .any(|trigger| trigger == event_type)
        })
}

fn scope_precedence(scope_type: &LoopScopeType) -> u8 {
    match scope_type {
        LoopScopeType::User => 0,
        LoopScopeType::Workspace => 1,
        LoopScopeType::Project => 2,
        LoopScopeType::Repo => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> LoopDefinitionRecord {
        builtin_loop_definitions()[0].to_record(Utc::now())
    }

    fn memory(
        summary: &str,
        importance: i32,
        confidence: f32,
        age_days: i64,
    ) -> MemoryEntryResponse {
        let now = Utc::now();
        MemoryEntryResponse {
            id: Uuid::new_v4(),
            project: "memory".to_string(),
            canonical_text: format!("{summary}\nDetailed context for {summary}."),
            summary: summary.to_string(),
            memory_type: mem_api::MemoryType::Architecture,
            importance,
            confidence,
            status: MemoryStatus::Active,
            tags: vec!["context".to_string()],
            sources: vec![mem_api::MemorySourceRecord {
                id: Uuid::new_v4(),
                task_id: None,
                file_path: Some("AGENTS.md".to_string()),
                git_commit: None,
                symbol_name: None,
                symbol_kind: None,
                source_kind: mem_api::SourceKind::File,
                excerpt: None,
                provenance: None,
            }],
            related_memories: Vec::new(),
            embedding_spaces: Vec::new(),
            created_at: now - chrono::Duration::days(age_days),
            updated_at: now - chrono::Duration::days(age_days),
            canonical_id: Uuid::new_v4(),
            version_no: 1,
            is_tombstone: false,
        }
    }

    #[test]
    fn builtins_are_valid_and_versioned() {
        let builtins = builtin_loop_definitions();
        assert_eq!(builtins.len(), 8);
        for builtin in &builtins {
            validate_definition(builtin).expect("builtin definition is valid");
            assert_eq!(builtin.version, 1);
        }
    }

    #[test]
    fn repo_scope_overrides_project_scope() {
        let definition = definition();
        let now = Utc::now();
        let settings = vec![
            LoopSettingRecord {
                id: Uuid::new_v4(),
                loop_id: definition.loop_id.clone(),
                scope_type: LoopScopeType::Project,
                scope_id: "memory".to_string(),
                project: Some("memory".to_string()),
                repo_root: None,
                enabled: Some(true),
                mode: Some(LoopMode::Observe),
                budgets: None,
                approval_overrides: None,
                paused_until: None,
                snoozed_until: None,
                updated_by: None,
                reason: None,
                updated_at: now,
            },
            LoopSettingRecord {
                id: Uuid::new_v4(),
                loop_id: definition.loop_id.clone(),
                scope_type: LoopScopeType::Repo,
                scope_id: "/repo".to_string(),
                project: Some("memory".to_string()),
                repo_root: Some("/repo".to_string()),
                enabled: Some(true),
                mode: Some(LoopMode::SuggestOnly),
                budgets: Some(json!({"remaining_runs": 1})),
                approval_overrides: None,
                paused_until: None,
                snoozed_until: None,
                updated_by: None,
                reason: None,
                updated_at: now,
            },
        ];
        let effective = resolve_effective_settings(&definition, &settings, false, false, now);
        assert_eq!(effective.scope_type, LoopScopeType::Repo);
        assert_eq!(effective.mode, LoopMode::SuggestOnly);
        assert!(effective.blocked_reasons.is_empty());
    }

    #[test]
    fn kill_switch_blocks_non_manual_runs_only() {
        let definition = definition();
        let now = Utc::now();
        let effective = resolve_effective_settings(&definition, &[], true, false, now);
        assert!(
            effective
                .blocked_reasons
                .contains(&"global_kill_switch_enabled".to_string())
        );
        let manual = resolve_effective_settings(&definition, &[], true, true, now);
        assert!(
            !manual
                .blocked_reasons
                .contains(&"global_kill_switch_enabled".to_string())
        );
    }

    #[test]
    fn forbidden_actions_are_denied() {
        let decision = evaluate_action(&LoopMode::AutonomousSafe, LoopActionKind::PushMain);
        assert!(!decision.allowed);
        assert_eq!(decision.reason, "forbidden_action");
    }

    #[test]
    fn draft_actions_require_approval() {
        let decision = evaluate_action(&LoopMode::DraftOutput, LoopActionKind::WriteRepo);
        assert!(decision.allowed);
        assert!(decision.requires_approval);
    }

    #[test]
    fn trigger_router_marks_supported_enabled_loop_eligible() {
        let definition = definition();
        let now = Utc::now();
        let effective = resolve_effective_settings(
            &definition,
            &[LoopSettingRecord {
                id: Uuid::new_v4(),
                loop_id: definition.loop_id.clone(),
                scope_type: LoopScopeType::Project,
                scope_id: "memory".to_string(),
                project: Some("memory".to_string()),
                repo_root: None,
                enabled: Some(true),
                mode: Some(LoopMode::SuggestOnly),
                budgets: None,
                approval_overrides: None,
                paused_until: None,
                snoozed_until: None,
                updated_by: None,
                reason: None,
                updated_at: now,
            }],
            false,
            false,
            now,
        );

        let decisions = route_trigger_event(
            "memory_changed",
            [TriggerRouteCandidate {
                definition,
                effective_settings: effective,
            }],
        );

        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].supported);
        assert!(decisions[0].eligible);
        assert!(decisions[0].skipped_reasons.is_empty());
    }

    #[test]
    fn trigger_router_explains_blocked_or_unsupported_candidates() {
        let definition = definition();
        let now = Utc::now();
        let disabled = resolve_effective_settings(&definition, &[], false, false, now);

        let decisions = route_trigger_event(
            "ci_failed",
            [TriggerRouteCandidate {
                definition,
                effective_settings: disabled,
            }],
        );

        assert_eq!(decisions.len(), 1);
        assert!(!decisions[0].supported);
        assert!(!decisions[0].eligible);
        assert!(
            decisions[0]
                .skipped_reasons
                .contains(&"unsupported_trigger".to_string())
        );
        assert!(
            decisions[0]
                .skipped_reasons
                .contains(&"loop_not_enabled".to_string())
        );
    }

    #[test]
    fn context_pack_enforces_budget_and_preserves_source_refs() {
        let included = memory("Important architecture", 5, 0.95, 2);
        let excluded = memory(&"Large context ".repeat(200), 1, 0.7, 2);
        let pack = build_context_pack(ContextPackBuildInput {
            loop_id: LOOP_CONTEXT_PACK_REFRESH.to_string(),
            project: "memory".to_string(),
            repo_root: Some("/repo".to_string()),
            run_id: Some(Uuid::new_v4()),
            generated_at: Utc::now(),
            token_budget: 80,
            instructions: vec![LoopContextInstructionRef {
                path: "AGENTS.md".to_string(),
                reason: "repo instructions".to_string(),
                estimated_tokens: 4,
            }],
            memories: vec![excluded, included],
            metadata: json!({}),
        });

        assert_eq!(pack.memories.len(), 1);
        assert_eq!(pack.memories[0].summary, "Important architecture");
        assert_eq!(pack.memories[0].source_refs.len(), 1);
        assert_eq!(pack.exclusions.len(), 1);
        assert_eq!(pack.exclusions[0].reason, "token budget exceeded");
    }

    #[test]
    fn context_pack_flags_stale_and_diff_changes() {
        let mut old = memory("Old convention", 4, 0.9, 365);
        old.sources[0].provenance = Some(mem_api::SourceProvenanceRecord {
            status: SourceProvenanceStatus::Stale,
            checked_at: Utc::now(),
            reason: Some("file changed".to_string()),
            resolved_path: Some("AGENTS.md".to_string()),
        });
        let current = build_context_pack(ContextPackBuildInput {
            loop_id: LOOP_CONTEXT_PACK_REFRESH.to_string(),
            project: "memory".to_string(),
            repo_root: None,
            run_id: Some(Uuid::new_v4()),
            generated_at: Utc::now(),
            token_budget: 500,
            instructions: Vec::new(),
            memories: vec![old.clone()],
            metadata: json!({}),
        });
        let previous = LoopContextPack {
            id: Uuid::new_v4(),
            loop_id: current.loop_id.clone(),
            project: current.project.clone(),
            repo_root: None,
            run_id: Some(Uuid::new_v4()),
            generated_at: Utc::now(),
            token_budget: 500,
            estimated_tokens: 0,
            instructions: Vec::new(),
            memories: Vec::new(),
            exclusions: Vec::new(),
            warnings: Vec::new(),
            metadata: json!({}),
        };

        assert!(current.memories[0].stale);
        assert_eq!(current.warnings.len(), 1);
        let diff = diff_context_packs(&current, Some(&previous)).expect("diff exists");
        assert_eq!(diff.added_memory_ids, vec![old.id]);
        assert!(diff.token_delta > 0);
    }
}
