use mem_api::{
    AgentWorkspaceFinishRequest, AgentWorkspaceStartRequest, AgentWorkspaceStatus,
    LoopApprovalDecisionRequest, LoopApprovalStatus, LoopMemoryProposalCreateRequest,
    LoopMemoryProposalDecisionRequest, LoopMode, LoopRunRequest, LoopRunStatus,
    LoopTriggerRouteRequest, LoopTrustLevel, MemoryStatus, MemoryType, ProjectMemoryGraphEdgeKind,
    ProjectMemoryGraphNodeKind, SourceProvenanceStatus,
};
use sqlx::PgPool;
use std::{fs, path::Path, path::PathBuf, process::Command};
use uuid::Uuid;

#[tokio::test]
async fn agent_workspace_repository_records_and_lists_active_work() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-agent-workspace");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    let first = mem_service::repository::handlers::agents::upsert_agent_workspace_start(
        &pool,
        &AgentWorkspaceStartRequest {
            project: project.clone(),
            repo_root: "/tmp/memory".to_string(),
            worktree_path: "/tmp/memory-agent-a".to_string(),
            branch: "agent/a".to_string(),
            task: Some("review graph work".to_string()),
            base_commit: Some("base-a".to_string()),
            head_commit: Some("head-a".to_string()),
            dirty_files: vec!["web/src/features/graph/GraphTab.tsx".to_string()],
            agent_cli: "codex".to_string(),
            agent_session_id: Some("session-a".to_string()),
            hostname: Some("host-a".to_string()),
            writer_id: Some("writer-a".to_string()),
            profile: Some("dev".to_string()),
            service_endpoint: Some("http://127.0.0.1:4250".to_string()),
        },
    )
    .await
    .expect("start first agent workspace");
    let second = mem_service::repository::handlers::agents::upsert_agent_workspace_start(
        &pool,
        &AgentWorkspaceStartRequest {
            project: project.clone(),
            repo_root: "/tmp/memory".to_string(),
            worktree_path: "/tmp/memory-agent-b".to_string(),
            branch: "agent/b".to_string(),
            task: Some("review docs".to_string()),
            base_commit: Some("base-b".to_string()),
            head_commit: Some("head-b".to_string()),
            dirty_files: vec![
                "web/src/features/graph/GraphTab.tsx".to_string(),
                "docs/user/cli/agents.md".to_string(),
            ],
            agent_cli: "opencode".to_string(),
            agent_session_id: Some("session-b".to_string()),
            hostname: Some("host-b".to_string()),
            writer_id: Some("writer-b".to_string()),
            profile: Some("dev".to_string()),
            service_endpoint: Some("http://127.0.0.1:4250".to_string()),
        },
    )
    .await
    .expect("start second agent workspace");

    assert_eq!(first.task.as_deref(), Some("review graph work"));
    assert_eq!(second.agent_cli, "opencode");

    let active =
        mem_service::repository::handlers::agents::fetch_agent_workspaces(&pool, &project, false)
            .await
            .expect("list active agent workspaces");
    assert_eq!(active.workspaces.len(), 2);
    assert!(active.warnings.iter().any(|warning| {
        warning.code == "dirty_file_overlap"
            && warning
                .message
                .contains("web/src/features/graph/GraphTab.tsx")
    }));

    mem_service::repository::handlers::agents::finish_agent_workspace_record(
        &pool,
        first.id,
        &AgentWorkspaceFinishRequest {
            status: Some(AgentWorkspaceStatus::Completed),
            head_commit: Some("head-a2".to_string()),
            dirty_files: Vec::new(),
            finish_summary: Some("pushed agent/a".to_string()),
            pushed_branch: Some(true),
            merged_commit: None,
        },
    )
    .await
    .expect("finish first agent workspace")
    .expect("finished workspace exists");

    let active_after_finish =
        mem_service::repository::handlers::agents::fetch_agent_workspaces(&pool, &project, false)
            .await
            .expect("list active agent workspaces after finish");
    assert_eq!(active_after_finish.workspaces.len(), 1);
    assert_eq!(active_after_finish.workspaces[0].id, second.id);

    let all =
        mem_service::repository::handlers::agents::fetch_agent_workspaces(&pool, &project, true)
            .await
            .expect("list all agent workspaces");
    assert_eq!(all.workspaces.len(), 2);
    assert!(all.workspaces.iter().any(|workspace| {
        workspace.id == first.id
            && workspace.status == AgentWorkspaceStatus::Completed
            && workspace.pushed_branch == Some(true)
    }));

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn repository_handler_write_and_read_paths_roundtrip_memory() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-repository");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project through repository handler");
    let memory_id = insert_memory_fixture(&pool, project_id).await;

    let memory = mem_service::repository::handlers::memory::fetch_memory_entry(&pool, memory_id)
        .await
        .expect("fetch memory through repository handler")
        .expect("memory entry exists");

    assert_eq!(memory.id, memory_id);
    assert_eq!(memory.project, project);
    assert_eq!(memory.summary, "Repository DB test memory");
    assert_eq!(
        memory.canonical_text,
        "Repository handler tests cover a write path and a read path."
    );
    assert_eq!(memory.memory_type, MemoryType::Implementation);
    assert_eq!(memory.status, MemoryStatus::Active);
    assert_eq!(memory.version_no, 1);
    assert!(!memory.is_tombstone);

    mem_test_support::cleanup_project(&pool, &memory.project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn repository_memory_graph_returns_provenance_and_relationship_layers() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-memory-graph");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project through repository handler");
    let source_memory_id = insert_hygiene_memory_fixture(
        &pool,
        project_id,
        "Graph endpoint exposes provenance",
        "Memory graph tests derive provenance from source records.",
        0.91,
        4,
    )
    .await;
    let target_memory_id = insert_hygiene_memory_fixture(
        &pool,
        project_id,
        "Graph endpoint exposes relations",
        "Memory graph tests derive relation edges from memory_relations.",
        0.88,
        3,
    )
    .await;
    let first_source_id = insert_graph_memory_source(&pool, source_memory_id).await;
    let _second_source_id = insert_graph_memory_source(&pool, target_memory_id).await;
    sqlx::query(
        r#"
        INSERT INTO memory_source_verifications (source_id, status, checked_at, reason, resolved_path)
        VALUES ($1, 'verified', now(), 'repository test', '/repo/src/graph.rs')
        "#,
    )
    .bind(first_source_id)
    .execute(&pool)
    .await
    .expect("insert source verification");
    sqlx::query(
        r#"
        INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id)
        VALUES ($1, $2, 'supports', $3)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(source_memory_id)
    .bind(target_memory_id)
    .execute(&pool)
    .await
    .expect("insert memory relation");

    let graph = mem_service::repository::fetch_project_memory_graph(&pool, &project, 250, 0)
        .await
        .expect("fetch memory graph");

    assert_eq!(graph.project, project);
    assert_eq!(graph.total_memories, 2);
    assert_eq!(graph.returned_memories, 2);
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.node_kind == ProjectMemoryGraphNodeKind::Memory)
            .count(),
        2
    );
    let source_nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.node_kind == ProjectMemoryGraphNodeKind::Source)
        .collect();
    assert_eq!(
        source_nodes.len(),
        1,
        "shared file/symbol sources should be deduped"
    );
    assert_eq!(source_nodes[0].file_path.as_deref(), Some("src/graph.rs"));
    assert_eq!(
        source_nodes[0].symbol_name.as_deref(),
        Some("build_memory_graph")
    );
    assert_eq!(
        source_nodes[0].provenance_status,
        Some(SourceProvenanceStatus::Verified)
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.edge_kind == ProjectMemoryGraphEdgeKind::Provenance)
            .count(),
        2
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.edge_kind == ProjectMemoryGraphEdgeKind::MemoryRelation
            && edge
                .relation_type
                .as_ref()
                .is_some_and(|relation| relation.to_string() == "supports")
    }));

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn repository_memory_graph_returns_empty_graph_for_empty_project() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-empty-memory-graph");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");
    mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
        .await
        .expect("upsert project through repository handler");

    let graph = mem_service::repository::fetch_project_memory_graph(&pool, &project, 250, 0)
        .await
        .expect("fetch memory graph");

    assert_eq!(graph.project, project);
    assert_eq!(graph.total_memories, 0);
    assert_eq!(graph.returned_memories, 0);
    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_registers_definitions_and_records_run() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let definitions =
        mem_service::repository::handlers::loops::list_registered_loop_definitions(&pool)
            .await
            .expect("fetch loop definitions");
    assert!(definitions.iter().any(|definition| {
        definition.loop_id == mem_loops::LOOP_CONTEXT_PACK_REFRESH && definition.version == 1
    }));

    let request = LoopRunRequest {
        project: Some(project.clone()),
        repo_root: Some(repo_root.clone()),
        scope_type: None,
        scope_id: None,
        dry_run: true,
        reason: Some("db repository integration test".to_string()),
        trigger_payload: None,
    };
    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_CONTEXT_PACK_REFRESH,
        &request,
    )
    .await
    .expect("create loop run")
    .run;

    assert_eq!(run.summary.loop_id, mem_loops::LOOP_CONTEXT_PACK_REFRESH);
    assert_eq!(run.summary.project.as_deref(), Some(project.as_str()));
    assert_eq!(run.summary.repo_root.as_deref(), Some(repo_root.as_str()));
    assert_eq!(run.summary.status, LoopRunStatus::Blocked);
    assert!(
        run.summary
            .blocked_reasons
            .contains(&"loop_not_enabled".to_string())
    );
    assert_eq!(run.traces.len(), 2);

    let loaded =
        mem_service::repository::handlers::loops::read_loop_run_detail(&pool, run.summary.id)
            .await
            .expect("read loop run");
    assert_eq!(loaded.summary.id, run.summary.id);
    assert_eq!(loaded.summary.trace_count, 2);
    assert_eq!(
        loaded.run_reason.as_deref(),
        Some("db repository integration test")
    );
    assert_eq!(
        loaded
            .trigger_event
            .as_ref()
            .map(|event| event.event_type.as_str()),
        Some("manual_run")
    );
    assert!(loaded.memory_proposals.is_empty());

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_routes_trigger_events_and_dedupes() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-route");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    enable_project_loop(&pool, project_id, &project).await;

    let request = LoopTriggerRouteRequest {
        source: "repository-test".to_string(),
        event_type: "memory_changed".to_string(),
        project: Some(project.clone()),
        repo_root: Some(repo_root.clone()),
        payload: serde_json::json!({"memory_id": "example"}),
        dedupe_key: Some(format!("test:{project}:memory_changed")),
        trust_level: LoopTrustLevel::High,
        debounce_seconds: Some(60),
        dry_run: false,
        reason: Some("db repository trigger route test".to_string()),
        candidate_loop_ids: vec![mem_loops::LOOP_CONTEXT_PACK_REFRESH.to_string()],
    };

    let response =
        mem_service::repository::handlers::loops::route_loop_trigger_event(&pool, &request)
            .await
            .expect("route trigger");

    assert!(!response.duplicate);
    assert!(!response.debounced);
    assert_eq!(response.runs.len(), 1);
    assert_eq!(response.decisions.len(), 1);
    assert!(response.decisions[0].supported);
    assert!(response.decisions[0].eligible);
    assert_eq!(response.decisions[0].run_id, Some(response.runs[0].id));
    assert_eq!(response.runs[0].status, LoopRunStatus::Succeeded);
    let loaded_run =
        mem_service::repository::handlers::loops::read_loop_run_detail(&pool, response.runs[0].id)
            .await
            .expect("read routed loop run");
    assert!(
        loaded_run.memory_proposals.len() >= 4,
        "context_pack_refresh should emit pending memory proposals"
    );
    assert!(
        loaded_run
            .memory_proposals
            .iter()
            .all(|proposal| proposal.status == "pending")
    );
    assert!(
        loaded_run
            .traces
            .iter()
            .any(|trace| trace.trace_type == "context_refresh")
    );

    let duplicate =
        mem_service::repository::handlers::loops::route_loop_trigger_event(&pool, &request)
            .await
            .expect("dedupe trigger");
    assert!(duplicate.duplicate);
    assert!(duplicate.runs.is_empty());
    assert!(
        duplicate.decisions[0]
            .skipped_reasons
            .contains(&"duplicate_trigger".to_string())
    );

    cleanup_loop_run(&pool, response.runs[0].id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_rejects_approval_and_blocks_run_safely() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-approval");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");

    let request = LoopRunRequest {
        project: Some(project.clone()),
        repo_root: Some(repo_root.clone()),
        scope_type: None,
        scope_id: None,
        dry_run: true,
        reason: Some("approval rejection test".to_string()),
        trigger_payload: None,
    };
    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_CONTEXT_PACK_REFRESH,
        &request,
    )
    .await
    .expect("create loop run")
    .run;
    sqlx::query("UPDATE loop_runs SET status = 'running', finished_at = NULL WHERE id = $1")
        .bind(run.summary.id)
        .execute(&pool)
        .await
        .expect("mark run active");

    let proposal_id = insert_memory_proposal_fixture(&pool, project_id, run.summary.id).await;
    let approval_id = insert_approval_fixture(&pool, project_id, run.summary.id, proposal_id).await;

    let response = mem_service::repository::handlers::loops::record_loop_approval_decision(
        &pool,
        approval_id,
        LoopApprovalStatus::Rejected,
        &LoopApprovalDecisionRequest {
            reviewer: Some("repository-test".to_string()),
            reason: Some("Reject unsafe durable memory mutation.".to_string()),
            edited_action: None,
        },
    )
    .await
    .expect("reject approval");

    assert_eq!(response.approval.status, LoopApprovalStatus::Rejected);
    assert_eq!(
        response.approval.decision_reason.as_deref(),
        Some("Reject unsafe durable memory mutation.")
    );

    let loaded =
        mem_service::repository::handlers::loops::read_loop_run_detail(&pool, run.summary.id)
            .await
            .expect("read loop run");
    assert_eq!(loaded.summary.status, LoopRunStatus::Blocked);
    assert!(
        loaded
            .summary
            .blocked_reasons
            .contains(&"approval_rejected".to_string())
    );
    assert!(loaded.traces.iter().any(|trace| {
        trace.trace_type == "approval" && trace.title == "Approval rejected" && !trace.redacted
    }));
    assert_eq!(loaded.memory_proposals.len(), 1);
    assert_eq!(loaded.memory_proposals[0].status, "rejected");

    cleanup_approval(&pool, approval_id).await;
    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_approves_memory_proposal_and_writes_provenance() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-proposal");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    let request = LoopMemoryProposalCreateRequest {
        project: project.clone(),
        loop_id: mem_loops::LOOP_CONTEXT_PACK_REFRESH.to_string(),
        proposal_type: "add".to_string(),
        run_id: None,
        target_memory_id: None,
        candidate: serde_json::json!({
            "canonical_text": "Loop memory proposals store reviewed durable memory text.",
            "summary": "Loop proposal approval writes memory",
            "memory_type": "implementation",
            "tags": ["loop-engineering", "proposal"]
        }),
        evidence: serde_json::json!([
            {
                "source_kind": "file",
                "file_path": "crates/mem-service/src/repository/handlers/loops.rs",
                "excerpt": "accepted proposals write memory provenance"
            }
        ]),
        confidence: 0.91,
        risk_notes: Some("repository test approval".to_string()),
    };

    let created =
        mem_service::repository::handlers::loops::create_memory_proposal_record(&pool, &request)
            .await
            .expect("create memory proposal");
    assert_eq!(created.proposal.status, "pending");

    let approved = mem_service::repository::handlers::loops::record_loop_memory_proposal_decision(
        &pool,
        &mem_api::ProceduralConfig::default(),
        created.proposal.id,
        "approved",
        &LoopMemoryProposalDecisionRequest {
            reviewer: Some("repository-test".to_string()),
            reason: Some("Approve test proposal.".to_string()),
            edited_candidate: None,
            edited_evidence: None,
            edited_risk_notes: None,
        },
    )
    .await
    .expect("approve memory proposal");

    assert_eq!(approved.proposal.status, "approved");
    let memory_id = approved.memory_id.expect("approved proposal wrote memory");
    let memory = mem_service::repository::handlers::memory::fetch_memory_entry(&pool, memory_id)
        .await
        .expect("fetch approved proposal memory")
        .expect("memory exists");
    assert_eq!(memory.project, project);
    assert_eq!(memory.summary, "Loop proposal approval writes memory");
    assert_eq!(
        memory.canonical_text,
        "Loop memory proposals store reviewed durable memory text."
    );
    assert_eq!(memory.sources.len(), 2);
    assert!(
        memory
            .sources
            .iter()
            .any(|source| source.file_path.as_deref()
                == Some("crates/mem-service/src/repository/handlers/loops.rs"))
    );
    assert!(memory.tags.iter().any(|tag| tag == "loop-engineering"));

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_applies_consolidate_proposal_atomically() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let project = mem_test_support::unique_project_slug("service-consolidate");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");

    // Three member memories the insight will summarize.
    let mut members = Vec::new();
    for i in 0..3 {
        members.push(
            insert_hygiene_memory_fixture(
                &pool,
                project_id,
                &format!("Member fact {i}"),
                &format!("Member canonical text {i}"),
                0.9,
                3,
            )
            .await,
        );
    }
    let member_ids: Vec<String> = members.iter().map(|id| id.to_string()).collect();

    let request = LoopMemoryProposalCreateRequest {
        project: project.clone(),
        loop_id: mem_loops::LOOP_MEMORY_CONSOLIDATION.to_string(),
        proposal_type: "consolidate".to_string(),
        run_id: None,
        target_memory_id: None,
        candidate: serde_json::json!({
            "canonical_text": "These three facts form one subsystem convention.",
            "summary": "Subsystem convention insight",
            "memory_type": "insight",
            "scope": "project",
            "importance": 4,
            "confidence": 0.7,
            "tags": ["insight", "consolidation"],
            "member_canonical_ids": member_ids,
            "theme": "subsystem convention"
        }),
        evidence: serde_json::json!(
            members
                .iter()
                .map(|id| serde_json::json!({ "source_kind": "memory", "excerpt": id.to_string() }))
                .collect::<Vec<_>>()
        ),
        confidence: 0.7,
        risk_notes: Some("consolidation test".to_string()),
    };

    let created =
        mem_service::repository::handlers::loops::create_memory_proposal_record(&pool, &request)
            .await
            .expect("create consolidate proposal");
    let approved = mem_service::repository::handlers::loops::record_loop_memory_proposal_decision(
        &pool,
        &mem_api::ProceduralConfig::default(),
        created.proposal.id,
        "approved",
        &LoopMemoryProposalDecisionRequest {
            reviewer: Some("repository-test".to_string()),
            reason: Some("Approve consolidation.".to_string()),
            edited_candidate: None,
            edited_evidence: None,
            edited_risk_notes: None,
        },
    )
    .await
    .expect("approve consolidate proposal");

    let meta_id = approved.memory_id.expect("consolidate wrote a memory");
    let meta = mem_service::repository::handlers::memory::fetch_memory_entry(&pool, meta_id)
        .await
        .expect("fetch insight memory")
        .expect("insight exists");
    assert_eq!(meta.memory_type, mem_api::MemoryType::Insight);

    // Exactly three summarizes relations to the members' latest version ids.
    let relation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_relations WHERE src_memory_id = $1 AND relation_type = 'summarizes'",
    )
    .bind(meta_id)
    .fetch_one(&pool)
    .await
    .expect("count relations");
    assert_eq!(relation_count, 3);
    for member in &members {
        let linked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_relations WHERE src_memory_id = $1 AND dst_memory_id = $2 AND relation_type = 'summarizes'",
        )
        .bind(meta_id)
        .bind(member)
        .fetch_one(&pool)
        .await
        .expect("count member link");
        assert_eq!(linked, 1, "member {member} must be linked");
    }

    // Member provenance recorded with source_kind = 'memory'.
    let memory_sources: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_sources WHERE memory_entry_id = $1 AND source_kind = 'memory'",
    )
    .bind(meta_id)
    .fetch_one(&pool)
    .await
    .expect("count memory provenance");
    assert_eq!(memory_sources, 3);

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn procedural_utility_learns_from_proposal_decisions() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };
    let project = mem_test_support::unique_project_slug("service-utility");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");
    let procedural = mem_api::ProceduralConfig::default();

    let make_request = |summary: &str| LoopMemoryProposalCreateRequest {
        project: project.clone(),
        loop_id: mem_loops::LOOP_MEMORY_HYGIENE.to_string(),
        proposal_type: "add".to_string(),
        run_id: None,
        target_memory_id: None,
        candidate: serde_json::json!({
            "canonical_text": format!("{summary} canonical text"),
            "summary": summary,
            "memory_type": "implementation",
        }),
        evidence: serde_json::json!([]),
        confidence: 0.8,
        risk_notes: None,
    };
    let decision = |reason: &str| LoopMemoryProposalDecisionRequest {
        reviewer: Some("utility-test".to_string()),
        reason: Some(reason.to_string()),
        edited_candidate: None,
        edited_evidence: None,
        edited_risk_notes: None,
    };
    let fetch_utility = |pool: PgPool, project: String| async move {
        sqlx::query_as::<_, (f64, i64)>(
            r#"
            SELECT pu.utility, pu.update_count
            FROM procedural_utility pu
            JOIN projects p ON p.id = pu.project_id
            WHERE p.slug = $1 AND pu.producer_kind = 'loop' AND pu.producer_id = $2
            "#,
        )
        .bind(&project)
        .bind(mem_loops::LOOP_MEMORY_HYGIENE)
        .fetch_optional(&pool)
        .await
        .expect("fetch utility row")
    };

    // Approve raises utility from zero.
    let created = mem_service::repository::handlers::loops::create_memory_proposal_record(
        &pool,
        &make_request("Utility approve fixture"),
    )
    .await
    .expect("create proposal");
    let approved = mem_service::repository::handlers::loops::record_loop_memory_proposal_decision(
        &pool,
        &procedural,
        created.proposal.id,
        "approved",
        &decision("approve"),
    )
    .await
    .expect("approve proposal");
    let (after_approve, count) = fetch_utility(pool.clone(), project.clone())
        .await
        .expect("utility row after approve");
    assert!(after_approve > 0.0, "approve must raise utility");
    assert_eq!(count, 1);

    // The approved memory carries a durable producer link, so a later
    // citation can reward the loop that created it.
    let memory_id = approved.memory_id.expect("approve wrote memory");
    let producers =
        mem_reinforce::repository::loop_producers_for_memories(&pool, &[memory_id])
            .await
            .expect("resolve loop producers");
    assert_eq!(producers.len(), 1);
    assert_eq!(producers[0].loop_id, mem_loops::LOOP_MEMORY_HYGIENE);

    // Reject lowers it, atomically with the status write.
    let created = mem_service::repository::handlers::loops::create_memory_proposal_record(
        &pool,
        &make_request("Utility reject fixture"),
    )
    .await
    .expect("create reject proposal");
    mem_service::repository::handlers::loops::record_loop_memory_proposal_decision(
        &pool,
        &procedural,
        created.proposal.id,
        "rejected",
        &decision("reject"),
    )
    .await
    .expect("reject proposal");
    let (after_reject, count) = fetch_utility(pool.clone(), project.clone())
        .await
        .expect("utility row after reject");
    assert!(after_reject < after_approve, "reject must lower utility");
    assert_eq!(count, 2);

    // Re-rejecting an already-resolved proposal must not double-penalize.
    mem_service::repository::handlers::loops::record_loop_memory_proposal_decision(
        &pool,
        &procedural,
        created.proposal.id,
        "rejected",
        &decision("reject again"),
    )
    .await
    .expect("re-reject proposal");
    let (after_second_reject, count) = fetch_utility(pool.clone(), project.clone())
        .await
        .expect("utility row after re-reject");
    assert_eq!(after_second_reject, after_reject);
    assert_eq!(count, 2);

    // Edited-then-approved earns the partial reward, exactly once.
    let created = mem_service::repository::handlers::loops::create_memory_proposal_record(
        &pool,
        &make_request("Utility edited fixture"),
    )
    .await
    .expect("create edited proposal");
    mem_service::repository::handlers::loops::record_loop_memory_proposal_edit(
        &pool,
        created.proposal.id,
        &LoopMemoryProposalDecisionRequest {
            reviewer: Some("utility-test".to_string()),
            reason: Some("tweak wording".to_string()),
            edited_candidate: Some(serde_json::json!({
                "canonical_text": "Edited canonical text",
                "summary": "Utility edited fixture",
                "memory_type": "implementation",
            })),
            edited_evidence: None,
            edited_risk_notes: None,
        },
    )
    .await
    .expect("edit proposal");
    let (after_edit, count_after_edit) = fetch_utility(pool.clone(), project.clone())
        .await
        .expect("utility row after edit");
    assert_eq!(
        (after_edit, count_after_edit),
        (after_second_reject, 2),
        "editing alone must not emit a reward"
    );
    mem_service::repository::handlers::loops::record_loop_memory_proposal_decision(
        &pool,
        &procedural,
        created.proposal.id,
        "approved",
        &decision("approve edited"),
    )
    .await
    .expect("approve edited proposal");
    let (_, count) = fetch_utility(pool.clone(), project.clone())
        .await
        .expect("utility row after edited approve");
    assert_eq!(count, 3);
    let edited_reason: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM procedural_utility_audit pa
        JOIN projects p ON p.id = pa.project_id
        WHERE p.slug = $1 AND pa.reason = 'proposal_edited_approved'
        "#,
    )
    .bind(&project)
    .fetch_one(&pool)
    .await
    .expect("count edited-approved audits");
    assert_eq!(edited_reason, 1);

    // Every decision left exactly one audit row.
    let audit_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM procedural_utility_audit pa
        JOIN projects p ON p.id = pa.project_id
        WHERE p.slug = $1
        "#,
    )
    .bind(&project)
    .fetch_one(&pool)
    .await
    .expect("count audit rows");
    assert_eq!(audit_count, 3);

    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_memory_hygiene_emits_cleanup_proposals() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-hygiene");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    enable_loop_for_project(&pool, project_id, &project, mem_loops::LOOP_MEMORY_HYGIENE).await;
    insert_hygiene_memory_fixture(
        &pool,
        project_id,
        "Duplicate hygiene memory",
        "Duplicate hygiene memory",
        0.92,
        4,
    )
    .await;
    insert_hygiene_memory_fixture(
        &pool,
        project_id,
        "Duplicate hygiene memory",
        "Duplicate hygiene memory",
        0.71,
        2,
    )
    .await;
    insert_hygiene_memory_fixture(
        &pool,
        project_id,
        "Low confidence hygiene memory",
        "Low confidence hygiene memory",
        0.31,
        1,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_MEMORY_HYGIENE,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("memory hygiene repository test".to_string()),
            trigger_payload: None,
        },
    )
    .await
    .expect("run memory hygiene loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Succeeded);
    assert!(
        run.memory_proposals
            .iter()
            .any(|proposal| proposal.proposal_type == "merge")
    );
    assert!(
        run.memory_proposals
            .iter()
            .any(|proposal| proposal.proposal_type == "deprecate")
    );
    assert!(
        run.traces
            .iter()
            .any(|trace| trace.trace_type == "memory_hygiene")
    );

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_ci_failure_triage_reports_and_proposes_follow_up() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-ci");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    enable_loop_for_project(
        &pool,
        project_id,
        &project,
        mem_loops::LOOP_CI_FAILURE_TRIAGE,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_CI_FAILURE_TRIAGE,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("ci triage repository test".to_string()),
            trigger_payload: Some(serde_json::json!({
                "workflow": "CI",
                "job": "cargo test",
                "log": "test failed: assertion expected true but got false"
            })),
        },
    )
    .await
    .expect("run ci triage loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Succeeded);
    assert!(
        run.traces
            .iter()
            .any(|trace| trace.trace_type == "ci_triage")
    );
    assert!(run.memory_proposals.iter().any(|proposal| {
        proposal.proposal_type == "add"
            && proposal
                .candidate
                .get("memory_type")
                .and_then(serde_json::Value::as_str)
                == Some("task")
    }));
    assert_eq!(run.output["ci_triage"]["code_written"], false);

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_agent_ready_issue_triage_creates_task_pack() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-issue");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    enable_loop_for_project(
        &pool,
        project_id,
        &project,
        mem_loops::LOOP_AGENT_READY_ISSUE_TRIAGE,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_AGENT_READY_ISSUE_TRIAGE,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("issue triage repository test".to_string()),
            trigger_payload: Some(serde_json::json!({
                "identifier": "MEM-1",
                "title": "Improve CLI command help text",
                "description": "The CLI help should explain expected output. Acceptance: update command docs and add a focused parser/help test."
            })),
        },
    )
    .await
    .expect("run issue triage loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Succeeded);
    assert!(
        run.traces
            .iter()
            .any(|trace| trace.trace_type == "issue_triage")
    );
    assert_eq!(
        run.output["issue_triage"]["suggested_labels"][0],
        "agent-ready"
    );
    assert!(run.memory_proposals.iter().any(|proposal| {
        proposal
            .candidate
            .get("summary")
            .and_then(serde_json::Value::as_str)
            == Some("Agent-ready issue task pack")
    }));

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_draft_pr_blocks_until_issue_is_approved() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-draft-gate");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    enable_loop_for_project_mode(
        &pool,
        project_id,
        &project,
        mem_loops::LOOP_DRAFT_PR,
        LoopMode::DraftOutput,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_DRAFT_PR,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("draft pr gate repository test".to_string()),
            trigger_payload: Some(serde_json::json!({
                "title": "Improve CLI help",
                "labels": ["agent-ready"]
            })),
        },
    )
    .await
    .expect("run draft pr loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Blocked);
    assert!(
        run.summary
            .blocked_reasons
            .contains(&"missing_explicit_issue_approval".to_string())
    );
    let pending_approvals: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM approval_requests WHERE run_id = $1 AND status = 'pending' AND action_type = 'write_repo'",
    )
    .bind(run.summary.id)
    .fetch_one(&pool)
    .await
    .expect("count pending approvals");
    assert_eq!(pending_approvals, 1);
    assert!(
        run.output["draft_pr"]["gate"]["approval_id"]
            .as_str()
            .is_some()
    );

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_draft_pr_creates_isolated_workspace_for_approved_issue() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-draft");
    let repo_root = init_git_repo("draft-pr");
    let repo_root_string = repo_root.display().to_string();
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    enable_loop_for_project_mode(
        &pool,
        project_id,
        &project,
        mem_loops::LOOP_DRAFT_PR,
        LoopMode::DraftOutput,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_DRAFT_PR,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root_string.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("draft pr approved repository test".to_string()),
            trigger_payload: Some(serde_json::json!({
                "title": "Improve CLI help",
                "description": "Acceptance: update help text.",
                "labels": ["agent-ready"],
                "approved": true,
                "run_checks": true,
                "allowed_commands": ["sh"],
                "checks": [{"program": "sh", "args": ["-c", "true"]}]
            })),
        },
    )
    .await
    .expect("run draft pr loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Succeeded);
    assert!(
        run.traces
            .iter()
            .any(|trace| trace.trace_type == "draft_pr")
    );
    assert_eq!(run.output["draft_pr"]["draft_pr"]["auto_merge"], false);
    assert_eq!(
        run.output["draft_pr"]["draft_pr"]["mode"].as_str(),
        Some("draft_only")
    );
    assert!(
        run.output["draft_pr"]["draft_pr"]["branch"]
            .as_str()
            .is_some_and(|branch| branch.starts_with("memory/loops/"))
    );
    assert_eq!(
        run.output["draft_pr"]["checks"][0]["status"].as_str(),
        Some("passed")
    );
    let worktree_path = run.output["draft_pr"]["draft_pr"]["worktree_path"]
        .as_str()
        .expect("worktree path");
    assert!(Path::new(worktree_path).exists());

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root_string).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
    let _ = fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn loop_repository_reviewer_drift_reports_findings_and_memory_proposal() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-reviewer");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    insert_typed_memory_fixture(
        &pool,
        project_id,
        "Service routes define the public API boundary.",
        "Route changes should be reviewed against API and architecture docs.",
        "architecture",
    )
    .await;
    enable_loop_for_project_mode(
        &pool,
        project_id,
        &project,
        mem_loops::LOOP_REVIEWER_DRIFT_DETECTION,
        LoopMode::SuggestOnly,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_REVIEWER_DRIFT_DETECTION,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("reviewer drift repository test".to_string()),
            trigger_payload: Some(serde_json::json!({
                "changed_files": ["crates/mem-service/src/routes.rs", "README.md"],
                "expected_paths": ["crates/mem-cli"],
                "diff": "Architecture public API change with TODO and no tests",
                "architecture_changed": true
            })),
        },
    )
    .await
    .expect("run reviewer drift loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Succeeded);
    assert!(
        run.traces
            .iter()
            .any(|trace| trace.trace_type == "reviewer_drift")
    );
    let findings = run.output["reviewer_drift"]["findings"]
        .as_array()
        .expect("findings");
    assert!(
        findings
            .iter()
            .any(|finding| finding["kind"] == "unrelated_changes")
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding["kind"] == "missing_tests")
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding["kind"] == "architecture_drift")
    );
    assert!(
        run.output["reviewer_drift"]["architecture_memory_proposal_id"]
            .as_str()
            .is_some()
    );
    assert!(run.memory_proposals.iter().any(|proposal| {
        proposal
            .candidate
            .get("summary")
            .and_then(serde_json::Value::as_str)
            == Some("Architecture drift update proposal")
    }));

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_skill_mining_creates_learned_skill_proposal() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-skill");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    enable_loop_for_project_mode(
        &pool,
        project_id,
        &project,
        mem_loops::LOOP_SKILL_MINING,
        LoopMode::SuggestOnly,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_SKILL_MINING,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("skill mining repository test".to_string()),
            trigger_payload: Some(serde_json::json!({
                "successful": true,
                "title": "Validate loop DB repository changes",
                "applicability": ["service repository loop handlers change"],
                "recipe": "Add a DB repository test, run focused cargo tests, then run web checks.",
                "commands": ["cargo test -p mem-service --test db_repository"],
                "validation_evidence": ["db_repository test passed"],
                "source_run": "3VI-619-test"
            })),
        },
    )
    .await
    .expect("run skill mining loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Succeeded);
    assert_eq!(run.output["skill_mining"]["suitable"], true);
    assert!(
        run.output["skill_mining"]["skill_proposal_id"]
            .as_str()
            .is_some()
    );
    assert!(run.memory_proposals.iter().any(|proposal| {
        proposal
            .candidate
            .get("tags")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|tags| tags.iter().any(|tag| tag == "learned-skill"))
    }));

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

#[tokio::test]
async fn loop_repository_memory_eval_reports_quality_metrics() {
    let Some(pool) = mem_test_support::migrated_pool().await else {
        return;
    };

    let project = mem_test_support::unique_project_slug("service-loop-eval");
    let repo_root = format!("/tmp/{project}");
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup old test project");

    mem_service::repository::handlers::loops::register_builtin_loop_definitions(&pool)
        .await
        .expect("register builtin loops");
    let project_id =
        mem_service::repository::handlers::bundle::upsert_project_slug(&pool, &project)
            .await
            .expect("upsert project");
    let expected_memory_id = insert_typed_memory_fixture(
        &pool,
        project_id,
        "Golden retrieval memory",
        "Memory eval should include this expected memory.",
        "reference",
    )
    .await;
    enable_loop_for_project_mode(
        &pool,
        project_id,
        &project,
        mem_loops::LOOP_MEMORY_EVAL,
        LoopMode::Observe,
    )
    .await;

    let run = mem_service::repository::handlers::loops::record_control_plane_loop_run(
        &pool,
        mem_loops::LOOP_MEMORY_EVAL,
        &LoopRunRequest {
            project: Some(project.clone()),
            repo_root: Some(repo_root.clone()),
            scope_type: None,
            scope_id: None,
            dry_run: true,
            reason: Some("memory eval repository test".to_string()),
            trigger_payload: Some(serde_json::json!({
                "golden_scenarios": [{
                    "id": "golden-reference",
                    "query": "expected memory",
                    "expected_memory_ids": [expected_memory_id.to_string()]
                }],
                "baseline": {"retriever": "previous"}
            })),
        },
    )
    .await
    .expect("run memory eval loop")
    .run;

    assert_eq!(run.summary.status, LoopRunStatus::Succeeded);
    assert!(
        run.traces
            .iter()
            .any(|trace| trace.trace_type == "memory_eval")
    );
    assert!(
        run.output["memory_eval"]["metrics"]["retrieval_recall_proxy"]
            .as_f64()
            .is_some_and(|value| value > 0.0)
    );
    assert_eq!(
        run.output["memory_eval"]["dashboard"]["kind"].as_str(),
        Some("internal_run_report")
    );
    assert_eq!(
        run.output["memory_eval"]["comparison"]["baseline"]["retriever"].as_str(),
        Some("previous")
    );

    cleanup_loop_run(&pool, run.summary.id).await;
    cleanup_loop_triggers(&pool, &repo_root).await;
    mem_test_support::cleanup_project(&pool, &project)
        .await
        .expect("cleanup test project");
}

async fn insert_memory_fixture(pool: &PgPool, project_id: Uuid) -> Uuid {
    let memory_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE,
                'Repository handler tests cover a write path and a read path.',
                'Repository DB test memory', 'implementation', 'project', 3, 0.9,
                'active', now(), now(), NULL,
                to_tsvector('english', 'Repository handler tests cover a write path and a read path. Repository DB test memory'))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .execute(pool)
    .await
    .expect("insert memory fixture");
    memory_id
}

async fn insert_graph_memory_source(pool: &PgPool, memory_id: Uuid) -> Uuid {
    let source_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_sources
            (id, memory_entry_id, source_kind, file_path, symbol_name, symbol_kind, created_at)
        VALUES ($1, $2, 'file', 'src/graph.rs', 'build_memory_graph', 'function', now())
        "#,
    )
    .bind(source_id)
    .bind(memory_id)
    .execute(pool)
    .await
    .expect("insert graph memory source");
    source_id
}

async fn insert_hygiene_memory_fixture(
    pool: &PgPool,
    project_id: Uuid,
    summary: &str,
    canonical_text: &str,
    confidence: f32,
    importance: i32,
) -> Uuid {
    let memory_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE, $3, $4, 'implementation', 'project',
                $5, $6, 'active', now(), now(), NULL,
                to_tsvector('english', $3 || ' ' || $4))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .bind(canonical_text)
    .bind(summary)
    .bind(importance)
    .bind(confidence)
    .execute(pool)
    .await
    .expect("insert hygiene memory fixture");
    sqlx::query("INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, 'hygiene-test')")
        .bind(memory_id)
        .execute(pool)
        .await
        .expect("insert hygiene tag");
    memory_id
}

async fn insert_typed_memory_fixture(
    pool: &PgPool,
    project_id: Uuid,
    summary: &str,
    canonical_text: &str,
    memory_type: &str,
) -> Uuid {
    let memory_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_entries
            (id, project_id, canonical_id, version_no, is_tombstone, canonical_text,
             summary, memory_type, scope, importance, confidence, status,
             created_at, updated_at, archived_at, search_document)
        VALUES ($1, $2, $1, 1, FALSE, $3, $4, $5, 'project',
                4, 0.92, 'active', now(), now(), NULL,
                to_tsvector('english', $3 || ' ' || $4))
        "#,
    )
    .bind(memory_id)
    .bind(project_id)
    .bind(canonical_text)
    .bind(summary)
    .bind(memory_type)
    .execute(pool)
    .await
    .expect("insert typed memory fixture");
    memory_id
}

async fn insert_memory_proposal_fixture(pool: &PgPool, project_id: Uuid, run_id: Uuid) -> Uuid {
    let proposal_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO memory_proposals (
            id, run_id, project_id, loop_id, proposal_type, candidate_json,
            evidence_json, confidence, risk_notes, status, created_at
        )
        VALUES (
            $1, $2, $3, $4, 'add',
            '{"summary": "Candidate memory from loop"}'::jsonb,
            '[{"source": "repository test"}]'::jsonb,
            0.9, 'requires review', 'pending', now()
        )
        "#,
    )
    .bind(proposal_id)
    .bind(run_id)
    .bind(project_id)
    .bind(mem_loops::LOOP_CONTEXT_PACK_REFRESH)
    .execute(pool)
    .await
    .expect("insert memory proposal fixture");
    proposal_id
}

async fn insert_approval_fixture(
    pool: &PgPool,
    project_id: Uuid,
    run_id: Uuid,
    proposal_id: Uuid,
) -> Uuid {
    let approval_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO approval_requests (
            id, run_id, project_id, loop_id, action_type, proposed_action_json,
            risk_reason, status, requester, created_at
        )
        VALUES (
            $1, $2, $3, $4, 'write_memory_proposal',
            jsonb_build_object('proposal_id', $5::uuid::text),
            'Durable memory write requires review.', 'pending', 'repository-test', now()
        )
        "#,
    )
    .bind(approval_id)
    .bind(run_id)
    .bind(project_id)
    .bind(mem_loops::LOOP_CONTEXT_PACK_REFRESH)
    .bind(proposal_id)
    .execute(pool)
    .await
    .expect("insert approval fixture");
    approval_id
}

async fn cleanup_loop_run(pool: &PgPool, run_id: Uuid) {
    sqlx::query("DELETE FROM loop_runs WHERE id = $1")
        .bind(run_id)
        .execute(pool)
        .await
        .expect("cleanup loop run");
}

async fn cleanup_approval(pool: &PgPool, approval_id: Uuid) {
    sqlx::query("DELETE FROM approval_requests WHERE id = $1")
        .bind(approval_id)
        .execute(pool)
        .await
        .expect("cleanup approval");
}

async fn enable_project_loop(pool: &PgPool, project_id: Uuid, project: &str) {
    enable_loop_for_project(
        pool,
        project_id,
        project,
        mem_loops::LOOP_CONTEXT_PACK_REFRESH,
    )
    .await;
}

async fn enable_loop_for_project(pool: &PgPool, project_id: Uuid, project: &str, loop_id: &str) {
    enable_loop_for_project_mode(pool, project_id, project, loop_id, LoopMode::SuggestOnly).await;
}

async fn enable_loop_for_project_mode(
    pool: &PgPool,
    project_id: Uuid,
    project: &str,
    loop_id: &str,
    mode: LoopMode,
) {
    sqlx::query(
        r#"
        INSERT INTO loop_settings (
            id, loop_id, scope_type, scope_id, project_id, enabled, mode, updated_at
        )
        VALUES (
            gen_random_uuid(), $1, 'project', $2, $3, TRUE, $4, now()
        )
        ON CONFLICT (loop_id, scope_type, scope_id) DO UPDATE SET
            project_id = EXCLUDED.project_id,
            enabled = EXCLUDED.enabled,
            mode = EXCLUDED.mode,
            updated_at = now()
        "#,
    )
    .bind(loop_id)
    .bind(project)
    .bind(project_id)
    .bind(mode.as_str())
    .execute(pool)
    .await
    .expect("enable project loop");
}

fn init_git_repo(name: &str) -> PathBuf {
    let repo = std::env::temp_dir().join(format!("mem-service-loop-{name}-{}", Uuid::new_v4()));
    fs::create_dir_all(&repo).expect("create temp repo");
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("README.md"), "initial\n").expect("write readme");
    git(&repo, &["add", "README.md"]);
    git(
        &repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "initial",
        ],
    );
    repo
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn cleanup_loop_triggers(pool: &PgPool, repo_root: &str) {
    sqlx::query("DELETE FROM trigger_events WHERE repo_root = $1")
        .bind(repo_root)
        .execute(pool)
        .await
        .expect("cleanup loop triggers");
}
