use chrono::{Local, TimeZone, Utc};
use crossterm::event::{Event, KeyCode, KeyEvent};

#[cfg_attr(target_os = "macos", allow(unused_imports))]
use super::{
    AgentSnapshot, App, BackendConnectionState, BackgroundEvent, ManagerState, MemoriesFocus,
    ProjectRefreshResult, QueryHistoryEntry, QueryRoundtripTiming, RefreshMode, TabKind, Theme,
    ToolVersions, UiStatus, activity_duration, activity_tokens, backend_activity_detail_lines,
    build_memory_detail_lines, collect_error_items, context_gradient_color, current_query_display,
    derive_manager_state, empty_overview, filled_bar_cells, format_context_percent,
    format_epoch_reset_time, format_query_citation_numbers, format_query_timing_with_percent,
    format_timestamp, format_timestamp_full, format_timestamp_medium, format_timestamp_short,
    format_timestamp_timeline, latest_plan_display, llm_audit_status_lines,
    manager_service_enabled, manager_service_running, manager_status_detail, manager_status_label,
    manager_unit_path, memory_detail_max_scroll, normalized_percent, query_input_display,
    query_timing_breakdown, query_timing_breakdown_lines, remaining_bar_cells,
    render_markdown_lines, service_status_detail, service_status_label,
    should_attempt_stream_reconnect, skill_bundle_status_color, tui_status_color,
    tui_status_detail, tui_status_label, watcher_bar_status_label,
};
use crate::commands::{
    service_support::TuiRestartNotice,
    skill_support::{SkillBundleStatus, project_skill_inventory},
};
use mem_agenttop::{AgentSession, SessionStatus as AgentSessionStatus};
use mem_api::{
    ActivityDetails, ActivityEvent, ActivityKind, DiagnosticInfo, DiagnosticSeverity,
    LlmAuditMessage, LlmAuditStatusResponse, MemoryEmbeddingSpace, MemoryEntryResponse,
    MemoryStatus, MemoryType, Profile, ProjectMemoriesResponse, QueryAnswerGeneration,
    QueryAnswerMethod, QueryDiagnostics, QueryFilters, QueryMatchKind, QueryRequest, QueryResponse,
    QueryResult, QueryResultDebug, ReplacementProposalListResponse, TokenUsage,
    WatcherPresenceSummary,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

#[test]
fn format_timestamp_returns_na_for_missing_value() {
    assert_eq!(format_timestamp(None), "n/a");
}

#[test]
fn local_timestamp_formatters_match_local_timezone_rendering() {
    let timestamp = Utc.with_ymd_and_hms(2026, 4, 1, 12, 34, 56).unwrap();
    let local = timestamp.with_timezone(&Local);

    let full = format_timestamp_full(timestamp);
    let medium = format_timestamp_medium(timestamp);
    let short = format_timestamp_short(timestamp);
    let timeline = format_timestamp_timeline(timestamp);

    assert_eq!(full, local.format("%Y-%m-%d %H:%M:%S %Z").to_string());
    assert_eq!(medium, local.format("%Y-%m-%d %H:%M %Z").to_string());
    assert_eq!(short, local.format("%H:%M:%S %Z").to_string());
    assert_eq!(timeline, local.format("%m-%d %H:%M %Z").to_string());
}

#[test]
fn documentation_memory_type_filter_matches_and_labels() {
    let filter = super::TypeFilter::Documentation;

    assert!(filter.matches(&MemoryType::Documentation));
    assert!(!filter.matches(&MemoryType::Implementation));
    assert_eq!(filter.label(), "documentation");
    assert_eq!(
        super::TypeFilter::DomainFact.next().label(),
        "documentation"
    );
}

#[test]
fn every_visible_tab_has_comprehensive_help() {
    for tab in super::VISIBLE_TABS {
        let help = super::tab_help_markdown(tab);
        assert!(help.contains("# "));
        assert!(
            help.contains("## Purpose"),
            "{} missing purpose",
            tab.label()
        );
        assert!(help.contains("## Layout"), "{} missing layout", tab.label());
        assert!(
            help.contains("## Controls"),
            "{} missing controls",
            tab.label()
        );
        assert!(
            help.contains("## Workflows"),
            "{} missing workflows",
            tab.label()
        );
        assert!(super::tab_help_lines(tab).len() > 12);
    }
}

#[test]
fn help_open_and_close_preserve_active_tab() {
    let mut app = new_test_app();
    app.active_tab = TabKind::Query;

    app.open_help_for_active_tab();
    assert!(app.chrome.help.help_open);
    assert_eq!(app.chrome.help.help_tab, TabKind::Query);
    assert_eq!(app.active_tab, TabKind::Query);

    app.handle_help_key(KeyEvent::from(KeyCode::Char('h')));
    assert!(!app.chrome.help.help_open);
    assert_eq!(app.active_tab, TabKind::Query);
}

#[test]
fn help_scroll_is_clamped_to_rendered_content() {
    let mut app = new_test_app();
    app.active_tab = TabKind::Embeddings;
    app.open_help_for_active_tab();
    let frame = ratatui::layout::Rect::new(0, 0, 100, 24);
    let max_scroll = super::help_max_scroll(app.chrome.help.help_tab, frame);
    assert!(max_scroll > 0);

    app.scroll_help_in_area(500, frame);
    assert_eq!(app.chrome.help.help_scroll, max_scroll);

    app.scroll_help_in_area(-500, frame);
    assert_eq!(app.chrome.help.help_scroll, 0);
}

#[test]
fn help_can_open_when_backend_is_unavailable() {
    let mut app = new_test_app();
    app.service.health_ok = false;
    app.active_tab = TabKind::Errors;

    app.open_help_for_active_tab();

    assert!(app.chrome.help.help_open);
    assert_eq!(app.chrome.help.help_tab, TabKind::Errors);
}

#[test]
fn h_is_no_longer_previous_tab_alias() {
    let mut app = new_test_app();
    app.active_tab = TabKind::Query;

    app.open_help_for_active_tab();

    assert_eq!(app.active_tab, TabKind::Query);
    assert_eq!(app.chrome.help.help_tab, TabKind::Query);
}

#[test]
fn query_citation_numbers_render_bracketed_result_ids() {
    assert_eq!(format_query_citation_numbers(&[]), "none");
    assert_eq!(format_query_citation_numbers(&[1, 3]), "[1] [3]");
}

fn test_query_response_with_timings() -> QueryResponse {
    QueryResponse {
        answer: "Use the selected memory. [1]".to_string(),
        confidence: 0.82,
        results: vec![QueryResult {
            memory_id: Uuid::new_v4(),
            summary: "Cached implementation memory".to_string(),
            memory_type: MemoryType::Implementation,
            score: 12.5,
            snippet: "Cached result snippet".to_string(),
            match_kind: QueryMatchKind::Hybrid,
            score_explanation: vec!["strong cached match".to_string()],
            debug: QueryResultDebug::default(),
            tags: vec!["implementation".to_string()],
            sources: Vec::new(),
            graph_connections: Vec::new(),
        }],
        insufficient_evidence: false,
        answer_generation: QueryAnswerGeneration {
            method: QueryAnswerMethod::Llm,
            cited_result_numbers: vec![1],
            evidence_count: 1,
            duration_ms: 80,
            fallback_reason: None,
            token_usage: None,
        },
        answer_citations: Vec::new(),
        diagnostics: QueryDiagnostics {
            total_duration_ms: 300,
            lexical_duration_ms: 40,
            semantic_duration_ms: 70,
            graph_duration_ms: 120,
            rerank_duration_ms: 30,
            lexical_candidates: 11,
            semantic_candidates: 7,
            graph_candidates: 3,
            semantic_status: "active_space_ok".to_string(),
            graph_status: "active".to_string(),
            ..Default::default()
        },
    }
}

fn test_query_response_with_two_results() -> QueryResponse {
    let mut response = test_query_response_with_timings();
    response.results.push(QueryResult {
        memory_id: Uuid::new_v4(),
        summary: "Second implementation memory".to_string(),
        memory_type: MemoryType::Implementation,
        score: 8.5,
        snippet: "Second cached result snippet".to_string(),
        match_kind: QueryMatchKind::Lexical,
        score_explanation: vec!["secondary match".to_string()],
        debug: QueryResultDebug::default(),
        tags: vec!["implementation".to_string()],
        sources: Vec::new(),
        graph_connections: Vec::new(),
    });
    response
}

fn test_query_request(query: &str) -> QueryRequest {
    QueryRequest {
        project: "memory".to_string(),
        query: query.to_string(),
        filters: QueryFilters::default(),
        top_k: 8,
        min_confidence: None,
        include_stale: false,
        history: false,
        retrieval_mode: None,
        answer_mode: None,
    }
}

#[test]
fn query_timing_breakdown_saturates_derived_values() {
    let response = test_query_response_with_timings();
    let timing = QueryRoundtripTiming {
        query_api_ms: 360,
        initial_detail_ms: Some(25),
        ui_ready_ms: 390,
    };

    let breakdown = query_timing_breakdown(&response, timing);

    assert_eq!(breakdown.backend_reported_ms, 380);
    assert_eq!(breakdown.transport_overhead_ms, 0);
    assert_eq!(breakdown.retrieval_other_ms, 40);
}

#[test]
fn query_timing_percent_formats_consistently() {
    assert_eq!(format_query_timing_with_percent(25, 100), "25 ms (25%)");
    assert_eq!(format_query_timing_with_percent(25, 0), "25 ms");
}

#[test]
fn query_timing_lines_render_roundtrip_and_phase_labels() {
    let response = test_query_response_with_timings();
    let timing = QueryRoundtripTiming {
        query_api_ms: 420,
        initial_detail_ms: Some(30),
        ui_ready_ms: 455,
    };

    let rendered = query_timing_breakdown_lines(&response, Some(timing))
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Timing Breakdown"));
    assert!(rendered.contains("UI ready"));
    assert!(rendered.contains("Query API"));
    assert!(rendered.contains("Initial detail"));
    assert!(rendered.contains("Backend"));
    assert!(rendered.contains("Answer"));
    assert!(rendered.contains("Lexical"));
    assert!(rendered.contains("Semantic"));
    assert!(rendered.contains("Graph"));
    assert!(rendered.contains("Rerank/relation"));
}

#[test]
fn latest_plan_display_shows_recent_plan_thread() {
    let mut app = new_test_app();
    let older = Utc.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap();
    let newer = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
    app.memories.all_memories = vec![
        test_project_memory_list_item("Implemented work", MemoryType::Implementation, newer),
        test_project_memory_list_item("Older Plan", MemoryType::Plan, older),
        test_project_memory_list_item("Latest Plan", MemoryType::Plan, newer),
    ];
    app.memories.all_memories[2]
        .tags
        .push("plan-thread:latest-plan".to_string());

    assert_eq!(latest_plan_display(&app), "Latest Plan (latest-plan)");
}

#[test]
fn query_input_display_renders_placeholder_and_cursor_start() {
    let display = query_input_display("", 12);
    assert!(display.placeholder);
    assert_eq!(display.text, "Ask project ");
    assert_eq!(display.cursor_col, 0);
}

#[test]
fn query_input_display_keeps_short_cursor_after_text() {
    let display = query_input_display("hello", 12);
    assert!(!display.placeholder);
    assert_eq!(display.text, "hello");
    assert_eq!(display.cursor_col, 5);
}

#[test]
fn query_input_display_truncates_long_text_from_left() {
    let display = query_input_display("long question", 8);
    assert!(!display.placeholder);
    assert_eq!(display.text, "<uestion");
    assert_eq!(display.cursor_col, 7);
}

#[test]
fn stale_query_completion_does_not_replace_current_query_state() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.2".to_string(),
            mem_service: "0.6.2".to_string(),
            watch_manager: "0.6.2".to_string(),
            memory_watch: "0.6.2".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.query.query_request_id = 2;
    app.query.query_loading = true;
    app.query.query_pending_question = Some("newer query".to_string());
    let request = QueryRequest {
        project: "memory".to_string(),
        query: "older query".to_string(),
        filters: QueryFilters::default(),
        top_k: 8,
        min_confidence: None,
        include_stale: false,
        history: false,
        retrieval_mode: None,
        answer_mode: None,
    };

    app.apply_query_completed(
        1,
        request,
        QueryRoundtripTiming {
            query_api_ms: 12,
            initial_detail_ms: None,
            ui_ready_ms: 12,
        },
        Err("older query failed".to_string()),
        None,
    );

    assert!(app.query.query_loading);
    assert_eq!(
        app.query.query_pending_question.as_deref(),
        Some("newer query")
    );
    assert!(app.query.query_error.is_none());
}

#[test]
fn query_completion_stores_roundtrip_timing() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.8.6".to_string(),
            mem_service: "0.8.6".to_string(),
            watch_manager: "0.8.6".to_string(),
            memory_watch: "0.8.6".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.query.query_request_id = 1;
    app.query.query_loading = true;
    let request = QueryRequest {
        project: "memory".to_string(),
        query: "timing".to_string(),
        filters: QueryFilters::default(),
        top_k: 8,
        min_confidence: None,
        include_stale: false,
        history: false,
        retrieval_mode: None,
        answer_mode: None,
    };
    let timing = QueryRoundtripTiming {
        query_api_ms: 420,
        initial_detail_ms: Some(35),
        ui_ready_ms: 460,
    };

    app.apply_query_completed(
        1,
        request,
        timing,
        Ok(test_query_response_with_timings()),
        None,
    );

    assert!(!app.query.query_loading);
    assert_eq!(app.query.query_roundtrip_timing, Some(timing));
    assert_eq!(app.query.query_last_duration_ms, Some(460));
}

#[test]
fn query_completion_stores_success_snapshot_in_history() {
    let mut app = new_test_app();
    app.query.query_request_id = 1;
    app.query.query_text = "cached success".to_string();
    app.start_query_history_run("cached success");
    let timing = QueryRoundtripTiming {
        query_api_ms: 210,
        initial_detail_ms: Some(20),
        ui_ready_ms: 235,
    };
    let detail = test_memory_detail("Cached canonical detail");

    app.apply_query_completed(
        1,
        test_query_request("cached success"),
        timing,
        Ok(test_query_response_with_timings()),
        Some(Ok(detail.clone())),
    );

    assert_eq!(app.query.query_history.len(), 1);
    let entry = &app.query.query_history[0];
    assert_eq!(entry.question, "cached success");
    assert!(entry.response.is_some());
    assert!(entry.error.is_none());
    assert_eq!(entry.timing, Some(timing));
    assert_eq!(
        entry
            .initial_detail
            .as_ref()
            .map(|detail| detail.canonical_text.as_str()),
        Some("Cached canonical detail")
    );
    assert!(!entry.running);
}

#[test]
fn query_history_up_restores_cached_success_results() {
    let mut app = new_test_app();
    let timing = QueryRoundtripTiming {
        query_api_ms: 210,
        initial_detail_ms: Some(20),
        ui_ready_ms: 235,
    };
    app.query.query_history.push(QueryHistoryEntry {
        question: "cached success".to_string(),
        response: Some(test_query_response_with_timings()),
        error: None,
        timing: Some(timing),
        initial_detail: Some(test_memory_detail("Restored canonical detail")),
        running: false,
    });
    app.clear_visible_query_state();

    let mut buffer = String::new();
    app.apply_query_history_delta(&mut buffer, -1);

    assert_eq!(buffer, "cached success");
    assert_eq!(app.query.query_text, "cached success");
    assert!(app.query.query_response.is_some());
    assert!(app.query.query_error.is_none());
    assert_eq!(app.query.query_roundtrip_timing, Some(timing));
    assert_eq!(app.query.query_table_state.selected(), Some(0));
    assert_eq!(
        app.query
            .query_selected_detail
            .as_ref()
            .map(|detail| detail.canonical_text.as_str()),
        Some("Restored canonical detail")
    );
    assert!(app.chrome.status_message.contains("with cached results"));
}

#[test]
fn query_history_up_restores_cached_error() {
    let mut app = new_test_app();
    let timing = QueryRoundtripTiming {
        query_api_ms: 90,
        initial_detail_ms: None,
        ui_ready_ms: 90,
    };
    app.query.query_history.push(QueryHistoryEntry {
        question: "broken query".to_string(),
        response: None,
        error: Some("provider unavailable".to_string()),
        timing: Some(timing),
        initial_detail: Some(test_memory_detail("stale detail should not show")),
        running: false,
    });
    app.query.query_response = Some(test_query_response_with_timings());

    let mut buffer = String::new();
    app.apply_query_history_delta(&mut buffer, -1);

    assert_eq!(buffer, "broken query");
    assert!(app.query.query_response.is_none());
    assert_eq!(
        app.query.query_error.as_deref(),
        Some("provider unavailable")
    );
    assert_eq!(app.query.query_roundtrip_timing, Some(timing));
    assert!(app.query.query_selected_detail.is_none());
    assert_eq!(app.query.query_table_state.selected(), None);
    assert!(app.chrome.status_message.contains("with cached error"));
}

#[test]
fn query_history_down_to_empty_clears_visible_results() {
    let mut app = new_test_app();
    app.query.query_history.push(QueryHistoryEntry {
        question: "cached success".to_string(),
        response: Some(test_query_response_with_timings()),
        error: None,
        timing: Some(QueryRoundtripTiming {
            query_api_ms: 210,
            initial_detail_ms: Some(20),
            ui_ready_ms: 235,
        }),
        initial_detail: Some(test_memory_detail("Restored canonical detail")),
        running: false,
    });
    let mut buffer = String::new();
    app.apply_query_history_delta(&mut buffer, -1);
    assert!(app.query.query_response.is_some());

    app.apply_query_history_delta(&mut buffer, 1);

    assert_eq!(buffer, "");
    assert!(app.query.query_response.is_none());
    assert!(app.query.query_error.is_none());
    assert!(app.query.query_roundtrip_timing.is_none());
    assert!(app.query.query_selected_detail.is_none());
    assert_eq!(app.query.query_table_state.selected(), None);
}

#[test]
fn empty_query_submit_clears_results_and_prompts_for_question() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.2".to_string(),
            mem_service: "0.6.2".to_string(),
            watch_manager: "0.6.2".to_string(),
            memory_watch: "0.6.2".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.query.query_text = "   ".to_string();
    app.query.query_loading = true;

    assert!(app.clear_empty_query_if_needed());

    assert!(!app.query.query_loading);
    assert_eq!(
        app.chrome.status_message,
        "Enter a query before running search."
    );
    assert!(app.query.query_response.is_none());
    assert!(app.query.query_error.is_none());
}

#[test]
fn query_input_starts_empty_and_history_navigates_with_arrows() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.3".to_string(),
            mem_service: "0.6.3".to_string(),
            watch_manager: "0.6.3".to_string(),
            memory_watch: "0.6.3".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );

    app.query.query_text = "previous visible query".to_string();
    app.start_query_input();
    assert_eq!(current_query_display(&app), "");

    app.query.query_text = "first query".to_string();
    app.remember_query_history_entry();
    app.query.query_text = "second query".to_string();
    app.remember_query_history_entry();

    let mut buffer = String::new();
    app.apply_query_history_delta(&mut buffer, -1);
    assert_eq!(buffer, "second query");
    app.apply_query_history_delta(&mut buffer, -1);
    assert_eq!(buffer, "first query");
    app.apply_query_history_delta(&mut buffer, 1);
    assert_eq!(buffer, "second query");
    app.apply_query_history_delta(&mut buffer, 1);
    assert_eq!(buffer, "");
}

#[test]
fn footer_statuses_do_not_use_stale_service_or_watcher_state_when_health_is_down() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.3".to_string(),
            mem_service: "0.4.3".to_string(),
            watch_manager: "0.4.3".to_string(),
            memory_watch: "0.4.3".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.chrome.ui_status = UiStatus::Error;
    app.service.health_ok = false;
    app.service.backend_connection_state = BackendConnectionState::Unavailable;
    app.meta.overview = empty_overview("memory".to_string());
    app.meta.overview.service_status = "ok".to_string();
    app.meta.overview.database_status = "up".to_string();
    app.meta.overview.watchers = Some(WatcherPresenceSummary {
        active_count: 2,
        unhealthy_count: 0,
        stale_after_seconds: 90,
        last_heartbeat_at: None,
        watchers: Vec::new(),
    });

    assert_eq!(service_status_label(&app), "down");
    assert_eq!(watcher_bar_status_label(&app), "unknown");
}

#[test]
fn footer_service_status_treats_healthy_relay_as_up() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.3".to_string(),
            mem_service: "0.4.3".to_string(),
            watch_manager: "0.4.3".to_string(),
            memory_watch: "0.4.3".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.service.health_ok = true;
    app.service.service_role = Some("relay".to_string());
    app.service.service_health_state = Some("ok".to_string());
    app.service.service_database_state = Some("down".to_string());
    app.meta.overview.service_status = "ok".to_string();
    app.meta.overview.database_status = "down".to_string();

    assert_eq!(service_status_label(&app), "up");
    assert_eq!(service_status_detail(&app), Some("relay".to_string()));
}

#[test]
fn backend_connection_state_starts_connecting_then_tracks_health() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.3".to_string(),
            mem_service: "0.6.3".to_string(),
            watch_manager: "0.6.3".to_string(),
            memory_watch: "0.6.3".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );

    assert_eq!(
        app.service.backend_connection_state,
        BackendConnectionState::Connecting
    );

    let mut result = ProjectRefreshResult {
        mode: RefreshMode::Startup,
        health: Ok(serde_json::json!({
            "status": "ok",
            "database": "up",
            "role": "primary",
            "version": "0.6.3"
        })),
        overview: Ok(empty_overview("memory".to_string())),
        memories: Ok(ProjectMemoriesResponse {
            project: "memory".to_string(),
            total: 0,
            items: Vec::new(),
        }),
        proposals: Ok(ReplacementProposalListResponse {
            project: "memory".to_string(),
            proposals: Vec::new(),
        }),
        skill_inventory: project_skill_inventory(Path::new("."), false),
    };
    app.apply_project_refresh(result.clone());
    assert_eq!(
        app.service.backend_connection_state,
        BackendConnectionState::Connected
    );

    result.health = Err("connection refused".to_string());
    app.apply_project_refresh(result);
    assert_eq!(
        app.service.backend_connection_state,
        BackendConnectionState::Unavailable
    );
}

#[test]
fn project_refresh_selects_first_memory_for_detail_loading() {
    let mut app = new_test_app();
    let updated_at = Utc.with_ymd_and_hms(2026, 5, 3, 18, 0, 0).unwrap();
    let first = test_project_memory_list_item("First memory", MemoryType::Project, updated_at);
    let first_id = first.id;
    let second =
        test_project_memory_list_item("Second memory", MemoryType::Implementation, updated_at);

    let loaded = app.apply_project_refresh(ProjectRefreshResult {
        mode: RefreshMode::Startup,
        health: Ok(serde_json::json!({
            "status": "ok",
            "database": "up",
            "role": "primary",
            "version": "0.8.2"
        })),
        overview: Ok(empty_overview("memory".to_string())),
        memories: Ok(ProjectMemoriesResponse {
            project: "memory".to_string(),
            total: 2,
            items: vec![first, second],
        }),
        proposals: Ok(ReplacementProposalListResponse {
            project: "memory".to_string(),
            proposals: Vec::new(),
        }),
        skill_inventory: project_skill_inventory(Path::new("."), false),
    });

    assert!(loaded);
    assert_eq!(app.memories.selected_index, 0);
    assert_eq!(app.memories.table_state.selected(), Some(0));
    assert_eq!(
        app.memories.filtered_memories.first().map(|item| item.id),
        Some(first_id)
    );
}

#[test]
fn manager_footer_status_mapping_prefers_active_then_installed_then_off() {
    assert_eq!(
        derive_manager_state(true, true, true, false, true),
        ManagerState::Active
    );
    assert_eq!(
        derive_manager_state(true, false, false, false, true),
        ManagerState::Installed
    );
    assert_eq!(
        derive_manager_state(false, false, false, true, true),
        ManagerState::Active
    );
    assert_eq!(
        derive_manager_state(false, false, false, false, true),
        ManagerState::Off
    );
    assert_eq!(
        derive_manager_state(false, false, false, false, false),
        ManagerState::Error
    );
}

#[test]
fn dev_manager_status_ignores_installed_service_probe() {
    assert_eq!(manager_unit_path(Profile::Dev), None);
    assert!(!manager_service_enabled(Profile::Dev));
    assert!(!manager_service_running(Profile::Dev));
}

#[test]
fn manager_process_detection_is_profile_scoped() {
    let prod = "/home/user/.local/bin/memory --config /home/user/.config/memory-layer/memory-layer.toml watcher manager run";
    let dev = "/home/user/project/target/debug/memory watcher manager run";
    let explicit_dev = "MEMORY_LAYER_PROFILE=dev /home/user/.local/bin/memory watcher manager run";

    assert!(super::command_is_manager_for_profile(prod, Profile::Prod));
    assert!(!super::command_is_manager_for_profile(prod, Profile::Dev));
    assert!(super::command_is_manager_for_profile(dev, Profile::Dev));
    assert!(!super::command_is_manager_for_profile(dev, Profile::Prod));
    assert!(super::command_is_manager_for_profile(
        explicit_dev,
        Profile::Dev
    ));
}

#[test]
fn manager_footer_detail_includes_session_and_warning_counts() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.3".to_string(),
            mem_service: "0.4.3".to_string(),
            watch_manager: "0.4.3".to_string(),
            memory_watch: "0.4.3".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.service.manager_status = Some(super::ManagerFooterStatus {
        state: ManagerState::Active,
        tracked_sessions: 2,
        warning_count: 1,
        mode: Some(super::ManagerMode::Foreground),
        runtime_mode: Some("event-driven".to_string()),
        last_reconcile_reason: Some("session-file-event".to_string()),
        event_count: 3,
        fallback_scan_count: 1,
    });

    assert_eq!(manager_status_label(&app), "active");
    assert_eq!(
        manager_status_detail(&app),
        Some(
            "manual, event-driven, last session-file-event, 2 sessions, 1 warn, 3 events, 1 fallback"
                .to_string()
        )
    );
}

#[test]
fn stream_reconnect_attempts_are_rate_limited() {
    let just_attempted = Instant::now();
    assert!(!should_attempt_stream_reconnect(
        false,
        false,
        just_attempted
    ));

    let overdue = Instant::now() - Duration::from_secs(2);
    assert!(should_attempt_stream_reconnect(false, false, overdue));
    assert!(!should_attempt_stream_reconnect(true, false, overdue));
    assert!(!should_attempt_stream_reconnect(false, true, overdue));
}

#[test]
fn stream_disconnect_does_not_mark_backend_unavailable() {
    let mut app = new_test_app();
    app.service.health_ok = true;
    app.service.backend_connection_state = BackendConnectionState::Connected;
    app.chrome.ui_status = UiStatus::Ready;
    app.meta.overview.service_status = "ok".to_string();
    app.meta.overview.database_status = "up".to_string();

    app.handle_stream_disconnect("stream connection closed");

    assert!(app.service.health_ok);
    assert_eq!(
        app.service.backend_connection_state,
        BackendConnectionState::Connected
    );
    assert_eq!(app.meta.overview.service_status, "ok");
    assert_eq!(app.meta.overview.database_status, "up");
    assert_eq!(app.chrome.ui_status, UiStatus::Ready);
    assert!(
        app.chrome
            .status_message
            .contains("backend health is unchanged")
    );
}

#[test]
fn tui_restart_notice_forces_red_restart_status() {
    let mut app = new_test_app();
    app.chrome.ui_status = UiStatus::Ready;
    app.service.restart_notice = Some(TuiRestartNotice {
        marker_path: PathBuf::from("/tmp/tui-restart-required.json"),
        version: "0.9.0".to_string(),
        reason: "install-or-upgrade".to_string(),
    });

    assert_eq!(tui_status_label(&app), "restart");
    assert_eq!(tui_status_color(&app), Theme::DANGER);
}

#[test]
fn context_percent_display_caps_over_budget_sessions() {
    assert_eq!(format_context_percent(68.4), "68%");
    assert_eq!(format_context_percent(100.0), "100%");
    assert_eq!(format_context_percent(182.3), "100%+");
}

#[test]
fn bar_helpers_normalize_and_cap_percentages() {
    assert_eq!(normalized_percent(-10.0), 0.0);
    assert_eq!(normalized_percent(42.5), 42.5);
    assert_eq!(normalized_percent(182.3), 100.0);
    assert_eq!(filled_bar_cells(0.0, 20), 0);
    assert_eq!(filled_bar_cells(50.0, 20), 10);
    assert_eq!(filled_bar_cells(182.3, 20), 20);
    assert_eq!(remaining_bar_cells(0.0, 20), 20);
    assert_eq!(remaining_bar_cells(50.0, 20), 10);
    assert_eq!(remaining_bar_cells(100.0, 20), 0);
}

#[test]
fn epoch_reset_time_formats_in_local_timezone() {
    let epoch_seconds = 1_775_352_000_u64;
    let timestamp = Utc.timestamp_opt(epoch_seconds as i64, 0).unwrap();
    assert_eq!(
        format_epoch_reset_time(epoch_seconds),
        format_timestamp_short(timestamp)
    );
}

#[test]
fn context_gradient_spans_success_to_danger() {
    assert_eq!(context_gradient_color(0.0), Theme::SUCCESS);
    assert_eq!(context_gradient_color(100.0), Theme::DANGER);
}

#[test]
fn skill_bundle_status_colors_match_footer_severity() {
    assert_eq!(
        skill_bundle_status_color(SkillBundleStatus::Ok),
        Theme::SUCCESS
    );
    assert_eq!(
        skill_bundle_status_color(SkillBundleStatus::Warn),
        Theme::WARNING
    );
    assert_eq!(
        skill_bundle_status_color(SkillBundleStatus::Error),
        Theme::DANGER
    );
}

fn test_agent_session(project_name: &str, session_id: &str) -> AgentSession {
    AgentSession {
        agent_cli: "codex",
        pid: 123,
        session_id: session_id.to_string(),
        cwd: format!("/tmp/{project_name}"),
        project_name: project_name.to_string(),
        started_at: 0,
        status: AgentSessionStatus::Waiting,
        model: "gpt-5.4".to_string(),
        context_percent: 42.0,
        total_input_tokens: 100,
        total_output_tokens: 20,
        total_cache_read: 0,
        total_cache_create: 0,
        turn_count: 1,
        current_tasks: vec!["waiting for input".to_string()],
        mem_mb: 128,
        version: "0.4.3".to_string(),
        git_branch: "main".to_string(),
        git_added: 0,
        git_modified: 0,
        token_history: vec![],
        subagents: vec![],
        mem_file_count: 0,
        mem_line_count: 0,
        children: vec![],
        initial_prompt: String::new(),
        first_assistant_text: String::new(),
    }
}

fn test_memory_detail(canonical_text: &str) -> MemoryEntryResponse {
    let timestamp = Utc.with_ymd_and_hms(2026, 4, 11, 8, 0, 0).unwrap();
    MemoryEntryResponse {
        id: Uuid::nil(),
        project: "memory".to_string(),
        canonical_text: canonical_text.to_string(),
        summary: "Improved TUI detail rendering".to_string(),
        memory_type: MemoryType::Implementation,
        importance: 8,
        confidence: 0.92,
        status: MemoryStatus::Active,
        tags: vec!["implementation".to_string(), "tui".to_string()],
        sources: Vec::new(),
        related_memories: Vec::new(),
        embedding_spaces: Vec::new(),
        created_at: timestamp,
        updated_at: timestamp,
        canonical_id: Uuid::nil(),
        version_no: 1,
        is_tombstone: false,
    }
}

fn test_project_memory_list_item(
    summary: &str,
    memory_type: MemoryType,
    updated_at: chrono::DateTime<Utc>,
) -> mem_api::ProjectMemoryListItem {
    let id = Uuid::new_v4();
    mem_api::ProjectMemoryListItem {
        id,
        summary: summary.to_string(),
        preview: summary.to_string(),
        memory_type,
        status: MemoryStatus::Active,
        confidence: 0.95,
        importance: 4,
        updated_at,
        tags: Vec::new(),
        tag_count: 0,
        source_count: 0,
        canonical_id: id,
        version_no: 1,
        is_tombstone: false,
    }
}

#[test]
fn agents_tab_initial_selection_prefers_current_project() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.3".to_string(),
            mem_service: "0.4.3".to_string(),
            watch_manager: "0.4.3".to_string(),
            memory_watch: "0.4.3".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    let snapshot = AgentSnapshot {
        collected_at: Utc::now(),
        sessions: vec![
            test_agent_session("other-project", "session-a"),
            test_agent_session("memory", "session-b"),
        ],
        orphan_ports: vec![],
        rate_limits: vec![],
    };

    app.apply_background_event(BackgroundEvent::AgentsLoaded {
        snapshot: Ok(snapshot),
    });

    assert_eq!(app.agents.agent_selected_index, 1);
    assert_eq!(app.agents.agent_table_state.selected(), Some(1));
}

#[test]
fn agents_tab_initial_selection_falls_back_to_first_row_without_match() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.3".to_string(),
            mem_service: "0.4.3".to_string(),
            watch_manager: "0.4.3".to_string(),
            memory_watch: "0.4.3".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    let snapshot = AgentSnapshot {
        collected_at: Utc::now(),
        sessions: vec![
            test_agent_session("alpha", "session-a"),
            test_agent_session("beta", "session-b"),
        ],
        orphan_ports: vec![],
        rate_limits: vec![],
    };

    app.apply_background_event(BackgroundEvent::AgentsLoaded {
        snapshot: Ok(snapshot),
    });

    assert_eq!(app.agents.agent_selected_index, 0);
    assert_eq!(app.agents.agent_table_state.selected(), Some(0));
}

#[test]
fn tab_order_restores_activity_after_query() {
    assert_eq!(TabKind::Memories.index(), 0);
    assert_eq!(TabKind::Agents.index(), 1);
    assert_eq!(TabKind::Query.index(), 2);
    assert_eq!(TabKind::Activity.index(), 3);
    assert_eq!(TabKind::Errors.index(), 4);
    assert_eq!(TabKind::Project.index(), 5);
    assert_eq!(TabKind::Review.index(), 6);
    assert_eq!(TabKind::Watchers.index(), 7);
    assert_eq!(TabKind::Embeddings.index(), 8);
    assert_eq!(TabKind::Resume.index(), 9);

    assert_eq!(TabKind::Memories.prev(), TabKind::Resume);
    assert_eq!(TabKind::Memories.next(), TabKind::Agents);
    assert_eq!(TabKind::Query.next(), TabKind::Activity);
    assert_eq!(TabKind::Activity.prev(), TabKind::Query);
    assert_eq!(TabKind::Activity.next(), TabKind::Errors);
    assert_eq!(TabKind::Errors.prev(), TabKind::Activity);
    assert_eq!(TabKind::Errors.next(), TabKind::Project);
    assert_eq!(TabKind::Project.prev(), TabKind::Errors);
    assert_eq!(TabKind::Project.next(), TabKind::Review);
    assert_eq!(TabKind::Review.prev(), TabKind::Project);
    assert_eq!(TabKind::Review.next(), TabKind::Watchers);
    assert_eq!(TabKind::Watchers.prev(), TabKind::Review);
    assert_eq!(TabKind::Watchers.next(), TabKind::Embeddings);
    assert_eq!(TabKind::Embeddings.prev(), TabKind::Watchers);
    assert_eq!(TabKind::Embeddings.next(), TabKind::Resume);
    assert_eq!(TabKind::Resume.prev(), TabKind::Embeddings);
    assert_eq!(TabKind::Resume.next(), TabKind::Memories);
}

#[test]
fn backend_activity_dedupes_by_persisted_event_id_and_formats_tokens() {
    let mut app = new_test_app();
    let id = Uuid::new_v4();
    let event = ActivityEvent {
        id,
        recorded_at: Utc::now(),
        project: "memory".to_string(),
        kind: ActivityKind::Query,
        memory_id: None,
        summary: "Query: activity model".to_string(),
        details: None,
        actor_id: None,
        actor_name: None,
        source: Some("query".to_string()),
        operation_id: None,
        duration_ms: Some(42),
        provider: Some("openai_compatible".to_string()),
        model: Some("gpt-test".to_string()),
        token_usage: Some(TokenUsage {
            input_tokens: 1000,
            output_tokens: 250,
            cache_read_tokens: 100,
            cache_write_tokens: 0,
            total_tokens: 1350,
        }),
    };

    app.record_backend_activity(event.clone());
    app.record_backend_activity(event);

    assert_eq!(app.activity.activity_events.len(), 1);
    assert_eq!(activity_tokens(&app.activity.activity_events[0]), "1.4k");
    assert_eq!(activity_duration(&app.activity.activity_events[0]), "42");
}

#[test]
fn llm_audit_status_lines_render_current_state() {
    let mut app = new_test_app();
    app.activity.llm_audit_status = Some(LlmAuditStatusResponse {
        enabled: true,
        redacted: true,
        max_message_chars: 8000,
        max_total_chars: 32000,
        profile: "dev".to_string(),
        config_path: Some("/repo/.mem/config.dev.toml".to_string()),
    });

    let rendered = llm_audit_status_lines(&app)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("LLM audit: on"));
    assert!(rendered.contains("redaction=on"));
    assert!(rendered.contains("profile=dev"));
    assert!(rendered.contains("A disable"));
    assert!(rendered.contains("/repo/.mem/config.dev.toml"));
}

#[test]
fn llm_audit_toggle_event_updates_status_message() {
    let mut app = new_test_app();

    app.apply_background_event(BackgroundEvent::LlmAuditToggled {
        enabled: true,
        response: Ok(LlmAuditStatusResponse {
            enabled: true,
            redacted: true,
            max_message_chars: 8000,
            max_total_chars: 32000,
            profile: "prod".to_string(),
            config_path: Some("/config/memory-layer.toml".to_string()),
        }),
    });

    assert!(!app.activity.llm_audit_toggling);
    assert!(app.activity.llm_audit_error.is_none());
    assert!(
        app.activity
            .llm_audit_status
            .as_ref()
            .is_some_and(|status| status.enabled)
    );
    assert_eq!(
        app.chrome.status_message,
        "LLM audit/debug logging enabled."
    );
    assert_eq!(app.chrome.ui_status, UiStatus::Ready);
}

#[test]
fn activity_help_mentions_llm_audit_toggle() {
    let help = super::tab_help_markdown(TabKind::Activity);
    assert!(help.contains("Shift+A"));
    assert!(help.contains("LLM audit/debug"));
}

#[test]
fn errors_tab_collects_persisted_diagnostics() {
    let mut app = new_test_app();
    app.service.health_ok = true;
    app.record_backend_activity(ActivityEvent {
        id: Uuid::new_v4(),
        recorded_at: Utc::now(),
        project: "memory".to_string(),
        kind: ActivityKind::Diagnostic,
        memory_id: None,
        summary: "embedding quota exceeded".to_string(),
        details: Some(ActivityDetails::Diagnostic {
            diagnostic: DiagnosticInfo {
                code: "embedding_quota_exceeded".to_string(),
                source: "provider".to_string(),
                component: "embeddings".to_string(),
                operation: "automatic_embedding_creation".to_string(),
                severity: DiagnosticSeverity::Warning,
                message: "embedding quota exceeded".to_string(),
                raw_error: Some("429 insufficient_quota".to_string()),
                explanation: Some("provider quota was exhausted".to_string()),
                fix_hint: Some("restore quota or disable automatic creation".to_string()),
                doctor_hint: Some("memory doctor".to_string()),
                command_hint: Some("memory embeddings list".to_string()),
            },
        }),
        actor_id: None,
        actor_name: None,
        source: Some("service".to_string()),
        operation_id: None,
        duration_ms: None,
        provider: None,
        model: None,
        token_usage: None,
    });

    let items = collect_error_items(&app);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].diagnostic.code, "embedding_quota_exceeded");
    assert_eq!(tui_status_detail(&app), Some("1 error".to_string()));
}

#[test]
fn backend_query_activity_detail_renders_graph_metadata() {
    let event = ActivityEvent {
        id: Uuid::new_v4(),
        recorded_at: Utc::now(),
        project: "memory".to_string(),
        kind: ActivityKind::Query,
        memory_id: None,
        summary: "Query: graph retrieval".to_string(),
        details: Some(ActivityDetails::Query {
            query: "GraphTarget".to_string(),
            top_k: 8,
            result_count: 2,
            confidence: 0.82,
            insufficient_evidence: false,
            total_duration_ms: 91,
            graph_status: Some("active".to_string()),
            graph_candidates: 4,
            graph_augmented_candidates: 2,
            graph_duration_ms: 17,
            graph_result_count: 1,
            graph_connection_count: 2,
            graph_connections: vec![mem_api::QueryGraphConnection {
                file_path: "src/lib.rs".to_string(),
                symbol: Some("GraphTarget".to_string()),
                symbol_kind: Some("function".to_string()),
                edge_kind: Some("calls".to_string()),
                neighbor_symbol: Some("caller".to_string()),
                direction: Some("incoming".to_string()),
                score_boost: 1.25,
                reason: "code symbol match".to_string(),
            }],
            answer: Some("Use the graph-aware result.".to_string()),
            error: None,
        }),
        actor_id: None,
        actor_name: None,
        source: Some("query".to_string()),
        operation_id: None,
        duration_ms: Some(91),
        provider: None,
        model: None,
        token_usage: None,
    };

    let rendered = backend_activity_detail_lines(&event)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Graph Retrieval"));
    assert!(rendered.contains("Status: active"));
    assert!(rendered.contains("Candidates: 4"));
    assert!(rendered.contains("Augmented results: 2"));
    assert!(rendered.contains("Graph Connections"));
    assert!(rendered.contains("code symbol match | src/lib.rs"));
}

#[test]
fn historical_query_activity_without_graph_metadata_omits_graph_section() {
    let details: ActivityDetails = serde_json::from_value(serde_json::json!({
        "type": "query",
        "query": "old query",
        "top_k": 8,
        "result_count": 1,
        "confidence": 0.7,
        "insufficient_evidence": false,
        "total_duration_ms": 42,
        "answer": "old answer"
    }))
    .expect("historical query details should deserialize");
    let event = ActivityEvent {
        id: Uuid::new_v4(),
        recorded_at: Utc::now(),
        project: "memory".to_string(),
        kind: ActivityKind::Query,
        memory_id: None,
        summary: "Query: old query".to_string(),
        details: Some(details),
        actor_id: None,
        actor_name: None,
        source: Some("query".to_string()),
        operation_id: None,
        duration_ms: Some(42),
        provider: None,
        model: None,
        token_usage: None,
    };

    let rendered = backend_activity_detail_lines(&event)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Query: old query"));
    assert!(!rendered.contains("Graph Retrieval"));
}

#[test]
fn backend_llm_audit_activity_detail_renders_prompt_messages() {
    let event = ActivityEvent {
        id: Uuid::new_v4(),
        recorded_at: Utc::now(),
        project: "memory".to_string(),
        kind: ActivityKind::LlmAudit,
        memory_id: None,
        summary: "LLM audit: query_answer success".to_string(),
        details: Some(ActivityDetails::LlmAudit {
            operation: "query_answer".to_string(),
            request_summary: "Question: audit".to_string(),
            status: "success".to_string(),
            redacted: true,
            truncated: false,
            messages: vec![LlmAuditMessage {
                role: "user".to_string(),
                content: "Question: audit".to_string(),
                truncated: false,
            }],
            error: None,
        }),
        actor_id: None,
        actor_name: None,
        source: Some("llm_audit".to_string()),
        operation_id: None,
        duration_ms: Some(12),
        provider: Some("openai_compatible".to_string()),
        model: Some("gpt-test".to_string()),
        token_usage: None,
    };

    let rendered = backend_activity_detail_lines(&event)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Kind: llm-audit"));
    assert!(rendered.contains("Operation: query_answer"));
    assert!(rendered.contains("LLM Messages"));
    assert!(rendered.contains("Role user: Question: audit"));
}

#[test]
fn backend_graph_extract_activity_detail_renders_counts() {
    let run_id = Uuid::new_v4();
    let event = ActivityEvent {
        id: Uuid::new_v4(),
        recorded_at: Utc::now(),
        project: "memory".to_string(),
        kind: ActivityKind::GraphExtract,
        memory_id: None,
        summary: "Extracted code graph: 10 symbols, 20 references, 9 graph edge(s).".to_string(),
        details: Some(ActivityDetails::GraphExtract {
            repo_root: "/repo".to_string(),
            git_head: Some("abc123".to_string()),
            since: None,
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
        }),
        actor_id: None,
        actor_name: None,
        source: Some("service".to_string()),
        operation_id: None,
        duration_ms: None,
        provider: None,
        model: None,
        token_usage: None,
    };

    let rendered = backend_activity_detail_lines(&event)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Kind: graph"));
    assert!(rendered.contains("Extraction run:"));
    assert!(rendered.contains("Symbols: 10"));
    assert!(rendered.contains("Graph edges: 9"));
    assert!(rendered.contains("Analyzer: mem-analyze-v2"));
}

#[test]
fn markdown_renderer_formats_rich_memory_text_readably() {
    let lines = render_markdown_lines(
        "# Heading\n\n- [x] shipped\n1. numbered\n> quoted\n\nVisit [docs](https://example.com) and use `cargo test`.\n\n```rust\nfn main() {}\n```",
    );
    let rendered = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Heading"));
    assert!(rendered.contains("[x] shipped"));
    assert!(rendered.contains("1. numbered"));
    assert!(rendered.contains("quoted"));
    assert!(rendered.contains("docs (https://example.com)"));
    assert!(rendered.contains("cargo test"));
    assert!(rendered.contains("fn main() {}"));
}

#[test]
fn build_memory_detail_lines_includes_rendered_canonical_text() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.5".to_string(),
            mem_service: "0.4.5".to_string(),
            watch_manager: "0.4.5".to_string(),
            memory_watch: "0.4.5".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.memories.selected_detail = Some(test_memory_detail(
        "# Canonical\n\n- [ ] readable\n\n```text\nblock\n```",
    ));

    let rendered = build_memory_detail_lines(&app)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Canonical Text"));
    assert!(rendered.contains("Canonical"));
    assert!(rendered.contains("[ ] readable"));
    assert!(rendered.contains("block"));
}

#[test]
fn build_memory_detail_lines_lists_each_embedding_space() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.0".to_string(),
            mem_service: "0.6.0".to_string(),
            watch_manager: "0.6.0".to_string(),
            memory_watch: "0.6.0".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    let mut detail = test_memory_detail("body");
    let updated = Utc.with_ymd_and_hms(2026, 4, 22, 23, 37, 0).unwrap();
    detail.embedding_spaces = vec![
        MemoryEmbeddingSpace {
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            chunk_count: 12,
            last_updated: Some(updated),
        },
        MemoryEmbeddingSpace {
            provider: "voyage".to_string(),
            model: "voyage-3".to_string(),
            base_url: "https://proxy.internal/voyage".to_string(),
            chunk_count: 1,
            last_updated: None,
        },
    ];
    app.memories.selected_detail = Some(detail);

    let rendered = build_memory_detail_lines(&app)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Embeddings"));
    assert!(rendered.contains("openai"));
    assert!(rendered.contains("text-embedding-3-small"));
    assert!(rendered.contains("12 chunks"));
    assert!(rendered.contains("voyage"));
    assert!(rendered.contains("voyage-3"));
    assert!(rendered.contains("1 chunk"));
    // OpenAI uses the default base URL, so it should not appear in the rendered output.
    assert!(!rendered.contains("https://api.openai.com/v1"));
    // Voyage is on a non-default base URL, so the URL appears on its own line.
    assert!(rendered.contains("https://proxy.internal/voyage"));
}

#[test]
fn build_memory_detail_lines_puts_embeddings_section_above_canonical_text() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.2".to_string(),
            mem_service: "0.6.2".to_string(),
            watch_manager: "0.6.2".to_string(),
            memory_watch: "0.6.2".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.memories.selected_detail = Some(test_memory_detail("body text"));

    let lines = build_memory_detail_lines(&app);
    let rendered = lines.iter().map(ToString::to_string).collect::<Vec<_>>();
    let embeddings_idx = rendered
        .iter()
        .position(|line| line.contains("Embeddings"))
        .expect("Embeddings header present");
    let canonical_idx = rendered
        .iter()
        .position(|line| line.contains("Canonical Text"))
        .expect("Canonical Text header present");
    assert!(
        embeddings_idx < canonical_idx,
        "Embeddings section must render above Canonical Text (embeddings at {embeddings_idx}, canonical at {canonical_idx})"
    );
}

#[test]
fn build_memory_detail_lines_shows_empty_state_when_no_embeddings() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.0".to_string(),
            mem_service: "0.6.0".to_string(),
            watch_manager: "0.6.0".to_string(),
            memory_watch: "0.6.0".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.memories.selected_detail = Some(test_memory_detail("body"));

    let rendered = build_memory_detail_lines(&app)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Embeddings"));
    assert!(rendered.contains("No embeddings for this memory yet."));
}

fn embeddings_test_response() -> mem_api::EmbeddingBackendsResponse {
    mem_api::EmbeddingBackendsResponse {
        backends: vec![
            mem_api::EmbeddingBackendInfo {
                name: "openai-3-small".to_string(),
                provider: "openai_compatible".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "text-embedding-3-small".to_string(),
                active: false,
                ready: true,
                create_enabled: true,
                project_chunk_count: Some(12),
                project_memory_count: Some(4),
            },
            mem_api::EmbeddingBackendInfo {
                name: "voyage-code".to_string(),
                provider: "voyage".to_string(),
                base_url: "https://api.voyageai.com".to_string(),
                model: "voyage-code-3".to_string(),
                active: true,
                ready: true,
                create_enabled: true,
                project_chunk_count: Some(12),
                project_memory_count: Some(4),
            },
        ],
        active: Some("voyage-code".to_string()),
        create_enabled: true,
    }
}

fn new_test_app() -> App {
    let (tx, _rx) = mpsc::unbounded_channel();
    App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.6.2".to_string(),
            mem_service: "0.6.2".to_string(),
            watch_manager: "0.6.2".to_string(),
            memory_watch: "0.6.2".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    )
}

fn tab_key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE))
}

#[test]
fn tab_update_functions_handle_representative_local_keys() {
    let mut app = new_test_app();

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::memories::update(&tab_key(KeyCode::Enter), &mut app.memories, &mut ctx),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.memories.memories_focus, MemoriesFocus::Detail);

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::agents::update(&tab_key(KeyCode::PageDown), &mut app.agents, &mut ctx),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.agents.agent_detail_scroll, 8);

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::activity::update(
            &tab_key(KeyCode::PageDown),
            &mut app.activity,
            &mut ctx,
        ),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.activity.activity_detail_scroll, 8);

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::errors::update(&tab_key(KeyCode::PageDown), &mut app.errors, &mut ctx),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.errors.errors_detail_scroll, 8);

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::project::update(&tab_key(KeyCode::Down), &mut app.project_tab, &mut ctx,),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.project_tab.project_scroll, 1);

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::watchers::update(&tab_key(KeyCode::Down), &mut app.watchers, &mut ctx,),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.watchers.watcher_scroll, 1);

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::resume::update(&tab_key(KeyCode::Down), &mut app.resume, &mut ctx),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.resume.resume_scroll, 1);

    app.embeddings.embedding_backends_snapshot = Some(embeddings_test_response());
    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::embeddings::update(
            &tab_key(KeyCode::Down),
            &mut app.embeddings,
            &mut ctx,
        ),
        crate::tui::tabs::TabAction::Redraw
    );
    assert_eq!(app.embeddings.embeddings_selected_index, 1);

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::review::update(&tab_key(KeyCode::Down), &mut app.review, &mut ctx),
        crate::tui::tabs::TabAction::Redraw
    );

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::query::update(&tab_key(KeyCode::Down), &mut app.query, &mut ctx),
        crate::tui::tabs::TabAction::None
    );
}

#[test]
fn query_tab_down_arrow_advances_selection_when_results_exist() {
    let mut app = new_test_app();
    app.query.query_response = Some(test_query_response_with_two_results());
    app.query.query_selected_detail = Some(test_memory_detail("stale detail"));
    app.query.query_detail_loading = true;

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::query::update(&tab_key(KeyCode::Down), &mut app.query, &mut ctx),
        crate::tui::tabs::TabAction::QuerySelectionChanged
    );

    assert_eq!(app.query.query_selected_index, 1);
    assert_eq!(app.query.query_table_state.selected(), Some(1));
    assert!(app.query.query_selected_detail.is_none());
    assert!(!app.query.query_detail_loading);
}

#[test]
fn query_tab_j_advances_selection_when_results_exist() {
    let mut app = new_test_app();
    app.query.query_response = Some(test_query_response_with_two_results());

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::query::update(&tab_key(KeyCode::Char('j')), &mut app.query, &mut ctx),
        crate::tui::tabs::TabAction::QuerySelectionChanged
    );

    assert_eq!(app.query.query_selected_index, 1);
    assert_eq!(app.query.query_table_state.selected(), Some(1));
}

#[test]
fn query_tab_up_arrow_clamped_at_zero() {
    let mut app = new_test_app();
    app.query.query_response = Some(test_query_response_with_two_results());

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::query::update(&tab_key(KeyCode::Up), &mut app.query, &mut ctx),
        crate::tui::tabs::TabAction::None
    );

    assert_eq!(app.query.query_selected_index, 0);
    assert_eq!(app.query.query_table_state.selected(), Some(0));
}

#[test]
fn query_tab_k_retreats_selection_when_possible() {
    let mut app = new_test_app();
    app.query.query_response = Some(test_query_response_with_two_results());
    app.query.query_selected_index = 1;
    app.query.query_table_state.select(Some(1));

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::query::update(&tab_key(KeyCode::Char('k')), &mut app.query, &mut ctx),
        crate::tui::tabs::TabAction::QuerySelectionChanged
    );

    assert_eq!(app.query.query_selected_index, 0);
    assert_eq!(app.query.query_table_state.selected(), Some(0));
}

#[test]
fn query_tab_navigation_no_op_with_empty_results() {
    let mut app = new_test_app();
    app.query.query_response = Some(QueryResponse {
        results: Vec::new(),
        ..test_query_response_with_timings()
    });

    let mut ctx = crate::tui::tabs::TabContext::new(&app);
    assert_eq!(
        crate::tui::tabs::query::update(&tab_key(KeyCode::Down), &mut app.query, &mut ctx),
        crate::tui::tabs::TabAction::None
    );

    assert_eq!(app.query.query_selected_index, 0);
    assert_eq!(app.query.query_table_state.selected(), Some(0));
}

#[test]
fn embeddings_loaded_event_populates_snapshot_and_clamps_selection() {
    let mut app = new_test_app();
    app.embeddings.embeddings_selected_index = 5; // out of range
    app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
        snapshot: Ok(embeddings_test_response()),
    });
    let snapshot = app
        .embeddings
        .embedding_backends_snapshot
        .as_ref()
        .expect("loaded");
    assert_eq!(snapshot.backends.len(), 2);
    assert_eq!(app.embeddings.embeddings_selected_index, 1);
    assert_eq!(app.embeddings.embeddings_table_state.selected(), Some(1));
    assert!(app.embeddings.embedding_backends_error.is_none());
}

#[test]
fn embeddings_loaded_event_selects_active_backend() {
    let mut app = new_test_app();
    app.embeddings.embeddings_selected_index = 0;

    app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
        snapshot: Ok(embeddings_test_response()),
    });

    assert_eq!(app.embeddings.embeddings_selected_index, 1);
    assert_eq!(app.embeddings.embeddings_table_state.selected(), Some(1));
    assert_eq!(
        app.selected_embedding_backend_name().as_deref(),
        Some("voyage-code")
    );
}

#[test]
fn embeddings_selection_wraps_cyclically() {
    let mut app = new_test_app();
    app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
        snapshot: Ok(embeddings_test_response()),
    });
    app.embeddings.embeddings_selected_index = 0;
    app.move_embeddings_selection(1);
    assert_eq!(app.embeddings.embeddings_selected_index, 1);
    assert_eq!(
        app.selected_embedding_backend_name().as_deref(),
        Some("voyage-code")
    );
    app.move_embeddings_selection(1);
    assert_eq!(app.embeddings.embeddings_selected_index, 0);
    app.move_embeddings_selection(-1);
    assert_eq!(app.embeddings.embeddings_selected_index, 1);
}

#[test]
fn embedding_backend_toggle_sets_success_message_and_updates_snapshot() {
    let mut app = new_test_app();
    // First load the initial list so selection is primed.
    app.apply_background_event(BackgroundEvent::EmbeddingBackendsLoaded {
        snapshot: Ok(embeddings_test_response()),
    });
    app.embeddings.embeddings_toggling = Some("openai-3-small".to_string());

    // Simulate the activate POST returning a response where openai is now active.
    let mut response = embeddings_test_response();
    response.active = Some("openai-3-small".to_string());
    response.backends[0].active = true;
    response.backends[1].active = false;
    app.apply_background_event(BackgroundEvent::EmbeddingBackendToggled {
        name: "openai-3-small".to_string(),
        result: Ok(response),
    });

    assert_eq!(app.embeddings.embeddings_toggling, None);
    assert_eq!(
        app.embeddings.embeddings_toggle_message.as_deref(),
        Some("Activated openai-3-small")
    );
    assert_eq!(
        app.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|s| s.active.as_deref()),
        Some("openai-3-small")
    );
}

#[test]
fn embedding_backend_toggle_off_sets_success_message() {
    let mut app = new_test_app();
    app.embeddings.embeddings_toggling = Some("turning off voyage-code".to_string());
    let mut response = embeddings_test_response();
    response.active = None;
    response.backends[1].active = false;

    app.apply_background_event(BackgroundEvent::EmbeddingBackendToggled {
        name: "voyage-code".to_string(),
        result: Ok(response),
    });

    assert_eq!(app.embeddings.embeddings_toggling, None);
    assert_eq!(
        app.embeddings.embeddings_toggle_message.as_deref(),
        Some("Embeddings off")
    );
    assert_eq!(
        app.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|s| s.active.as_deref()),
        None
    );
}

#[test]
fn embedding_creation_toggle_updates_snapshot_and_status() {
    let mut app = new_test_app();
    app.embeddings.embeddings_creation_toggling = true;
    let mut response = embeddings_test_response();
    response.backends[1].create_enabled = false;

    app.apply_background_event(BackgroundEvent::EmbeddingCreationToggled {
        name: "voyage-code".to_string(),
        enabled: false,
        result: Ok(response),
    });

    assert!(!app.embeddings.embeddings_creation_toggling);
    assert_eq!(
        app.embeddings.embeddings_toggle_message.as_deref(),
        Some("Automatic embedding creation off for voyage-code")
    );
    assert_eq!(
        app.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.backends.get(1))
            .map(|backend| backend.create_enabled),
        Some(false)
    );
}

#[test]
fn embedding_reembed_completion_updates_snapshot_and_status() {
    let mut app = new_test_app();
    app.embeddings.embeddings_selected_index = 0;
    app.embeddings.embeddings_operation =
        Some("creating embeddings for openai-3-small".to_string());
    let mut snapshot = embeddings_test_response();
    snapshot.backends[0].project_chunk_count = Some(18);

    app.apply_background_event(BackgroundEvent::EmbeddingReembedCompleted {
        name: "openai-3-small".to_string(),
        result: Ok((
            mem_api::ReembedResponse {
                reembedded_chunks: 6,
                dry_run: false,
            },
            snapshot,
        )),
    });

    assert_eq!(app.embeddings.embeddings_operation, None);
    assert_eq!(
        app.embeddings.embeddings_toggle_message.as_deref(),
        Some("Created 6 chunk embedding(s) for openai-3-small")
    );
    assert_eq!(app.embeddings.embeddings_selected_index, 0);
    assert_eq!(
        app.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.backends.first())
            .and_then(|backend| backend.project_chunk_count),
        Some(18)
    );
}

#[test]
fn embedding_reindex_completion_updates_snapshot_and_status() {
    let mut app = new_test_app();
    app.embeddings.embeddings_selected_index = 1;
    app.embeddings.embeddings_operation = Some("reindexing all backends".to_string());
    let mut snapshot = embeddings_test_response();
    snapshot.backends[1].project_chunk_count = Some(20);

    app.apply_background_event(BackgroundEvent::EmbeddingReindexCompleted {
        name: "all backends".to_string(),
        result: Ok((
            mem_api::ReindexResponse {
                reindexed_entries: 4,
                dry_run: false,
            },
            snapshot,
        )),
    });

    assert_eq!(app.embeddings.embeddings_operation, None);
    assert_eq!(
        app.embeddings.embeddings_toggle_message.as_deref(),
        Some("Reindexed 4 memory entries for all backends")
    );
    assert_eq!(app.embeddings.embeddings_selected_index, 1);
    assert_eq!(
        app.embeddings
            .embedding_backends_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.backends.get(1))
            .and_then(|backend| backend.project_chunk_count),
        Some(20)
    );
}

#[test]
fn embedding_reembed_failure_shows_error_message() {
    let mut app = new_test_app();
    app.embeddings.embeddings_operation = Some("creating embeddings for broken".to_string());

    app.apply_background_event(BackgroundEvent::EmbeddingReembedCompleted {
        name: "broken".to_string(),
        result: Err("provider unavailable".to_string()),
    });

    assert_eq!(app.embeddings.embeddings_operation, None);
    assert_eq!(
        app.embeddings.embeddings_toggle_message.as_deref(),
        Some("Embedding creation failed for broken: provider unavailable")
    );
}

#[test]
fn embedding_backend_toggle_failure_shows_error_message() {
    let mut app = new_test_app();
    app.embeddings.embeddings_toggling = Some("broken".to_string());
    app.apply_background_event(BackgroundEvent::EmbeddingBackendToggled {
        name: "broken".to_string(),
        result: Err("400 unknown backend".to_string()),
    });
    assert_eq!(app.embeddings.embeddings_toggling, None);
    let msg = app
        .embeddings
        .embeddings_toggle_message
        .as_deref()
        .unwrap_or("");
    assert!(msg.starts_with("Toggle failed:"));
    assert!(msg.contains("400 unknown backend"));
}

#[test]
fn memories_focus_toggle_and_escape_return_to_list() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.5".to_string(),
            mem_service: "0.4.5".to_string(),
            watch_manager: "0.4.5".to_string(),
            memory_watch: "0.4.5".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    app.memories.selected_detail = Some(test_memory_detail("detail"));

    assert_eq!(app.memories.memories_focus, MemoriesFocus::List);
    app.toggle_memories_focus();
    assert_eq!(app.memories.memories_focus, MemoriesFocus::Detail);
    app.focus_memories_list();
    assert_eq!(app.memories.memories_focus, MemoriesFocus::List);
}

#[test]
fn memory_detail_scroll_is_clamped_to_rendered_content() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        "memory".to_string(),
        PathBuf::from("/tmp/memory"),
        ToolVersions {
            mem_cli: "0.4.5".to_string(),
            mem_service: "0.4.5".to_string(),
            watch_manager: "0.4.5".to_string(),
            memory_watch: "0.4.5".to_string(),
        },
        false,
        Profile::Prod,
        tx,
    );
    let canonical = (0..40)
        .map(|idx| format!("- [x] item {idx} with enough text to wrap in the detail pane"))
        .collect::<Vec<_>>()
        .join("\n");
    app.memories.selected_detail = Some(test_memory_detail(&canonical));
    let frame = ratatui::layout::Rect::new(0, 0, 100, 24);

    let max_scroll = memory_detail_max_scroll(&app, frame);
    assert!(max_scroll > 0);

    app.scroll_memory_detail_in_area(500, frame);
    assert_eq!(app.memories.memory_detail_scroll, max_scroll);

    app.scroll_memory_detail_in_area(-500, frame);
    assert_eq!(app.memories.memory_detail_scroll, 0);
}
