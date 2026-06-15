use chrono::{DateTime, Utc};
use mem_api::{
    EffectiveLoopSettings, LoopActionKind, LoopDefinitionRecord, LoopMode, LoopRiskLevel,
    LoopScopeType, LoopSettingRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
}
