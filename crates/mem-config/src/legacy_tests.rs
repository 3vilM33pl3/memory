// SPDX-License-Identifier: AGPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use std::{
    env, fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use chrono::Utc;
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[allow(unused_imports)]
use crate::*;
#[allow(unused_imports)]
use mem_record::*;

#[test]
fn auth_defaults_preserve_single_user_installations() {
    let config: AppConfig =
        toml::from_str("[service]\n\n[database]\nurl = \"postgresql://memory\"\n")
            .expect("parse minimal config");

    assert_eq!(config.auth.mode, AuthMode::SingleUser);
    assert_eq!(config.auth.session_ttl, Duration::from_secs(12 * 60 * 60));
    assert!(!config.auth.multi_user_legacy_token_enabled);
    assert_eq!(config.auth.oidc.groups_claim, "groups");
    assert_eq!(config.auth.oidc.scopes, ["openid", "profile", "email"]);
}

#[test]
fn auth_config_parses_multiuser_group_mappings() {
    let config: AppConfig = toml::from_str(
        r#"
                [service]

                [database]
                url = "postgresql://memory"

                [auth]
                mode = "multi_user"
                public_base_url = "https://memory.example.test"
                session_ttl = "8h"

                [auth.oidc]
                issuer_url = "https://auth.example.test/application/o/memory/"
                client_id = "memory"

                [[auth.group_mappings.rules]]
                group = "memory-admins"
                role = "admin"
                global = true

                [[auth.group_mappings.rules]]
                group = "memory-writers"
                role = "writer"
                project = "memory"
            "#,
    )
    .expect("parse multiuser config");

    assert_eq!(config.auth.mode, AuthMode::MultiUser);
    assert_eq!(config.auth.session_ttl, Duration::from_secs(8 * 60 * 60));
    assert_eq!(config.auth.group_mappings.rules.len(), 2);
    assert!(config.auth.group_mappings.rules[0].global);
    assert_eq!(
        config.auth.group_mappings.rules[1].project.as_deref(),
        Some("memory")
    );
    assert!(AuthRole::Admin > AuthRole::Operator);
}

#[test]
fn new_memory_types_display_as_snake_case() {
    assert_eq!(MemoryType::Task.to_string(), "task");
    assert_eq!(MemoryType::Documentation.to_string(), "documentation");
    assert_eq!(MemoryType::Refactor.to_string(), "refactor");
}

#[test]
fn loop_blocked_reasons_serialize_when_empty() {
    let effective = EffectiveLoopSettings {
        loop_id: "memory_hygiene".to_string(),
        enabled: true,
        mode: LoopMode::AutonomousSafe,
        scope_type: LoopScopeType::Repo,
        scope_id: "/repo".to_string(),
        global_kill_switch: false,
        blocked_reasons: Vec::new(),
        budgets: None,
        approval_overrides: None,
        paused_until: None,
        snoozed_until: None,
    };
    let effective_json = serde_json::to_value(&effective).expect("serialize effective settings");
    assert_eq!(effective_json["blocked_reasons"], serde_json::json!([]));

    let summary = LoopRunSummary {
        id: Uuid::nil(),
        loop_id: "memory_hygiene".to_string(),
        definition_version: 1,
        project: Some("memory".to_string()),
        repo_root: Some("/repo".to_string()),
        mode: LoopMode::AutonomousSafe,
        status: LoopRunStatus::Succeeded,
        started_at: Utc::now(),
        finished_at: None,
        output_summary: None,
        trace_count: 0,
        blocked_reasons: Vec::new(),
    };
    let summary_json = serde_json::to_value(&summary).expect("serialize run summary");
    assert_eq!(summary_json["blocked_reasons"], serde_json::json!([]));
}

#[test]
fn ollama_llm_uses_local_default_and_no_inherited_openai_key() {
    let config = LlmConfig {
        provider: OLLAMA_PROVIDER.to_string(),
        base_url: OPENAI_COMPATIBLE_BASE_URL.to_string(),
        api_key_env: "OPENAI_API_KEY".to_string(),
        model: "llama3.2".to_string(),
        ..LlmConfig::default()
    };
    assert_eq!(effective_llm_base_url(&config), OLLAMA_BASE_URL);
    assert_eq!(llm_max_output_tokens_field(&config.provider), "max_tokens");
    assert!(!llm_requires_api_key(&config));
    assert!(resolve_llm_api_key(&config).is_none());
    let empty_key_config = LlmConfig {
        api_key_env: String::new(),
        ..config
    };
    assert!(!llm_requires_api_key(&empty_key_config));
}

#[test]
fn openai_compatible_llm_keeps_existing_defaults() {
    let config = LlmConfig::default();
    assert_eq!(effective_llm_base_url(&config), OPENAI_COMPATIBLE_BASE_URL);
    assert_eq!(
        llm_max_output_tokens_field(&config.provider),
        "max_completion_tokens"
    );
    assert!(llm_requires_api_key(&config));
}

#[test]
fn procedural_config_defaults_on_and_roundtrips() {
    let config: AppConfig = toml::from_str(
        r#"
            [service]
            bind_addr = "127.0.0.1:4040"
            api_token = "token"
            request_timeout = "30s"

            [database]
            url = "postgresql://memory"
            "#,
    )
    .expect("parse config without procedural section");
    // On by default: deterministic and advisory, like activation scoring.
    assert!(config.procedural.enabled);
    assert!(!config.procedural.utility_floor_enabled);
    assert_eq!(config.procedural.alpha, 0.2);
    assert_eq!(config.procedural.reward_approved, 1.0);
    assert_eq!(config.procedural.reward_rejected, -1.0);

    let overridden: AppConfig = toml::from_str(
        r#"
            [service]
            bind_addr = "127.0.0.1:4040"
            api_token = "token"
            request_timeout = "30s"

            [database]
            url = "postgresql://memory"

            [procedural]
            enabled = false
            alpha = 0.5
            reward_rejected = -2.0
            "#,
    )
    .expect("parse config with procedural overrides");
    assert!(!overridden.procedural.enabled);
    assert_eq!(overridden.procedural.alpha, 0.5);
    assert_eq!(overridden.procedural.reward_rejected, -2.0);
    // Unspecified knobs keep their defaults.
    assert_eq!(overridden.procedural.reward_approved, 1.0);
    assert_eq!(overridden.procedural.min_samples, 5);
}

#[test]
fn consolidation_config_defaults_off_and_roundtrips() {
    let config: AppConfig = toml::from_str(
        r#"
            [service]
            bind_addr = "127.0.0.1:4040"
            api_token = "token"
            request_timeout = "30s"

            [database]
            url = "postgresql://memory"
            "#,
    )
    .expect("parse config without consolidation section");
    // Off and dry-run by default, matching the reinforcement posture.
    assert!(!config.consolidation.enabled);
    assert!(config.consolidation.dry_run);
    assert!(config.consolidation.auto_trigger);
    assert_eq!(config.consolidation.min_size, 3);
    assert_eq!(config.consolidation.sim_floor, 0.82);

    let overridden: AppConfig = toml::from_str(
        r#"
            [service]
            bind_addr = "127.0.0.1:4040"
            api_token = "token"
            request_timeout = "30s"

            [database]
            url = "postgresql://memory"

            [consolidation]
            enabled = true
            dry_run = false
            min_size = 4
            "#,
    )
    .expect("parse config with consolidation overrides");
    assert!(overridden.consolidation.enabled);
    assert!(!overridden.consolidation.dry_run);
    assert_eq!(overridden.consolidation.min_size, 4);
    // Unspecified knobs keep their defaults.
    assert_eq!(overridden.consolidation.max_size, 25);
}

#[test]
fn llm_audit_config_defaults_to_safe_disabled_mode() {
    let config: AppConfig = toml::from_str(
        r#"
            [service]
            bind_addr = "127.0.0.1:4040"
            api_token = "token"
            request_timeout = "30s"

            [database]
            url = "postgresql://memory"
            "#,
    )
    .expect("parse config without llm_audit");

    assert!(!config.llm_audit.enabled);
    assert!(config.llm_audit.redact);
    assert_eq!(config.llm_audit.max_message_chars, 8_000);
    assert_eq!(config.llm_audit.max_total_chars, 32_000);
}

#[test]
fn llm_audit_activity_details_roundtrip() {
    let details = ActivityDetails::LlmAudit {
        operation: "query_answer".to_string(),
        request_summary: "Question: activity".to_string(),
        status: "success".to_string(),
        redacted: true,
        truncated: false,
        messages: vec![LlmAuditMessage {
            role: "user".to_string(),
            content: "Question: activity".to_string(),
            truncated: false,
        }],
        error: None,
    };

    let encoded = serde_json::to_value(&details).expect("serialize details");
    assert_eq!(encoded["type"], "llm_audit");
    let decoded: ActivityDetails = serde_json::from_value(encoded).expect("deserialize details");

    match decoded {
        ActivityDetails::LlmAudit {
            operation,
            messages,
            redacted,
            ..
        } => {
            assert_eq!(operation, "query_answer");
            assert!(redacted);
            assert_eq!(messages[0].role, "user");
        }
        other => panic!("unexpected activity details: {other:?}"),
    }
}

#[test]
fn profile_display_version_adds_dev_suffix_only_in_dev() {
    assert_eq!(Profile::Prod.version_suffix(), "");
    assert_eq!(Profile::Dev.version_suffix(), "-dev");
    assert_eq!(Profile::Prod.display_version("0.6.0"), "0.6.0");
    assert_eq!(Profile::Dev.display_version("0.6.0"), "0.6.0-dev");
}

fn parse_embeddings(input: &str) -> EmbeddingsConfig {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default)]
        embeddings: EmbeddingsConfig,
    }
    let wrap: Wrap = toml::from_str(input).expect("parse embeddings TOML");
    wrap.embeddings
}

#[test]
fn embeddings_config_legacy_singleton_deserializes_and_auto_names() {
    let mut cfg = parse_embeddings(
        r#"
            [embeddings]
            provider = "voyage"
            model = "voyage-code-3"
            api_key_env = "VOYAGE_API_KEY"
            "#,
    );
    cfg.normalize_backend_names();
    assert_eq!(cfg.backends.len(), 1);
    let only = &cfg.backends[0];
    assert_eq!(only.provider, "voyage");
    assert_eq!(only.model, "voyage-code-3");
    assert_eq!(only.name, "voyage-voyage-code-3");
    assert_eq!(cfg.active.as_deref(), Some("voyage-voyage-code-3"));
}

#[test]
fn embeddings_config_new_form_with_multiple_backends() {
    let mut cfg = parse_embeddings(
        r#"
            [embeddings]
            active = "voyage-code"

            [[embeddings.backends]]
            name = "openai-3-small"
            provider = "openai"
            model = "text-embedding-3-small"
            api_key_env = "OPENAI_API_KEY"
            dimensions = 512

            [[embeddings.backends]]
            name = "voyage-code"
            provider = "voyage"
            model = "voyage-code-3"
            api_key_env = "VOYAGE_API_KEY"
            "#,
    );
    cfg.normalize_backend_names();
    assert_eq!(cfg.backends.len(), 2);
    assert!(cfg.enabled);
    assert!(cfg.create_enabled);
    assert_eq!(cfg.active.as_deref(), Some("voyage-code"));
    assert_eq!(cfg.backend("openai-3-small").unwrap().provider, "openai");
    assert_eq!(cfg.backend("openai-3-small").unwrap().dimensions, Some(512));
    assert_eq!(cfg.active_backend().unwrap().model, "voyage-code-3");
}

#[test]
fn embeddings_config_create_enabled_false_keeps_search_enabled() {
    let mut cfg = parse_embeddings(
        r#"
            [embeddings]
            create_enabled = false
            active = "openai"

            [[embeddings.backends]]
            name = "openai"
            provider = "openai"
            model = "text-embedding-3-small"
            create_enabled = false
            "#,
    );
    cfg.normalize_backend_names();

    assert!(cfg.enabled);
    assert!(!cfg.create_enabled);
    assert!(!cfg.backend("openai").unwrap().create_enabled);
    assert_eq!(cfg.active_backend().unwrap().name, "openai");
}

#[test]
fn embeddings_config_enabled_false_disables_active_backend() {
    let mut cfg = parse_embeddings(
        r#"
            [embeddings]
            enabled = false
            active = "openai"

            [[embeddings.backends]]
            name = "openai"
            provider = "openai"
            model = "text-embedding-3-small"
            "#,
    );
    cfg.normalize_backend_names();

    assert!(!cfg.enabled);
    assert_eq!(cfg.active.as_deref(), Some("openai"));
    assert!(cfg.active_backend().is_none());
}

#[test]
fn embeddings_config_duplicate_names_get_unique_suffixes() {
    let mut cfg = parse_embeddings(
        r#"
            [[embeddings.backends]]
            name = "shared"
            provider = "openai_compatible"
            model = "a"

            [[embeddings.backends]]
            name = "shared"
            provider = "voyage"
            model = "b"
            "#,
    );
    cfg.normalize_backend_names();
    let names: Vec<_> = cfg.backends.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["shared", "shared-2"]);
}

#[test]
fn embeddings_config_unknown_active_falls_back_to_sole_backend() {
    let mut cfg = parse_embeddings(
        r#"
            [embeddings]
            active = "does-not-exist"

            [[embeddings.backends]]
            name = "openai"
            provider = "openai"
            model = "text-embedding-3-small"
            "#,
    );
    cfg.normalize_backend_names();
    // With exactly one backend configured, an unknown `active`
    // collapses onto that backend rather than leaving search
    // silently disabled.
    assert_eq!(cfg.active.as_deref(), Some("openai"));
    assert_eq!(
        cfg.active_backend().unwrap().model,
        "text-embedding-3-small"
    );
}

#[test]
fn embeddings_config_unknown_active_with_multiple_backends_clears_active() {
    let mut cfg = parse_embeddings(
        r#"
            [embeddings]
            active = "does-not-exist"

            [[embeddings.backends]]
            name = "a"
            provider = "openai"
            model = "m1"

            [[embeddings.backends]]
            name = "b"
            provider = "voyage"
            model = "m2"
            "#,
    );
    cfg.normalize_backend_names();
    assert_eq!(cfg.active, None);
    assert!(cfg.active_backend().is_none());
}

#[test]
fn embeddings_config_empty_table_produces_no_backends() {
    let cfg = parse_embeddings("[embeddings]\n");
    assert!(cfg.backends.is_empty());
    assert!(cfg.active.is_none());
}

#[test]
fn prune_history_rejects_missing_thresholds() {
    let request = PruneHistoryRequest::default();
    let err = request
        .validate()
        .expect_err("missing thresholds must fail");
    let message = format!("{err}");
    assert!(
        message.contains("no retention threshold configured"),
        "unexpected message: {message}"
    );
}

#[test]
fn prune_history_accepts_either_threshold_alone() {
    let req = PruneHistoryRequest {
        tombstone_after: Some(Duration::from_secs(86_400)),
        ..PruneHistoryRequest::default()
    };
    assert!(req.validate().is_ok());

    let req = PruneHistoryRequest {
        superseded_after: Some(Duration::from_secs(3_600)),
        ..PruneHistoryRequest::default()
    };
    assert!(req.validate().is_ok());
}

#[test]
fn prune_history_rejects_empty_project_override() {
    let req = PruneHistoryRequest {
        project: Some(String::new()),
        tombstone_after: Some(Duration::from_secs(10)),
        ..PruneHistoryRequest::default()
    };
    assert!(req.validate().is_err());
}

#[test]
fn query_request_rejects_empty_query() {
    let request = QueryRequest {
        project: "memory".to_string(),
        query: String::new(),
        filters: QueryFilters::default(),
        top_k: 8,
        min_confidence: None,
        include_stale: false,
        history: false,
        retrieval_mode: None,
        answer_mode: None,
    };

    assert!(request.validate().is_err());
}

#[test]
fn global_query_request_rejects_empty_query_without_project() {
    let request = GlobalQueryRequest {
        query: String::new(),
        filters: QueryFilters::default(),
        top_k: 8,
        min_confidence: None,
        include_stale: false,
        history: false,
        retrieval_mode: None,
        answer_mode: None,
    };

    assert!(request.validate().is_err());
}

#[test]
fn query_response_defaults_answer_metadata_for_older_json() {
    let payload = serde_json::json!({
        "answer": "Stored answer",
        "confidence": 0.7,
        "results": [{
            "memory_id": "11111111-1111-1111-1111-111111111111",
            "summary": "Old result",
            "memory_type": "implementation",
            "score": 1.0,
            "snippet": "Older clients did not send project metadata."
        }],
        "insufficient_evidence": false
    });

    let response: QueryResponse =
        serde_json::from_value(payload).expect("query response should deserialize");

    assert_eq!(response.answer, "Stored answer");
    assert_eq!(
        response.answer_generation.method,
        QueryAnswerMethod::Deterministic
    );
    assert_eq!(response.results.len(), 1);
    assert!(response.results[0].project.is_none());
    assert!(response.results[0].project_name.is_none());
    assert!(response.results[0].repo_root.is_none());
    assert!(response.answer_generation.cited_result_numbers.is_empty());
    assert!(response.answer_citations.is_empty());
}

#[test]
fn query_activity_details_defaults_graph_metadata_for_older_json() {
    let payload = serde_json::json!({
        "type": "query",
        "query": "How does query activity work?",
        "top_k": 8,
        "result_count": 2,
        "confidence": 0.7,
        "insufficient_evidence": false,
        "total_duration_ms": 42,
        "answer": "Stored answer"
    });

    let details: ActivityDetails =
        serde_json::from_value(payload).expect("query activity details should deserialize");

    match details {
        ActivityDetails::Query {
            graph_status,
            graph_candidates,
            graph_augmented_candidates,
            graph_duration_ms,
            graph_result_count,
            graph_connection_count,
            graph_connections,
            ..
        } => {
            assert_eq!(graph_status, None);
            assert_eq!(graph_candidates, 0);
            assert_eq!(graph_augmented_candidates, 0);
            assert_eq!(graph_duration_ms, 0);
            assert_eq!(graph_result_count, 0);
            assert_eq!(graph_connection_count, 0);
            assert!(graph_connections.is_empty());
        }
        other => panic!("unexpected activity details: {other:?}"),
    }
}

#[test]
fn capture_task_requires_project() {
    let request = CaptureTaskRequest {
        project: String::new(),
        task_title: "task".to_string(),
        user_prompt: "prompt".to_string(),
        writer_id: "codex-writer".to_string(),
        writer_name: Some("Codex".to_string()),
        agent_summary: "summary".to_string(),
        files_changed: Vec::new(),
        git_diff_summary: None,
        git_commit: None,
        tests: Vec::new(),
        notes: Vec::new(),
        structured_candidates: Vec::new(),
        command_output: None,
        idempotency_key: None,
        dry_run: false,
    };

    assert!(request.validate().is_err());
}

#[test]
fn commit_sync_request_requires_commits() {
    let request = CommitSyncRequest {
        project: "memory".to_string(),
        repo_root: "/repo".to_string(),
        commits: Vec::new(),
        dry_run: false,
    };

    assert!(request.validate().is_err());
}

#[test]
fn plan_activity_request_requires_valid_counts() {
    let request = PlanActivityRequest {
        project: "memory".to_string(),
        action: PlanActivityAction::FinishBlocked,
        title: "Plan".to_string(),
        thread_key: "thread".to_string(),
        total_items: 1,
        completed_items: 2,
        remaining_items: vec!["left".to_string()],
        source_path: None,
    };

    assert!(request.validate().is_err());
}

#[test]
fn plan_activity_request_requires_thread_key() {
    let request = PlanActivityRequest {
        project: "memory".to_string(),
        action: PlanActivityAction::Started,
        title: "Plan".to_string(),
        thread_key: String::new(),
        total_items: 1,
        completed_items: 0,
        remaining_items: vec!["left".to_string()],
        source_path: None,
    };

    assert!(request.validate().is_err());
}

#[test]
fn graph_activity_request_requires_persisted_run_for_non_dry_run() {
    let request = GraphActivityRequest {
        project: "memory".to_string(),
        repo_root: "/repo".to_string(),
        git_head: Some("abc123".to_string()),
        since: None,
        extraction_run_id: None,
        dry_run: false,
        reused_existing_run: false,
        index_reused: true,
        analyzer_version: "mem-analyze-v2".to_string(),
        strategy_version: "code-graph-resolution-v1".to_string(),
        symbol_count: 1,
        reference_count: 2,
        resolved_reference_count: 1,
        unresolved_reference_count: 1,
        ambiguous_reference_count: 0,
        graph_node_count: 1,
        graph_edge_count: 1,
        evidence_count: 2,
    };

    assert!(request.validate().is_err());
}

#[test]
fn graph_extract_activity_details_roundtrip() {
    let run_id = Uuid::new_v4();
    let details = ActivityDetails::GraphExtract {
        repo_root: "/repo".to_string(),
        git_head: Some("abc123".to_string()),
        since: Some("HEAD~1".to_string()),
        extraction_run_id: Some(run_id),
        dry_run: false,
        reused_existing_run: false,
        index_reused: true,
        analyzer_version: "mem-analyze-v2".to_string(),
        strategy_version: "code-graph-resolution-v1".to_string(),
        symbol_count: 10,
        reference_count: 20,
        resolved_reference_count: 12,
        unresolved_reference_count: 7,
        ambiguous_reference_count: 1,
        graph_node_count: 10,
        graph_edge_count: 9,
        evidence_count: 19,
    };

    let encoded = serde_json::to_value(&details).expect("serialize details");
    let decoded: ActivityDetails = serde_json::from_value(encoded).expect("deserialize details");

    match decoded {
        ActivityDetails::GraphExtract {
            extraction_run_id,
            symbol_count,
            graph_edge_count,
            ..
        } => {
            assert_eq!(extraction_run_id, Some(run_id));
            assert_eq!(symbol_count, 10);
            assert_eq!(graph_edge_count, 9);
        }
        other => panic!("unexpected activity details: {other:?}"),
    }
}

#[test]
fn finds_repo_local_mem_config() {
    let temp_dir = unique_temp_dir("mem-api-config");
    let mem_dir = temp_dir.join(".mem");
    fs::create_dir_all(&mem_dir).unwrap();
    let config_path = mem_dir.join("config.toml");
    fs::write(&config_path, "test = true\n").unwrap();

    let nested = temp_dir.join("nested").join("deeper");
    fs::create_dir_all(&nested).unwrap();

    let discovered = find_repo_config_path(&nested).unwrap();
    assert_eq!(discovered, config_path);

    let _ = fs::remove_dir_all(temp_dir);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn prefers_xdg_global_config_path_when_present() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-global");
    let config_home = temp_dir.join("config-home");
    fs::create_dir_all(config_home.join("memory-layer")).unwrap();
    let global_path = config_home.join("memory-layer").join("memory-layer.toml");
    fs::write(&global_path, "test = true\n").unwrap();

    unsafe {
        env::set_var("XDG_CONFIG_HOME", &config_home);
    }
    let discovered = discover_global_config_path().unwrap();
    unsafe {
        env::remove_var("XDG_CONFIG_HOME");
    }

    assert_eq!(discovered, global_path);
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn repo_config_is_found_from_nested_directory() {
    let temp_dir = unique_temp_dir("mem-api-repo");
    let mem_dir = temp_dir.join(".mem");
    fs::create_dir_all(&mem_dir).unwrap();
    let config_path = mem_dir.join("config.toml");
    fs::write(&config_path, "[automation]\nenabled = false\n").unwrap();

    let nested = temp_dir.join("nested").join("src");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(find_repo_config_path(&nested).unwrap(), config_path);
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn project_home_config_wins_over_legacy_repo_config() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-project-home");
    let repo_dir = temp_dir.join("repo");
    let config_home = temp_dir.join("config-home");
    #[cfg(not(target_os = "windows"))]
    let state_home = temp_dir.join("state-home");
    #[cfg(not(target_os = "windows"))]
    let cache_home = temp_dir.join("cache-home");
    #[cfg(target_os = "windows")]
    let old_local_app_data = env::var("LOCALAPPDATA").ok();
    #[cfg(not(target_os = "windows"))]
    let old_config_home = env::var("XDG_CONFIG_HOME").ok();
    #[cfg(not(target_os = "windows"))]
    let old_state_home = env::var("XDG_STATE_HOME").ok();
    #[cfg(not(target_os = "windows"))]
    let old_cache_home = env::var("XDG_CACHE_HOME").ok();
    fs::create_dir_all(repo_dir.join(".mem")).unwrap();
    fs::write(
        repo_dir.join(".mem").join("project.toml"),
        "slug = \"demo\"\n",
    )
    .unwrap();
    fs::write(
        repo_dir.join(".mem").join("config.toml"),
        "[automation]\nenabled = false\n",
    )
    .unwrap();

    unsafe {
        #[cfg(target_os = "windows")]
        env::set_var("LOCALAPPDATA", &config_home);
        #[cfg(not(target_os = "windows"))]
        {
            env::set_var("XDG_CONFIG_HOME", &config_home);
            env::set_var("XDG_STATE_HOME", &state_home);
            env::set_var("XDG_CACHE_HOME", &cache_home);
        }
    }
    let paths = project_paths_for_repo(&repo_dir).unwrap();
    fs::create_dir_all(&paths.config_dir).unwrap();
    fs::write(paths.config_path(), "[automation]\nenabled = true\n").unwrap();

    assert_eq!(
        find_repo_config_path(&repo_dir).unwrap(),
        paths.config_path()
    );

    unsafe {
        #[cfg(target_os = "windows")]
        if let Some(value) = old_local_app_data {
            env::set_var("LOCALAPPDATA", value);
        } else {
            env::remove_var("LOCALAPPDATA");
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Some(value) = old_config_home {
                env::set_var("XDG_CONFIG_HOME", value);
            } else {
                env::remove_var("XDG_CONFIG_HOME");
            }
            if let Some(value) = old_state_home {
                env::set_var("XDG_STATE_HOME", value);
            } else {
                env::remove_var("XDG_STATE_HOME");
            }
            if let Some(value) = old_cache_home {
                env::set_var("XDG_CACHE_HOME", value);
            } else {
                env::remove_var("XDG_CACHE_HOME");
            }
        }
    }
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn loads_repo_agent_settings_from_agents_directory() {
    let temp_dir = unique_temp_dir("mem-api-agent-settings");
    let agents_dir = temp_dir.join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
            agents_dir.join("memory-layer.toml"),
            "[capture]\ninclude_paths = [\"ops/\"]\nignore_paths = [\"tmp/\"]\n\n[analysis]\nanalyzers = [\"rust\"]\n\n[retrieval]\ngraph_enabled = true\n",
        )
        .unwrap();

    let settings = load_repo_agent_settings(&temp_dir).unwrap();

    assert_eq!(settings.capture.include_paths, vec!["ops/"]);
    assert_eq!(settings.analysis.analyzers, vec!["rust"]);
    assert!(settings.retrieval.graph_enabled);
    let _ = fs::remove_dir_all(temp_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn prefers_macos_application_support_global_config_path_when_present() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-macos-global");
    let home = temp_dir.join("home");
    let app_support = home
        .join("Library")
        .join("Application Support")
        .join("memory-layer");
    fs::create_dir_all(&app_support).unwrap();
    let global_path = app_support.join("memory-layer.toml");
    fs::write(&global_path, "test = true\n").unwrap();
    let original_home = env::var("HOME").ok();

    unsafe {
        env::remove_var("XDG_CONFIG_HOME");
        env::set_var("HOME", &home);
    }
    let discovered = discover_global_config_path().unwrap();
    unsafe {
        if let Some(value) = original_home {
            env::set_var("HOME", value);
        } else {
            env::remove_var("HOME");
        }
    }

    assert_eq!(discovered, global_path);
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn shared_env_file_overrides_config_for_explicit_path() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-shared-env");
    let config_dir = temp_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("memory-layer.toml");
    fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
    fs::write(
            config_dir.join("memory-layer.env"),
            "MEMORY_LAYER__DATABASE__URL=postgresql://from-env\nMEMORY_LAYER__SERVICE__API_TOKEN=from-env\nOPENAI_API_KEY=test\n",
        )
        .unwrap();

    unsafe {
        env::remove_var("MEMORY_LAYER__DATABASE__URL");
        env::remove_var("MEMORY_LAYER__SERVICE__API_TOKEN");
    }
    let config = AppConfig::load_with_profile(Some(config_path), Profile::Prod).unwrap();
    unsafe {
        env::remove_var("MEMORY_LAYER__DATABASE__URL");
        env::remove_var("MEMORY_LAYER__SERVICE__API_TOKEN");
    }

    assert_eq!(config.database.url, "postgresql://from-env");
    assert_eq!(config.service.api_token, "from-env");
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn mcp_config_defaults_to_read_only_http_mount() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-mcp-config");
    let config_path = temp_dir.join("memory-layer.toml");
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();

    let config = AppConfig::load_with_profile(Some(config_path), Profile::Prod).unwrap();

    assert!(config.mcp.enabled);
    assert!(config.mcp.http_enabled);
    assert_eq!(config.mcp.http_path, "/mcp");
    assert!(config.mcp.require_token);
    assert!(config.mcp.read_only);
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn process_env_still_wins_over_env_file_for_explicit_path() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-env-precedence");
    let config_dir = temp_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("memory-layer.toml");
    fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
    fs::write(
        config_dir.join("memory-layer.env"),
        "MEMORY_LAYER__DATABASE__URL=postgresql://from-env-file\n",
    )
    .unwrap();

    unsafe {
        env::remove_var("MEMORY_LAYER__DATABASE__URL");
        env::set_var(
            "MEMORY_LAYER__DATABASE__URL",
            "postgresql://from-process-env",
        );
    }
    let config = AppConfig::load_with_profile(Some(config_path), Profile::Prod).unwrap();
    unsafe {
        env::remove_var("MEMORY_LAYER__DATABASE__URL");
    }

    assert_eq!(config.database.url, "postgresql://from-process-env");
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn credential_env_value_reads_adjacent_env_file() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-credential-env-file");
    let config_path = temp_dir.join("memory-layer.toml");
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
    fs::write(
        temp_dir.join("memory-layer.env"),
        "MEMORY_LAYER_OIDC_CLIENT_SECRET=from-env-file\n",
    )
    .unwrap();

    unsafe {
        env::remove_var("MEMORY_LAYER_OIDC_CLIENT_SECRET");
    }
    let config = AppConfig::load_with_profile(Some(config_path), Profile::Prod).unwrap();

    assert_eq!(
        config
            .credential_env_value("MEMORY_LAYER_OIDC_CLIENT_SECRET")
            .as_deref(),
        Some("from-env-file")
    );
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn credential_env_value_prefers_process_environment() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-credential-process-env");
    let config_path = temp_dir.join("memory-layer.toml");
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(
            &config_path,
            "[service]\nbind_addr = \"127.0.0.1:4040\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n",
        )
        .unwrap();
    fs::write(
        temp_dir.join("memory-layer.env"),
        "MEMORY_LAYER_OIDC_CLIENT_SECRET=from-env-file\n",
    )
    .unwrap();

    unsafe {
        env::set_var("MEMORY_LAYER_OIDC_CLIENT_SECRET", "from-process-env");
    }
    let config = AppConfig::load_with_profile(Some(config_path), Profile::Prod).unwrap();
    let value = config.credential_env_value("MEMORY_LAYER_OIDC_CLIENT_SECRET");
    unsafe {
        env::remove_var("MEMORY_LAYER_OIDC_CLIENT_SECRET");
    }

    assert_eq!(value.as_deref(), Some("from-process-env"));
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn read_repo_project_slug_uses_project_metadata() {
    let repo_root = unique_temp_dir("mem-api-project-slug");
    fs::create_dir_all(repo_root.join(".mem")).unwrap();
    fs::write(
        repo_root.join(".mem").join("project.toml"),
        "slug = \"sctp\"\nrepo_root = \"/tmp/sctp-conformance\"\n",
    )
    .unwrap();

    assert_eq!(read_repo_project_slug(&repo_root).as_deref(), Some("sctp"));

    let _ = fs::remove_dir_all(repo_root);
}

#[test]
fn legacy_and_new_capture_threshold_keys_can_be_merged() {
    let _guard = env_lock().lock().unwrap();
    let temp_dir = unique_temp_dir("mem-api-threshold-merge");
    let config_home = temp_dir.join("config-home");
    let repo_dir = temp_dir.join("repo");
    let global_dir = config_home.join("memory-layer");
    let mem_dir = repo_dir.join(".mem");
    fs::create_dir_all(&global_dir).unwrap();
    fs::create_dir_all(&mem_dir).unwrap();

    fs::write(
            global_dir.join("memory-layer.toml"),
            "[service]\nbind_addr = \"127.0.0.1:4040\"\napi_token = \"from-config\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://config\"\n\n[automation]\nidle_threshold = \"5m\"\n",
        )
        .unwrap();
    fs::write(
        mem_dir.join("config.toml"),
        "[automation]\ncapture_idle_threshold = \"10m\"\n",
    )
    .unwrap();

    let original_dir = env::current_dir().unwrap();
    unsafe {
        env::set_var("XDG_CONFIG_HOME", &config_home);
    }
    env::set_current_dir(&repo_dir).unwrap();
    let config = AppConfig::load_with_profile(None, Profile::Prod).unwrap();
    env::set_current_dir(original_dir).unwrap();
    unsafe {
        env::remove_var("XDG_CONFIG_HOME");
    }

    assert_eq!(
        config.automation.capture_idle_threshold,
        Duration::from_secs(600)
    );
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn dev_profile_overlays_config_dev_toml_on_top_of_base() {
    let temp_dir = unique_temp_dir("mem-api-dev-overlay");
    let mem_dir = temp_dir.join(".mem");
    fs::create_dir_all(&mem_dir).unwrap();
    fs::write(
            mem_dir.join("config.toml"),
            "[service]\nbind_addr = \"10.0.0.1:4150\"\napi_token = \"t\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://shared\"\n",
        )
        .unwrap();
    fs::write(
        mem_dir.join("config.dev.toml"),
        "[service]\nbind_addr = \"127.0.0.1:4250\"\n",
    )
    .unwrap();
    fs::write(
        mem_dir.join("memory-layer.env"),
        "MEMORY_LAYER_TEST_DEV_CREDENTIAL=from-dev-env-file\n",
    )
    .unwrap();

    let config =
        AppConfig::load_with_profile(Some(mem_dir.join("config.toml")), Profile::Dev).unwrap();

    assert_eq!(config.profile, Profile::Dev);
    assert_eq!(config.service.bind_addr, "127.0.0.1:4250");
    assert_eq!(config.database.url, "postgresql://shared");
    assert_eq!(
        config.resolved_dev_overlay_path.as_deref(),
        Some(mem_dir.join("config.dev.toml").as_path())
    );
    assert_eq!(
        config
            .credential_env_value("MEMORY_LAYER_TEST_DEV_CREDENTIAL")
            .as_deref(),
        Some("from-dev-env-file")
    );
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn dev_profile_errors_when_overlay_is_missing() {
    let temp_dir = unique_temp_dir("mem-api-dev-overlay-missing");
    let mem_dir = temp_dir.join(".mem");
    fs::create_dir_all(&mem_dir).unwrap();
    fs::write(
            mem_dir.join("config.toml"),
            "[service]\nbind_addr = \"10.0.0.1:4150\"\napi_token = \"t\"\nrequest_timeout = \"30s\"\n\n[database]\nurl = \"postgresql://shared\"\n",
        )
        .unwrap();

    let err =
        AppConfig::load_with_profile(Some(mem_dir.join("config.toml")), Profile::Dev).unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("config.dev.toml"), "message: {message}");
    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn code_graph_filters_default_to_bounded_neighborhood() {
    let filters = CodeGraphViewRequest::default().normalize();

    assert_eq!(filters.depth, CODE_GRAPH_DEFAULT_DEPTH);
    assert_eq!(filters.limit_nodes, CODE_GRAPH_DEFAULT_NODE_LIMIT);
    assert_eq!(filters.limit_edges, CODE_GRAPH_DEFAULT_EDGE_LIMIT);
    assert!(!filters.has_seed_filter());
}

#[test]
fn code_graph_filters_trim_empty_values_and_apply_caps() {
    let filters = CodeGraphViewRequest {
        q: Some("  MemoryType  ".to_string()),
        file_path: Some("   ".to_string()),
        symbol: Some(" query_memory ".to_string()),
        edge_kind: Some(" references ".to_string()),
        depth: Some(99),
        limit_nodes: Some(0),
        limit_edges: Some(99_999),
        run_id: None,
    }
    .normalize();

    assert_eq!(filters.q.as_deref(), Some("MemoryType"));
    assert_eq!(filters.file_path, None);
    assert_eq!(filters.symbol.as_deref(), Some("query_memory"));
    assert_eq!(filters.edge_kind.as_deref(), Some("references"));
    assert_eq!(filters.depth, CODE_GRAPH_MAX_DEPTH);
    assert_eq!(filters.limit_nodes, 1);
    assert_eq!(filters.limit_edges, CODE_GRAPH_MAX_EDGE_LIMIT);
    assert!(filters.has_seed_filter());
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }
    path
}

fn env_lock() -> &'static Mutex<()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}
