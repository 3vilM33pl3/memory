use crate::prelude::*;
use crate::*;
use mem_api::AutomationMode;

#[test]
fn llm_audit_redacts_common_secret_shapes() {
    let content =
        "bearer sk-live-secret-token password=hunter2 postgresql://memory:dbpass@localhost/db";
    let redacted = redact_llm_audit_content(content, Some("sk-live-secret-token"));

    assert!(!redacted.contains("sk-live-secret-token"));
    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains("dbpass"));
    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn llm_audit_truncates_by_character_limit() {
    let (content, truncated) = truncate_chars("abcdefghijklmnop", 15);

    assert!(truncated);
    assert_eq!(content, "abc\n[truncated]");
    assert_eq!(content.chars().count(), 15);
}

#[test]
fn verify_source_path_classifies_existing_and_missing_files() {
    let root = std::env::temp_dir().join(format!("memory-provenance-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("src")).expect("create temp repo");
    fs::write(root.join("src/lib.rs"), "pub fn marker() {}\n").expect("write source file");

    let existing = verify_source_path(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Existing source".to_string(),
        SourceKind::File,
        Some("src/lib.rs".to_string()),
        None,
        None,
        root.to_str().expect("temp path utf8"),
    );
    assert_eq!(existing.status, SourceProvenanceStatus::Verified);
    assert_eq!(
        existing.resolved_path.as_deref(),
        Some(
            root.join("src/lib.rs")
                .to_str()
                .expect("resolved path utf8")
        )
    );

    let missing = verify_source_path(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Missing source".to_string(),
        SourceKind::File,
        Some("src/missing.rs".to_string()),
        None,
        None,
        root.to_str().expect("temp path utf8"),
    );
    assert_eq!(missing.status, SourceProvenanceStatus::MissingFile);
    assert!(
        missing
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no longer exists"))
    );

    fs::remove_dir_all(root).expect("cleanup temp repo");
}

#[test]
fn verify_source_path_marks_non_file_sources_unverifiable() {
    let verification = verify_source_path(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Note source".to_string(),
        SourceKind::Note,
        None,
        None,
        None,
        "/repo",
    );

    assert_eq!(verification.status, SourceProvenanceStatus::Unverifiable);
    assert!(verification.resolved_path.is_none());
    assert!(
        verification
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("do not reference a file path"))
    );
}

#[test]
fn verify_source_path_requires_repo_root_for_relative_files() {
    let verification = verify_source_path(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Relative source".to_string(),
        SourceKind::File,
        Some("src/lib.rs".to_string()),
        None,
        None,
        "",
    );

    assert_eq!(verification.status, SourceProvenanceStatus::Unverifiable);
    assert!(verification.resolved_path.is_none());
    assert!(
        verification
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("without a repo root"))
    );
}

fn test_presence(
    watcher_id: &str,
    project: &str,
    hostname: &str,
    mode: AutomationMode,
    last_heartbeat_at: chrono::DateTime<chrono::Utc>,
) -> WatcherPresence {
    WatcherPresence {
        watcher_id: watcher_id.to_string(),
        project: project.to_string(),
        repo_root: "/repo".to_string(),
        hostname: hostname.to_string(),
        pid: 111,
        mode,
        started_at: last_heartbeat_at,
        last_heartbeat_at,
        host_service_id: "service-a".to_string(),
        managed_by_service: true,
        health: WatcherHealth::Healthy,
        agent_cli: None,
        agent_session_id: None,
        agent_pid: None,
        agent_started_at: None,
        last_restart_attempt_at: None,
        restart_attempt_count: 0,
    }
}

fn test_query_response() -> QueryResponse {
    QueryResponse {
        answer: "fallback answer".to_string(),
        confidence: 0.5,
        results: vec![mem_api::QueryResult {
            memory_id: uuid::Uuid::new_v4(),
            summary: "Primary memory".to_string(),
            memory_type: mem_api::MemoryType::Architecture,
            score: 12.0,
            snippet: "Primary evidence snippet".to_string(),
            match_kind: mem_api::QueryMatchKind::Hybrid,
            score_explanation: Vec::new(),
            debug: mem_api::QueryResultDebug::default(),
            tags: Vec::new(),
            sources: Vec::new(),
            graph_connections: Vec::new(),
        }],
        insufficient_evidence: false,
        answer_generation: QueryAnswerGeneration::default(),
        answer_citations: Vec::new(),
        diagnostics: mem_api::QueryDiagnostics::default(),
    }
}

#[test]
fn embedding_backend_toml_update_can_activate_and_deactivate() {
    let activated = set_active_embedding_backend_in_toml(
        r#"
            [embeddings]
            enabled = false
            active = "voyage"
            "#,
        Some("openai"),
    )
    .expect("activate toml");

    assert!(activated.contains("enabled = true"));
    assert!(activated.contains("active = \"openai\""));

    let deactivated =
        set_active_embedding_backend_in_toml(&activated, None).expect("deactivate toml");

    assert!(deactivated.contains("enabled = false"));
    assert!(deactivated.contains("active = \"openai\""));
}

#[test]
fn embedding_creation_toml_update_sets_create_enabled() {
    let disabled = set_embedding_creation_enabled_in_toml(
        r#"
            [embeddings]
            enabled = true
            active = "voyage"

            [[embeddings.backends]]
            name = "voyage"
            provider = "voyage"
            model = "voyage-code-3"
            "#,
        "voyage",
        false,
    )
    .expect("disable creation toml");

    assert!(disabled.contains("enabled = true"));
    assert!(disabled.contains("active = \"voyage\""));
    assert!(disabled.contains("create_enabled = true"));
    assert!(disabled.contains("create_enabled = false"));

    let enabled = set_embedding_creation_enabled_in_toml(&disabled, "voyage", true)
        .expect("enable creation toml");

    assert!(enabled.contains("create_enabled = true"));
}

#[test]
fn llm_audit_toml_update_creates_safe_defaults() {
    let updated =
        set_llm_audit_enabled_in_toml("[service]\nbind_addr = \"127.0.0.1:4040\"\n", true)
            .expect("enable llm audit");

    assert!(updated.contains("[llm_audit]"));
    assert!(updated.contains("enabled = true"));
    assert!(updated.contains("redact = true"));
    assert!(updated.contains("max_message_chars = 8000"));
    assert!(updated.contains("max_total_chars = 32000"));
}

#[test]
fn llm_audit_toml_update_preserves_existing_limits() {
    let updated = set_llm_audit_enabled_in_toml(
        r#"
            [llm_audit]
            enabled = false
            redact = true
            max_message_chars = 1200
            max_total_chars = 2400
            "#,
        true,
    )
    .expect("enable llm audit");

    assert!(updated.contains("enabled = true"));
    assert!(updated.contains("max_message_chars = 1200"));
    assert!(updated.contains("max_total_chars = 2400"));
}

#[test]
fn runtime_skill_status_reports_current_bundle() {
    let root = std::env::temp_dir().join(format!("memory-skill-status-{}", Uuid::new_v4()));
    for skill in MEMORY_SKILL_NAMES {
        let dir = root.join(".agents").join("skills").join(skill);
        fs::create_dir_all(&dir).expect("create skill dir");
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: test\nversion: 0.8.6-dev\n---\n",
        )
        .expect("write skill");
    }

    let status = runtime_skill_status(root.to_str(), "0.8.6-dev");

    assert_eq!(status.status, "ok");
    assert_eq!(status.bundle_version, "0.8.6-dev");
    assert!(status.summary.contains("skills current"));

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn runtime_skill_status_warns_on_outdated_or_missing_bundle() {
    let root = std::env::temp_dir().join(format!("memory-skill-status-{}", Uuid::new_v4()));
    let dir = root
        .join(".agents")
        .join("skills")
        .join(MEMORY_SKILL_NAMES[0]);
    fs::create_dir_all(&dir).expect("create skill dir");
    fs::write(dir.join("SKILL.md"), "---\nversion: 0.1.0\n---\n").expect("write skill");

    let status = runtime_skill_status(root.to_str(), "0.8.6-dev");

    assert_eq!(status.status, "warn");
    assert!(status.summary.contains("outdated"));
    assert!(status.summary.contains("missing"));

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn openai_embedding_space_aliases_legacy_and_compatible_keys() {
    assert_eq!(
        equivalent_openai_embedding_space_key(
            "openai|https://api.openai.com/v1|text-embedding-3-small"
        )
        .as_deref(),
        Some("openai_compatible|https://api.openai.com/v1|text-embedding-3-small")
    );
    assert_eq!(
        equivalent_openai_embedding_space_key(
            "openai_compatible|https://api.openai.com/v1|text-embedding-3-small"
        )
        .as_deref(),
        Some("openai|https://api.openai.com/v1|text-embedding-3-small")
    );
    assert!(
        equivalent_openai_embedding_space_key("voyage|https://api.voyageai.com|voyage-code-3")
            .is_none()
    );
}

#[test]
fn llm_query_answer_content_accepts_valid_citations() {
    let response = test_query_response();
    let parsed = parse_llm_query_answer_content(
            r#"{"answer":"Use the primary memory. [1]","citations":[1],"confidence":0.88,"insufficient_evidence":false}"#,
            &response,
        )
        .expect("valid llm answer");

    assert_eq!(parsed.answer, "Use the primary memory. [1]");
    assert_eq!(parsed.cited_result_numbers, vec![1]);
    assert_eq!(parsed.citations.len(), 1);
    assert_eq!(parsed.confidence, 0.88);
    assert!(!parsed.insufficient_evidence);
}

#[test]
fn llm_query_answer_content_rejects_unavailable_citation() {
    let response = test_query_response();
    let err = parse_llm_query_answer_content(
            r#"{"answer":"Unsupported","citations":[2],"confidence":0.8,"insufficient_evidence":false}"#,
            &response,
        )
        .expect_err("invalid citation should fail");

    assert!(err.to_string().contains("cited unavailable result 2"));
}

#[test]
fn query_answer_prompt_includes_graph_connections() {
    let mut response = test_query_response();
    response.results[0].graph_connections = vec![mem_api::QueryGraphConnection {
        file_path: "src/lib.rs".to_string(),
        symbol: Some("GraphTarget".to_string()),
        symbol_kind: Some("function".to_string()),
        edge_kind: Some("calls".to_string()),
        neighbor_symbol: Some("caller".to_string()),
        direction: Some("incoming".to_string()),
        score_boost: 1.25,
        reason: "code symbol match".to_string(),
    }];

    let prompt = build_query_answer_prompt(
        &QueryRequest {
            project: "memory".to_string(),
            query: "GraphTarget".to_string(),
            filters: Default::default(),
            top_k: 8,
            min_confidence: None,
            include_stale: false,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        },
        &response,
    );

    assert!(prompt.contains("graph: code symbol match | src/lib.rs"));
    assert!(prompt.contains("symbol=GraphTarget"));
    assert!(prompt.contains("edge=calls"));
}

#[test]
fn query_activity_details_include_graph_diagnostics() {
    let mut response = test_query_response();
    response.diagnostics.graph_status = "active".to_string();
    response.diagnostics.graph_candidates = 4;
    response.diagnostics.graph_augmented_candidates = 2;
    response.diagnostics.graph_duration_ms = 17;
    response.diagnostics.total_duration_ms = 91;
    response.results[0].debug.graph_boost = 1.25;
    response.results[0].graph_connections = vec![
        mem_api::QueryGraphConnection {
            file_path: "src/lib.rs".to_string(),
            symbol: Some("GraphTarget".to_string()),
            symbol_kind: Some("function".to_string()),
            edge_kind: Some("calls".to_string()),
            neighbor_symbol: Some("caller".to_string()),
            direction: Some("incoming".to_string()),
            score_boost: 1.25,
            reason: "code symbol match".to_string(),
        },
        mem_api::QueryGraphConnection {
            file_path: "src/other.rs".to_string(),
            symbol: Some("OtherTarget".to_string()),
            symbol_kind: Some("struct".to_string()),
            edge_kind: None,
            neighbor_symbol: None,
            direction: None,
            score_boost: 1.0,
            reason: "code reference match".to_string(),
        },
    ];

    let details = query_activity_details(
        &QueryRequest {
            project: "memory".to_string(),
            query: "GraphTarget".to_string(),
            filters: Default::default(),
            top_k: 8,
            min_confidence: None,
            include_stale: false,
            history: false,
            retrieval_mode: None,
            answer_mode: None,
        },
        &response,
    );

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
            assert_eq!(graph_status.as_deref(), Some("active"));
            assert_eq!(graph_candidates, 4);
            assert_eq!(graph_augmented_candidates, 2);
            assert_eq!(graph_duration_ms, 17);
            assert_eq!(graph_result_count, 1);
            assert_eq!(graph_connection_count, 2);
            assert_eq!(graph_connections.len(), 2);
            assert_eq!(graph_connections[0].file_path, "src/lib.rs");
        }
        other => panic!("unexpected activity details: {other:?}"),
    }
}

#[test]
fn graph_activity_summary_and_details_capture_extraction_counts() {
    let run_id = Uuid::new_v4();
    let request = GraphActivityRequest {
        project: "memory".to_string(),
        repo_root: "/repo".to_string(),
        git_head: Some("abc123".to_string()),
        since: None,
        extraction_run_id: Some(run_id),
        dry_run: false,
        reused_existing_run: true,
        index_reused: true,
        analyzer_version: "mem-analyze-v2".to_string(),
        strategy_version: "code-graph-resolution-v1".to_string(),
        symbol_count: 1919,
        reference_count: 80116,
        resolved_reference_count: 14621,
        unresolved_reference_count: 61249,
        ambiguous_reference_count: 4246,
        graph_node_count: 1919,
        graph_edge_count: 13812,
        evidence_count: 15731,
    };

    let summary = graph_activity_summary(&request);
    assert!(summary.contains("Reused code graph extraction"));
    assert!(summary.contains("1919 symbols"));
    assert!(summary.contains("13812 graph edge"));

    match graph_activity_details(&request) {
        ActivityDetails::GraphExtract {
            extraction_run_id,
            reference_count,
            graph_edge_count,
            reused_existing_run,
            ..
        } => {
            assert_eq!(extraction_run_id, Some(run_id));
            assert_eq!(reference_count, 80116);
            assert_eq!(graph_edge_count, 13812);
            assert!(reused_existing_run);
        }
        other => panic!("unexpected activity details: {other:?}"),
    }
}

#[test]
fn token_usage_from_chat_body_reads_openai_compatible_usage() {
    let usage = token_usage_from_chat_body(
            r#"{"usage":{"prompt_tokens":1200,"completion_tokens":300,"cached_input_tokens":200,"cache_creation_input_tokens":50,"total_tokens":1750}}"#,
        )
        .expect("usage");

    assert_eq!(usage.input_tokens, 1200);
    assert_eq!(usage.output_tokens, 300);
    assert_eq!(usage.cache_read_tokens, 200);
    assert_eq!(usage.cache_write_tokens, 50);
    assert_eq!(usage.total_tokens, 1750);
}

#[test]
fn mcp_http_auth_accepts_bearer_or_x_api_token() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        "Bearer service-token".parse().unwrap(),
    );
    assert!(mcp_http::mcp_token_matches(&headers, "service-token"));

    headers.clear();
    headers.insert("x-api-token", "service-token".parse().unwrap());
    assert!(mcp_http::mcp_token_matches(&headers, "service-token"));

    headers.insert("x-api-token", "wrong".parse().unwrap());
    assert!(!mcp_http::mcp_token_matches(&headers, "service-token"));
}

#[test]
fn mcp_origin_validation_rejects_cross_origin() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ORIGIN, "http://127.0.0.1".parse().unwrap());
    assert!(mcp_http::validate_mcp_origin(&headers, "127.0.0.1:4040").is_ok());

    headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());
    assert_eq!(
        mcp_http::validate_mcp_origin(&headers, "127.0.0.1:4040"),
        Err(StatusCode::FORBIDDEN)
    );
}

#[test]
fn api_auth_requires_explicit_token_even_for_local_origins() {
    let mut headers = HeaderMap::new();
    assert_eq!(
        require_token(&headers, "service-token", "127.0.0.1:4040")
            .expect_err("missing token should fail")
            .message,
        "missing x-api-token header"
    );

    headers.insert(header::ORIGIN, "http://127.0.0.1".parse().unwrap());
    assert_eq!(
        require_token(&headers, "service-token", "127.0.0.1:4040")
            .expect_err("local origin should not authenticate")
            .message,
        "missing x-api-token header"
    );

    headers.clear();
    headers.insert(header::REFERER, "http://localhost/app".parse().unwrap());
    assert_eq!(
        require_token(&headers, "service-token", "127.0.0.1:4040")
            .expect_err("local referer should not authenticate")
            .message,
        "missing x-api-token header"
    );
}

#[test]
fn api_auth_accepts_only_matching_x_api_token() {
    let mut headers = HeaderMap::new();
    headers.insert("x-api-token", "service-token".parse().unwrap());
    require_token(&headers, "service-token", "127.0.0.1:4040").expect("matching token");

    headers.insert("x-api-token", "wrong".parse().unwrap());
    assert_eq!(
        require_token(&headers, "service-token", "127.0.0.1:4040")
            .expect_err("wrong token should fail")
            .message,
        "invalid api token"
    );
}

#[test]
fn web_auth_token_response_names_x_api_token_header() {
    let response = WebAuthTokenResponse {
        api_token: "service-token".to_string(),
        header: "x-api-token",
    };

    let json = serde_json::to_value(&response).expect("serialize response");
    assert_eq!(json["api_token"], "service-token");
    assert_eq!(json["header"], "x-api-token");
}

#[test]
fn up_to_speed_briefing_includes_token_summary() {
    let token_usage = TokenUsageSummary {
        action_count: 2,
        total_input_tokens: 100,
        total_output_tokens: 40,
        total_cache_read_tokens: 20,
        total_cache_write_tokens: 5,
        total_tokens: 165,
    };
    let briefing = build_up_to_speed_briefing(
        "memory",
        &["Recent work focused on activity history.".to_string()],
        &["Persisted activity events".to_string()],
        &[],
        &[],
        &[],
        &token_usage,
    );

    assert!(briefing.contains("Get up to speed"));
    assert!(briefing.contains("165 total"));
    assert!(briefing.contains("2 recent action"));
}

#[tokio::test]
async fn recent_activity_responses_replays_latest_project_events() {
    let recent_activity = Mutex::new(VecDeque::from(vec![
        ServiceEvent {
            id: Uuid::new_v4(),
            project: "memory".to_string(),
            memory_id: None,
            kind: ActivityKind::Curate,
            summary: "Curated memory".to_string(),
            details: None,
            recorded_at: chrono::Utc::now(),
            actor_id: None,
            actor_name: None,
            source: Some("service".to_string()),
            operation_id: None,
            duration_ms: None,
            provider: None,
            model: None,
            token_usage: None,
            include_activity: true,
        },
        ServiceEvent {
            id: Uuid::new_v4(),
            project: "other".to_string(),
            memory_id: None,
            kind: ActivityKind::CaptureTask,
            summary: "Captured task".to_string(),
            details: None,
            recorded_at: chrono::Utc::now(),
            actor_id: None,
            actor_name: None,
            source: Some("service".to_string()),
            operation_id: None,
            duration_ms: None,
            provider: None,
            model: None,
            token_usage: None,
            include_activity: true,
        },
        ServiceEvent {
            id: Uuid::new_v4(),
            project: "memory".to_string(),
            memory_id: None,
            kind: ActivityKind::Reindex,
            summary: "Reindexed entries".to_string(),
            details: None,
            recorded_at: chrono::Utc::now(),
            actor_id: None,
            actor_name: None,
            source: Some("service".to_string()),
            operation_id: None,
            duration_ms: None,
            provider: None,
            model: None,
            token_usage: None,
            include_activity: true,
        },
    ]));

    let responses = recent_activity_responses(&recent_activity, "memory").await;
    assert_eq!(responses.len(), 2);

    let summaries = responses
        .into_iter()
        .map(|response| match response {
            StreamResponse::Activity { event } => event.summary,
            other => panic!("unexpected response: {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(summaries, vec!["Curated memory", "Reindexed entries"]);
}

#[test]
fn watcher_registry_refreshes_without_double_counting() {
    let watchers = Mutex::new(HashMap::new());
    let started_at = chrono::Utc::now();
    let request = WatcherHeartbeatRequest {
        watcher_id: "watcher-1".to_string(),
        project: "memory".to_string(),
        repo_root: "/repo".to_string(),
        hostname: "host-a".to_string(),
        pid: 111,
        mode: AutomationMode::Suggest,
        started_at,
        host_service_id: "service-a".to_string(),
        managed_by_service: true,
        agent_cli: None,
        agent_session_id: None,
        agent_pid: None,
        agent_started_at: None,
    };

    let (first, first_changed, _) = register_watcher_heartbeat(&watchers, request.clone());
    let (second, second_changed, transition) = register_watcher_heartbeat(&watchers, request);

    assert_eq!(first.active_count, 1);
    assert_eq!(second.active_count, 1);
    assert_eq!(second.unhealthy_count, 0);
    assert_eq!(second.watchers.len(), 1);
    assert_eq!(second.watchers[0].watcher_id, "watcher-1");
    assert!(first_changed);
    assert!(!second_changed);
    assert!(transition.is_none());
}

#[test]
fn watcher_summary_filters_by_project_and_marks_stale_entries_unhealthy() {
    let now = chrono::Utc::now();
    let mut registry = HashMap::new();
    registry.insert(
        "watcher-live".to_string(),
        test_presence(
            "watcher-live",
            "memory",
            "host-a",
            AutomationMode::Suggest,
            now,
        ),
    );
    registry.insert(
        "watcher-other".to_string(),
        test_presence(
            "watcher-other",
            "other",
            "host-b",
            AutomationMode::Auto,
            now,
        ),
    );
    registry.insert(
        "watcher-stale".to_string(),
        test_presence(
            "watcher-stale",
            "memory",
            "host-c",
            AutomationMode::Suggest,
            now - chrono::Duration::seconds(WATCHER_STALE_AFTER_SECONDS as i64 + 1),
        ),
    );
    let watchers = Mutex::new(registry);

    let summary = watcher_summary_for_project(&watchers, "memory");

    assert_eq!(summary.active_count, 1);
    assert_eq!(summary.unhealthy_count, 1);
    assert_eq!(summary.watchers.len(), 2);
    assert_eq!(summary.watchers[0].watcher_id, "watcher-live");
    assert_eq!(summary.watchers[1].watcher_id, "watcher-stale");
}

#[test]
fn stale_manual_watcher_is_counted_as_unhealthy() {
    let now = chrono::Utc::now();
    let watchers = Mutex::new(HashMap::from([(
        "watcher-manual".to_string(),
        WatcherPresence {
            managed_by_service: false,
            ..test_presence(
                "watcher-manual",
                "memory",
                "host-a",
                AutomationMode::Suggest,
                now - chrono::Duration::seconds(WATCHER_STALE_AFTER_SECONDS as i64 + 1),
            )
        },
    )]));

    let summary = watcher_summary_for_project(&watchers, "memory");
    assert_eq!(summary.active_count, 0);
    assert_eq!(summary.unhealthy_count, 1);
}

#[test]
fn watcher_restart_service_name_prefers_managed_session_identity() {
    let managed = WatcherRestartRequest {
        project: "memory".to_string(),
        watcher_id: "watcher-1".to_string(),
        host_service_id: "service-a".to_string(),
        agent_session_id: Some("session 123".to_string()),
    };
    assert_eq!(
        local_watcher_restart_service_name(&managed),
        mem_platform::managed_watch_service_name("session 123")
    );

    let legacy = WatcherRestartRequest {
        project: "customer portal".to_string(),
        watcher_id: "watcher-2".to_string(),
        host_service_id: "service-a".to_string(),
        agent_session_id: None,
    };
    assert_eq!(
        local_watcher_restart_service_name(&legacy),
        mem_platform::watch_service_unit_name("customer portal")
    );
}
