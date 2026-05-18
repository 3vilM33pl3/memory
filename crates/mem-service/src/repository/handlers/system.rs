use crate::prelude::*;
use crate::*;

pub(crate) async fn healthz(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(health_payload(&state).await.map_err(ApiError::io)?))
}

#[derive(Debug, Serialize)]
pub(crate) struct WebAuthTokenResponse {
    pub(crate) api_token: String,
    pub(crate) header: &'static str,
}

pub(crate) async fn web_auth_token(
    State(state): State<AppState>,
) -> Result<Json<WebAuthTokenResponse>, ApiError> {
    Ok(Json(WebAuthTokenResponse {
        api_token: state.api_token,
        header: "x-api-token",
    }))
}

pub(crate) async fn admin_shutdown(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_strict_token(&headers, &state.api_token)?;
    request_runtime_shutdown(&state.shutdown);
    Ok(Json(serde_json::json!({
        "accepted": true,
        "message": "shutdown requested"
    })))
}

pub(crate) async fn web_unavailable() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(
            r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Memory Layer Web UI unavailable</title>
    <style>
      body { font-family: ui-sans-serif, system-ui, sans-serif; background: #0f1722; color: #e6edf5; margin: 0; }
      main { max-width: 760px; margin: 8rem auto; padding: 2rem; background: #182233; border: 1px solid #42506a; border-radius: 18px; }
      code { color: #ffd17d; }
      h1 { margin-top: 0; }
      p { line-height: 1.6; }
    </style>
  </head>
  <body>
    <main>
      <h1>Memory Layer Web UI is not installed</h1>
      <p><code>mem-service</code> is running, but it could not find built web assets.</p>
      <p>Build the frontend under <code>web/</code> or install a package that ships <code>share/memory-layer/web</code>.</p>
    </main>
  </body>
</html>"#,
        ),
    )
}

pub(crate) async fn stats(State(state): State<AppState>) -> Result<Json<StatsResponse>, ApiError> {
    if !state.is_primary() {
        return Ok(Json(proxy_get_json(&state, "/v1/stats").await?));
    }
    let pool = state.pool()?;
    let counts = [
        ("projects", "SELECT COUNT(*) AS count FROM projects"),
        ("sessions", "SELECT COUNT(*) AS count FROM sessions"),
        ("tasks", "SELECT COUNT(*) AS count FROM tasks"),
        ("raw_captures", "SELECT COUNT(*) AS count FROM raw_captures"),
        (
            "memory_entries",
            "SELECT COUNT(*) AS count FROM memory_entries",
        ),
        (
            "curation_runs",
            "SELECT COUNT(*) AS count FROM curation_runs",
        ),
    ];

    let mut values = Vec::with_capacity(counts.len());
    for (_, sql) in counts {
        let row = sqlx::query(sql)
            .fetch_one(pool)
            .await
            .map_err(ApiError::sql)?;
        values.push(row.try_get::<i64, _>("count").map_err(ApiError::sql)?);
    }

    Ok(Json(StatsResponse {
        projects: values[0],
        sessions: values[1],
        tasks: values[2],
        raw_captures: values[3],
        memory_entries: values[4],
        curation_runs: values[5],
    }))
}
