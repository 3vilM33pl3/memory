use crate::prelude::*;
use crate::*;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProjectGraphParams {
    run_id: Option<Uuid>,
    q: Option<String>,
    file_path: Option<String>,
    symbol: Option<String>,
    edge_kind: Option<String>,
    depth: Option<u8>,
    limit_nodes: Option<usize>,
    limit_edges: Option<usize>,
}

impl From<ProjectGraphParams> for CodeGraphViewRequest {
    fn from(params: ProjectGraphParams) -> Self {
        Self {
            run_id: params.run_id,
            q: params.q,
            file_path: params.file_path,
            symbol: params.symbol,
            edge_kind: params.edge_kind,
            depth: params.depth,
            limit_nodes: params.limit_nodes,
            limit_edges: params.limit_edges,
        }
    }
}

pub(crate) async fn project_graph_status(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<CodeGraphStatusResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &format!("/v1/projects/{slug}/graph/status")).await?,
        ));
    }

    let repository = mem_graph::PostgresGraphRepository::new(state.pool()?.clone());
    Ok(Json(
        repository
            .status_response(&slug)
            .await
            .map_err(ApiError::io)?,
    ))
}

pub(crate) async fn project_graph(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(params): Query<ProjectGraphParams>,
) -> Result<Json<CodeGraphResponse>, ApiError> {
    let request: CodeGraphViewRequest = params.into();
    let filters = request.normalize();
    if !state.is_primary() {
        return Ok(Json(
            proxy_get_json(&state, &project_graph_path(&slug, &filters)).await?,
        ));
    }

    let repository = mem_graph::PostgresGraphRepository::new(state.pool()?.clone());
    Ok(Json(
        repository
            .visualization_graph(&slug, filters)
            .await
            .map_err(ApiError::io)?,
    ))
}

fn project_graph_path(slug: &str, filters: &CodeGraphViewFilters) -> String {
    let mut params = Vec::new();
    if let Some(run_id) = filters.run_id {
        params.push(format!("run_id={run_id}"));
    }
    if let Some(q) = &filters.q {
        params.push(format!("q={}", urlencoding::encode(q)));
    }
    if let Some(file_path) = &filters.file_path {
        params.push(format!("file_path={}", urlencoding::encode(file_path)));
    }
    if let Some(symbol) = &filters.symbol {
        params.push(format!("symbol={}", urlencoding::encode(symbol)));
    }
    if let Some(edge_kind) = &filters.edge_kind {
        params.push(format!("edge_kind={}", urlencoding::encode(edge_kind)));
    }
    params.push(format!("depth={}", filters.depth));
    params.push(format!("limit_nodes={}", filters.limit_nodes));
    params.push(format!("limit_edges={}", filters.limit_edges));

    format!("/v1/projects/{slug}/graph?{}", params.join("&"))
}
