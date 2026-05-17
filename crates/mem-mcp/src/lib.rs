use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mem_api::{
    ActivityListResponse, AppConfig, MemoryEntryResponse, MemoryHistoryResponse,
    ProjectMemoriesResponse, ProjectOverviewResponse, QueryAnswerMode, QueryFilters, QueryRequest,
    QueryResponse, ReplacementProposalListResponse, ResumeCheckpoint, ResumeRequest,
    ResumeResponse, UpToSpeedRequest, UpToSpeedResponse, read_repo_project_slug,
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
const TOOL_MEMORY_RESUME: &str = "memory_resume";
const TOOL_MEMORY_UP_TO_SPEED: &str = "memory_up_to_speed";
const TOOL_MEMORY_OVERVIEW: &str = "memory_overview";
const TOOL_MEMORY_LIST_MEMORIES: &str = "memory_list_memories";
const TOOL_MEMORY_GET_MEMORY: &str = "memory_get_memory";
const TOOL_MEMORY_MEMORY_HISTORY: &str = "memory_memory_history";
const TOOL_MEMORY_LIST_ACTIVITIES: &str = "memory_list_activities";
const TOOL_MEMORY_LIST_REPLACEMENT_PROPOSALS: &str = "memory_list_replacement_proposals";

const PROMPT_GET_UP_TO_SPEED: &str = "memory_get_up_to_speed";
const PROMPT_ANSWER_WITH_CONTEXT: &str = "memory_answer_with_context";

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
            "Read-only Memory Layer MCP adapter. Use tools/resources to query project memory; write operations are intentionally not exposed.",
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
            history: args.history.unwrap_or(false),
            retrieval_mode: None,
            answer_mode: args.answer_mode,
        };
        request.validate().map_err(validation)?;
        let response = self.client.query(&request).await.map_err(api_error)?;
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
    history: Option<bool>,
    answer_mode: Option<QueryAnswerMode>,
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

fn from_args<T: serde::de::DeserializeOwned>(arguments: Map<String, Value>) -> Result<T, McpError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|error| invalid_params(format!("invalid tool arguments: {error}")))
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
                "- [{}] {} ({})",
                citation.result_number, citation.summary, citation.memory_id
            ));
        }
    }
    if !response.results.is_empty() {
        lines.push(String::new());
        lines.push("Result summaries:".to_string());
        for (idx, result) in response.results.iter().enumerate() {
            lines.push(format!(
                "- [{}] {} [{}] {}",
                idx + 1,
                result.summary,
                result.match_kind,
                result.memory_id
            ));
        }
    }
    lines.join("\n")
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
                optional_boolean("history", "Search historical memory versions."),
                optional_enum("answer_mode", &["auto", "deterministic", "llm"]),
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

#[derive(Debug, Clone)]
struct SchemaField {
    name: &'static str,
    schema: Value,
    required: bool,
}

fn required_string(name: &'static str, description: &'static str) -> SchemaField {
    field(name, "string", description, true)
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
    fn exposes_expected_read_only_tools() {
        let names = tool_definitions()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>();
        assert!(names.contains(&TOOL_MEMORY_QUERY.to_string()));
        assert!(names.contains(&TOOL_MEMORY_LIST_REPLACEMENT_PROPOSALS.to_string()));
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
}
