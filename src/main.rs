use std::env;
use std::sync::Arc;
use dotenv::dotenv;
use tracing::{info, error, warn, instrument};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use cosmic_sync_server::{
    server::startup::start_server,
    config::settings::{Config, ServerConfig, DatabaseConfig, LoggingConfig, FeatureFlags, StorageConfig},
    error::{Result, SyncError},
    storage::init_storage,
    container::ContainerBuilder,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();
    
    // Initialize structured logging
    init_tracing()?;
    
    // Check if we should use the new container-based architecture
    let use_container = env::var("COSMIC_SYNC_USE_CONTAINER")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    if use_container {
        info!("🔧 Starting with new container-based architecture");
        start_with_container().await
    } else {
        info!("🔧 Starting with legacy architecture");
        start_legacy().await
    }
}

/// 새로운 컨테이너 기반 서버 시작
async fn start_with_container() -> Result<()> {
    info!("🚀 Starting COSMIC Sync Server v{} (Container Mode)", env!("CARGO_PKG_VERSION"));
    
    // 컨테이너 빌드
    let container = ContainerBuilder::build_production().await?;
    
    info!("📊 Container initialized with all dependencies");
    
    // 헬스체크
    if !container.health_check().await? {
        return Err(SyncError::ServiceUnavailable("Health check failed".to_string()));
    }
    
    // 서버 시작 (향후 컨테이너를 사용하는 새로운 시작 방식으로 변경 예정)
    let config = container.config();
    let storage = container.storage();
    
    match start_server_with_storage(config.server.clone(), storage).await {
        Ok(_) => {
            info!("✅ Container-based server shutdown completed successfully");
            container.shutdown().await?;
            Ok(())
        }
        Err(e) => {
            error!("❌ Container-based server failed: {}", e);
            container.shutdown().await.ok(); // 에러 시에도 정리
            Err(e)
        }
    }
}

/// 기존 방식의 서버 시작 (레거시)
async fn start_legacy() -> Result<()> {
    // Build configuration with validation
    let config = build_config().await?;
    
    // Initialize storage layer with connection pooling
    let storage = init_storage(&config.database).await?;
    
    info!("🚀 Starting COSMIC Sync Server v{}", env!("CARGO_PKG_VERSION"));
    info!("📊 Configuration loaded: workers={}, max_requests={}", 
          config.server.worker_threads, config.server.max_concurrent_requests);
    
    // Start the optimized server
    match start_server_with_storage(config.server, storage).await {
        Ok(_) => {
            info!("✅ Server shutdown completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("❌ Server failed: {}", e);
            Err(e)
        }
    }
}

/// Initialize structured logging with performance optimizations
#[instrument]
fn init_tracing() -> Result<()> {
    let log_level = env::var("RUST_LOG")
        .unwrap_or_else(|_| "cosmic_sync_server=info,info".to_string());
    
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&log_level))
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_file(false) // Disable for performance
                .with_line_number(false) // Disable for performance
                .compact()
        );
    
    // JSON logging for production
    if env::var("LOG_FORMAT").unwrap_or_default() == "json" {
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(false)
            .with_span_list(false); // Optimize for performance
        
        subscriber.with(json_layer).init();
    } else {
        subscriber.init();
    }
    
    info!("✅ Structured logging initialized with level: {}", log_level);
    Ok(())
}

/// Build optimized configuration with validation
#[instrument]
async fn build_config() -> Result<Config> {
    info!("📋 Loading server configuration...");
    
    // Optimized environment variable helpers
    let get_env = |key: &str, default: &str| -> String {
        env::var(key).unwrap_or_else(|_| default.to_string())
    };
    
    let parse_env_var = |key: &str, default: u64| -> Result<u64> {
        env::var(key)
            .map(|v| v.parse().map_err(|_| SyncError::Config(format!("Invalid {}: {}", key, v))))
            .unwrap_or(Ok(default))
    };
    
    // Load and validate database configuration
    let database_config = DatabaseConfig::load();
    
    // Validate database connection early
    validate_database_config(&database_config).await?;
    
    let server_config = ServerConfig {
        host: get_env("SERVER_HOST", "0.0.0.0"),
        port: parse_env_var("SERVER_PORT", 50051)? as u16,
        storage_path: Some(database_config.url()),
        auth_token_expiry_hours: parse_env_var("AUTH_TOKEN_EXPIRY_HOURS", 24)? as i64,
        max_concurrent_requests: parse_env_var("MAX_CONCURRENT_REQUESTS", 1000)? as usize,
        max_file_size: parse_env_var("MAX_FILE_SIZE", 50 * 1024 * 1024)? as usize, // 50MB
        worker_threads: parse_env_var("WORKER_THREADS", num_cpus::get() as u64)? as usize,
    };
    
    // Validate server configuration
    validate_server_config(&server_config)?;
    
    let config = Config {
        server: server_config,
        database: database_config,
        logging: LoggingConfig::default(),
        features: FeatureFlags::default(),
        storage: StorageConfig::load(),
    };
    
    info!("✅ Configuration validated successfully");
    Ok(config)
}

/// Validate database configuration with connection test
async fn validate_database_config(config: &DatabaseConfig) -> Result<()> {
    if config.host.is_empty() {
        return Err(SyncError::Config("Database host cannot be empty".to_string()));
    }
    
    if config.name.is_empty() {
        return Err(SyncError::Config("Database name cannot be empty".to_string()));
    }
    
    if config.user.is_empty() {
        return Err(SyncError::Config("Database username cannot be empty".to_string()));
    }
    
    // Test database connectivity (non-blocking)
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        test_database_connection(config)
    ).await {
        Ok(Ok(_)) => info!("✅ Database connectivity verified"),
        Ok(Err(e)) => warn!("⚠️ Database connection test failed: {}", e),
        Err(_) => warn!("⚠️ Database connection test timed out"),
    }
    
    Ok(())
}

/// Test database connection
async fn test_database_connection(config: &DatabaseConfig) -> Result<()> {
    use sqlx::mysql::MySqlPoolOptions;
    
    let pool = MySqlPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(&config.url())
        .await
        .map_err(|e| SyncError::Database(format!("Connection failed: {}", e)))?;
    
    // Simple ping test
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|e| SyncError::Database(format!("Ping failed: {}", e)))?;
    
    pool.close().await;
    Ok(())
}

/// Validate server configuration
fn validate_server_config(config: &ServerConfig) -> Result<()> {
    if config.host.is_empty() {
        return Err(SyncError::Config("Server host cannot be empty".to_string()));
    }
    
    if config.port < 1024 || config.port > 65535 {
        return Err(SyncError::Config(
            format!("Invalid port: {}. Must be between 1024 and 65535", config.port)
        ));
    }
    
    if config.worker_threads == 0 || config.worker_threads > 256 {
        return Err(SyncError::Config(
            format!("Invalid worker_threads: {}. Must be between 1 and 256", config.worker_threads)
        ));
    }
    
    if config.max_concurrent_requests == 0 {
        return Err(SyncError::Config("max_concurrent_requests must be greater than 0".to_string()));
    }
    
    if config.max_file_size == 0 || config.max_file_size > 1024 * 1024 * 1024 {
        return Err(SyncError::Config(
            format!("Invalid max_file_size: {}. Must be between 1 byte and 1GB", config.max_file_size)
        ));
    }
    
    Ok(())
}

/// Start server with optimized storage layer
async fn start_server_with_storage(
    config: ServerConfig, 
    storage: Arc<dyn cosmic_sync_server::storage::Storage>
) -> Result<()> {
    // Print startup banner
    print_startup_banner(&config);
    
    // Start server with storage dependency injection
    cosmic_sync_server::server::startup::start_server_with_storage(config, storage).await
}

/// Print optimized startup banner
fn print_startup_banner(config: &ServerConfig) {
    println!("
╭─────────────────────────────────────────────────────╮
│                COSMIC Sync Server                   │
│                   v{}                    │
├─────────────────────────────────────────────────────┤
│ 🌐 Address: {}:{}                        │
│ 🧵 Workers: {} threads                             │
│ 🚀 Max Requests: {}                               │
│ 📁 Max File Size: {}MB                            │
│ 🔧 Features: {}                                   │
╰─────────────────────────────────────────────────────╯
",
        env!("CARGO_PKG_VERSION"),
        config.host,
        config.port,
        config.worker_threads,
        config.max_concurrent_requests,
        config.max_file_size / 1024 / 1024,
        get_enabled_features()
    );
}

/// Get enabled features list
fn get_enabled_features() -> String {
    let mut features = Vec::new();
    
    #[cfg(feature = "redis-cache")]
    features.push("Redis");
    
    #[cfg(feature = "metrics")]
    features.push("Metrics");
    
    #[cfg(feature = "compression")]
    features.push("Compression");
    
    if features.is_empty() {
        "None".to_string()
    } else {
        features.join(", ")
    }
}
