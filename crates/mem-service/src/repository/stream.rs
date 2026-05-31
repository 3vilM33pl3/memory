use crate::prelude::*;
use crate::*;

pub(crate) struct ProtoServers {
    #[cfg(unix)]
    pub(crate) unix_listener: UnixListener,
    pub(crate) tcp_listener: TcpListener,
}

pub(crate) async fn start_proto_servers(state: AppState) -> Result<ProtoServers> {
    #[cfg(unix)]
    let unix_listener = {
        let unix_path = PathBuf::from(&state.config.service.capnp_unix_socket);
        if let Some(parent) = unix_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create {}", parent.display()))?;
        }
        if unix_path.exists() {
            tokio::fs::remove_file(&unix_path)
                .await
                .with_context(|| format!("remove stale socket {}", unix_path.display()))?;
        }

        UnixListener::bind(&unix_path)
            .with_context(|| format!("bind unix socket {}", unix_path.display()))?
    };

    let tcp_addr: SocketAddr = state
        .config
        .service
        .capnp_tcp_addr
        .parse()
        .context("parse capnp tcp addr")?;
    let tcp_listener = bind_tcp_listener_with_addr_in_use_wait(tcp_addr, "capnp tcp addr")
        .await
        .context("bind capnp tcp addr")?;

    Ok(ProtoServers {
        #[cfg(unix)]
        unix_listener,
        tcp_listener,
    })
}

#[cfg(unix)]
pub(crate) async fn run_proto_unix(listener: UnixListener, state: AppState) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_proto_connection(stream, state.clone()));
    }
}

pub(crate) async fn run_proto_tcp(listener: TcpListener, state: AppState) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_proto_connection(stream, state.clone()));
    }
}

#[derive(Default)]
pub(crate) struct ConnectionSubscriptions {
    project: Option<String>,
    memory_id: Option<Uuid>,
}

pub(crate) async fn websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if state.is_primary() {
            handle_websocket_connection(socket, state).await;
        } else if let Err(error) = bridge_relay_websocket(socket, state).await {
            tracing::warn!(error = %error, "relay websocket bridge failed");
        }
    })
}

pub(crate) async fn handle_websocket_connection(mut socket: WebSocket, state: AppState) {
    let mut subscriptions = ConnectionSubscriptions::default();
    let mut events = state.events.subscribe();

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(result) = incoming else {
                    break;
                };
                match result {
                    Ok(Message::Text(text)) => {
                        let request = match serde_json::from_str::<StreamRequest>(&text) {
                            Ok(request) => request,
                            Err(error) => {
                                if send_ws_response(
                                    &mut socket,
                                    StreamResponse::Error {
                                        message: format!("parse stream request: {error}"),
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                        };
                        match process_stream_request(&state, &mut subscriptions, request).await {
                            Ok(responses) => {
                                for response in responses {
                                    if send_ws_response(&mut socket, response).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(error) => {
                                if send_ws_response(
                                    &mut socket,
                                    StreamResponse::Error {
                                        message: error.to_string(),
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            event = events.recv() => {
                let Ok(event) = event else {
                    continue;
                };
                match render_subscription_updates(&state, &subscriptions, &event).await {
                    Ok(responses) => {
                        for response in responses {
                            if send_ws_response(&mut socket, response).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        if send_ws_response(
                            &mut socket,
                            StreamResponse::Error {
                                message: error.to_string(),
                            },
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
    }
}

pub(crate) async fn bridge_relay_websocket(socket: WebSocket, state: AppState) -> Result<()> {
    let upstream = selected_primary_peer(&state)
        .ok_or_else(|| anyhow::anyhow!("no primary available for relay websocket"))?;
    let mut request = format!("ws://{}/ws", upstream.advertise_addr).into_client_request()?;
    request.headers_mut().insert(
        "x-api-token",
        state
            .api_token
            .parse()
            .context("parse relay api token header")?,
    );
    let (upstream_stream, _) = connect_async(request)
        .await
        .context("connect upstream websocket")?;

    let (mut client_sender, mut client_receiver) = socket.split();
    let (mut upstream_sender, mut upstream_receiver) = upstream_stream.split();

    let client_to_upstream = async {
        while let Some(message) = client_receiver.next().await {
            let message = message?;
            let mapped = match message {
                Message::Text(text) => {
                    tokio_tungstenite::tungstenite::Message::Text(text.to_string())
                }
                Message::Binary(binary) => {
                    tokio_tungstenite::tungstenite::Message::Binary(binary.to_vec())
                }
                Message::Ping(payload) => {
                    tokio_tungstenite::tungstenite::Message::Ping(payload.to_vec())
                }
                Message::Pong(payload) => {
                    tokio_tungstenite::tungstenite::Message::Pong(payload.to_vec())
                }
                Message::Close(frame) => {
                    let close =
                        frame.map(
                            |frame| tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code: frame.code.into(),
                                reason: frame.reason.to_string().into(),
                            },
                        );
                    tokio_tungstenite::tungstenite::Message::Close(close)
                }
            };
            upstream_sender.send(mapped).await?;
        }
        Result::<(), anyhow::Error>::Ok(())
    };

    let upstream_to_client = async {
        while let Some(message) = upstream_receiver.next().await {
            let message = message?;
            let mapped = match message {
                tokio_tungstenite::tungstenite::Message::Text(text) => Message::Text(text.into()),
                tokio_tungstenite::tungstenite::Message::Binary(binary) => {
                    Message::Binary(binary.into())
                }
                tokio_tungstenite::tungstenite::Message::Ping(payload) => {
                    Message::Ping(payload.into())
                }
                tokio_tungstenite::tungstenite::Message::Pong(payload) => {
                    Message::Pong(payload.into())
                }
                tokio_tungstenite::tungstenite::Message::Close(frame) => {
                    let close = frame.map(|frame| axum::extract::ws::CloseFrame {
                        code: frame.code.into(),
                        reason: frame.reason.to_string().into(),
                    });
                    Message::Close(close)
                }
                tokio_tungstenite::tungstenite::Message::Frame(_) => continue,
            };
            client_sender.send(mapped).await?;
        }
        Result::<(), anyhow::Error>::Ok(())
    };

    tokio::select! {
        result = client_to_upstream => result?,
        result = upstream_to_client => result?,
    }

    Ok(())
}

pub(crate) async fn send_ws_response(
    socket: &mut WebSocket,
    response: StreamResponse,
) -> Result<()> {
    socket
        .send(Message::Text(serde_json::to_string(&response)?.into()))
        .await
        .context("send websocket response")?;
    Ok(())
}

pub(crate) async fn handle_proto_connection<S>(stream: S, state: AppState) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut subscriptions = ConnectionSubscriptions::default();
    let mut events = state.events.subscribe();

    loop {
        tokio::select! {
            incoming = read_capnp_text_frame(&mut reader) => {
                let Some(text) = incoming? else {
                    break;
                };
                let request: StreamRequest = serde_json::from_str(&text)
                    .map_err(|error| anyhow::anyhow!("parse stream request: {error}"))?;
                for response in process_stream_request(&state, &mut subscriptions, request).await? {
                    let text = serde_json::to_string(&response)?;
                    write_capnp_text_frame(&mut writer, &text).await?;
                }
            }
            event = events.recv() => {
                let Ok(event) = event else {
                    continue;
                };
                for response in render_subscription_updates(&state, &subscriptions, &event).await? {
                    let text = serde_json::to_string(&response)?;
                    write_capnp_text_frame(&mut writer, &text).await?;
                }
            }
        }
    }

    Ok(())
}

pub(crate) async fn process_stream_request(
    state: &AppState,
    subscriptions: &mut ConnectionSubscriptions,
    request: StreamRequest,
) -> Result<Vec<StreamResponse>> {
    let pool = state
        .pool
        .as_ref()
        .context("primary state missing database pool")?;
    let mut responses = Vec::new();
    match request {
        StreamRequest::Health => responses.push(StreamResponse::Health {
            value: health_payload(state).await?,
        }),
        StreamRequest::ProjectOverview { project } => {
            responses.push(StreamResponse::ProjectOverview {
                value: fetch_project_overview_with_watchers(state, &project).await?,
            });
        }
        StreamRequest::ProjectMemories { project } => {
            responses.push(StreamResponse::ProjectMemories {
                value: fetch_project_memories(pool, &project, None, 500, 0).await?,
            });
        }
        StreamRequest::MemoryDetail { memory_id } => {
            responses.push(StreamResponse::MemoryDetail {
                value: fetch_memory_entry(pool, memory_id).await?,
            });
        }
        StreamRequest::SubscribeProject { project } => {
            subscriptions.project = Some(project.clone());
            let overview = fetch_project_overview_with_watchers(state, &project).await?;
            let memories = fetch_project_memories(pool, &project, None, 500, 0).await?;
            responses.push(StreamResponse::ProjectSnapshot { overview, memories });
            responses.extend(recent_activity_responses(&state.recent_activity, &project).await);
        }
        StreamRequest::SubscribeMemory { memory_id } => {
            subscriptions.memory_id = Some(memory_id);
            let detail = fetch_memory_entry(pool, memory_id).await?;
            responses.push(StreamResponse::MemorySnapshot { detail });
        }
        StreamRequest::UnsubscribeMemory => {
            subscriptions.memory_id = None;
            responses.push(StreamResponse::Ack {
                message: "memory subscription cleared".to_string(),
            });
        }
        StreamRequest::Ping => responses.push(StreamResponse::Pong),
    }
    Ok(responses)
}

pub(crate) async fn render_subscription_updates(
    state: &AppState,
    subscriptions: &ConnectionSubscriptions,
    event: &ServiceEvent,
) -> Result<Vec<StreamResponse>> {
    let pool = state
        .pool
        .as_ref()
        .context("primary state missing database pool")?;
    let mut responses = Vec::new();
    if let Some(project) = &subscriptions.project
        && project == &event.project
    {
        if event.include_activity {
            responses.push(stream_activity_response(event.clone()));
        }
        let overview = fetch_project_overview_with_watchers(state, project).await?;
        let memories = fetch_project_memories(pool, project, None, 500, 0).await?;
        responses.push(StreamResponse::ProjectChanged { overview, memories });
    }

    if let Some(memory_id) = subscriptions.memory_id
        && event.memory_id == Some(memory_id)
    {
        let detail = fetch_memory_entry(pool, memory_id).await?;
        responses.push(StreamResponse::MemoryChanged { detail });
    }

    Ok(responses)
}

pub(crate) async fn recent_activity_responses(
    recent_activity: &Mutex<VecDeque<ServiceEvent>>,
    project: &str,
) -> Vec<StreamResponse> {
    let history = recent_activity
        .lock()
        .expect("activity history mutex poisoned");
    history
        .iter()
        .filter(|event| event.project == project)
        .cloned()
        .map(stream_activity_response)
        .collect()
}

pub(crate) async fn health_payload(state: &AppState) -> Result<serde_json::Value> {
    if state.is_primary() {
        let pool = state
            .pool
            .as_ref()
            .context("primary state missing database pool")?;
        sqlx::query("SELECT 1").execute(pool).await?;
        Ok(serde_json::json!({
            "status": "ok",
            "role": "primary",
            "database": "up",
            "instance_id": state.instance_id,
            "service_id": state.config.cluster.service_id,
            "version": state.config.profile.display_version(env!("CARGO_PKG_VERSION"))
        }))
    } else {
        let upstream = relay_upstream_health(state).await?;
        Ok(serde_json::json!({
            "status": if upstream.is_some() { "ok" } else { "degraded" },
            "role": "relay",
            "database": "down",
            "instance_id": state.instance_id,
            "service_id": state.config.cluster.service_id,
            "version": state.config.profile.display_version(env!("CARGO_PKG_VERSION")),
            "upstream": upstream
        }))
    }
}

pub(crate) async fn relay_upstream_health(state: &AppState) -> Result<Option<serde_json::Value>> {
    let Some(peer) = selected_primary_peer(state) else {
        return Ok(None);
    };
    let health = state
        .http_client
        .get(format!("http://{}/healthz", peer.advertise_addr))
        .send()
        .await;
    let status = match health {
        Ok(response) => {
            let code = response.status();
            let body = response
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({}));
            serde_json::json!({
                "service_id": peer.service_id,
                "address": peer.advertise_addr,
                "version": peer.version,
                "status": code.as_u16(),
                "health": body
            })
        }
        Err(error) => serde_json::json!({
            "service_id": peer.service_id,
            "address": peer.advertise_addr,
            "version": peer.version,
            "status": "unreachable",
            "error": error.to_string()
        }),
    };
    Ok(Some(status))
}

pub(crate) fn relay_target(state: &AppState) -> Result<ClusterPeer, ApiError> {
    selected_primary_peer(state).ok_or_else(|| {
        ApiError::service_unavailable("no primary memory service available on the local network")
    })
}

pub(crate) async fn proxy_get_json<T: serde::de::DeserializeOwned>(
    state: &AppState,
    path: &str,
) -> Result<T, ApiError> {
    let peer = relay_target(state)?;
    let response = state
        .http_client
        .get(format!("http://{}{}", peer.advertise_addr, path))
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    parse_proxy_json(response).await
}

pub(crate) async fn proxy_post_json<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
    state: &AppState,
    path: &str,
    request: &Req,
    include_token: bool,
) -> Result<Resp, ApiError> {
    let peer = relay_target(state)?;
    let mut builder = state
        .http_client
        .post(format!("http://{}{}", peer.advertise_addr, path));
    if include_token {
        builder = builder.header("x-api-token", &state.api_token);
    }
    let response = builder
        .json(request)
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    parse_proxy_json(response).await
}

pub(crate) async fn proxy_delete_json<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
    state: &AppState,
    path: &str,
    request: &Req,
) -> Result<Resp, ApiError> {
    let peer = relay_target(state)?;
    let response = state
        .http_client
        .delete(format!("http://{}{}", peer.advertise_addr, path))
        .header("x-api-token", &state.api_token)
        .json(request)
        .send()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    parse_proxy_json(response).await
}

pub(crate) async fn parse_proxy_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ApiError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| ApiError::io(error.into()))?;
    if !status.is_success() {
        return Err(ApiError::status_message(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            if body.trim().is_empty() {
                format!("upstream request failed with {status}")
            } else {
                body
            },
        ));
    }
    serde_json::from_str(&body).map_err(|error| ApiError::io(error.into()))
}
