use cosmic_sync_server::config::constants;
use cosmic_sync_server::config::settings::LoggingConfig;
use dotenv::dotenv;
use std::env;
use tracing::{error, info, instrument, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use cosmic_sync_server::{
    config::settings::{DatabaseConfig, FeatureFlags, ServerConfig, StorageConfig},
    config::{Config, ConfigLoader, Environment},
    container::ContainerBuilder,
    error::{Result, SyncError},
    server::startup::{start_server, start_server_with_storage},
    storage::init_storage,
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
    info!(
        "🚀 Starting COSMIC Sync Server v{} (Container Mode)",
        env!("CARGO_PKG_VERSION")
    );

    // 컨테이너 빌드
    let container = ContainerBuilder::build_production().await?;

    info!("📊 Container initialized with all dependencies");

    // 헬스체크
    if !container.health_check().await? {
        return Err(SyncError::ServiceUnavailable(
            "Health check failed".to_string(),
        ));
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

    info!(
        "🚀 Starting COSMIC Sync Server v{}",
        env!("CARGO_PKG_VERSION")
    );
    info!(
        "📊 Configuration loaded: workers={}, max_requests={}",
        config.server.worker_threads, config.server.max_concurrent_requests
    );

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
    // Use unified app logging config
    let logging_cfg = LoggingConfig::load();
    let log_level = logging_cfg.level;

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(&format!(
                    "cosmic_sync_server={},info",
                    log_level
                ))
            }),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_file(false) // Disable for performance
                .with_line_number(false) // Disable for performance
                .compact(),
        );
    // JSON logging for production (unified via LOG_FORMAT)
    if logging_cfg.format.to_lowercase() == "json" {
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(false)
            .with_span_list(false); // Optimize for performance

        subscriber.with(json_layer).init();
    } else {
        subscriber.init();
    }

    info!(
        "✅ Structured logging initialized with level: {}",
        log_level
    );
    Ok(())
}

/// Build optimized configuration with validation
#[instrument]
async fn build_config() -> Result<Config> {
    info!("📋 Loading server configuration...");

    let environment = Environment::current();
    info!(
        "🔧 Detected environment: {:?} (from ENVIRONMENT variable)",
        environment
    );

    // Load configuration using the new async loader with AWS Secrets Manager support
    let config = match Config::load_async().await {
        Ok(config) => config,
        Err(e) => {
            error!(
                "❌ Failed to load configuration from AWS Secrets Manager: {}",
                e
            );
            warn!("📋 Falling back to legacy environment variable loading");
            Config::load()
        }
    };

    // Validate database connection early
    validate_database_config(&config.database).await?;

    // Validate server configuration
    validate_server_config(&config.server)?;

    info!("✅ Configuration validated successfully");
    info!(
        "🌐 gRPC server will listen on {}:{}",
        config.server.host, config.server.port
    );
    info!(
        "🗄️ Database: {}@{}",
        config.database.user, config.database.host
    );

    if environment.is_cloud() {
        let secret_path = if environment == Environment::Staging {
            "staging/so-dod/cosmic-sync/config"
        } else {
            "production/pop-os/cosmic-sync/config"
        };
        info!("☁️ Cloud environment - using secrets from: {}", secret_path);
    } else {
        info!("🏠 Development environment detected - using local environment variables");
    }

    Ok(config)
}

/// Validate database configuration with connection test
async fn validate_database_config(config: &DatabaseConfig) -> Result<()> {
    if config.host.is_empty() {
        return Err(SyncError::Config(
            "Database host cannot be empty".to_string(),
        ));
    }

    if config.name.is_empty() {
        return Err(SyncError::Config(
            "Database name cannot be empty".to_string(),
        ));
    }

    if config.user.is_empty() {
        return Err(SyncError::Config(
            "Database username cannot be empty".to_string(),
        ));
    }

    // Test database connectivity (non-blocking)
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        test_database_connection(config),
    )
    .await
    {
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

    if config.port < constants::MIN_VALID_PORT as u16
        || config.port > constants::MAX_VALID_PORT as u16
    {
        return Err(SyncError::Config(format!(
            "Invalid port: {}. Must be between {} and {}",
            config.port,
            constants::MIN_VALID_PORT,
            constants::MAX_VALID_PORT
        )));
    }

    if config.worker_threads == 0 || config.worker_threads > 256 {
        return Err(SyncError::Config(format!(
            "Invalid worker_threads: {}. Must be between 1 and 256",
            config.worker_threads
        )));
    }

    if config.max_concurrent_requests == 0 {
        return Err(SyncError::Config(
            "max_concurrent_requests must be greater than 0".to_string(),
        ));
    }

    if config.max_file_size == 0 || config.max_file_size > 1024 * 1024 * 1024 {
        return Err(SyncError::Config(format!(
            "Invalid max_file_size: {}. Must be between 1 byte and 1GB",
            config.max_file_size
        )));
    }

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
