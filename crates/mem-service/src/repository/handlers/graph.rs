// SPDX-License-Identifier: AGPL-3.0-or-later

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

    let repository = mem_graph::PostgresGraphRepository::new(state.pool()?.clone());
    Ok(Json(
        repository
            .visualization_graph(&slug, filters)
            .await
            .map_err(ApiError::io)?,
    ))
}
