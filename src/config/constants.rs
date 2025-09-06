// Centralized configuration constants

// Network / gRPC
pub const DEFAULT_GRPC_HOST: &str = "0.0.0.0";
pub const DEFAULT_GRPC_PORT: u16 = 50051;
// HTTP (Actix) default port retained for compatibility with HTTP endpoints
pub const DEFAULT_HTTP_PORT: u16 = 8080;
pub const MIN_VALID_PORT: u16 = 1024;
pub const MAX_VALID_PORT: u16 = 65535;

// Server
pub const DEFAULT_WORKER_THREADS: usize = 4;
pub const DEFAULT_AUTH_TOKEN_EXPIRY_HOURS: i64 = 24;
/// 50MB
pub const DEFAULT_MAX_FILE_SIZE_BYTES: usize = 50 * 1024 * 1024;
pub const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 100;

// Database (MySQL)
pub const DEFAULT_DB_USER: &str = "user";
pub const DEFAULT_DB_PASS: &str = "password";
pub const DEFAULT_DB_NAME: &str = "cosmic_sync";
pub const DEFAULT_DB_HOST: &str = "localhost";
pub const DEFAULT_DB_PORT: u16 = 3306;
pub const DEFAULT_DB_POOL: u32 = 5;
pub const DEFAULT_DB_CONN_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_DB_LOG_QUERIES: bool = false;

// Logging
pub const DEFAULT_LOG_LEVEL: &str = "info";
pub const DEFAULT_LOG_TO_FILE: bool = true;
pub const DEFAULT_LOG_FILE: &str = "logs/cosmic-sync-server.log";
/// 10MB
pub const DEFAULT_LOG_MAX_FILE_SIZE_BYTES: usize = 10 * 1024 * 1024;
pub const DEFAULT_LOG_MAX_BACKUPS: usize = 5;

// S3
pub const DEFAULT_S3_REGION: &str = "us-east-2";
pub const DEFAULT_S3_BUCKET: &str = "cosmic-sync-files";
pub const DEFAULT_S3_KEY_PREFIX: &str = "files/";
pub const DEFAULT_S3_FORCE_PATH_STYLE: bool = false;
pub const DEFAULT_S3_USE_SECRET_MANAGER: bool = false;
pub const DEFAULT_S3_TIMEOUT_SECONDS: u64 = 30;
pub const DEFAULT_S3_MAX_RETRIES: u32 = 3;

// Timeouts (seconds) for HTTP compatibility (if used via Actix)
pub const HTTP_KEEPALIVE_SECS: u64 = 60;
pub const HTTP2_KEEPALIVE_INTERVAL_SECS: u64 = 30;
pub const HTTP2_KEEPALIVE_TIMEOUT_SECS: u64 = 90;

// Heartbeat
pub const DEFAULT_HEARTBEAT_INTERVAL_SECS: u64 = 10;

// CORS
pub const DEFAULT_CORS_MAX_AGE_SECS: u64 = 3600;

// Retention/TTL
pub const DEFAULT_FILE_TTL_SECS: i64 = 14 * 24 * 3600; // 14 days
pub const DEFAULT_MAX_FILE_REVISIONS: i32 = 10;
