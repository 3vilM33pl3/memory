use anyhow::Result;
use mem_api::{
    ActivityListResponse, AgentWorkspaceFinishRequest, AgentWorkspaceHeartbeatRequest,
    AgentWorkspaceListResponse, AgentWorkspaceRecord, AgentWorkspaceStartRequest, AppConfig,
    ArchiveMemoryResponse, ArchiveRequest, ArchiveResponse, CaptureTaskRequest,
    CheckpointActivityRequest, CommitDetailResponse, CommitSyncRequest, CommitSyncResponse,
    CurateRequest, CurateResponse, DeleteMemoryRequest, DeleteMemoryResponse, GraphActivityRequest,
    LoopApprovalDecisionRequest, LoopApprovalDecisionResponse, LoopApprovalStatus,
    LoopApprovalsResponse, LoopCancelRequest, LoopContextPackResponse, LoopDefinitionResponse,
    LoopDefinitionsResponse, LoopFeedbackRequest, LoopGlobalStateResponse,
    LoopGlobalStateUpdateRequest, LoopMemoryProposalCreateRequest,
    LoopMemoryProposalDecisionRequest, LoopMemoryProposalDecisionResponse,
    LoopMemoryProposalsResponse, LoopRunRequest, LoopRunResponse, LoopRunStatus, LoopRunsResponse,
    LoopSettingsUpdateRequest, MemoryEntryResponse, PlanActivityRequest, ProjectCommitsResponse,
    ProjectMemoriesResponse, ProjectMemoryBundlePreview, ProjectMemoryExportOptions,
    ProjectMemoryImportPreview, ProjectMemoryImportResponse, ProjectOverviewResponse,
    ProvenanceVerificationRequest, ProvenanceVerificationResponse, PruneEmbeddingsRequest,
    PruneEmbeddingsResponse, QueryRequest, QueryResponse, ReembedRequest, ReembedResponse,
    ReindexRequest, ReindexResponse, ReplacementPolicy, ResumeRequest, ResumeResponse,
    ScanActivityRequest, UpToSpeedRequest, UpToSpeedResponse,
};
use reqwest::Client;
use uuid::Uuid;

use crate::commands::{
    memory_ops::SourceKindString,
    output::{service_url, write_headers},
};

#[derive(Clone)]
pub(crate) struct ApiClient {
    pub(crate) client: Client,
    pub(crate) config: AppConfig,
}

impl ApiClient {
    pub(crate) fn new(client: Client, config: AppConfig) -> Self {
        Self { client, config }
    }

    pub(crate) async fn health(&self) -> Result<serde_json::Value> {
        get_json(
            self.client
                .get(service_url(&self.config, "/healthz"))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_memories(&self, project: &str) -> Result<ProjectMemoriesResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/memories"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_overview(&self, project: &str) -> Result<ProjectOverviewResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/overview"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn replacement_proposals(
        &self,
        project: &str,
    ) -> Result<mem_api::ReplacementProposalListResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/replacement-proposals"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn memory_scores(
        &self,
        project: &str,
        needs_review: bool,
        limit: i64,
    ) -> Result<mem_api::MemoryScoresResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!(
                        "/v1/projects/{project}/memory-scores?needs_review={needs_review}&limit={limit}"
                    ),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn validate_memory(
        &self,
        memory_id: Uuid,
        dry_run: Option<bool>,
    ) -> Result<mem_api::ValidationRunInfo> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/memory/{memory_id}/validate"),
                ))
                .headers(write_headers(&self.config)?)
                .json(&mem_api::ValidateMemoryRequest { dry_run })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn validation_runs(
        &self,
        project: &str,
        pending_only: bool,
        limit: i64,
    ) -> Result<mem_api::ValidationRunsResponse> {
        let review = if pending_only { "pending" } else { "" };
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!(
                        "/v1/projects/{project}/validation-runs?review={review}&limit={limit}"
                    ),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn review_validation_run(
        &self,
        run_id: Uuid,
        action: &str,
    ) -> Result<mem_api::ReviewValidationResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/validation-runs/{run_id}/review"),
                ))
                .headers(write_headers(&self.config)?)
                .json(&mem_api::ReviewValidationRequest {
                    action: action.to_string(),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn approve_replacement_proposal(
        &self,
        project: &str,
        proposal_id: Uuid,
    ) -> Result<mem_api::ReplacementProposalResolutionResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/replacement-proposals/{proposal_id}/approve"),
                ))
                .headers(write_headers(&self.config)?)
                .json(&serde_json::json!({}))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reject_replacement_proposal(
        &self,
        project: &str,
        proposal_id: Uuid,
    ) -> Result<mem_api::ReplacementProposalResolutionResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/replacement-proposals/{proposal_id}/reject"),
                ))
                .headers(write_headers(&self.config)?)
                .json(&serde_json::json!({}))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn resume(&self, request: &ResumeRequest) -> Result<ResumeResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{}/resume", request.project),
                ))
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_activities(
        &self,
        project: &str,
        limit: usize,
        kind: Option<&str>,
    ) -> Result<ActivityListResponse> {
        let mut path = format!("/v1/projects/{project}/activities?limit={limit}");
        if let Some(kind) = kind {
            path.push_str("&kind=");
            path.push_str(kind);
        }
        get_json(
            self.client
                .get(service_url(&self.config, &path))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn up_to_speed(
        &self,
        request: &UpToSpeedRequest,
    ) -> Result<UpToSpeedResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{}/up-to-speed", request.project),
                ))
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_commits(
        &self,
        project: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ProjectCommitsResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/commits?limit={limit}&offset={offset}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn project_commit(
        &self,
        project: &str,
        commit: &str,
    ) -> Result<CommitDetailResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/commits/{commit}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn export_bundle_preview(
        &self,
        project: &str,
        options: &ProjectMemoryExportOptions,
    ) -> Result<ProjectMemoryBundlePreview> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/bundle/export/preview"),
                ))
                .headers(write_headers(&self.config)?)
                .json(options)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn export_bundle(
        &self,
        project: &str,
        options: &ProjectMemoryExportOptions,
    ) -> Result<Vec<u8>> {
        let response = self
            .client
            .post(service_url(
                &self.config,
                &format!("/v1/projects/{project}/bundle/export"),
            ))
            .headers(write_headers(&self.config)?)
            .json(options)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {}", String::from_utf8_lossy(&bytes));
        }
        Ok(bytes.to_vec())
    }

    pub(crate) async fn import_bundle_preview(
        &self,
        project: &str,
        bytes: Vec<u8>,
    ) -> Result<ProjectMemoryImportPreview> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/bundle/import/preview"),
                ))
                .headers(write_headers(&self.config)?)
                .header("content-type", "application/octet-stream")
                .body(bytes)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn import_bundle(
        &self,
        project: &str,
        bytes: Vec<u8>,
    ) -> Result<ProjectMemoryImportResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/projects/{project}/bundle/import"),
                ))
                .headers(write_headers(&self.config)?)
                .header("content-type", "application/octet-stream")
                .body(bytes)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn query(&self, request: &QueryRequest) -> Result<QueryResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/query"))
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn agent_workspaces(
        &self,
        project: &str,
        include_finished: bool,
    ) -> Result<AgentWorkspaceListResponse> {
        get_json(
            self.client
                .get(service_url(&self.config, "/v1/agents/workspaces"))
                .query(&[
                    ("project", project.to_string()),
                    ("include_finished", include_finished.to_string()),
                ])
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn start_agent_workspace(
        &self,
        request: &AgentWorkspaceStartRequest,
    ) -> Result<AgentWorkspaceRecord> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/agents/workspaces/start"))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn heartbeat_agent_workspace(
        &self,
        workspace_id: Uuid,
        request: &AgentWorkspaceHeartbeatRequest,
    ) -> Result<AgentWorkspaceRecord> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/agents/workspaces/{workspace_id}/heartbeat"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn finish_agent_workspace(
        &self,
        workspace_id: Uuid,
        request: &AgentWorkspaceFinishRequest,
    ) -> Result<AgentWorkspaceRecord> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/agents/workspaces/{workspace_id}/finish"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn verify_provenance(
        &self,
        request: &ProvenanceVerificationRequest,
    ) -> Result<ProvenanceVerificationResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/provenance/verify"))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn log_scan_activity(&self, request: &ScanActivityRequest) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/scan/activity"))
            .headers(write_headers(&self.config)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {body}");
        }
        Ok(())
    }

    pub(crate) async fn log_graph_activity(&self, request: &GraphActivityRequest) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/graph/activity"))
            .headers(write_headers(&self.config)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {body}");
        }
        Ok(())
    }

    pub(crate) async fn log_checkpoint_activity(
        &self,
        request: &CheckpointActivityRequest,
    ) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/checkpoint/activity"))
            .headers(write_headers(&self.config)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {body}");
        }
        Ok(())
    }

    pub(crate) async fn log_plan_activity(&self, request: &PlanActivityRequest) -> Result<()> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/plan/activity"))
            .headers(write_headers(&self.config)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("{status} {body}");
        }
        Ok(())
    }

    pub(crate) async fn memory_detail(&self, memory_id: &str) -> Result<MemoryEntryResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/memory/{memory_id}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn list_embedding_backends(
        &self,
        project: Option<&str>,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        let mut request = self
            .client
            .get(service_url(&self.config, "/v1/embeddings/backends"));
        if let Some(slug) = project {
            request = request.query(&[("project", slug)]);
        }
        get_json(request.send().await?).await
    }

    pub(crate) async fn activate_embedding_backend(
        &self,
        name: &str,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/embeddings/activate"))
                .headers(write_headers(&self.config)?)
                .json(&mem_api::ActivateEmbeddingBackendRequest {
                    name: name.to_string(),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn deactivate_embedding_backend(
        &self,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/embeddings/deactivate"))
            .headers(write_headers(&self.config)?)
            .json(&mem_api::DeactivateEmbeddingBackendRequest::default())
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            anyhow::bail!(
                "service does not support turning embeddings off yet; restart or upgrade memory-service so /v1/embeddings/deactivate is available"
            );
        }
        get_json(response).await
    }

    pub(crate) async fn set_embedding_creation_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> Result<mem_api::EmbeddingBackendsResponse> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/embeddings/create-enabled"))
            .headers(write_headers(&self.config)?)
            .json(&mem_api::SetEmbeddingCreationRequest {
                name: name.to_string(),
                enabled,
            })
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            anyhow::bail!(
                "service does not support toggling automatic embedding creation yet; restart or upgrade memory-service so /v1/embeddings/create-enabled is available"
            );
        }
        get_json(response).await
    }

    pub(crate) async fn llm_audit_status(&self) -> Result<mem_api::LlmAuditStatusResponse> {
        get_json(
            self.client
                .get(service_url(&self.config, "/v1/config/llm-audit"))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn set_llm_audit_enabled(
        &self,
        enabled: bool,
    ) -> Result<mem_api::LlmAuditStatusResponse> {
        let response = self
            .client
            .post(service_url(&self.config, "/v1/config/llm-audit"))
            .headers(write_headers(&self.config)?)
            .json(&mem_api::SetLlmAuditRequest { enabled })
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            anyhow::bail!(
                "service does not support toggling LLM audit yet; restart or upgrade memory-service so /v1/config/llm-audit is available"
            );
        }
        get_json(response).await
    }

    pub(crate) async fn memory_history(
        &self,
        memory_id: &str,
    ) -> Result<mem_api::MemoryHistoryResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/memory/{memory_id}/history"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn sync_commits(
        &self,
        request: &CommitSyncRequest,
    ) -> Result<CommitSyncResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/commits/sync"))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn capture_task(
        &self,
        request: &CaptureTaskRequest,
    ) -> Result<mem_api::CaptureTaskResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/capture/task"))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn curate(
        &self,
        project: &str,
        replacement_policy: ReplacementPolicy,
        dry_run: bool,
    ) -> Result<CurateResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/curate"))
                .headers(write_headers(&self.config)?)
                .json(&CurateRequest {
                    project: project.to_string(),
                    batch_size: None,
                    replacement_policy: Some(replacement_policy),
                    raw_capture_id: None,
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn curate_capture(
        &self,
        project: &str,
        raw_capture_id: Uuid,
        replacement_policy: ReplacementPolicy,
        dry_run: bool,
    ) -> Result<CurateResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/curate"))
                .headers(write_headers(&self.config)?)
                .json(&CurateRequest {
                    project: project.to_string(),
                    batch_size: Some(1),
                    raw_capture_id: Some(raw_capture_id),
                    replacement_policy: Some(replacement_policy),
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reindex(
        &self,
        project: &str,
        dry_run: bool,
        backend: Option<&str>,
    ) -> Result<ReindexResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reindex"))
                .headers(write_headers(&self.config)?)
                .json(&ReindexRequest {
                    project: project.to_string(),
                    dry_run,
                    backend: backend.map(str::to_string),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn reembed(
        &self,
        project: &str,
        dry_run: bool,
        backend: Option<&str>,
    ) -> Result<ReembedResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/reembed"))
                .headers(write_headers(&self.config)?)
                .json(&ReembedRequest {
                    project: project.to_string(),
                    dry_run,
                    backend: backend.map(str::to_string),
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn prune_embeddings(
        &self,
        project: &str,
        dry_run: bool,
    ) -> Result<PruneEmbeddingsResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/prune-embeddings"))
                .headers(write_headers(&self.config)?)
                .json(&PruneEmbeddingsRequest {
                    project: project.to_string(),
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn archive_low_value(
        &self,
        project: &str,
        dry_run: bool,
    ) -> Result<ArchiveResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/archive"))
                .headers(write_headers(&self.config)?)
                .json(&ArchiveRequest {
                    project: project.to_string(),
                    max_confidence: 0.3,
                    max_importance: 1,
                    dry_run,
                })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn archive_memory(&self, memory_id: Uuid) -> Result<ArchiveMemoryResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/memory/{memory_id}/archive"),
                ))
                .headers(write_headers(&self.config)?)
                .json(&serde_json::json!({}))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn delete_memory(&self, memory_id: Uuid) -> Result<DeleteMemoryResponse> {
        get_json(
            self.client
                .delete(service_url(&self.config, "/v1/memory"))
                .headers(write_headers(&self.config)?)
                .json(&DeleteMemoryRequest { memory_id })
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_definitions(&self) -> Result<LoopDefinitionsResponse> {
        get_json(
            self.client
                .get(service_url(&self.config, "/v1/loops"))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_definition(
        &self,
        loop_id: &str,
        project: Option<&str>,
        repo_root: Option<&str>,
    ) -> Result<LoopDefinitionResponse> {
        let mut request = self
            .client
            .get(service_url(&self.config, &format!("/v1/loops/{loop_id}")));
        let mut query = Vec::new();
        if let Some(project) = project {
            query.push(("project", project));
        }
        if let Some(repo_root) = repo_root {
            query.push(("repo_root", repo_root));
        }
        if !query.is_empty() {
            request = request.query(&query);
        }
        get_json(request.send().await?).await
    }

    pub(crate) async fn loop_enable(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<mem_api::LoopSettingResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/{loop_id}/enable"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_disable(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<mem_api::LoopSettingResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/{loop_id}/disable"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_pause(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<mem_api::LoopSettingResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/{loop_id}/pause"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_snooze(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<mem_api::LoopSettingResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/{loop_id}/snooze"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_run(
        &self,
        loop_id: &str,
        request: &LoopRunRequest,
    ) -> Result<LoopRunResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/{loop_id}/run"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_runs(
        &self,
        project: Option<&str>,
        loop_id: Option<&str>,
        status: Option<LoopRunStatus>,
        limit: i64,
    ) -> Result<LoopRunsResponse> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(project) = project {
            query.push(("project", project.to_string()));
        }
        if let Some(loop_id) = loop_id {
            query.push(("loop_id", loop_id.to_string()));
        }
        if let Some(status) = status {
            query.push(("status", status.as_str().to_string()));
        }
        get_json(
            self.client
                .get(service_url(&self.config, "/v1/loops/runs"))
                .query(&query)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_run_detail(&self, run_id: Uuid) -> Result<LoopRunResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/loops/runs/{run_id}"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_context_pack(
        &self,
        loop_id: &str,
        project: Option<&str>,
        repo_root: Option<&str>,
        run_id: Option<Uuid>,
        token_budget: usize,
        limit: usize,
    ) -> Result<LoopContextPackResponse> {
        let mut query = vec![
            ("token_budget", token_budget.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(project) = project {
            query.push(("project", project.to_string()));
        }
        if let Some(repo_root) = repo_root {
            query.push(("repo_root", repo_root.to_string()));
        }
        if let Some(run_id) = run_id {
            query.push(("run_id", run_id.to_string()));
        }
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/loops/{loop_id}/context-pack"),
                ))
                .query(&query)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_run_context_pack(
        &self,
        run_id: Uuid,
    ) -> Result<LoopContextPackResponse> {
        get_json(
            self.client
                .get(service_url(
                    &self.config,
                    &format!("/v1/loops/runs/{run_id}/context-pack"),
                ))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_cancel(
        &self,
        run_id: Uuid,
        request: &LoopCancelRequest,
    ) -> Result<LoopRunResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/runs/{run_id}/cancel"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_feedback(
        &self,
        run_id: Uuid,
        request: &LoopFeedbackRequest,
    ) -> Result<LoopRunResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/runs/{run_id}/feedback"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_approvals(
        &self,
        project: Option<&str>,
        run_id: Option<Uuid>,
        loop_id: Option<&str>,
        status: Option<LoopApprovalStatus>,
        limit: i64,
    ) -> Result<LoopApprovalsResponse> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(project) = project {
            query.push(("project", project.to_string()));
        }
        if let Some(run_id) = run_id {
            query.push(("run_id", run_id.to_string()));
        }
        if let Some(loop_id) = loop_id {
            query.push(("loop_id", loop_id.to_string()));
        }
        if let Some(status) = status {
            query.push(("status", status.as_str().to_string()));
        }
        get_json(
            self.client
                .get(service_url(&self.config, "/v1/loops/approvals"))
                .query(&query)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_approval_edit(
        &self,
        approval_id: Uuid,
        request: &LoopApprovalDecisionRequest,
    ) -> Result<LoopApprovalDecisionResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/approvals/{approval_id}/edit"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_approval_decision(
        &self,
        approval_id: Uuid,
        approved: bool,
        request: &LoopApprovalDecisionRequest,
    ) -> Result<LoopApprovalDecisionResponse> {
        let action = if approved { "approve" } else { "reject" };
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/approvals/{approval_id}/{action}"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_memory_proposals(
        &self,
        project: Option<&str>,
        run_id: Option<Uuid>,
        loop_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> Result<LoopMemoryProposalsResponse> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(project) = project {
            query.push(("project", project.to_string()));
        }
        if let Some(run_id) = run_id {
            query.push(("run_id", run_id.to_string()));
        }
        if let Some(loop_id) = loop_id {
            query.push(("loop_id", loop_id.to_string()));
        }
        if let Some(status) = status {
            query.push(("status", status.to_string()));
        }
        get_json(
            self.client
                .get(service_url(&self.config, "/v1/loops/memory-proposals"))
                .query(&query)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn create_loop_memory_proposal(
        &self,
        request: &LoopMemoryProposalCreateRequest,
    ) -> Result<LoopMemoryProposalDecisionResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/loops/memory-proposals"))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_memory_proposal_decision(
        &self,
        proposal_id: Uuid,
        action: &str,
        request: &LoopMemoryProposalDecisionRequest,
    ) -> Result<LoopMemoryProposalDecisionResponse> {
        get_json(
            self.client
                .post(service_url(
                    &self.config,
                    &format!("/v1/loops/memory-proposals/{proposal_id}/{action}"),
                ))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_global_state(&self) -> Result<LoopGlobalStateResponse> {
        get_json(
            self.client
                .get(service_url(&self.config, "/v1/loops/global-kill-switch"))
                .send()
                .await?,
        )
        .await
    }

    pub(crate) async fn loop_set_global_state(
        &self,
        request: &LoopGlobalStateUpdateRequest,
    ) -> Result<LoopGlobalStateResponse> {
        get_json(
            self.client
                .post(service_url(&self.config, "/v1/loops/global-kill-switch"))
                .headers(write_headers(&self.config)?)
                .json(request)
                .send()
                .await?,
        )
        .await
    }
}

pub(crate) async fn get_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{}", format_api_error(status, &body));
    }
    Ok(serde_json::from_str(&body)?)
}

pub(crate) async fn print_json_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{}", format_api_error(status, &body));
    }
    println!("{body}");
    Ok(())
}

pub(crate) fn format_api_error(status: reqwest::StatusCode, body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return format!("{status} {body}");
    };
    let mut parts = vec![format!(
        "{status} {}",
        value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(body)
    )];
    for (label, key) in [
        ("code", "code"),
        ("explanation", "explanation"),
        ("fix", "fix_hint"),
        ("doctor", "doctor_hint"),
        ("command", "command_hint"),
    ] {
        if let Some(text) = value.get(key).and_then(serde_json::Value::as_str) {
            parts.push(format!("{label}: {text}"));
        }
    }
    parts.join("\n")
}

pub(crate) fn print_embedding_backends(payload: &mem_api::EmbeddingBackendsResponse) {
    if payload.backends.is_empty() {
        println!("No embedding backends configured.");
        return;
    }
    let active = payload.active.as_deref();
    let name_width = payload
        .backends
        .iter()
        .map(|b| b.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let provider_width = payload
        .backends
        .iter()
        .map(|b| b.provider.len())
        .max()
        .unwrap_or(8)
        .max(8);
    println!(
        "  {:name_width$}  {:provider_width$}  CREATE  MODEL",
        "NAME",
        "PROVIDER",
        name_width = name_width,
        provider_width = provider_width
    );
    for backend in &payload.backends {
        let marker = if Some(backend.name.as_str()) == active {
            "*"
        } else if !backend.ready {
            "!"
        } else {
            " "
        };
        println!(
            "{marker} {:name_width$}  {:provider_width$}  {:7} {}",
            backend.name,
            backend.provider,
            if backend.create_enabled { "on" } else { "off" },
            backend.model,
            name_width = name_width,
            provider_width = provider_width
        );
    }
    println!();
    if let Some(name) = active {
        println!("Active: {name}");
    } else {
        println!("Active: (none) — run `memory embeddings activate <name>` to pick one.");
    }
    let not_ready: Vec<&str> = payload
        .backends
        .iter()
        .filter(|b| !b.ready)
        .map(|b| b.name.as_str())
        .collect();
    if !not_ready.is_empty() {
        println!(
            "Not ready ({} — missing API key or model): {}",
            not_ready.len(),
            not_ready.join(", ")
        );
    }
}

pub(crate) fn print_memory_history(payload: &mem_api::MemoryHistoryResponse) {
    println!(
        "Canonical {} in project {} — {} version(s)",
        payload.canonical_id,
        payload.project,
        payload.versions.len()
    );
    for version in &payload.versions {
        let marker = if version.is_tombstone {
            " [tombstone]"
        } else {
            ""
        };
        let status_label = match version.status {
            mem_api::MemoryStatus::Active => "active",
            mem_api::MemoryStatus::Archived => "archived",
        };
        println!(
            "\nv{} — {} ({}){}\n  id: {}\n  updated: {}",
            version.version_no,
            version.memory_type,
            status_label,
            marker,
            version.id,
            version.updated_at.to_rfc3339(),
        );
        if version.is_tombstone {
            println!("  (empty — memory was deleted at this point)");
        } else {
            println!("  summary: {}", version.summary);
            let preview: String = version.canonical_text.chars().take(240).collect();
            let ellipsis = if version.canonical_text.chars().count() > 240 {
                "..."
            } else {
                ""
            };
            println!("  text: {preview}{ellipsis}");
        }
    }
}

pub(crate) fn print_query_response(payload: QueryResponse) {
    println!("Answer:\n{}\n", payload.answer);
    println!(
        "Confidence: {:.2} | Evidence: {} | Method: {} | Citations: {}\n",
        payload.confidence,
        if payload.insufficient_evidence {
            "insufficient"
        } else {
            "sufficient"
        },
        payload.answer_generation.method,
        format_query_citations(&payload.answer_generation.cited_result_numbers)
    );
    if let Some(reason) = &payload.answer_generation.fallback_reason {
        println!("Fallback: {reason}\n");
    }
    if !payload.diagnostics.provenance_warnings.is_empty() {
        println!("Provenance warnings:");
        for warning in &payload.diagnostics.provenance_warnings {
            println!(
                "  - [{}] {}",
                diagnostic_severity_name(&warning.severity),
                warning.message
            );
            if let Some(fix_hint) = &warning.fix_hint {
                println!("    hint: {fix_hint}");
            }
        }
        println!();
    }
    println!(
        "Diagnostics: lexical {} ({} ms) | semantic {} ({} ms) | graph {} [{}] ({} ms) | merged {} | returned {} | rerank {} ms | total {} ms\n",
        payload.diagnostics.lexical_candidates,
        payload.diagnostics.lexical_duration_ms,
        payload.diagnostics.semantic_candidates,
        payload.diagnostics.semantic_duration_ms,
        payload.diagnostics.graph_candidates,
        payload.diagnostics.graph_status,
        payload.diagnostics.graph_duration_ms,
        payload.diagnostics.merged_candidates,
        payload.diagnostics.returned_results,
        payload.diagnostics.rerank_duration_ms,
        payload.diagnostics.total_duration_ms,
    );
    if !payload.answer_citations.is_empty() {
        println!("Cited memories:");
        for citation in &payload.answer_citations {
            println!(
                "{}. {} [{}] {}",
                citation.result_number, citation.summary, citation.memory_type, citation.snippet
            );
        }
        println!();
    }
    for (index, result) in payload.results.into_iter().enumerate() {
        println!(
            "{}. {} [{} / {}] score={:.2}",
            index + 1,
            result.summary,
            result.memory_type,
            result.match_kind,
            result.score
        );
        println!("  {}", result.snippet);
        println!(
            "  debug: chunk {:.2} | entry {:.2} | semantic {:.2} | relation {:.2} | graph {:.2}",
            result.debug.chunk_fts,
            result.debug.entry_fts,
            result.debug.semantic_similarity,
            result.debug.relation_boost,
            result.debug.graph_boost,
        );
        if !result.score_explanation.is_empty() {
            println!("  why: {}", result.score_explanation.join(" | "));
        }
        for connection in &result.graph_connections {
            let symbol = connection
                .symbol
                .as_deref()
                .map(|value| format!(" symbol={value}"))
                .unwrap_or_default();
            let edge = connection
                .edge_kind
                .as_deref()
                .map(|value| format!(" edge={value}"))
                .unwrap_or_default();
            let neighbor = connection
                .neighbor_symbol
                .as_deref()
                .map(|value| format!(" neighbor={value}"))
                .unwrap_or_default();
            println!(
                "  graph: {} {}{}{}{} boost={:.2}",
                connection.reason,
                connection.file_path,
                symbol,
                edge,
                neighbor,
                connection.score_boost
            );
        }
        if !result.tags.is_empty() {
            println!("  tags: {}", result.tags.join(", "));
        }
        for source in result.sources {
            let path = source.file_path.unwrap_or_else(|| "<no-file>".to_string());
            if let Some(provenance) = source.provenance {
                println!(
                    "  source: {} {} provenance={}",
                    path,
                    source.source_kind.source_kind_string(),
                    provenance.status.as_str()
                );
                if let Some(reason) = provenance.reason {
                    println!("    provenance reason: {reason}");
                }
            } else {
                println!(
                    "  source: {} {}",
                    path,
                    source.source_kind.source_kind_string()
                );
            }
        }
    }
}

pub(crate) fn print_provenance_verification_response(response: &ProvenanceVerificationResponse) {
    println!(
        "Provenance verification for `{}` at {}",
        response.project, response.repo_root
    );
    println!(
        "checked={} verified={} missing_file={} missing_symbol={} unverifiable={} stale={} stored={} dry_run={}",
        response.checked_count,
        response.verified_count,
        response.missing_file_count,
        response.missing_symbol_count,
        response.unverifiable_count,
        response.stale_count,
        response.stored_count,
        response.dry_run
    );
    if !response.warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &response.warnings {
            println!(
                "  - [{}] {}",
                diagnostic_severity_name(&warning.severity),
                warning.message
            );
            if let Some(fix_hint) = &warning.fix_hint {
                println!("    hint: {fix_hint}");
            }
        }
    }
    let problem_items: Vec<_> = response
        .items
        .iter()
        .filter(|item| item.status != mem_api::SourceProvenanceStatus::Verified)
        .take(25)
        .collect();
    if !problem_items.is_empty() {
        println!("\nNon-verified sources:");
        for item in problem_items {
            println!(
                "  - {} {} {}",
                item.status.as_str(),
                item.file_path.as_deref().unwrap_or("<no-file>"),
                item.memory_summary
            );
            if let Some(reason) = &item.reason {
                println!("    {reason}");
            }
        }
    }
}

pub(crate) fn diagnostic_severity_name(severity: &mem_api::DiagnosticSeverity) -> &'static str {
    match severity {
        mem_api::DiagnosticSeverity::Info => "info",
        mem_api::DiagnosticSeverity::Warning => "warning",
        mem_api::DiagnosticSeverity::Error => "error",
    }
}

pub(crate) fn format_query_citations(numbers: &[usize]) -> String {
    if numbers.is_empty() {
        "none".to_string()
    } else {
        numbers
            .iter()
            .map(|number| format!("[{number}]"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}
