use serde::{Deserialize, Serialize};
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Main configuration container for the application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Server configuration settings
    pub server: ServerConfig,
    /// Database configuration settings
    pub database: DatabaseConfig,
    /// Storage configuration settings
    pub storage: StorageConfig,
    /// Logging configuration settings
    pub logging: LoggingConfig,
    /// Feature flags
    pub features: FeatureFlags,
    /// Message broker (RabbitMQ) configuration
    pub message_broker: MessageBrokerConfig,
    /// Server-side encoding key (hex) for path/filename encryption (optional)
    #[serde(skip)]
    pub server_encode_key: Option<Vec<u8>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            storage: StorageConfig::default(),
            logging: LoggingConfig::default(),
            features: FeatureFlags::default(),
            message_broker: MessageBrokerConfig::default(),
            server_encode_key: None,
        }
    }
}

impl Config {
    /// Load configuration from environment variables or use defaults
    pub fn load() -> Self {
        Self {
            server: ServerConfig::load(),
            database: DatabaseConfig::load(),
            storage: StorageConfig::load(),
            logging: LoggingConfig::load(),
            features: FeatureFlags::load(),
            message_broker: MessageBrokerConfig::load(),
            server_encode_key: None,
        }
    }

    /// Load configuration asynchronously with AWS Secrets Manager support
    pub async fn load_async() -> Result<Self, Box<dyn std::error::Error>> {
        let loader = super::secrets::ConfigLoader::new().await?;
        Ok(loader.load_config().await)
    }
}

/// Server configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host address to listen on
    pub host: String,
    /// Port to listen on
    pub port: u16,
    /// Storage path for files
    pub storage_path: Option<String>,
    /// Number of worker threads
    pub worker_threads: usize,
    /// Authentication token expiration time in hours
    pub auth_token_expiry_hours: i64,
    /// Maximum file size in bytes
    pub max_file_size: usize,
    /// Maximum number of concurrent requests
    pub max_concurrent_requests: usize,
    /// Heartbeat interval seconds for streaming keepalive
    pub heartbeat_interval_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: crate::config::constants::DEFAULT_GRPC_HOST.to_string(),
            port: crate::config::constants::DEFAULT_GRPC_PORT,
            storage_path: None,
            worker_threads: crate::config::constants::DEFAULT_WORKER_THREADS,
            auth_token_expiry_hours: crate::config::constants::DEFAULT_AUTH_TOKEN_EXPIRY_HOURS,
            max_file_size: crate::config::constants::DEFAULT_MAX_FILE_SIZE_BYTES,
            max_concurrent_requests: crate::config::constants::DEFAULT_MAX_CONCURRENT_REQUESTS,
            heartbeat_interval_secs: crate::config::constants::DEFAULT_HEARTBEAT_INTERVAL_SECS,
        }
    }
}

impl ServerConfig {
    /// Load configuration from environment variables or use defaults
    pub fn load() -> Self {
        let host = env::var("SERVER_HOST")
            .unwrap_or_else(|_| crate::config::constants::DEFAULT_GRPC_HOST.to_string());
        let port = env::var("GRPC_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_GRPC_PORT);
        let storage_path = env::var("STORAGE_PATH").ok().map(|p| p.to_string());
        let worker_threads = env::var("WORKER_THREADS")
            .ok()
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_WORKER_THREADS);
        let auth_token_expiry_hours = env::var("AUTH_TOKEN_EXPIRY_HOURS")
            .ok()
            .and_then(|h| h.parse::<i64>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_AUTH_TOKEN_EXPIRY_HOURS);
        let max_file_size = env::var("MAX_FILE_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_MAX_FILE_SIZE_BYTES);
        let max_concurrent_requests = env::var("MAX_CONCURRENT_REQUESTS")
            .ok()
            .and_then(|r| r.parse::<usize>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_MAX_CONCURRENT_REQUESTS);
        let heartbeat_interval_secs = env::var("HEARTBEAT_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_HEARTBEAT_INTERVAL_SECS);

        Self {
            host,
            port,
            storage_path,
            worker_threads,
            auth_token_expiry_hours,
            max_file_size,
            max_concurrent_requests,
            heartbeat_interval_secs,
        }
    }

    /// Get socket address from host and port
    pub fn address(&self) -> Result<SocketAddr, std::io::Error> {
        format!("{}:{}", self.host, self.port)
            .parse::<SocketAddr>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
    }

    /// Get storage path as PathBuf
    pub fn storage_path_buf(&self) -> PathBuf {
        self.storage_path
            .as_ref()
            .map_or_else(|| PathBuf::new(), PathBuf::from)
    }
}

/// Database configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database user
    pub user: String,
    /// Database password
    pub password: String,
    /// Database name
    pub name: String,
    /// Database host
    pub host: String,
    /// Database port
    pub port: u16,
    /// Maximum number of connections in the pool
    pub max_connections: u32,
    /// Connection timeout in seconds
    pub connection_timeout: u64,
    /// Enable database query logging
    pub log_queries: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            user: crate::config::constants::DEFAULT_DB_USER.to_string(),
            password: crate::config::constants::DEFAULT_DB_PASS.to_string(),
            name: crate::config::constants::DEFAULT_DB_NAME.to_string(),
            host: crate::config::constants::DEFAULT_DB_HOST.to_string(),
            port: crate::config::constants::DEFAULT_DB_PORT,
            max_connections: crate::config::constants::DEFAULT_DB_POOL,
            connection_timeout: crate::config::constants::DEFAULT_DB_CONN_TIMEOUT_SECS,
            log_queries: crate::config::constants::DEFAULT_DB_LOG_QUERIES,
        }
    }
}

impl DatabaseConfig {
    /// Load database configuration from environment variables or use defaults
    pub fn load() -> Self {
        let user = env::var("DB_USER")
            .unwrap_or_else(|_| crate::config::constants::DEFAULT_DB_USER.to_string());
        let password = env::var("DB_PASS")
            .unwrap_or_else(|_| crate::config::constants::DEFAULT_DB_PASS.to_string());
        let name = env::var("DB_NAME")
            .unwrap_or_else(|_| crate::config::constants::DEFAULT_DB_NAME.to_string());
        let host = env::var("DB_HOST")
            .unwrap_or_else(|_| crate::config::constants::DEFAULT_DB_HOST.to_string());
        let port = env::var("DB_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_DB_PORT);
        let max_connections = env::var("DB_POOL")
            .ok()
            .and_then(|c| c.parse::<u32>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_DB_POOL);
        let connection_timeout = env::var("DATABASE_CONNECTION_TIMEOUT")
            .ok()
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_DB_CONN_TIMEOUT_SECS);
        let log_queries = env::var("DATABASE_LOG_QUERIES")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(crate::config::constants::DEFAULT_DB_LOG_QUERIES);

        Self {
            user,
            password,
            name,
            host,
            port,
            max_connections,
            connection_timeout,
            log_queries,
        }
    }

    /// Generate database URL from individual components
    pub fn url(&self) -> String {
        format!(
            "mysql://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.name
        )
    }
}

/// Logging configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
    /// Enable file logging
    pub file_logging: bool,
    /// Log file path
    pub log_file: String,
    /// Maximum log file size in bytes before rotation
    pub max_file_size: usize,
    /// Maximum number of backups to keep
    pub max_backups: usize,
    /// Log output format: text or json
    pub format: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: crate::config::constants::DEFAULT_LOG_LEVEL.to_string(),
            file_logging: crate::config::constants::DEFAULT_LOG_TO_FILE,
            log_file: crate::config::constants::DEFAULT_LOG_FILE.to_string(),
            max_file_size: crate::config::constants::DEFAULT_LOG_MAX_FILE_SIZE_BYTES,
            max_backups: crate::config::constants::DEFAULT_LOG_MAX_BACKUPS,
            format: "text".to_string(),
        }
    }
}

impl LoggingConfig {
    /// Load logging configuration from environment variables or use defaults
    pub fn load() -> Self {
        let level = env::var("LOG_LEVEL")
            .unwrap_or_else(|_| crate::config::constants::DEFAULT_LOG_LEVEL.to_string());
        let file_logging = env::var("LOG_TO_FILE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(crate::config::constants::DEFAULT_LOG_TO_FILE);
        let log_file = env::var("LOG_FILE")
            .unwrap_or_else(|_| crate::config::constants::DEFAULT_LOG_FILE.to_string());
        let max_file_size = env::var("LOG_MAX_FILE_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_LOG_MAX_FILE_SIZE_BYTES);
        let max_backups = env::var("LOG_MAX_BACKUPS")
            .ok()
            .and_then(|b| b.parse::<usize>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_LOG_MAX_BACKUPS);
        let format = env::var("LOG_FORMAT").unwrap_or_else(|_| "text".to_string());

        Self {
            level,
            file_logging,
            log_file,
            max_file_size,
            max_backups,
            format,
        }
    }
}

/// Feature flags configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// Enable test mode which allows test tokens for authentication
    pub test_mode: bool,
    /// Enable debug mode which skips authentication for easier development
    pub debug_mode: bool,
    /// Enable detailed performance metrics collection
    pub metrics_enabled: bool,
    /// Enable encryption for stored files
    pub storage_encryption: bool,
    /// Enable request validation for improved security
    pub request_validation: bool,
    /// Encrypt metadata (path/name) on transport to clients
    pub transport_encrypt_metadata: bool,
    /// Enable developer mode (unifies COSMIC_SYNC_DEV_MODE)
    pub dev_mode: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            test_mode: false,
            debug_mode: false,
            metrics_enabled: false,
            storage_encryption: true,
            request_validation: true,
            transport_encrypt_metadata: true,
            dev_mode: false,
        }
    }
}

impl FeatureFlags {
    /// Load feature flags from environment variables or use defaults
    pub fn load() -> Self {
        let test_mode = env::var("COSMIC_SYNC_TEST_MODE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let debug_mode = env::var("COSMIC_SYNC_DEBUG_MODE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let metrics_enabled = env::var("ENABLE_METRICS")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let storage_encryption = env::var("STORAGE_ENCRYPTION")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);
        let request_validation = env::var("REQUEST_VALIDATION")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);
        let transport_encrypt_metadata = env::var("COSMIC_TRANSPORT_ENCRYPT_METADATA")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);
        let dev_mode = env::var("COSMIC_SYNC_DEV_MODE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        Self {
            test_mode,
            debug_mode,
            metrics_enabled,
            storage_encryption,
            request_validation,
            transport_encrypt_metadata,
            dev_mode,
        }
    }
}

/// Storage type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StorageType {
    /// Store files in database as BLOB
    Database,
    /// Store files in AWS S3
    S3,
}

impl Default for StorageType {
    fn default() -> Self {
        StorageType::Database
    }
}

impl std::str::FromStr for StorageType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "database" | "db" => Ok(StorageType::Database),
            "s3" => Ok(StorageType::S3),

            _ => Err(format!("Invalid storage type: {}", s)),
        }
    }
}

/// Storage configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Storage type to use
    pub storage_type: StorageType,
    /// S3 configuration (when storage_type is S3)
    pub s3: S3Config,
    /// Retention TTL in seconds for deleted data (logical -> physical purge)
    pub file_ttl_secs: i64,
    /// Maximum number of revisions to keep per file path
    pub max_file_revisions: i32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_type: StorageType::Database,
            s3: S3Config::default(),
            file_ttl_secs: crate::config::constants::DEFAULT_FILE_TTL_SECS,
            max_file_revisions: crate::config::constants::DEFAULT_MAX_FILE_REVISIONS,
        }
    }
}

impl StorageConfig {
    /// Load storage configuration from environment variables or use defaults
    pub fn load() -> Self {
        let storage_type = env::var("STORAGE_TYPE")
            .unwrap_or_else(|_| "database".to_string())
            .parse()
            .unwrap_or(StorageType::Database);

        Self {
            storage_type,
            s3: S3Config::load(),
            file_ttl_secs: env::var("FILE_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(crate::config::constants::DEFAULT_FILE_TTL_SECS),
            max_file_revisions: env::var("MAX_FILE_REVISIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(crate::config::constants::DEFAULT_MAX_FILE_REVISIONS),
        }
    }
}

/// S3 configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    /// AWS region
    pub region: String,
    /// S3 bucket name
    pub bucket: String,
    /// S3 object key prefix
    pub key_prefix: String,
    /// AWS access key ID (optional - can use IAM role)
    pub access_key_id: Option<String>,
    /// AWS secret access key (optional - can use IAM role)
    pub secret_access_key: Option<String>,
    /// AWS session token (optional - for temporary credentials)
    pub session_token: Option<String>,
    /// S3 endpoint URL (for S3-compatible services)
    pub endpoint_url: Option<String>,
    /// Force path style addressing
    pub force_path_style: bool,
    /// Use AWS Secret Manager for credentials
    pub use_secret_manager: bool,
    /// Secret Manager secret name (when use_secret_manager is true)
    pub secret_name: Option<String>,
    /// Connection timeout in seconds
    pub timeout_seconds: u64,
    /// Maximum number of retries
    pub max_retries: u32,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            region: crate::config::constants::DEFAULT_S3_REGION.to_string(),
            bucket: crate::config::constants::DEFAULT_S3_BUCKET.to_string(),
            key_prefix: crate::config::constants::DEFAULT_S3_KEY_PREFIX.to_string(),
            access_key_id: None,
            secret_access_key: None,
            session_token: None,
            endpoint_url: None,
            force_path_style: crate::config::constants::DEFAULT_S3_FORCE_PATH_STYLE,
            use_secret_manager: crate::config::constants::DEFAULT_S3_USE_SECRET_MANAGER,
            secret_name: None,
            timeout_seconds: crate::config::constants::DEFAULT_S3_TIMEOUT_SECONDS,
            max_retries: crate::config::constants::DEFAULT_S3_MAX_RETRIES,
        }
    }
}

impl S3Config {
    /// Load S3 configuration from environment variables or use defaults
    pub fn load() -> Self {
        Self {
            region: env::var("AWS_REGION")
                .unwrap_or_else(|_| crate::config::constants::DEFAULT_S3_REGION.to_string()),
            bucket: env::var("AWS_S3_BUCKET")
                .unwrap_or_else(|_| crate::config::constants::DEFAULT_S3_BUCKET.to_string()),
            key_prefix: env::var("S3_KEY_PREFIX")
                .unwrap_or_else(|_| crate::config::constants::DEFAULT_S3_KEY_PREFIX.to_string()),
            access_key_id: env::var("AWS_ACCESS_KEY_ID").ok(),
            secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
            session_token: env::var("AWS_SESSION_TOKEN").ok(),
            endpoint_url: env::var("S3_ENDPOINT_URL").ok(),
            force_path_style: env::var("S3_FORCE_PATH_STYLE")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(crate::config::constants::DEFAULT_S3_FORCE_PATH_STYLE),
            use_secret_manager: env::var("USE_AWS_SECRET_MANAGER")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(crate::config::constants::DEFAULT_S3_USE_SECRET_MANAGER),
            secret_name: env::var("AWS_SECRET_NAME").ok(),
            timeout_seconds: env::var("S3_TIMEOUT_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(crate::config::constants::DEFAULT_S3_TIMEOUT_SECONDS),
            max_retries: env::var("S3_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(crate::config::constants::DEFAULT_S3_MAX_RETRIES),
        }
    }
}

/// Message broker configuration settings (RabbitMQ)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBrokerConfig {
    /// Enable RabbitMQ-backed EventBus
    pub enabled: bool,
    /// AMQP URL, e.g. amqp://user:pass@host:5672/vhost
    pub url: String,
    /// Exchange name to publish/consume
    pub exchange: String,
    /// Queue name prefix for dynamically created queues
    pub queue_prefix: String,
    /// Prefetch size per consumer for better throughput
    pub prefetch: u16,
    /// Durable exchange/queue
    pub durable: bool,
}

impl Default for MessageBrokerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: env::var("RABBITMQ_URL")
                .unwrap_or_else(|_| "amqp://guest:guest@127.0.0.1:5672/%2f".to_string()),
            exchange: env::var("RABBITMQ_EXCHANGE").unwrap_or_else(|_| "cosmic.sync".to_string()),
            queue_prefix: env::var("RABBITMQ_QUEUE_PREFIX")
                .unwrap_or_else(|_| "cosmic.sync".to_string()),
            prefetch: env::var("RABBITMQ_PREFETCH")
                .ok()
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(200),
            durable: env::var("RABBITMQ_DURABLE")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(true),
        }
    }
}

impl MessageBrokerConfig {
    pub fn load() -> Self {
        let enabled = env::var("RABBITMQ_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        Self {
            enabled,
            ..Self::default()
        }
    }
}
