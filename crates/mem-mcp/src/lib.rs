use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::{
    ActivityListResponse, AppConfig, GlobalQueryRequest, LoopApprovalDecisionRequest,
    LoopApprovalDecisionResponse, LoopApprovalStatus, LoopApprovalsResponse, LoopCancelRequest,
    LoopDefinitionResponse, LoopDefinitionsResponse, LoopFeedbackRequest, LoopGlobalStateResponse,
    LoopGlobalStateUpdateRequest, LoopMode, LoopRunRequest, LoopRunResponse, LoopRunStatus,
    LoopRunsResponse, LoopScopeType, LoopSettingResponse, LoopSettingsUpdateRequest,
    MemoryEntryResponse, MemoryHistoryResponse, MemoryType, ProjectMemoriesResponse,
    ProjectOverviewResponse, QueryAnswerMode, QueryFilters, QueryRequest, QueryResponse,
    ReplacementProposalListResponse, ResumeCheckpoint, ResumeRequest, ResumeResponse,
    UpToSpeedRequest, UpToSpeedResponse, read_repo_project_slug,
};
use reqwest::Client;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        AnnotateAble, CallToolRequestParams, CallToolResult, Content, GetPromptRequestParams,
        GetPromptResult, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
        ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument, PromptMessage,
        PromptMessageRole, RawResourceTemplate, ReadResourceRequestParams, ReadResourceResult,
        ResourceContents, ResourceTemplate, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

const TOOL_MEMORY_QUERY: &str = "memory_query";
const TOOL_MEMORY_SEARCH_ALL: &str = "memory_search_all";
const TOOL_MEMORY_RESUME: &str = "memory_resume";
const TOOL_MEMORY_UP_TO_SPEED: &str = "memory_up_to_speed";
const TOOL_MEMORY_OVERVIEW: &str = "memory_overview";
const TOOL_MEMORY_LIST_MEMORIES: &str = "memory_list_memories";
const TOOL_MEMORY_GET_MEMORY: &str = "memory_get_memory";
const TOOL_MEMORY_MEMORY_HISTORY: &str = "memory_memory_history";
const TOOL_MEMORY_LIST_ACTIVITIES: &str = "memory_list_activities";
const TOOL_MEMORY_LIST_REPLACEMENT_PROPOSALS: &str = "memory_list_replacement_proposals";
const TOOL_MEMORY_LOOP_LIST: &str = "memory_loop_list";
const TOOL_MEMORY_LOOP_GET: &str = "memory_loop_get";
const TOOL_MEMORY_LOOP_ENABLE: &str = "memory_loop_enable";
const TOOL_MEMORY_LOOP_DISABLE: &str = "memory_loop_disable";
const TOOL_MEMORY_LOOP_PAUSE: &str = "memory_loop_pause";
const TOOL_MEMORY_LOOP_SNOOZE: &str = "memory_loop_snooze";
const TOOL_MEMORY_LOOP_RUN: &str = "memory_loop_run";
const TOOL_MEMORY_LOOP_RUNS: &str = "memory_loop_runs";
const TOOL_MEMORY_LOOP_INSPECT: &str = "memory_loop_inspect";
const TOOL_MEMORY_LOOP_CANCEL: &str = "memory_loop_cancel";
const TOOL_MEMORY_LOOP_FEEDBACK: &str = "memory_loop_feedback";
const TOOL_MEMORY_LOOP_LIST_APPROVALS: &str = "memory_loop_list_approvals";
const TOOL_MEMORY_LOOP_APPROVE: &str = "memory_loop_approve";
const TOOL_MEMORY_LOOP_REJECT: &str = "memory_loop_reject";
const TOOL_MEMORY_LOOP_EDIT_APPROVAL: &str = "memory_loop_edit_approval";
const TOOL_MEMORY_LOOP_GLOBAL_STATE: &str = "memory_loop_global_state";
const TOOL_MEMORY_LOOP_SET_GLOBAL_KILL_SWITCH: &str = "memory_loop_set_global_kill_switch";

const PROMPT_GET_UP_TO_SPEED: &str = "memory_get_up_to_speed";
const PROMPT_ANSWER_WITH_CONTEXT: &str = "memory_answer_with_context";
const PROMPT_ROUTE_CROSS_PROJECT_TASK: &str = "memory_route_cross_project_task";

#[derive(Debug, Clone)]
pub struct MemoryMcpServer {
    client: MemoryApiClient,
    mode: ProjectResolutionMode,
}

#[derive(Debug, Clone)]
pub enum ProjectResolutionMode {
    Stdio {
        default_project: Option<String>,
        cwd_project: Option<String>,
    },
    Http,
}

impl MemoryMcpServer {
    pub fn new(client: MemoryApiClient, mode: ProjectResolutionMode) -> Self {
        Self { client, mode }
    }

    pub fn stdio(config: AppConfig, project: Option<String>, cwd: &Path) -> Self {
        Self::new(
            MemoryApiClient::new(config),
            ProjectResolutionMode::Stdio {
                default_project: project,
                cwd_project: discover_cwd_project(cwd),
            },
        )
    }

    pub fn http(config: AppConfig) -> Self {
        Self::new(MemoryApiClient::new(config), ProjectResolutionMode::Http)
    }

    fn resolve_project(&self, explicit: Option<&str>) -> Result<String, McpError> {
        if let Some(project) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
            return Ok(project.to_string());
        }
        match &self.mode {
            ProjectResolutionMode::Stdio {
                default_project,
                cwd_project,
            } => default_project
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or(cwd_project.as_deref())
                .map(ToOwned::to_owned)
                .ok_or_else(|| invalid_params("project is required when no stdio default project or initialized repo slug is available")),
            ProjectResolutionMode::Http => Err(invalid_params(
                "project is required for HTTP MCP tools because the service has no trustworthy current directory",
            )),
        }
    }

    pub fn tool_definitions() -> Vec<Tool> {
        tool_definitions()
    }

    pub fn resource_templates() -> Vec<ResourceTemplate> {
        resource_templates()
    }

    pub fn prompt_definitions() -> Vec<Prompt> {
        prompt_definitions()
    }
}

impl ServerHandler for MemoryMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_instructions(
            "Memory Layer MCP adapter. Memory tools are read-only. Loop tools expose the loop control plane over the service API and require explicit user approval for enabling loops.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tool_definitions()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let arguments = request.arguments.unwrap_or_default();
        match request.name.as_ref() {
            TOOL_MEMORY_QUERY => self.tool_query(arguments).await,
            TOOL_MEMORY_SEARCH_ALL => self.tool_search_all(arguments).await,
            TOOL_MEMORY_RESUME => self.tool_resume(arguments).await,
            TOOL_MEMORY_UP_TO_SPEED => self.tool_up_to_speed(arguments).await,
            TOOL_MEMORY_OVERVIEW => self.tool_overview(arguments).await,
            TOOL_MEMORY_LIST_MEMORIES => self.tool_list_memories(arguments).await,
            TOOL_MEMORY_GET_MEMORY => self.tool_get_memory(arguments).await,
            TOOL_MEMORY_MEMORY_HISTORY => self.tool_memory_history(arguments).await,
            TOOL_MEMORY_LIST_ACTIVITIES => self.tool_list_activities(arguments).await,
            TOOL_MEMORY_LIST_REPLACEMENT_PROPOSALS => {
                self.tool_list_replacement_proposals(arguments).await
            }
            TOOL_MEMORY_LOOP_LIST => self.tool_loop_list(arguments).await,
            TOOL_MEMORY_LOOP_GET => self.tool_loop_get(arguments).await,
            TOOL_MEMORY_LOOP_ENABLE => self.tool_loop_enable(arguments).await,
            TOOL_MEMORY_LOOP_DISABLE => self.tool_loop_disable(arguments).await,
            TOOL_MEMORY_LOOP_PAUSE => self.tool_loop_pause(arguments).await,
            TOOL_MEMORY_LOOP_SNOOZE => self.tool_loop_snooze(arguments).await,
            TOOL_MEMORY_LOOP_RUN => self.tool_loop_run(arguments).await,
            TOOL_MEMORY_LOOP_RUNS => self.tool_loop_runs(arguments).await,
            TOOL_MEMORY_LOOP_INSPECT => self.tool_loop_inspect(arguments).await,
            TOOL_MEMORY_LOOP_CANCEL => self.tool_loop_cancel(arguments).await,
            TOOL_MEMORY_LOOP_FEEDBACK => self.tool_loop_feedback(arguments).await,
            TOOL_MEMORY_LOOP_LIST_APPROVALS => self.tool_loop_list_approvals(arguments).await,
            TOOL_MEMORY_LOOP_APPROVE => self.tool_loop_approve(arguments).await,
            TOOL_MEMORY_LOOP_REJECT => self.tool_loop_reject(arguments).await,
            TOOL_MEMORY_LOOP_EDIT_APPROVAL => self.tool_loop_edit_approval(arguments).await,
            TOOL_MEMORY_LOOP_GLOBAL_STATE => self.tool_loop_global_state(arguments).await,
            TOOL_MEMORY_LOOP_SET_GLOBAL_KILL_SWITCH => {
                self.tool_loop_set_global_kill_switch(arguments).await
            }
            other => Err(McpError::new(
                rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                format!("unknown Memory MCP tool: {other}"),
                None,
            )),
        }
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(
            resource_templates(),
        ))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(Vec::new()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let value = match parse_resource_uri(&request.uri)? {
            MemoryResource::ProjectOverview { project } => {
                json!(
                    self.client
                        .project_overview(&project)
                        .await
                        .map_err(api_error)?
                )
            }
            MemoryResource::ProjectMemories { project } => {
                json!(
                    self.client
                        .project_memories(&project, None, 100, 0)
                        .await
                        .map_err(api_error)?
                )
            }
            MemoryResource::ProjectActivities { project } => {
                json!(
                    self.client
                        .project_activities(&project, 100, None, None, None, true)
                        .await
                        .map_err(api_error)?
                )
            }
            MemoryResource::Memory { memory_id } => {
                json!(self.client.memory(&memory_id).await.map_err(api_error)?)
            }
            MemoryResource::MemoryHistory { memory_id } => {
                json!(
                    self.client
                        .memory_history(&memory_id)
                        .await
                        .map_err(api_error)?
                )
            }
        };
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(pretty_json(&value)?, request.uri)
                .with_mime_type("application/json"),
        ]))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(prompt_definitions()))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let args = request.arguments.unwrap_or_default();
        let message = match request.name.as_str() {
            PROMPT_GET_UP_TO_SPEED => {
                let project = args
                    .get("project")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_params("prompt argument `project` is required"))?;
                format!(
                    "Use Memory Layer MCP to get up to speed on project `{project}`. First call `{TOOL_MEMORY_UP_TO_SPEED}` with project `{project}`, then use the briefing, blockers, next actions, and useful memories to orient your work."
                )
            }
            PROMPT_ANSWER_WITH_CONTEXT => {
                let project = args
                    .get("project")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_params("prompt argument `project` is required"))?;
                let question = args
                    .get("question")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_params("prompt argument `question` is required"))?;
                format!(
                    "Answer this project-memory question using Memory Layer context. Call `{TOOL_MEMORY_QUERY}` with project `{project}` and question `{question}`, then answer from the returned answer text, citations, summaries, match kinds, and diagnostics. Say when evidence is insufficient."
                )
            }
            PROMPT_ROUTE_CROSS_PROJECT_TASK => {
                let task = args
                    .get("task")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_params("prompt argument `task` is required"))?;
                format!(
                    "Route this cross-project task using Memory Layer: {task}\n\nCall `{TOOL_MEMORY_SEARCH_ALL}` with the task as the question. Compare returned citations and result summaries, then choose the repository from the strongest result's project and repo_root metadata. Do not take repository-specific follow-up actions until the selected result clearly identifies a repo_root; if evidence is ambiguous, ask the user to choose."
                )
            }
            other => {
                return Err(McpError::new(
                    rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                    format!("unknown Memory MCP prompt: {other}"),
                    None,
                ));
            }
        };
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            message,
        )]))
    }
}

impl MemoryMcpServer {
    async fn tool_query(&self, arguments: Map<String, Value>) -> Result<CallToolResult, McpError> {
        let args: MemoryQueryArgs = from_args(arguments)?;
        let project = self.resolve_project(args.project.as_deref())?;
        let request = QueryRequest {
            project,
            query: args.question,
            filters: QueryFilters::default(),
            top_k: args.top_k.unwrap_or(8),
            min_confidence: args.min_confidence,
            include_stale: args.include_stale.unwrap_or(false),
            history: args.history.unwrap_or(false),
            retrieval_mode: None,
            answer_mode: args.answer_mode,
        };
        request.validate().map_err(validation)?;
        let response = self.client.query(&request).await.map_err(api_error)?;
        let text = format_query_text(&response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_search_all(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: MemorySearchAllArgs = from_args(arguments)?;
        let request = GlobalQueryRequest {
            query: args.question,
            filters: QueryFilters {
                types: args.types.unwrap_or_default(),
                tags: args.tags.unwrap_or_default(),
            },
            top_k: args.top_k.unwrap_or(12),
            min_confidence: args.min_confidence,
            include_stale: args.include_stale.unwrap_or(false),
            history: args.history.unwrap_or(false),
            retrieval_mode: None,
            answer_mode: args.answer_mode,
        };
        request.validate().map_err(validation)?;
        let response = self
            .client
            .query_global(&request)
            .await
            .map_err(api_error)?;
        let text = format_query_text(&response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_resume(&self, arguments: Map<String, Value>) -> Result<CallToolResult, McpError> {
        let args: MemoryResumeArgs = from_args(arguments)?;
        let project = self.resolve_project(args.project.as_deref())?;
        let request = ResumeRequest {
            project: project.clone(),
            checkpoint: args.repo_root.as_deref().map(|repo_root| ResumeCheckpoint {
                project: project.clone(),
                repo_root: repo_root.to_string(),
                marked_at: Utc::now(),
                note: Some("MCP resume request".to_string()),
                git_branch: None,
                git_head: None,
            }),
            repo_root: args.repo_root,
            since: None,
            include_llm_summary: args.include_llm_summary.unwrap_or(false),
            limit: args.limit.unwrap_or(12),
        };
        request.validate().map_err(validation)?;
        let response = self
            .client
            .resume(&project, &request)
            .await
            .map_err(api_error)?;
        structured_with_text(response.briefing.clone(), json!({ "response": response }))
    }

    async fn tool_up_to_speed(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: MemoryUpToSpeedArgs = from_args(arguments)?;
        let project = self.resolve_project(args.project.as_deref())?;
        let request = UpToSpeedRequest {
            project: project.clone(),
            include_llm_summary: args.include_llm_summary.unwrap_or(false),
            limit: args.limit.unwrap_or(20),
        };
        request.validate().map_err(validation)?;
        let response = self
            .client
            .up_to_speed(&project, &request)
            .await
            .map_err(api_error)?;
        structured_with_text(response.briefing.clone(), json!({ "response": response }))
    }

    async fn tool_overview(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: ProjectArg = from_args(arguments)?;
        let project = self.resolve_project(args.project.as_deref())?;
        let response = self
            .client
            .project_overview(&project)
            .await
            .map_err(api_error)?;
        let text = format!(
            "{}: {} active memories, {} archived, {} pending proposals, database {}",
            response.project,
            response.active_memories,
            response.archived_memories,
            response.pending_replacement_proposals,
            response.database_status
        );
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_list_memories(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: MemoryListArgs = from_args(arguments)?;
        let project = self.resolve_project(args.project.as_deref())?;
        let response = self
            .client
            .project_memories(
                &project,
                args.status.as_deref(),
                args.limit.unwrap_or(50).clamp(1, 500),
                args.offset.unwrap_or(0).max(0),
            )
            .await
            .map_err(api_error)?;
        let text = response
            .items
            .iter()
            .map(|item| format!("- {} [{}] {}", item.id, item.memory_type, item.summary))
            .collect::<Vec<_>>()
            .join("\n");
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_get_memory(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: MemoryIdArg = from_args(arguments)?;
        let response = self
            .client
            .memory(&args.memory_id)
            .await
            .map_err(api_error)?;
        structured_with_text(
            response.canonical_text.clone(),
            json!({ "response": response }),
        )
    }

    async fn tool_memory_history(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: MemoryIdArg = from_args(arguments)?;
        let response = self
            .client
            .memory_history(&args.memory_id)
            .await
            .map_err(api_error)?;
        let text = response
            .versions
            .iter()
            .map(|entry| {
                format!(
                    "- v{} {} {}",
                    entry.version_no,
                    entry.updated_at.to_rfc3339(),
                    entry.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_list_activities(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: ActivityListArgs = from_args(arguments)?;
        let project = self.resolve_project(args.project.as_deref())?;
        let response = self
            .client
            .project_activities(
                &project,
                args.limit.unwrap_or(50).clamp(1, 500),
                args.kind.as_deref(),
                args.since,
                args.before,
                args.include_details.unwrap_or(true),
            )
            .await
            .map_err(api_error)?;
        let text = response
            .items
            .iter()
            .map(|item| {
                format!(
                    "- {} {:?}: {}",
                    item.recorded_at.to_rfc3339(),
                    item.kind,
                    item.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_list_replacement_proposals(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: ProjectArg = from_args(arguments)?;
        let project = self.resolve_project(args.project.as_deref())?;
        let response = self
            .client
            .replacement_proposals(&project)
            .await
            .map_err(api_error)?;
        let text = response
            .proposals
            .iter()
            .map(|proposal| {
                format!(
                    "- {} replaces {} with {}",
                    proposal.id, proposal.target_summary, proposal.candidate_summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_list(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let _: EmptyArgs = from_args(arguments)?;
        let response = self.client.loop_definitions().await.map_err(api_error)?;
        let text = response
            .definitions
            .iter()
            .map(|definition| {
                format!(
                    "- {} v{} [{} default={}] {}",
                    definition.loop_id,
                    definition.version,
                    definition.risk_level,
                    definition.default_mode,
                    definition.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_get(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopGetArgs = from_args(arguments)?;
        let project = optional_project_for_http(&self.mode, args.project)?;
        let response = self
            .client
            .loop_definition(&args.loop_id, project.as_deref(), args.repo_root.as_deref())
            .await
            .map_err(api_error)?;
        let text = format!(
            "{} v{} [{} default={}]\n{}",
            response.definition.loop_id,
            response.definition.version,
            response.definition.risk_level,
            response.definition.default_mode,
            response.definition.description
        );
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_enable(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopSettingArgs = from_args(arguments)?;
        let request = args.to_update_request(Some(true), args.mode.clone(), None, None);
        let response = self
            .client
            .loop_enable(&args.loop_id, &request)
            .await
            .map_err(api_error)?;
        let text = format_loop_setting_text("enable", &response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_disable(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopSettingArgs = from_args(arguments)?;
        let request = args.to_update_request(Some(false), Some(LoopMode::Off), None, None);
        let response = self
            .client
            .loop_disable(&args.loop_id, &request)
            .await
            .map_err(api_error)?;
        let text = format_loop_setting_text("disable", &response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_pause(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopPauseArgs = from_args(arguments)?;
        let request = args.setting.to_update_request(
            None,
            Some(LoopMode::Paused),
            Some(args.paused_until),
            None,
        );
        let response = self
            .client
            .loop_pause(&args.setting.loop_id, &request)
            .await
            .map_err(api_error)?;
        let text = format_loop_setting_text("pause", &response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_snooze(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopSnoozeArgs = from_args(arguments)?;
        let request = args.setting.to_update_request(
            None,
            Some(LoopMode::Snoozed),
            None,
            Some(args.snoozed_until),
        );
        let response = self
            .client
            .loop_snooze(&args.setting.loop_id, &request)
            .await
            .map_err(api_error)?;
        let text = format_loop_setting_text("snooze", &response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_run(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopRunArgs = from_args(arguments)?;
        let request = LoopRunRequest {
            project: args.project,
            repo_root: args.repo_root,
            scope_type: args.scope_type,
            scope_id: args.scope_id,
            dry_run: args.dry_run.unwrap_or(true),
            reason: args.reason,
            trigger_payload: args.trigger_payload,
        };
        request.validate().map_err(validation)?;
        let response = self
            .client
            .loop_run(&args.loop_id, &request)
            .await
            .map_err(api_error)?;
        let text = format_loop_run_text(&response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_runs(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopRunsArgs = from_args(arguments)?;
        let response = self
            .client
            .loop_runs(
                args.project.as_deref(),
                args.loop_id.as_deref(),
                args.status.as_ref(),
                args.limit.unwrap_or(50).clamp(1, 200),
            )
            .await
            .map_err(api_error)?;
        let text = response
            .runs
            .iter()
            .map(|run| {
                format!(
                    "- {} {} {} {} traces={}",
                    run.id,
                    run.loop_id,
                    run.status.as_str(),
                    run.started_at.to_rfc3339(),
                    run.trace_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_inspect(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: RunIdArg = from_args(arguments)?;
        let response = self
            .client
            .loop_run_detail(&args.run_id)
            .await
            .map_err(api_error)?;
        let text = format_loop_run_text(&response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_cancel(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopCancelArgs = from_args(arguments)?;
        let request = LoopCancelRequest {
            reason: args.reason,
        };
        let response = self
            .client
            .loop_cancel(&args.run_id, &request)
            .await
            .map_err(api_error)?;
        let text = format_loop_run_text(&response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_feedback(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopFeedbackArgs = from_args(arguments)?;
        let request = LoopFeedbackRequest {
            rating: args.rating,
            note: args.note,
        };
        request.validate().map_err(validation)?;
        let response = self
            .client
            .loop_feedback(&args.run_id, &request)
            .await
            .map_err(api_error)?;
        let text = format_loop_run_text(&response);
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_list_approvals(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopApprovalsArgs = from_args(arguments)?;
        let response = self
            .client
            .loop_approvals(
                args.project.as_deref(),
                args.run_id.as_deref(),
                args.loop_id.as_deref(),
                args.status.as_ref(),
                args.limit.unwrap_or(50).clamp(1, 200),
            )
            .await
            .map_err(api_error)?;
        let text = response
            .approvals
            .iter()
            .map(|approval| {
                format!(
                    "- {} {} {} [{}] {}",
                    approval.id,
                    approval.loop_id,
                    approval.action_type,
                    approval.status.as_str(),
                    approval.risk_reason
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        structured_with_text(text, json!({ "response": response }))
    }

    async fn tool_loop_approve(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopApprovalDecisionArgs = from_args(arguments)?;
        let request = LoopApprovalDecisionRequest {
            reviewer: args.reviewer,
            reason: args.reason,
            edited_action: None,
        };
        let response = self
            .client
            .loop_approval_decision(&args.approval_id, true, &request)
            .await
            .map_err(api_error)?;
        structured_with_text(
            format!(
                "Approved {} for loop {}.",
                response.approval.id, response.approval.loop_id
            ),
            json!({ "response": response }),
        )
    }

    async fn tool_loop_reject(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopApprovalDecisionArgs = from_args(arguments)?;
        let request = LoopApprovalDecisionRequest {
            reviewer: args.reviewer,
            reason: args.reason,
            edited_action: None,
        };
        let response = self
            .client
            .loop_approval_decision(&args.approval_id, false, &request)
            .await
            .map_err(api_error)?;
        structured_with_text(
            format!(
                "Rejected {} for loop {}.",
                response.approval.id, response.approval.loop_id
            ),
            json!({ "response": response }),
        )
    }

    async fn tool_loop_edit_approval(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopApprovalEditArgs = from_args(arguments)?;
        let request = LoopApprovalDecisionRequest {
            reviewer: args.reviewer,
            reason: args.reason,
            edited_action: Some(args.proposed_action),
        };
        let response = self
            .client
            .loop_approval_edit(&args.approval_id, &request)
            .await
            .map_err(api_error)?;
        structured_with_text(
            format!(
                "Edited {} for loop {}.",
                response.approval.id, response.approval.loop_id
            ),
            json!({ "response": response }),
        )
    }

    async fn tool_loop_global_state(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let _: EmptyArgs = from_args(arguments)?;
        let response = self.client.loop_global_state().await.map_err(api_error)?;
        structured_with_text(
            format!(
                "Global kill switch: {}",
                if response.kill_switch_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            json!({ "response": response }),
        )
    }

    async fn tool_loop_set_global_kill_switch(
        &self,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, McpError> {
        let args: LoopGlobalKillSwitchArgs = from_args(arguments)?;
        let request = LoopGlobalStateUpdateRequest {
            kill_switch_enabled: args.kill_switch_enabled,
            updated_by: args.updated_by,
            reason: args.reason,
        };
        let response = self
            .client
            .loop_set_global_state(&request)
            .await
            .map_err(api_error)?;
        structured_with_text(
            format!(
                "Global kill switch: {}",
                if response.kill_switch_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            json!({ "response": response }),
        )
    }
}

#[derive(Debug, Clone)]
pub struct MemoryApiClient {
    client: Client,
    base_url: String,
    token: String,
}

impl MemoryApiClient {
    pub fn new(config: AppConfig) -> Self {
        Self {
            client: Client::new(),
            base_url: format!("http://{}", config.service.bind_addr),
            token: config.service.api_token,
        }
    }

    pub fn from_parts(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    pub async fn health(&self) -> Result<Value> {
        self.get("/healthz").await
    }

    pub async fn query(&self, request: &QueryRequest) -> Result<QueryResponse> {
        self.post("/v1/query", request).await
    }

    pub async fn query_global(&self, request: &GlobalQueryRequest) -> Result<QueryResponse> {
        self.post("/v1/query/global", request).await
    }

    pub async fn resume(&self, project: &str, request: &ResumeRequest) -> Result<ResumeResponse> {
        self.post(&format!("/v1/projects/{project}/resume"), request)
            .await
    }

    pub async fn up_to_speed(
        &self,
        project: &str,
        request: &UpToSpeedRequest,
    ) -> Result<UpToSpeedResponse> {
        self.post(&format!("/v1/projects/{project}/up-to-speed"), request)
            .await
    }

    pub async fn project_overview(&self, project: &str) -> Result<ProjectOverviewResponse> {
        self.get(&format!("/v1/projects/{project}/overview")).await
    }

    pub async fn project_memories(
        &self,
        project: &str,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<ProjectMemoriesResponse> {
        let mut path = format!("/v1/projects/{project}/memories?limit={limit}&offset={offset}");
        if let Some(status) = status {
            path.push_str("&status=");
            path.push_str(&urlencoding::encode(status));
        }
        self.get(&path).await
    }

    pub async fn memory(&self, memory_id: &str) -> Result<MemoryEntryResponse> {
        self.get(&format!("/v1/memory/{memory_id}")).await
    }

    pub async fn memory_history(&self, memory_id: &str) -> Result<MemoryHistoryResponse> {
        self.get(&format!("/v1/memory/{memory_id}/history")).await
    }

    pub async fn project_activities(
        &self,
        project: &str,
        limit: usize,
        kind: Option<&str>,
        since: Option<DateTime<Utc>>,
        before: Option<DateTime<Utc>>,
        include_details: bool,
    ) -> Result<ActivityListResponse> {
        let mut params = vec![
            format!("limit={limit}"),
            format!("include_details={include_details}"),
        ];
        if let Some(kind) = kind {
            params.push(format!("kind={}", urlencoding::encode(kind)));
        }
        if let Some(since) = since {
            params.push(format!(
                "since={}",
                urlencoding::encode(&since.to_rfc3339())
            ));
        }
        if let Some(before) = before {
            params.push(format!(
                "before={}",
                urlencoding::encode(&before.to_rfc3339())
            ));
        }
        self.get(&format!(
            "/v1/projects/{project}/activities?{}",
            params.join("&")
        ))
        .await
    }

    pub async fn replacement_proposals(
        &self,
        project: &str,
    ) -> Result<ReplacementProposalListResponse> {
        self.get(&format!("/v1/projects/{project}/replacement-proposals"))
            .await
    }

    pub async fn loop_definitions(&self) -> Result<LoopDefinitionsResponse> {
        self.get("/v1/loops").await
    }

    pub async fn loop_definition(
        &self,
        loop_id: &str,
        project: Option<&str>,
        repo_root: Option<&str>,
    ) -> Result<LoopDefinitionResponse> {
        let mut params = Vec::new();
        if let Some(project) = project {
            params.push(format!("project={}", urlencoding::encode(project)));
        }
        if let Some(repo_root) = repo_root {
            params.push(format!("repo_root={}", urlencoding::encode(repo_root)));
        }
        let suffix = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.get(&format!("/v1/loops/{loop_id}{suffix}")).await
    }

    pub async fn loop_enable(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<LoopSettingResponse> {
        self.post(&format!("/v1/loops/{loop_id}/enable"), request)
            .await
    }

    pub async fn loop_disable(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<LoopSettingResponse> {
        self.post(&format!("/v1/loops/{loop_id}/disable"), request)
            .await
    }

    pub async fn loop_pause(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<LoopSettingResponse> {
        self.post(&format!("/v1/loops/{loop_id}/pause"), request)
            .await
    }

    pub async fn loop_snooze(
        &self,
        loop_id: &str,
        request: &LoopSettingsUpdateRequest,
    ) -> Result<LoopSettingResponse> {
        self.post(&format!("/v1/loops/{loop_id}/snooze"), request)
            .await
    }

    pub async fn loop_run(
        &self,
        loop_id: &str,
        request: &LoopRunRequest,
    ) -> Result<LoopRunResponse> {
        self.post(&format!("/v1/loops/{loop_id}/run"), request)
            .await
    }

    pub async fn loop_runs(
        &self,
        project: Option<&str>,
        loop_id: Option<&str>,
        status: Option<&LoopRunStatus>,
        limit: i64,
    ) -> Result<LoopRunsResponse> {
        let mut params = vec![format!("limit={limit}")];
        if let Some(project) = project {
            params.push(format!("project={}", urlencoding::encode(project)));
        }
        if let Some(loop_id) = loop_id {
            params.push(format!("loop_id={}", urlencoding::encode(loop_id)));
        }
        if let Some(status) = status {
            params.push(format!("status={}", status.as_str()));
        }
        self.get(&format!("/v1/loops/runs?{}", params.join("&")))
            .await
    }

    pub async fn loop_run_detail(&self, run_id: &str) -> Result<LoopRunResponse> {
        self.get(&format!("/v1/loops/runs/{run_id}")).await
    }

    pub async fn loop_cancel(
        &self,
        run_id: &str,
        request: &LoopCancelRequest,
    ) -> Result<LoopRunResponse> {
        self.post(&format!("/v1/loops/runs/{run_id}/cancel"), request)
            .await
    }

    pub async fn loop_feedback(
        &self,
        run_id: &str,
        request: &LoopFeedbackRequest,
    ) -> Result<LoopRunResponse> {
        self.post(&format!("/v1/loops/runs/{run_id}/feedback"), request)
            .await
    }

    pub async fn loop_approvals(
        &self,
        project: Option<&str>,
        run_id: Option<&str>,
        loop_id: Option<&str>,
        status: Option<&LoopApprovalStatus>,
        limit: i64,
    ) -> Result<LoopApprovalsResponse> {
        let mut params = vec![format!("limit={limit}")];
        if let Some(project) = project {
            params.push(format!("project={}", urlencoding::encode(project)));
        }
        if let Some(run_id) = run_id {
            params.push(format!("run_id={}", urlencoding::encode(run_id)));
        }
        if let Some(loop_id) = loop_id {
            params.push(format!("loop_id={}", urlencoding::encode(loop_id)));
        }
        if let Some(status) = status {
            params.push(format!("status={}", status.as_str()));
        }
        self.get(&format!("/v1/loops/approvals?{}", params.join("&")))
            .await
    }

    pub async fn loop_approval_decision(
        &self,
        approval_id: &str,
        approved: bool,
        request: &LoopApprovalDecisionRequest,
    ) -> Result<LoopApprovalDecisionResponse> {
        let action = if approved { "approve" } else { "reject" };
        self.post(
            &format!("/v1/loops/approvals/{approval_id}/{action}"),
            request,
        )
        .await
    }

    pub async fn loop_approval_edit(
        &self,
        approval_id: &str,
        request: &LoopApprovalDecisionRequest,
    ) -> Result<LoopApprovalDecisionResponse> {
        self.post(&format!("/v1/loops/approvals/{approval_id}/edit"), request)
            .await
    }

    pub async fn loop_global_state(&self) -> Result<LoopGlobalStateResponse> {
        self.get("/v1/loops/global-kill-switch").await
    }

    pub async fn loop_set_global_state(
        &self,
        request: &LoopGlobalStateUpdateRequest,
    ) -> Result<LoopGlobalStateResponse> {
        self.post("/v1/loops/global-kill-switch", request).await
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .header("x-api-token", &self.token)
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        decode_json(response, path).await
    }

    async fn post<T: serde::Serialize, U: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<U> {
        let response = self
            .client
            .post(format!("{}{}", self.base_url, path))
            .header("x-api-token", &self.token)
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        decode_json(response, path).await
    }
}

async fn decode_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    path: &str,
) -> Result<T> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{path} returned {status}: {body}");
    }
    serde_json::from_str(&body).with_context(|| format!("decode response from {path}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpcStatusReport {
    pub service_reachable: bool,
    pub service_error: Option<String>,
    pub project: Option<String>,
    pub project_overview_ok: bool,
    pub project_error: Option<String>,
    pub http_enabled: bool,
    pub http_path: String,
    pub read_only: bool,
    pub require_token: bool,
    pub protocol_version: String,
    pub tools: Vec<String>,
    pub resource_templates: Vec<String>,
    pub prompts: Vec<String>,
}

pub async fn status_report(config: AppConfig, project: Option<String>) -> MpcStatusReport {
    let client = MemoryApiClient::new(config.clone());
    let (service_reachable, service_error) = match client.health().await {
        Ok(_) => (true, None),
        Err(error) => (false, Some(error.to_string())),
    };
    let (project_overview_ok, project_error) = if let Some(project) = project.as_deref() {
        match client.project_overview(project).await {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.to_string())),
        }
    } else {
        (false, Some("no project supplied".to_string()))
    };
    MpcStatusReport {
        service_reachable,
        service_error,
        project,
        project_overview_ok,
        project_error,
        http_enabled: config.mcp.enabled && config.mcp.http_enabled,
        http_path: config.mcp.http_path,
        read_only: config.mcp.read_only,
        require_token: config.mcp.require_token,
        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        tools: tool_definitions()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect(),
        resource_templates: resource_templates()
            .into_iter()
            .map(|template| template.uri_template.clone())
            .collect(),
        prompts: prompt_definitions()
            .into_iter()
            .map(|prompt| prompt.name)
            .collect(),
    }
}

pub async fn run_stdio(config: AppConfig, project: Option<String>, cwd: &Path) -> Result<()> {
    let server = MemoryMcpServer::stdio(config, project, cwd);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct MemoryQueryArgs {
    project: Option<String>,
    question: String,
    top_k: Option<i64>,
    min_confidence: Option<f32>,
    include_stale: Option<bool>,
    history: Option<bool>,
    answer_mode: Option<QueryAnswerMode>,
}

#[derive(Debug, Deserialize)]
struct MemorySearchAllArgs {
    question: String,
    top_k: Option<i64>,
    min_confidence: Option<f32>,
    include_stale: Option<bool>,
    history: Option<bool>,
    answer_mode: Option<QueryAnswerMode>,
    types: Option<Vec<MemoryType>>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct MemoryResumeArgs {
    project: Option<String>,
    repo_root: Option<String>,
    limit: Option<usize>,
    include_llm_summary: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MemoryUpToSpeedArgs {
    project: Option<String>,
    limit: Option<usize>,
    include_llm_summary: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ProjectArg {
    project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryListArgs {
    project: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MemoryIdArg {
    memory_id: String,
}

#[derive(Debug, Deserialize)]
struct ActivityListArgs {
    project: Option<String>,
    limit: Option<usize>,
    kind: Option<String>,
    since: Option<DateTime<Utc>>,
    before: Option<DateTime<Utc>>,
    include_details: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct EmptyArgs {}

#[derive(Debug, Deserialize)]
struct LoopGetArgs {
    loop_id: String,
    project: Option<String>,
    repo_root: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoopSettingArgs {
    loop_id: String,
    scope_type: Option<LoopScopeType>,
    scope_id: Option<String>,
    project: Option<String>,
    repo_root: Option<String>,
    mode: Option<LoopMode>,
    updated_by: Option<String>,
    reason: Option<String>,
    explicit_user_approval: Option<bool>,
}

impl LoopSettingArgs {
    fn to_update_request(
        &self,
        enabled: Option<bool>,
        mode: Option<LoopMode>,
        paused_until: Option<DateTime<Utc>>,
        snoozed_until: Option<DateTime<Utc>>,
    ) -> LoopSettingsUpdateRequest {
        LoopSettingsUpdateRequest {
            scope_type: self.scope_type.clone(),
            scope_id: self.scope_id.clone(),
            project: self.project.clone(),
            repo_root: self.repo_root.clone(),
            enabled,
            mode,
            budgets: None,
            approval_overrides: None,
            paused_until,
            snoozed_until,
            updated_by: self.updated_by.clone(),
            reason: self.reason.clone(),
            explicit_user_approval: self.explicit_user_approval.unwrap_or(false),
        }
    }
}

#[derive(Debug, Deserialize)]
struct LoopPauseArgs {
    #[serde(flatten)]
    setting: LoopSettingArgs,
    paused_until: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct LoopSnoozeArgs {
    #[serde(flatten)]
    setting: LoopSettingArgs,
    snoozed_until: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct LoopRunArgs {
    loop_id: String,
    project: Option<String>,
    repo_root: Option<String>,
    scope_type: Option<LoopScopeType>,
    scope_id: Option<String>,
    dry_run: Option<bool>,
    reason: Option<String>,
    trigger_payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct LoopRunsArgs {
    project: Option<String>,
    loop_id: Option<String>,
    status: Option<LoopRunStatus>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RunIdArg {
    run_id: String,
}

#[derive(Debug, Deserialize)]
struct LoopCancelArgs {
    run_id: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoopFeedbackArgs {
    run_id: String,
    rating: String,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoopApprovalsArgs {
    project: Option<String>,
    run_id: Option<String>,
    loop_id: Option<String>,
    status: Option<LoopApprovalStatus>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LoopApprovalDecisionArgs {
    approval_id: String,
    reviewer: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoopApprovalEditArgs {
    approval_id: String,
    proposed_action: Value,
    reviewer: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoopGlobalKillSwitchArgs {
    kill_switch_enabled: bool,
    updated_by: Option<String>,
    reason: Option<String>,
}

fn from_args<T: serde::de::DeserializeOwned>(arguments: Map<String, Value>) -> Result<T, McpError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|error| invalid_params(format!("invalid tool arguments: {error}")))
}

fn optional_project_for_http(
    mode: &ProjectResolutionMode,
    project: Option<String>,
) -> Result<Option<String>, McpError> {
    if matches!(mode, ProjectResolutionMode::Http) && project.as_deref().is_none_or(str::is_empty) {
        return Err(invalid_params(
            "project is required for HTTP MCP tools because the service has no trustworthy current directory",
        ));
    }
    Ok(project)
}

fn invalid_params(message: impl Into<String>) -> McpError {
    McpError::invalid_params(message.into(), None)
}

fn validation(error: mem_api::ValidationError) -> McpError {
    invalid_params(error.to_string())
}

fn api_error(error: anyhow::Error) -> McpError {
    McpError::internal_error(error.to_string(), None)
}

fn structured_with_text(text: String, value: Value) -> Result<CallToolResult, McpError> {
    let mut result = CallToolResult::structured(value);
    result.content = vec![Content::text(text)];
    Ok(result)
}

fn pretty_json(value: &Value) -> Result<String, McpError> {
    serde_json::to_string_pretty(value)
        .map_err(|error| McpError::internal_error(error.to_string(), None))
}

fn format_query_text(response: &QueryResponse) -> String {
    let mut lines = vec![response.answer.clone()];
    if !response.answer_citations.is_empty() {
        lines.push(String::new());
        lines.push("Citations:".to_string());
        for citation in &response.answer_citations {
            lines.push(format!(
                "- [{}]{} {} ({})",
                citation.result_number,
                route_suffix(
                    citation.project.as_deref(),
                    citation.project_name.as_deref(),
                    citation.repo_root.as_deref(),
                ),
                citation.summary,
                citation.memory_id
            ));
        }
    }
    if !response.results.is_empty() {
        lines.push(String::new());
        lines.push("Result summaries:".to_string());
        for (idx, result) in response.results.iter().enumerate() {
            lines.push(format!(
                "- [{}]{} {} [{}] {}",
                idx + 1,
                route_suffix(
                    result.project.as_deref(),
                    result.project_name.as_deref(),
                    result.repo_root.as_deref(),
                ),
                result.summary,
                result.match_kind,
                result.memory_id
            ));
        }
    }
    lines.join("\n")
}

fn route_suffix(
    project: Option<&str>,
    project_name: Option<&str>,
    repo_root: Option<&str>,
) -> String {
    let Some(project) = project else {
        return String::new();
    };
    let mut parts = vec![format!("project={project}")];
    if let Some(name) = project_name.filter(|value| !value.trim().is_empty()) {
        parts.push(format!("name={name}"));
    }
    if let Some(repo_root) = repo_root.filter(|value| !value.trim().is_empty()) {
        parts.push(format!("repo_root={repo_root}"));
    }
    format!(" [{}]", parts.join(" | "))
}

fn format_loop_setting_text(action: &str, response: &LoopSettingResponse) -> String {
    if let Some(approval) = &response.approval {
        return format!(
            "Loop {action} requires approval. Approval {} is {} for loop {}.",
            approval.id,
            approval.status.as_str(),
            approval.loop_id
        );
    }
    format!(
        "Loop {action} applied for {} at {}:{}; effective mode={} enabled={} blocked={}",
        response.setting.loop_id,
        response.setting.scope_type,
        response.setting.scope_id,
        response.effective_settings.mode,
        response.effective_settings.enabled,
        response.effective_settings.blocked_reasons.join(", ")
    )
}

fn format_loop_run_text(response: &LoopRunResponse) -> String {
    let run = &response.run.summary;
    format!(
        "{} {} status={} mode={} traces={} blocked={}",
        run.id,
        run.loop_id,
        run.status.as_str(),
        run.mode,
        run.trace_count,
        run.blocked_reasons.join(", ")
    )
}

fn tool_definitions() -> Vec<Tool> {
    vec![
        tool(
            TOOL_MEMORY_QUERY,
            "Ask a project-specific question against Memory Layer.",
            object_schema(&[
                required_string("question", "Question to answer from project memory."),
                optional_string("project", "Project slug. Required for HTTP MCP."),
                optional_integer("top_k", "Maximum number of memories to retrieve."),
                optional_number("min_confidence", "Minimum memory confidence, 0.0 to 1.0."),
                optional_boolean(
                    "include_stale",
                    "Bypass provenance-based stale-source de-ranking.",
                ),
                optional_boolean("history", "Search historical memory versions."),
                optional_enum("answer_mode", &["auto", "deterministic", "llm"]),
            ]),
        ),
        tool(
            TOOL_MEMORY_SEARCH_ALL,
            "Search across all Memory Layer projects and return project/repo routing metadata for follow-up actions.",
            object_schema(&[
                required_string("question", "Question to answer from all project memory."),
                optional_integer("top_k", "Maximum number of memories to retrieve."),
                optional_number("min_confidence", "Minimum memory confidence, 0.0 to 1.0."),
                optional_boolean(
                    "include_stale",
                    "Bypass provenance-based stale-source de-ranking.",
                ),
                optional_boolean("history", "Search historical memory versions."),
                optional_enum("answer_mode", &["auto", "deterministic", "llm"]),
                optional_string_array(
                    "types",
                    "Memory type filters, for example implementation or refactor.",
                ),
                optional_string_array("tags", "Memory tag filters."),
            ]),
        ),
        tool(
            TOOL_MEMORY_RESUME,
            "Generate a resume briefing for a project.",
            object_schema(&[
                optional_string("project", "Project slug. Required for HTTP MCP."),
                optional_string(
                    "repo_root",
                    "Repository root used to load a local checkpoint.",
                ),
                optional_integer("limit", "Maximum number of timeline items."),
                optional_boolean("include_llm_summary", "Request an LLM synthesized summary."),
            ]),
        ),
        tool(
            TOOL_MEMORY_UP_TO_SPEED,
            "Generate a new-agent get-up-to-speed briefing.",
            object_schema(&[
                optional_string("project", "Project slug. Required for HTTP MCP."),
                optional_integer("limit", "Maximum number of recent activities."),
                optional_boolean("include_llm_summary", "Request an LLM synthesized summary."),
            ]),
        ),
        tool(
            TOOL_MEMORY_OVERVIEW,
            "Return counts, embedding status, watcher summary, and pending proposals.",
            project_schema(),
        ),
        tool(
            TOOL_MEMORY_LIST_MEMORIES,
            "List compact memory rows for a project.",
            object_schema(&[
                optional_string("project", "Project slug. Required for HTTP MCP."),
                optional_enum("status", &["active", "archived"]),
                optional_integer("limit", "Maximum rows to return."),
                optional_integer("offset", "Rows to skip."),
            ]),
        ),
        tool(
            TOOL_MEMORY_GET_MEMORY,
            "Return full canonical text and metadata for one memory.",
            object_schema(&[required_string("memory_id", "Memory UUID.")]),
        ),
        tool(
            TOOL_MEMORY_MEMORY_HISTORY,
            "Return the version chain for one canonical memory.",
            object_schema(&[required_string("memory_id", "Memory UUID.")]),
        ),
        tool(
            TOOL_MEMORY_LIST_ACTIVITIES,
            "List persisted project activity events.",
            object_schema(&[
                optional_string("project", "Project slug. Required for HTTP MCP."),
                optional_integer("limit", "Maximum events to return."),
                optional_string("kind", "Activity kind filter."),
                optional_string("since", "Lower RFC3339 timestamp bound."),
                optional_string("before", "Upper RFC3339 timestamp bound."),
                optional_boolean("include_details", "Include detailed event payloads."),
            ]),
        ),
        tool(
            TOOL_MEMORY_LIST_REPLACEMENT_PROPOSALS,
            "Read-only view of pending replacement proposals.",
            project_schema(),
        ),
        tool(
            TOOL_MEMORY_LOOP_LIST,
            "List built-in loop definitions and their default modes.",
            object_schema(&[]),
        ),
        tool(
            TOOL_MEMORY_LOOP_GET,
            "Inspect one loop definition and optional effective settings.",
            object_schema(&[
                required_string("loop_id", "Loop identifier."),
                optional_string("project", "Project slug. Required for HTTP MCP."),
                optional_string("repo_root", "Repository root for repo-scoped settings."),
            ]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_ENABLE,
            "Request or apply enabling a loop. Set explicit_user_approval=true only after the user explicitly approved this change.",
            loop_setting_schema(&[
                optional_enum("mode", &loop_mode_values()),
                optional_boolean(
                    "explicit_user_approval",
                    "True only when the user explicitly approved enabling this loop.",
                ),
            ]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_DISABLE,
            "Disable a loop for a user, project, workspace, or repo scope.",
            loop_setting_schema(&[]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_PAUSE,
            "Pause a loop until a specific RFC3339 timestamp.",
            loop_setting_schema(&[required_string(
                "paused_until",
                "RFC3339 timestamp when the pause expires.",
            )]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_SNOOZE,
            "Snooze a loop until a specific RFC3339 timestamp.",
            loop_setting_schema(&[required_string(
                "snoozed_until",
                "RFC3339 timestamp when the snooze expires.",
            )]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_RUN,
            "Create a policy-checked control-plane loop run record. This slice does not execute real loop implementations.",
            object_schema(&[
                required_string("loop_id", "Loop identifier."),
                optional_string("project", "Project slug."),
                optional_string("repo_root", "Repository root for repo context."),
                optional_enum("scope_type", &loop_scope_values()),
                optional_string("scope_id", "Explicit scope identifier."),
                optional_boolean("dry_run", "Record the run as dry-run intent."),
                optional_string("reason", "Reason for the manual run."),
                optional_object("trigger_payload", "Additional trigger payload JSON."),
            ]),
        ),
        tool(
            TOOL_MEMORY_LOOP_RUNS,
            "List loop run ledger rows.",
            object_schema(&[
                optional_string("project", "Project slug filter."),
                optional_string("loop_id", "Loop identifier filter."),
                optional_enum("status", &loop_run_status_values()),
                optional_integer("limit", "Maximum runs to return."),
            ]),
        ),
        tool(
            TOOL_MEMORY_LOOP_INSPECT,
            "Inspect one loop run with traces, policy decisions, and output JSON.",
            object_schema(&[required_string("run_id", "Loop run UUID.")]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_CANCEL,
            "Request cancellation for a queued or running loop run.",
            object_schema(&[
                required_string("run_id", "Loop run UUID."),
                optional_string("reason", "Cancellation reason."),
            ]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_FEEDBACK,
            "Attach user feedback to a loop run trace.",
            object_schema(&[
                required_string("run_id", "Loop run UUID."),
                required_string(
                    "rating",
                    "Feedback rating such as good, bad, or needs_review.",
                ),
                optional_string("note", "Feedback note."),
            ]),
        ),
        tool(
            TOOL_MEMORY_LOOP_LIST_APPROVALS,
            "List pending or resolved loop approval requests.",
            object_schema(&[
                optional_string("project", "Project slug filter."),
                optional_string("run_id", "Loop run UUID filter."),
                optional_string("loop_id", "Loop identifier filter."),
                optional_enum("status", &loop_approval_status_values()),
                optional_integer("limit", "Maximum approvals to return."),
            ]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_APPROVE,
            "Approve a pending loop approval request after explicit user confirmation.",
            object_schema(&[
                required_string("approval_id", "Loop approval UUID."),
                optional_string("reviewer", "Reviewer identity."),
                optional_string("reason", "Approval reason."),
            ]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_REJECT,
            "Reject a pending loop approval request.",
            object_schema(&[
                required_string("approval_id", "Loop approval UUID."),
                optional_string("reviewer", "Reviewer identity."),
                optional_string("reason", "Rejection reason."),
            ]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_EDIT_APPROVAL,
            "Edit a loop approval request proposed action after human review.",
            object_schema(&[
                required_string("approval_id", "Loop approval UUID."),
                required_object("proposed_action", "Edited proposed action JSON."),
                optional_string("reviewer", "Reviewer identity."),
                optional_string("reason", "Edit reason."),
            ]),
        ),
        tool(
            TOOL_MEMORY_LOOP_GLOBAL_STATE,
            "Read the loop automation global kill-switch state.",
            object_schema(&[]),
        ),
        mutating_tool(
            TOOL_MEMORY_LOOP_SET_GLOBAL_KILL_SWITCH,
            "Enable or disable the loop automation global kill switch.",
            object_schema(&[
                required_boolean("kill_switch_enabled", "Desired global kill-switch state."),
                optional_string("updated_by", "Actor changing the kill switch."),
                optional_string("reason", "Reason for the change."),
            ]),
        ),
    ]
}

fn tool(name: &'static str, description: &'static str, schema: Map<String, Value>) -> Tool {
    Tool::new(name, description, schema).with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

fn mutating_tool(
    name: &'static str,
    description: &'static str,
    schema: Map<String, Value>,
) -> Tool {
    Tool::new(name, description, schema).with_annotations(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(false)
            .open_world(false),
    )
}

fn resource_templates() -> Vec<ResourceTemplate> {
    [
        (
            "memory://projects/{project}/overview",
            "project_overview",
            "Project overview JSON.",
        ),
        (
            "memory://projects/{project}/memories",
            "project_memories",
            "Project memory list JSON.",
        ),
        (
            "memory://projects/{project}/activities",
            "project_activities",
            "Project activity list JSON.",
        ),
        (
            "memory://memories/{memory_id}",
            "memory_detail",
            "Full memory detail JSON.",
        ),
        (
            "memory://memories/{memory_id}/history",
            "memory_history",
            "Memory version history JSON.",
        ),
    ]
    .into_iter()
    .map(|(uri, name, description)| {
        RawResourceTemplate::new(uri, name)
            .with_description(description)
            .with_mime_type("application/json")
            .no_annotation()
    })
    .collect()
}

fn prompt_definitions() -> Vec<Prompt> {
    vec![
        Prompt::new(
            PROMPT_GET_UP_TO_SPEED,
            Some("Start work in a repo using Memory Layer context."),
            Some(vec![PromptArgument::new("project").with_required(true)]),
        ),
        Prompt::new(
            PROMPT_ANSWER_WITH_CONTEXT,
            Some("Answer a project question using Memory query results."),
            Some(vec![
                PromptArgument::new("project").with_required(true),
                PromptArgument::new("question").with_required(true),
            ]),
        ),
        Prompt::new(
            PROMPT_ROUTE_CROSS_PROJECT_TASK,
            Some("Route a task to the right repository using all-project memory search."),
            Some(vec![PromptArgument::new("task").with_required(true)]),
        ),
    ]
}

fn object_schema(fields: &[SchemaField]) -> Map<String, Value> {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in fields {
        properties.insert(field.name.to_string(), field.schema.clone());
        if field.required {
            required.push(Value::String(field.name.to_string()));
        }
    }
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".to_string(), Value::Array(required));
    }
    schema
}

fn project_schema() -> Map<String, Value> {
    object_schema(&[optional_string(
        "project",
        "Project slug. Required for HTTP MCP.",
    )])
}

fn loop_setting_schema(extra: &[SchemaField]) -> Map<String, Value> {
    let mut fields = vec![
        required_string("loop_id", "Loop identifier."),
        optional_enum("scope_type", &loop_scope_values()),
        optional_string("scope_id", "Explicit scope identifier."),
        optional_string(
            "project",
            "Project slug for project or repo scoped settings.",
        ),
        optional_string("repo_root", "Repository root for repo scoped settings."),
        optional_string("updated_by", "Actor changing the setting."),
        optional_string("reason", "Reason for the setting change."),
    ];
    fields.extend_from_slice(extra);
    object_schema(&fields)
}

#[derive(Debug, Clone)]
struct SchemaField {
    name: &'static str,
    schema: Value,
    required: bool,
}

fn required_string(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "string", description, true)
}

fn required_boolean(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "boolean", description, true)
}

fn required_object(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "object", description, true)
}

fn optional_string(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "string", description, false)
}

fn optional_integer(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "integer", description, false)
}

fn optional_number(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "number", description, false)
}

fn optional_boolean(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "boolean", description, false)
}

fn optional_string_array(name: &'static str, description: &'static str) -> SchemaField {
    SchemaField {
        name,
        schema: json!({
            "type": "array",
            "items": { "type": "string" },
            "description": description
        }),
        required: false,
    }
}

fn optional_object(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "object", description, false)
}

fn optional_enum(name: &'static str, values: &[&str]) -> SchemaField {
    SchemaField {
        name,
        schema: json!({ "type": "string", "enum": values }),
        required: false,
    }
}

fn field(
    name: &'static str,
    kind: &'static str,
    description: &'static str,
    required: bool,
) -> SchemaField {
    SchemaField {
        name,
        schema: json!({ "type": kind, "description": description }),
        required,
    }
}

fn loop_mode_values() -> [&'static str; 7] {
    [
        "off",
        "observe",
        "suggest_only",
        "draft_output",
        "autonomous_safe",
        "paused",
        "snoozed",
    ]
}

fn loop_scope_values() -> [&'static str; 4] {
    ["user", "workspace", "project", "repo"]
}

fn loop_run_status_values() -> [&'static str; 6] {
    [
        "queued",
        "running",
        "succeeded",
        "failed",
        "cancelled",
        "blocked",
    ]
}

fn loop_approval_status_values() -> [&'static str; 4] {
    ["pending", "approved", "rejected", "edited"]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryResource {
    ProjectOverview { project: String },
    ProjectMemories { project: String },
    ProjectActivities { project: String },
    Memory { memory_id: String },
    MemoryHistory { memory_id: String },
}

pub fn parse_resource_uri(uri: &str) -> Result<MemoryResource, McpError> {
    let Some(rest) = uri.strip_prefix("memory://") else {
        return Err(invalid_params("resource URI must start with memory://"));
    };
    let parts = rest.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["projects", project, "overview"] if !project.is_empty() => {
            Ok(MemoryResource::ProjectOverview {
                project: (*project).to_string(),
            })
        }
        ["projects", project, "memories"] if !project.is_empty() => {
            Ok(MemoryResource::ProjectMemories {
                project: (*project).to_string(),
            })
        }
        ["projects", project, "activities"] if !project.is_empty() => {
            Ok(MemoryResource::ProjectActivities {
                project: (*project).to_string(),
            })
        }
        ["memories", memory_id] if !memory_id.is_empty() => Ok(MemoryResource::Memory {
            memory_id: (*memory_id).to_string(),
        }),
        ["memories", memory_id, "history"] if !memory_id.is_empty() => {
            Ok(MemoryResource::MemoryHistory {
                memory_id: (*memory_id).to_string(),
            })
        }
        _ => Err(invalid_params(format!(
            "unsupported Memory resource URI: {uri}"
        ))),
    }
}

pub fn discover_cwd_project(cwd: &Path) -> Option<String> {
    let repo_root = mem_platform::discover_project_root(cwd)?;
    read_repo_project_slug(&repo_root).or_else(|| {
        repo_root
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToOwned::to_owned)
    })
}

pub fn format_status_text(report: &MpcStatusReport) -> String {
    let mut lines = vec![
        format!(
            "Service: {}",
            if report.service_reachable {
                "ok"
            } else {
                "unreachable"
            }
        ),
        format!(
            "Project: {}",
            report
                .project
                .as_deref()
                .map(|project| {
                    if report.project_overview_ok {
                        format!("{project} ok")
                    } else {
                        format!("{project} error")
                    }
                })
                .unwrap_or_else(|| "not configured".to_string())
        ),
        format!(
            "HTTP MCP: {} {}",
            if report.http_enabled {
                "enabled"
            } else {
                "disabled"
            },
            report.http_path
        ),
        format!("Read-only: {}", report.read_only),
        format!("Token required: {}", report.require_token),
        format!("Protocol: {}", report.protocol_version),
        format!("Tools: {}", report.tools.join(", ")),
        format!("Resources: {}", report.resource_templates.join(", ")),
        format!("Prompts: {}", report.prompts.join(", ")),
    ];
    if let Some(error) = &report.service_error {
        lines.push(format!("Service error: {error}"));
    }
    if let Some(error) = &report.project_error {
        lines.push(format!("Project error: {error}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};

    #[test]
    fn exposes_expected_tools() {
        let names = tool_definitions()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>();
        assert!(names.contains(&TOOL_MEMORY_QUERY.to_string()));
        assert!(names.contains(&TOOL_MEMORY_SEARCH_ALL.to_string()));
        assert!(names.contains(&TOOL_MEMORY_LIST_REPLACEMENT_PROPOSALS.to_string()));
        assert!(names.contains(&TOOL_MEMORY_LOOP_LIST.to_string()));
        assert!(names.contains(&TOOL_MEMORY_LOOP_ENABLE.to_string()));
        assert!(names.contains(&TOOL_MEMORY_LOOP_SET_GLOBAL_KILL_SWITCH.to_string()));
        assert!(!names.iter().any(|name| name.contains("remember")));
    }

    #[test]
    fn query_schema_requires_question_not_project() {
        let query = tool_definitions()
            .into_iter()
            .find(|tool| tool.name == TOOL_MEMORY_QUERY)
            .expect("query tool exists");
        let required = query
            .schema_as_json_value()
            .get("required")
            .cloned()
            .unwrap_or_default();
        assert_eq!(required, json!(["question"]));
    }

    #[test]
    fn search_all_schema_requires_question_not_project() {
        let query = tool_definitions()
            .into_iter()
            .find(|tool| tool.name == TOOL_MEMORY_SEARCH_ALL)
            .expect("search all tool exists");
        let required = query
            .schema_as_json_value()
            .get("required")
            .cloned()
            .unwrap_or_default();
        assert_eq!(required, json!(["question"]));
        assert_eq!(
            query
                .schema_as_json_value()
                .pointer("/properties/types/type")
                .cloned(),
            Some(json!("array"))
        );
    }

    #[test]
    fn prompts_include_cross_project_routing_prompt() {
        let names = prompt_definitions()
            .into_iter()
            .map(|prompt| prompt.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&PROMPT_ROUTE_CROSS_PROJECT_TASK.to_string()));
    }

    #[test]
    fn loop_enable_schema_requires_loop_and_exposes_approval_flag() {
        let enable = tool_definitions()
            .into_iter()
            .find(|tool| tool.name == TOOL_MEMORY_LOOP_ENABLE)
            .expect("loop enable tool exists");
        let schema = enable.schema_as_json_value();
        assert_eq!(
            schema.get("required").cloned().unwrap_or_default(),
            json!(["loop_id"])
        );
        assert_eq!(
            schema.pointer("/properties/explicit_user_approval/type"),
            Some(&json!("boolean"))
        );
    }

    #[test]
    fn parses_resource_uris() {
        assert_eq!(
            parse_resource_uri("memory://projects/memory/overview").unwrap(),
            MemoryResource::ProjectOverview {
                project: "memory".to_string()
            }
        );
        assert_eq!(
            parse_resource_uri("memory://memories/11111111-1111-1111-1111-111111111111/history")
                .unwrap(),
            MemoryResource::MemoryHistory {
                memory_id: "11111111-1111-1111-1111-111111111111".to_string()
            }
        );
        assert!(parse_resource_uri("file:///tmp/nope").is_err());
    }

    #[test]
    fn resolves_stdio_default_before_cwd() {
        let server = MemoryMcpServer::new(
            MemoryApiClient::from_parts("http://127.0.0.1:1", "t"),
            ProjectResolutionMode::Stdio {
                default_project: Some("explicit-default".to_string()),
                cwd_project: Some("cwd".to_string()),
            },
        );
        assert_eq!(server.resolve_project(None).unwrap(), "explicit-default");
        assert_eq!(server.resolve_project(Some("tool")).unwrap(), "tool");
    }

    #[test]
    fn http_requires_project() {
        let server = MemoryMcpServer::new(
            MemoryApiClient::from_parts("http://127.0.0.1:1", "t"),
            ProjectResolutionMode::Http,
        );
        assert!(server.resolve_project(None).is_err());
        assert_eq!(server.resolve_project(Some("memory")).unwrap(), "memory");
    }

    #[tokio::test]
    async fn memory_query_tool_uses_http_backend() {
        let app = Router::new().route(
            "/v1/query",
            post(|| async {
                Json(json!({
                    "answer": "Use the MCP adapter.",
                    "confidence": 0.9,
                    "results": [{
                        "memory_id": "11111111-1111-1111-1111-111111111111",
                        "summary": "MCP implementation",
                        "memory_type": "implementation",
                        "score": 1.0,
                        "snippet": "MCP adapter uses the service API.",
                        "match_kind": "lexical",
                        "tags": [],
                        "sources": []
                    }],
                    "insufficient_evidence": false,
                    "answer_generation": {
                        "method": "deterministic",
                        "cited_result_numbers": [1],
                        "evidence_count": 1,
                        "duration_ms": 1
                    },
                    "answer_citations": [{
                        "result_number": 1,
                        "memory_id": "11111111-1111-1111-1111-111111111111",
                        "memory_type": "implementation",
                        "summary": "MCP implementation",
                        "snippet": "MCP adapter uses the service API."
                    }],
                    "diagnostics": {}
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let server = MemoryMcpServer::new(
            MemoryApiClient::from_parts(format!("http://{addr}"), "token"),
            ProjectResolutionMode::Http,
        );
        let mut args = Map::new();
        args.insert("project".to_string(), json!("memory"));
        args.insert("question".to_string(), json!("How is MCP wired?"));

        let result = server.tool_query(args).await.unwrap();

        handle.abort();
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/response/answer"))
                .and_then(Value::as_str),
            Some("Use the MCP adapter.")
        );
    }

    #[tokio::test]
    async fn memory_search_all_tool_returns_project_routing_metadata() {
        let app = Router::new().route(
            "/v1/query/global",
            post(|| async {
                Json(json!({
                    "answer": "Use memories from the correct repository.",
                    "confidence": 0.9,
                    "results": [{
                        "memory_id": "11111111-1111-1111-1111-111111111111",
                        "project": "memory",
                        "project_name": "Memory Layer",
                        "repo_root": "/home/olivier/Projects/memory",
                        "summary": "MCP global search",
                        "memory_type": "implementation",
                        "score": 1.0,
                        "snippet": "Global search includes project routing metadata.",
                        "match_kind": "lexical",
                        "tags": [],
                        "sources": []
                    }],
                    "insufficient_evidence": false,
                    "answer_generation": {
                        "method": "deterministic",
                        "cited_result_numbers": [1],
                        "evidence_count": 1,
                        "duration_ms": 1
                    },
                    "answer_citations": [{
                        "result_number": 1,
                        "memory_id": "11111111-1111-1111-1111-111111111111",
                        "project": "memory",
                        "project_name": "Memory Layer",
                        "repo_root": "/home/olivier/Projects/memory",
                        "memory_type": "implementation",
                        "summary": "MCP global search",
                        "snippet": "Global search includes project routing metadata."
                    }],
                    "diagnostics": {}
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let server = MemoryMcpServer::new(
            MemoryApiClient::from_parts(format!("http://{addr}"), "token"),
            ProjectResolutionMode::Http,
        );
        let mut args = Map::new();
        args.insert("question".to_string(), json!("Where is global MCP search?"));
        args.insert("types".to_string(), json!(["implementation"]));

        let result = server.tool_search_all(args).await.unwrap();

        handle.abort();
        assert_eq!(result.is_error, Some(false));
        assert!(
            result
                .content
                .iter()
                .filter_map(|content| content.as_text())
                .any(|content| content.text.contains("project=memory")
                    && content
                        .text
                        .contains("repo_root=/home/olivier/Projects/memory"))
        );
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/response/results/0/project"))
                .and_then(Value::as_str),
            Some("memory")
        );
    }
}
