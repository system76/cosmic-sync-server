use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tonic::transport::Server;
use tracing::{error, info, instrument, warn};

use crate::{
    config::settings::ServerConfig,
    error::{Result, SyncError},
    handlers,
    monitoring::PerformanceMonitor,
    server::{
        app_state::AppState,
        service::{SyncClientServiceImpl, SyncServiceImpl},
    },
    storage::{init_storage, Storage},
    sync::{
        sync_client_service_server::SyncClientServiceServer, sync_service_server::SyncServiceServer,
    },
};

use actix_cors::Cors;
use actix_web::{middleware, web, App, HttpServer};

/// Optimized server startup with performance monitoring
#[instrument(skip(config))]
pub async fn start_server(config: ServerConfig) -> Result<()> {
    info!("🚀 Starting COSMIC Sync Server with optimized configuration");

    // Initialize performance monitoring (reserved, not used yet)
    let _performance_monitor = Arc::new(PerformanceMonitor::new());

    // Initialize storage with connection pooling (MySQL if provided, otherwise memory)
    let storage = init_storage_from_config(&config).await?;

    // Initialize optimized app state using the same storage instance
    let app_state =
        Arc::new(AppState::new_with_storage_and_server_config(storage.clone(), &config).await?);

    // Start both gRPC and HTTP servers
    let grpc_server = start_grpc_server(&config, app_state.clone());
    let http_server = start_http_server(&config, app_state.clone());
    let shutdown_signal = setup_shutdown_signal();

    // Print startup banner
    print_startup_banner(&config);
    // Log effective storage configuration
    if let Some(storage_path) = &config.storage_path {
        tracing::info!("Effective storage_path: {}", storage_path);
    } else {
        tracing::info!("Effective storage_path: <none> (memory fallback if not set)");
    }
    // Run servers with graceful shutdown
    tokio::select! {
        result = grpc_server => {
            if let Err(e) = result {
                error!("❌ gRPC server error: {}", e);
                return Err(e);
            }
        },
        result = http_server => {
            if let Err(e) = result {
                error!("❌ HTTP server error: {}", e);
                return Err(e);
            }
        },
        _ = shutdown_signal => {
            info!("🛑 Shutdown signal received");
        }
    }

    // Graceful cleanup
    graceful_shutdown(app_state).await?;

    info!("✅ Server shutdown completed successfully");
    Ok(())
}

/// Start server with custom storage (for dependency injection)
#[instrument(skip(config, storage))]
pub async fn start_server_with_storage(
    config: ServerConfig,
    storage: Arc<dyn Storage>,
) -> Result<()> {
    info!("🚀 Starting server with custom storage");

    // Initialize app state with provided storage (avoid creating in-memory storage)
    let app_state =
        Arc::new(AppState::new_with_storage_and_server_config(storage.clone(), &config).await?);

    // Start servers
    let grpc_server = start_grpc_server(&config, app_state.clone());
    let http_server = start_http_server(&config, app_state.clone());
    let shutdown_signal = setup_shutdown_signal();

    print_startup_banner(&config);

    // Run with graceful shutdown
    tokio::select! {
        result = grpc_server => {
            if let Err(e) = result {
                error!("❌ gRPC server error: {}", e);
                return Err(e);
            }
        },
        result = http_server => {
            if let Err(e) = result {
                error!("❌ HTTP server error: {}", e);
                return Err(e);
            }
        },
        _ = shutdown_signal => {
            info!("🛑 Shutdown signal received");
        }
    }

    graceful_shutdown(app_state).await?;

    info!("✅ Server with custom storage shutdown completed");
    Ok(())
}

/// Start optimized gRPC server
#[instrument(skip(config, app_state))]
async fn start_grpc_server(config: &ServerConfig, app_state: Arc<AppState>) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port)
        .parse::<SocketAddr>()
        .map_err(|e| SyncError::Config(format!("Invalid server address: {}", e)))?;

    info!("🌐 Starting gRPC server on {}", addr);

    // Initialize services with dependency injection
    let sync_service = SyncServiceImpl::new(app_state.clone());
    let sync_client_service = SyncClientServiceImpl::new(app_state.clone());

    // Build optimized gRPC server
    let server = Server::builder()
        // Timeout configurations
        .timeout(Duration::from_secs(config.auth_token_expiry_hours as u64))
        .http2_keepalive_interval(Some(Duration::from_secs(30)))
        .http2_keepalive_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(60)))
        // Performance optimizations
        .max_concurrent_streams(Some(config.max_concurrent_requests as u32))
        .initial_connection_window_size(Some(4 * 1024 * 1024)) // 4MB
        .initial_stream_window_size(Some(2 * 1024 * 1024)) // 2MB
        .tcp_nodelay(true)
        .accept_http1(true)
        // Compression (methods not available in current tonic version)
        // .accept_compressed(tonic::codec::CompressionEncoding::Gzip)
        // Add services
        .add_service(SyncServiceServer::new(sync_service))
        .add_service(SyncClientServiceServer::new(sync_client_service));

    // Add reflection service in development
    #[cfg(feature = "reflection")]
    let server = {
        let reflection_service = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(tonic::include_file_descriptor_set!(
                "sync_descriptor"
            ))
            .build()
            .map_err(|e| {
                SyncError::Internal(format!("Failed to create reflection service: {}", e))
            })?;

        server.add_service(reflection_service)
    };

    // Start server with graceful shutdown
    server
        .serve_with_shutdown(addr, setup_shutdown_signal())
        .await
        .map_err(|e| SyncError::Internal(format!("gRPC server error: {}", e)))?;

    info!("🌐 gRPC server stopped");
    Ok(())
}

/// Start optimized HTTP server
#[instrument(skip(config, app_state))]
async fn start_http_server(config: &ServerConfig, app_state: Arc<AppState>) -> Result<()> {
    let http_port = crate::config::constants::DEFAULT_HTTP_PORT as u16; // central constant
    let addr = format!("{}:{}", config.host, http_port);

    info!("🌐 Starting HTTP server on {}", addr);

    // Clone app state for the closure
    let app_state_clone = app_state.clone();

    // Build HTTP server with middleware
    HttpServer::new(move || {
        // Create AuthHandler for dependency injection
        let auth_handler = crate::handlers::auth_handler::AuthHandler::new(app_state_clone.clone());

        App::new()
            // App data
            .app_data(web::Data::new(app_state_clone.clone()))
            .app_data(web::Data::new(auth_handler))
            // Middleware stack (optimized order)
            .wrap(middleware::Compress::default())
            .wrap(middleware::Logger::default())
            .wrap(
                middleware::DefaultHeaders::new()
                    .add(("X-Version", env!("CARGO_PKG_VERSION")))
                    .add(("X-Server", "COSMIC-Sync")),
            )
            // CORS configuration
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .allow_any_header()
                    .max_age(crate::config::constants::DEFAULT_CORS_MAX_AGE_SECS as usize),
            )
            // Health endpoints
            .route("/health", web::get().to(handlers::health::health_check))
            .route(
                "/health/ready",
                web::get().to(handlers::health::readiness_check),
            )
            .route(
                "/health/live",
                web::get().to(handlers::health::liveness_check),
            )
            // Metrics endpoints
            .route(
                "/metrics",
                web::get().to(handlers::metrics::prometheus_metrics),
            )
            .route(
                "/metrics/detailed",
                web::get().to(handlers::metrics::detailed_metrics),
            )
            // OAuth endpoints
            .route(
                "/oauth/login",
                web::get().to(handlers::oauth::handle_oauth_login),
            )
            .route(
                "/oauth/callback",
                web::get().to(handlers::oauth::handle_oauth_callback),
            )
            // Auth endpoints
            .route(
                "/auth/session",
                web::post().to(handlers::auth_handler::handle_register_session),
            )
            .route(
                "/auth/status",
                web::get().to(handlers::auth_handler::handle_check_auth_status),
            )
            // 🔧 디버깅: 핸들러 참조 확인
            // handlers::auth_handler::handle_register_session은 올바르게 참조되고 있습니다
            // API info endpoints
            .route("/api/info", web::get().to(handlers::api::api_info))
            .route("/api/version", web::get().to(handlers::api::api_version))
            .route("/api/status", web::get().to(handlers::api::api_status))
    })
    .workers(config.worker_threads)
    .keep_alive(Duration::from_secs(75))
    .client_request_timeout(Duration::from_secs(120))
    .shutdown_timeout(30)
    .bind(&addr)
    .map_err(|e| SyncError::Network(format!("Failed to bind HTTP server: {}", e)))?
    .run()
    .await
    .map_err(|e| SyncError::Internal(format!("HTTP server error: {}", e)))?;

    info!("🌐 HTTP server stopped");
    Ok(())
}

/// Initialize storage from configuration
#[instrument(skip(config))]
async fn init_storage_from_config(config: &ServerConfig) -> Result<Arc<dyn Storage>> {
    if let Some(storage_path) = &config.storage_path {
        info!("📊 Initializing storage from path: {}", storage_path);

        if storage_path.starts_with("mysql://") {
            // Parse MySQL URL and create DatabaseConfig
            let database_config = parse_mysql_url(storage_path)?;
            return init_storage(&database_config).await;
        }
    }

    // Fallback to memory storage
    warn!("⚠️ No valid storage configuration found, using memory storage");
    Ok(Arc::new(crate::storage::memory::MemoryStorage::new()))
}

/// Parse MySQL URL into DatabaseConfig
fn parse_mysql_url(url: &str) -> Result<crate::config::settings::DatabaseConfig> {
    let url =
        url::Url::parse(url).map_err(|e| SyncError::Config(format!("Invalid MySQL URL: {}", e)))?;

    let host = url
        .host_str()
        .ok_or_else(|| SyncError::Config("Missing host in MySQL URL".to_string()))?
        .to_string();

    let port = url.port().unwrap_or(3306);
    let username = url.username().to_string();
    let password = url.password().unwrap_or("").to_string();
    let database = url.path().trim_start_matches('/').to_string();

    if database.is_empty() {
        return Err(SyncError::Config(
            "Missing database name in MySQL URL".to_string(),
        ));
    }

    Ok(crate::config::settings::DatabaseConfig {
        host,
        port,
        user: username,
        password,
        name: database,
        max_connections: 50,
        connection_timeout: 30,
        log_queries: false,
    })
}

/// Setup graceful shutdown signal handling
async fn setup_shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = signal::ctrl_c().await {
            error!("Failed to install Ctrl+C handler: {}", e);
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(e) => {
                error!("Failed to install signal handler: {}", e);
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("🛑 Received Ctrl+C signal, initiating graceful shutdown...");
        },
        _ = terminate => {
            info!("🛑 Received TERM signal, initiating graceful shutdown...");
        },
    }

    // Give some time for cleanup
    tokio::time::sleep(Duration::from_millis(100)).await;
    info!("✅ Shutdown signal processing complete");
}

/// Perform graceful shutdown
#[instrument(skip(app_state))]
async fn graceful_shutdown(app_state: Arc<AppState>) -> Result<()> {
    info!("🔄 Starting graceful shutdown sequence...");

    // 1. Stop accepting new requests (handled by server shutdown)

    // 2. Wait for existing requests to complete
    let mut wait_count = 0;
    while app_state
        .connection_tracker
        .get_total_active_connections()
        .await
        > 0
        && wait_count < 100
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
        wait_count += 1;
    }

    if app_state
        .connection_tracker
        .get_total_active_connections()
        .await
        > 0
    {
        warn!(
            "⚠️ {} connections still active during shutdown",
            app_state
                .connection_tracker
                .get_total_active_connections()
                .await
        );
    }

    // 3. Close storage connections
    if let Err(e) = app_state.storage.close().await {
        error!("❌ Error closing storage connections: {}", e);
    }

    // 4. Shutdown app state
    // Shutdown storage connections
    if let Err(e) = app_state.storage.close().await {
        error!("❌ Error during shutdown: {}", e);
    }

    info!("✅ Graceful shutdown sequence completed");
    Ok(())
}

/// Print optimized startup banner
fn print_startup_banner(config: &ServerConfig) {
    println!(
        "
╭─────────────────────────────────────────────────────╮
│                COSMIC Sync Server                   │
│                   v{}                    │
├─────────────────────────────────────────────────────┤
│ 🌐 gRPC: {}:{}                        │
│ 🌐 HTTP: {}:8080                        │
│ 🧵 Workers: {} threads                             │
│ 🚀 Max Requests: {}                               │
│ 📁 Max File Size: {}MB                            │
│ 🔧 Features: {}                                   │
╰─────────────────────────────────────────────────────╯
",
        env!("CARGO_PKG_VERSION"),
        config.host,
        config.port,
        config.host,
        config.worker_threads,
        config.max_concurrent_requests,
        config.max_file_size / 1024 / 1024,
        get_enabled_features()
    );

    info!("✅ COSMIC Sync Server startup completed");
}

/// Get list of enabled features
fn get_enabled_features() -> String {
    let mut features = Vec::new();

    #[cfg(feature = "redis-cache")]
    features.push("Redis");

    #[cfg(feature = "metrics")]
    features.push("Metrics");

    #[cfg(feature = "compression")]
    features.push("Compression");

    #[cfg(feature = "reflection")]
    features.push("Reflection");

    if features.is_empty() {
        "Basic".to_string()
    } else {
        features.join(", ")
    }
}

/// Health check for server readiness
pub async fn server_ready_check(storage: &Arc<dyn Storage>) -> Result<()> {
    // Check storage connectivity
    storage
        .health_check()
        .await
        .map_err(|e| SyncError::ServiceUnavailable(format!("Storage not ready: {}", e)))?;

    Ok(())
}

/// Legacy support functions for backward compatibility
pub async fn init_storage_legacy(db_url: Option<String>) -> Arc<dyn Storage> {
    match db_url {
        Some(url) if url.starts_with("mysql://") => match parse_mysql_url(&url) {
            Ok(config) => match init_storage(&config).await {
                Ok(storage) => return storage,
                Err(e) => {
                    error!("Failed to initialize MySQL storage: {}", e);
                }
            },
            Err(e) => {
                error!("Failed to parse MySQL URL: {}", e);
            }
        },
        _ => {
            info!("Using memory storage (legacy mode)");
        }
    }

    Arc::new(crate::storage::memory::MemoryStorage::new())
}
