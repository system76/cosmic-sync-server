use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            storage: StorageConfig::default(),
            logging: LoggingConfig::default(),
            features: FeatureFlags::default(),
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
        }
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "[::1]".to_string(),
            port: 50051,
            storage_path: None,
            worker_threads: 4,
            auth_token_expiry_hours: 24,
            max_file_size: 50 * 1024 * 1024, // 50MB is appropriate for the server
            max_concurrent_requests: 100,
        }
    }
}

impl ServerConfig {
    /// Load configuration from environment variables or use defaults
    pub fn load() -> Self {
        let host = env::var("SERVER_HOST").unwrap_or_else(|_| "[::1]".to_string());
        let port = env::var("SERVER_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(50051);
        let storage_path = env::var("STORAGE_PATH").ok().map(|p| p.to_string());
        let worker_threads = env::var("WORKER_THREADS")
            .ok()
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(4);
        let auth_token_expiry_hours = env::var("AUTH_TOKEN_EXPIRY_HOURS")
            .ok()
            .and_then(|h| h.parse::<i64>().ok())
            .unwrap_or(24);
        let max_file_size = env::var("MAX_FILE_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(50 * 1024 * 1024);
        let max_concurrent_requests = env::var("MAX_CONCURRENT_REQUESTS")
            .ok()
            .and_then(|r| r.parse::<usize>().ok())
            .unwrap_or(100);
        
        Self {
            host,
            port,
            storage_path,
            worker_threads,
            auth_token_expiry_hours,
            max_file_size,
            max_concurrent_requests,
        }
    }
    
    /// Get socket address from host and port
    pub fn address(&self) -> Result<SocketAddr, std::io::Error> {
        format!("{}:{}", self.host, self.port).parse::<SocketAddr>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
    }
    
    /// Get storage path as PathBuf
    pub fn storage_path_buf(&self) -> PathBuf {
        self.storage_path.as_ref().map_or_else(|| PathBuf::new(), PathBuf::from)
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
            user: "user".to_string(),
            password: "password".to_string(),
            name: "cosmic_sync".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            max_connections: 5,
            connection_timeout: 30,
            log_queries: false,
        }
    }
}

impl DatabaseConfig {
    /// Load database configuration from environment variables or use defaults
    pub fn load() -> Self {
        let user = env::var("DB_USER").unwrap_or_else(|_| "user".to_string());
        let password = env::var("DB_PASS").unwrap_or_else(|_| "password".to_string());
        let name = env::var("DB_NAME").unwrap_or_else(|_| "cosmic_sync".to_string());
        let host = env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = env::var("DB_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(3306);
        let max_connections = env::var("DB_POOL")
            .ok()
            .and_then(|c| c.parse::<u32>().ok())
            .unwrap_or(5);
        let connection_timeout = env::var("DATABASE_CONNECTION_TIMEOUT")
            .ok()
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(30);
        let log_queries = env::var("DATABASE_LOG_QUERIES")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
            
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
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file_logging: true,
            log_file: "logs/cosmic-sync-server.log".to_string(),
            max_file_size: 10 * 1024 * 1024, // 10MB
            max_backups: 5,
        }
    }
}

impl LoggingConfig {
    /// Load logging configuration from environment variables or use defaults
    pub fn load() -> Self {
        let level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
        let file_logging = env::var("LOG_TO_FILE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);
        let log_file = env::var("LOG_FILE").unwrap_or_else(|_| "logs/cosmic-sync-server.log".to_string());
        let max_file_size = env::var("LOG_MAX_FILE_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(10 * 1024 * 1024);
        let max_backups = env::var("LOG_MAX_BACKUPS")
            .ok()
            .and_then(|b| b.parse::<usize>().ok())
            .unwrap_or(5);
            
        Self {
            level,
            file_logging,
            log_file,
            max_file_size,
            max_backups,
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
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            test_mode: false,
            debug_mode: false,
            metrics_enabled: false,
            storage_encryption: true,
            request_validation: true,
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
            
        Self {
            test_mode,
            debug_mode,
            metrics_enabled,
            storage_encryption,
            request_validation,
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

}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_type: StorageType::Database,
            s3: S3Config::default(),

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
            region: "us-east-1".to_string(),
            bucket: "cosmic-sync-files".to_string(),
            key_prefix: "files/".to_string(),
            access_key_id: None,
            secret_access_key: None,
            session_token: None,
            endpoint_url: None,
            force_path_style: false,
            use_secret_manager: false,
            secret_name: None,
            timeout_seconds: 30,
            max_retries: 3,
        }
    }
}

impl S3Config {
    /// Load S3 configuration from environment variables or use defaults
    pub fn load() -> Self {
        Self {
            region: env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            bucket: env::var("S3_BUCKET").unwrap_or_else(|_| "cosmic-sync-files".to_string()),
            key_prefix: env::var("S3_KEY_PREFIX").unwrap_or_else(|_| "files/".to_string()),
            access_key_id: env::var("AWS_ACCESS_KEY_ID").ok(),
            secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
            session_token: env::var("AWS_SESSION_TOKEN").ok(),
            endpoint_url: env::var("S3_ENDPOINT_URL").ok(),
            force_path_style: env::var("S3_FORCE_PATH_STYLE")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
            use_secret_manager: env::var("USE_AWS_SECRET_MANAGER")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
            secret_name: env::var("AWS_SECRET_NAME").ok(),
            timeout_seconds: env::var("S3_TIMEOUT_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            max_retries: env::var("S3_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
        }
    }
}

 