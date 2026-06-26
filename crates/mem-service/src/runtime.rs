use crate::prelude::*;
use crate::*;

const HTTP_SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(2);

pub async fn run_service(config_path: Option<PathBuf>) -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let mut config_fingerprint = config_path_fingerprint(config_path.as_deref())
        .await
        .context("inspect config file")?;

    loop {
        let config = AppConfig::load_from_path(config_path.clone()).context("load config")?;
        let addr: SocketAddr = config
            .service
            .bind_addr
            .parse()
            .context("parse bind_addr")?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let shutdown_handle = Arc::new(Mutex::new(Some(shutdown_tx)));
        let state = build_state(config.clone(), shutdown_handle.clone()).await?;
        let app = routes::build_http_app(state.clone());
        let listener = bind_http_listener_with_handoff(&config, addr)
            .await
            .context("bind http listener")?;
        let mut http_server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
        });
        let mut proto_tasks = Vec::new();
        if state.is_primary() {
            let proto_servers = start_proto_servers(state.clone()).await?;
            #[cfg(unix)]
            proto_tasks.push(tokio::spawn(run_proto_unix(
                proto_servers.unix_listener,
                state.clone(),
            )));
            proto_tasks.push(tokio::spawn(run_proto_tcp(
                proto_servers.tcp_listener,
                state.clone(),
            )));
        }
        let mut cluster_tasks = start_cluster_tasks(state.clone()).await?;

        let version = config.profile.display_version(env!("CARGO_PKG_VERSION"));
        eprintln!(
            "memory-layer v{version} listening on {addr} (profile={profile}, role={role}, service_id={service_id}, cluster={cluster})",
            profile = config.profile,
            role = state.role_name(),
            service_id = config.cluster.service_id,
            cluster = if config.cluster.enabled {
                "enabled"
            } else {
                "disabled"
            },
        );
        if let Some(path) = config.resolved_config_path.as_deref() {
            eprintln!("  config: {}", path.display());
        }
        if let Some(path) = config.resolved_dev_overlay_path.as_deref() {
            eprintln!("  dev overlay: {}", path.display());
        }
        eprintln!(
            "  database: {}",
            if state.pool.is_some() {
                "connected"
            } else {
                "unavailable"
            }
        );
        eprintln!("  capnp unix: {}", config.service.capnp_unix_socket);
        eprintln!("  capnp tcp: {}", config.service.capnp_tcp_addr);

        tracing::info!(
            %addr,
            role = %state.role_name(),
            unix_socket = %config.service.capnp_unix_socket,
            tcp_addr = %config.service.capnp_tcp_addr,
            "memory-layer listening"
        );

        if let Some(path) = config_path.as_deref() {
            tokio::select! {
                result = &mut http_server => {
                    result.context("join mem-service task")??;
                    break;
                }
                result = tokio::signal::ctrl_c() => {
                    result.context("listen for ctrl-c")?;
                    shutdown_http_server(&shutdown_handle, &mut http_server, "ctrl-c").await?;
                    abort_tasks(&mut proto_tasks);
                    abort_tasks(&mut cluster_tasks);
                    break;
                }
                result = wait_for_config_change(path, config_fingerprint) => {
                    config_fingerprint = result.context("watch config file")?;
                    tracing::info!(path = %path.display(), "config changed; restarting backend");
                    shutdown_http_server(&shutdown_handle, &mut http_server, "config-reload").await?;
                    abort_tasks(&mut proto_tasks);
                    abort_tasks(&mut cluster_tasks);
                }
            }
        } else {
            tokio::select! {
                result = &mut http_server => {
                    result.context("join mem-service task")??;
                    break;
                }
                result = tokio::signal::ctrl_c() => {
                    result.context("listen for ctrl-c")?;
                    shutdown_http_server(&shutdown_handle, &mut http_server, "ctrl-c").await?;
                    abort_tasks(&mut proto_tasks);
                    abort_tasks(&mut cluster_tasks);
                    break;
                }
            }
        }
    }

    Ok(())
}

pub(crate) async fn build_state(
    config: AppConfig,
    shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
) -> Result<AppState> {
    let http_client = reqwest::Client::builder()
        .timeout(config.service.request_timeout)
        .build()
        .context("build service http client")?;
    let pool_attempt = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database.url)
        .await;
    let (events, _) = broadcast::channel(128);
    let (role, pool) = match pool_attempt {
        Ok(pool) => {
            let mut migrator = sqlx::migrate!("../../migrations");
            if config.profile == mem_api::Profile::Dev {
                migrator.set_ignore_missing(true);
            }
            migrator.run(&pool).await.context(
                "run migrations (pgvector extension 'vector' must be installed in PostgreSQL)",
            )?;
            register_builtin_loop_definitions(&pool)
                .await
                .context("register builtin loop definitions")?;
            (ServiceRole::Primary, Some(pool))
        }
        Err(error) if config.cluster.enabled => {
            tracing::warn!(
                error = %error,
                "postgres unavailable; starting in relay mode"
            );
            (ServiceRole::Relay, None)
        }
        Err(error) => return Err(error).context("connect postgres"),
    };

    let embedders = Arc::new(tokio::sync::RwLock::new(EmbeddingRegistry::from_config(
        &config.embeddings,
    )));
    let automated_embedding_creation_enabled =
        Arc::new(AtomicBool::new(config.embeddings.create_enabled));
    let llm_audit = Arc::new(RwLock::new(config.llm_audit.clone()));

    Ok(AppState {
        role,
        instance_id: Uuid::new_v4().to_string(),
        startup_at: chrono::Utc::now(),
        pool,
        api_token: config.service.api_token.clone(),
        web_root: discover_web_root(&config),
        http_client,
        embedders,
        automated_embedding_creation_enabled,
        llm_audit,
        config,
        events,
        recent_activity: Arc::new(Mutex::new(VecDeque::with_capacity(20))),
        watchers: Arc::new(Mutex::new(HashMap::new())),
        provenance: Arc::new(Mutex::new(ProvenanceRuntimeState {
            status: "idle".to_string(),
            ..ProvenanceRuntimeState::default()
        })),
        cluster: ClusterRuntime {
            peers: Arc::new(Mutex::new(HashMap::new())),
        },
        shutdown,
    })
}

pub(crate) fn discover_web_root(config: &AppConfig) -> Option<PathBuf> {
    if let Some(root) = &config.service.web_root {
        let path = PathBuf::from(root);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }

    let candidates = vec![
        Some(PathBuf::from("web").join("dist")),
        mem_platform::current_exe_share_subdir("web"),
        mem_platform::preferred_user_state_dir().map(|dir| dir.join("web")),
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".local/share/memory-layer/web")),
        Some(PathBuf::from("/usr/share/memory-layer/web")),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.join("index.html").is_file())
}

pub(crate) fn abort_tasks(tasks: &mut Vec<JoinHandle<Result<()>>>) {
    for task in tasks.drain(..) {
        task.abort();
    }
}

pub(crate) fn request_runtime_shutdown(shutdown: &Arc<Mutex<Option<oneshot::Sender<()>>>>) {
    if let Some(sender) = shutdown.lock().expect("shutdown mutex poisoned").take() {
        let _ = sender.send(());
    }
}

pub(crate) async fn shutdown_http_server(
    shutdown: &Arc<Mutex<Option<oneshot::Sender<()>>>>,
    http_server: &mut JoinHandle<std::io::Result<()>>,
    reason: &str,
) -> Result<()> {
    shutdown_http_server_with_timeout(shutdown, http_server, HTTP_SHUTDOWN_GRACE_PERIOD, reason)
        .await
}

pub(crate) async fn shutdown_http_server_with_timeout(
    shutdown: &Arc<Mutex<Option<oneshot::Sender<()>>>>,
    http_server: &mut JoinHandle<std::io::Result<()>>,
    grace_period: Duration,
    reason: &str,
) -> Result<()> {
    request_runtime_shutdown(shutdown);
    tokio::select! {
        result = &mut *http_server => {
            result.context("join mem-service task")??;
        }
        _ = tokio::time::sleep(grace_period) => {
            tracing::warn!(
                reason,
                grace_period_ms = grace_period.as_millis(),
                "HTTP server did not stop during graceful shutdown window; aborting"
            );
            http_server.abort();
            match http_server.await {
                Ok(result) => result.context("run mem-service task after forced shutdown")?,
                Err(error) if error.is_cancelled() => {}
                Err(error) => return Err(error).context("join mem-service task after forced shutdown"),
            }
        }
    }
    Ok(())
}

pub(crate) async fn bind_http_listener_with_handoff(
    config: &AppConfig,
    addr: SocketAddr,
) -> Result<TcpListener> {
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            if !request_existing_instance_shutdown(config).await? {
                return Err(error).context("address already in use and handoff was refused");
            }
            wait_for_listener_release(addr).await?;
            bind_tcp_listener_with_addr_in_use_wait(addr, "http listener after handoff").await
        }
        Err(error) => Err(error).context("bind http listener"),
    }
}

pub(crate) async fn request_existing_instance_shutdown(config: &AppConfig) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("build handoff client")?;
    let response = match client
        .post(format!(
            "http://{}/v1/admin/shutdown",
            config.service.bind_addr
        ))
        .header("x-api-token", &config.service.api_token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(error = %error, "failed to contact existing backend for handoff");
            return Ok(false);
        }
    };
    Ok(response.status().is_success())
}

pub(crate) async fn wait_for_listener_release(addr: SocketAddr) -> Result<()> {
    for _ in 0..20 {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                drop(listener);
                return Ok(());
            }
            Err(error) if error.kind() == ErrorKind::AddrInUse => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(error) => return Err(error).context("wait for listener release"),
        }
    }
    anyhow::bail!("timed out waiting for existing backend to release {addr}");
}

pub(crate) async fn bind_tcp_listener_with_addr_in_use_wait(
    addr: SocketAddr,
    context_label: &'static str,
) -> Result<TcpListener> {
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            wait_for_listener_release(addr).await?;
            tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("bind {context_label} after waiting for release"))
        }
        Err(error) => Err(error).with_context(|| format!("bind {context_label}")),
    }
}

pub(crate) async fn start_cluster_tasks(state: AppState) -> Result<Vec<JoinHandle<Result<()>>>> {
    let mut tasks = Vec::new();
    if state.is_primary() {
        tasks.push(tokio::spawn(run_watcher_watchdog(state.clone())));
        if state.config.provenance.reverify_enabled {
            tasks.push(tokio::spawn(run_provenance_reverify_scheduler(
                state.clone(),
            )));
        }
    }
    if !state.config.cluster.enabled {
        return Ok(tasks);
    }

    let socket = Arc::new(bind_cluster_socket(
        &state.config.cluster.discovery_multicast_addr,
    )?);
    tasks.push(tokio::spawn(run_cluster_listener(
        socket.clone(),
        state.clone(),
    )));
    if state.is_primary() {
        tasks.push(tokio::spawn(run_cluster_announcer(socket, state)));
    } else {
        tasks.push(tokio::spawn(run_cluster_discoverer(socket, state)));
    }
    Ok(tasks)
}

pub(crate) fn bind_cluster_socket(multicast_addr: &str) -> Result<UdpSocket> {
    let addr: SocketAddr = multicast_addr
        .parse()
        .context("parse cluster multicast addr")?;
    let ip = match addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(_) => {
            anyhow::bail!("cluster.discovery_multicast_addr must be an IPv4 multicast address")
        }
    };

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("create discovery socket")?;
    socket
        .set_reuse_address(true)
        .context("set discovery SO_REUSEADDR")?;
    #[cfg(target_vendor = "apple")]
    enable_socket_reuse_port(&socket)?;
    socket
        .bind(&SocketAddr::from(([0, 0, 0, 0], addr.port())).into())
        .context("bind discovery socket")?;
    socket
        .join_multicast_v4(&ip, &std::net::Ipv4Addr::UNSPECIFIED)
        .context("join discovery multicast group")?;
    socket
        .set_multicast_loop_v4(true)
        .context("enable multicast loopback")?;
    socket
        .set_nonblocking(true)
        .context("set discovery socket nonblocking")?;
    UdpSocket::from_std(socket.into()).context("convert discovery socket")
}

#[cfg(target_vendor = "apple")]
pub(crate) fn enable_socket_reuse_port(socket: &Socket) -> Result<()> {
    let enabled: libc::c_int = 1;
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_REUSEPORT,
            &enabled as *const _ as *const libc::c_void,
            std::mem::size_of_val(&enabled) as libc::socklen_t,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("set discovery SO_REUSEPORT")
    }
}

pub(crate) async fn run_cluster_listener(socket: Arc<UdpSocket>, state: AppState) -> Result<()> {
    let mut buffer = vec![0_u8; 4096];
    loop {
        let (len, from) = socket.recv_from(&mut buffer).await?;
        let Ok(packet) = serde_json::from_slice::<DiscoveryPacket>(&buffer[..len]) else {
            continue;
        };
        if packet.service_id == state.config.cluster.service_id {
            continue;
        }

        match packet.kind {
            DiscoveryKind::Discover if state.is_primary() => {
                let announce = build_discovery_packet(&state, DiscoveryKind::Announce);
                let payload = serde_json::to_vec(&announce)?;
                socket.send_to(&payload, from).await?;
            }
            DiscoveryKind::Announce => update_cluster_peer(&state.cluster, packet),
            DiscoveryKind::Discover => {}
        }
    }
}

pub(crate) async fn run_cluster_announcer(socket: Arc<UdpSocket>, state: AppState) -> Result<()> {
    let mut interval = tokio::time::interval(state.config.cluster.announce_interval);
    loop {
        interval.tick().await;
        let packet = build_discovery_packet(&state, DiscoveryKind::Announce);
        let payload = serde_json::to_vec(&packet)?;
        socket
            .send_to(&payload, &state.config.cluster.discovery_multicast_addr)
            .await?;
    }
}

pub(crate) async fn run_cluster_discoverer(socket: Arc<UdpSocket>, state: AppState) -> Result<()> {
    let mut interval = tokio::time::interval(state.config.cluster.announce_interval);
    loop {
        interval.tick().await;
        let packet = build_discovery_packet(&state, DiscoveryKind::Discover);
        let payload = serde_json::to_vec(&packet)?;
        socket
            .send_to(&payload, &state.config.cluster.discovery_multicast_addr)
            .await?;
        prune_cluster_peers(&state.cluster, state.config.cluster.peer_ttl);
    }
}

pub(crate) fn build_discovery_packet(state: &AppState, kind: DiscoveryKind) -> DiscoveryPacket {
    DiscoveryPacket {
        kind,
        service_id: state.config.cluster.service_id.clone(),
        advertise_addr: advertised_http_addr(&state.config),
        version: state
            .config
            .profile
            .display_version(env!("CARGO_PKG_VERSION")),
        priority: state.config.cluster.priority,
        sent_at: chrono::Utc::now(),
    }
}

pub(crate) fn advertised_http_addr(config: &AppConfig) -> String {
    config
        .cluster
        .advertise_addr
        .clone()
        .unwrap_or_else(|| config.service.bind_addr.clone())
}

pub(crate) fn update_cluster_peer(cluster: &ClusterRuntime, packet: DiscoveryPacket) {
    let mut peers = cluster.peers.lock().expect("cluster peer mutex poisoned");
    peers.insert(
        packet.service_id.clone(),
        ClusterPeer {
            service_id: packet.service_id,
            advertise_addr: packet.advertise_addr,
            version: packet.version,
            priority: packet.priority,
            last_seen: chrono::Utc::now(),
        },
    );
}

pub(crate) fn prune_cluster_peers(cluster: &ClusterRuntime, ttl: Duration) {
    let ttl = chrono::Duration::from_std(ttl).expect("valid cluster peer ttl");
    let now = chrono::Utc::now();
    let mut peers = cluster.peers.lock().expect("cluster peer mutex poisoned");
    peers.retain(|_, peer| now - peer.last_seen <= ttl);
}

pub(crate) fn selected_primary_peer(state: &AppState) -> Option<ClusterPeer> {
    prune_cluster_peers(&state.cluster, state.config.cluster.peer_ttl);
    let peers = state
        .cluster
        .peers
        .lock()
        .expect("cluster peer mutex poisoned");
    let mut peers = peers.values().cloned().collect::<Vec<_>>();
    peers.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| right.last_seen.cmp(&left.last_seen))
            .then_with(|| left.service_id.cmp(&right.service_id))
    });
    peers.into_iter().next()
}

pub(crate) fn cluster_peer_by_service_id(
    state: &AppState,
    service_id: &str,
) -> Option<ClusterPeer> {
    prune_cluster_peers(&state.cluster, state.config.cluster.peer_ttl);
    let peers = state
        .cluster
        .peers
        .lock()
        .expect("cluster peer mutex poisoned");
    peers.get(service_id).cloned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFingerprint {
    exists: bool,
    modified: Option<SystemTime>,
    len: Option<u64>,
}

pub(crate) async fn wait_for_config_change(
    path: &FsPath,
    previous: ConfigFingerprint,
) -> Result<ConfigFingerprint> {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let current = config_path_fingerprint(Some(path)).await?;
        if current != previous {
            return Ok(current);
        }
    }
}

pub(crate) async fn config_path_fingerprint(path: Option<&FsPath>) -> Result<ConfigFingerprint> {
    let Some(path) = path else {
        return Ok(ConfigFingerprint {
            exists: false,
            modified: None,
            len: None,
        });
    };

    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(ConfigFingerprint {
            exists: true,
            modified: metadata.modified().ok(),
            len: Some(metadata.len()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFingerprint {
            exists: false,
            modified: None,
            len: None,
        }),
        Err(error) => Err(error).with_context(|| format!("read metadata for {}", path.display())),
    }
}
