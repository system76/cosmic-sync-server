pub mod mysql;
pub mod memory;
pub mod mysql_models;

// MySQL specific modules with optimized implementations
mod mysql_account;
mod mysql_auth;
mod mysql_device;
mod mysql_file;
pub mod mysql_watcher;

// File storage abstraction layer
pub mod file_storage;

// Performance optimized connection pooling
pub mod pool;

// Caching layer
pub mod cache;

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use std::path::PathBuf;
use std::fs;
use std::io;
use tracing::{debug, info, warn, error, instrument};
use thiserror::Error;
use mysql_async::Opts;
use mysql_async::prelude::Queryable;

use crate::{
    models::{
        Account, Device, FileInfo, AuthToken, 
        WatcherGroup, WatcherPreset, WatcherCondition,
        FileEntry, file::FileNotice, 
        watcher::Watcher,
    },
    sync::{DeviceInfo, WatcherData, WatcherGroupData},
    config::settings::{DatabaseConfig, StorageConfig, StorageType},
    error::{SyncError, Result as AppResult},
};

use self::mysql::MySqlStorage;

/// Storage Result type for backward compatibility
pub type Result<T> = std::result::Result<T, StorageError>;



/// Optimized error types for storage operations
#[derive(Debug, Error, Clone)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(String),
    
    #[error("Connection error: {0}")]
    Connection(String),
    
    #[error("Connection pool exhausted: {0}")]
    PoolExhausted(String),
    
    #[error("Query timeout: {0}")]
    QueryTimeout(String),
    
    #[error("Transaction error: {0}")]
    Transaction(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    
    #[error("Invalid data: {0}")]
    InvalidData(String),
    
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    
    #[error("General error: {0}")]
    General(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("S3 error: {0}")]
    S3Error(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Operation not supported: {0}")]
    NotSupported(String),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

impl StorageError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, 
            StorageError::Connection(_) |
            StorageError::PoolExhausted(_) |
            StorageError::QueryTimeout(_) |
            StorageError::NetworkError(_)
        )
    }
    
    /// Get error category for metrics
    pub fn category(&self) -> &'static str {
        match self {
            StorageError::Database(_) => "database",
            StorageError::Connection(_) => "connection",
            StorageError::PoolExhausted(_) => "pool",
            StorageError::QueryTimeout(_) => "timeout",
            StorageError::Transaction(_) => "transaction",
            StorageError::NotFound(_) => "not_found",
            StorageError::SerializationError(_) => "serialization",
            StorageError::DeserializationError(_) => "deserialization",
            StorageError::InvalidData(_) => "validation",
            StorageError::PermissionDenied(_) => "permission",
            StorageError::ValidationError(_) => "validation",
            StorageError::ConfigurationError(_) => "config",
            StorageError::NetworkError(_) => "network",
            StorageError::S3Error(_) => "s3",
            StorageError::CacheError(_) => "cache",
            StorageError::NotSupported(_) => "not_supported",
            StorageError::Internal(_) => "internal",
            StorageError::NotImplemented(_) => "not_implemented",
            StorageError::General(_) => "general",
        }
    }
}

// Error conversions for better integration
impl From<sqlx::Error> for StorageError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => Self::NotFound("Record not found".to_string()),
            sqlx::Error::Database(db_err) => Self::Database(db_err.to_string()),
            sqlx::Error::Io(io_err) => Self::NetworkError(io_err.to_string()),
            sqlx::Error::PoolTimedOut => Self::PoolExhausted("Connection pool timeout".to_string()),
            sqlx::Error::PoolClosed => Self::Connection("Connection pool closed".to_string()),
            _ => Self::Database(error.to_string()),
        }
    }
}

impl From<StorageError> for SyncError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound(msg) => SyncError::NotFound(msg),
            StorageError::PermissionDenied(msg) => SyncError::Authorization(msg),
            StorageError::ValidationError(msg) => SyncError::Validation(msg),
            StorageError::ConfigurationError(msg) => SyncError::Config(msg),
            StorageError::NetworkError(msg) => SyncError::Network(msg),
            StorageError::QueryTimeout(msg) => SyncError::Timeout(msg),
            _ => SyncError::Storage(err.to_string()),
        }
    }
}

/// Performance metrics for storage operations
#[derive(Debug, Clone, Default)]
pub struct StorageMetrics {
    pub total_queries: u64,
    pub successful_queries: u64,
    pub failed_queries: u64,
    pub average_query_time_ms: f64,
    pub active_connections: u32,
    pub idle_connections: u32,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

/// Optimized file storage abstraction with performance features
#[async_trait]
pub trait FileStorage: Send + Sync {
    /// Store file data with compression support
    async fn store_file_data(&self, file_id: u64, data: Vec<u8>) -> Result<String>;
    
    /// Store file data with custom options
    async fn store_file_data_with_options(&self, file_id: u64, data: Vec<u8>, compress: bool) -> Result<String>;
    
    /// Retrieve file data with caching
    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>>;
    
    /// Delete file data with cleanup
    async fn delete_file_data(&self, file_id: u64) -> Result<()>;
    
    /// Batch delete multiple files
    async fn batch_delete_file_data(&self, file_ids: Vec<u64>) -> Result<Vec<(u64, bool)>>;
    
    /// Check if file data exists (optimized)
    async fn file_data_exists(&self, file_id: u64) -> Result<bool>;
    
    /// Get file data size without downloading
    async fn get_file_size(&self, file_id: u64) -> Result<Option<u64>>;
    
    /// Health check with detailed status
    async fn health_check(&self) -> Result<()>;
    
    /// Get storage metrics
    async fn get_metrics(&self) -> Result<StorageMetrics>;
    
    /// Get storage type identifier
    fn storage_type(&self) -> &'static str;
    
    /// Cleanup orphaned data
    async fn cleanup_orphaned_data(&self) -> Result<u64>;
}

/// Enhanced storage trait with performance optimizations
#[async_trait]
pub trait Storage: Sync + Send {
    /// Get the storage instance as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Health check with connection validation
    async fn health_check(&self) -> Result<bool>;
    
    /// Get storage metrics and performance data
    async fn get_metrics(&self) -> Result<StorageMetrics>;
    
    /// Close all connections gracefully
    async fn close(&self) -> Result<()>;
    
    // Account related methods with caching
    async fn create_account(&self, account: &Account) -> Result<()>;
    async fn get_account_by_id(&self, id: &str) -> Result<Option<Account>>;
    async fn get_account_by_email(&self, email: &str) -> Result<Option<Account>>;
    async fn get_account_by_hash(&self, account_hash: &str) -> Result<Option<Account>>;
    async fn update_account(&self, account: &Account) -> Result<()>;
    async fn delete_account(&self, account_hash: &str) -> Result<()>;
    
    // Batch account operations
    async fn batch_create_accounts(&self, accounts: &[Account]) -> Result<Vec<bool>>;
    async fn list_accounts(&self, limit: Option<u32>, offset: Option<u64>) -> Result<Vec<Account>>;
    
    // AuthToken related methods with optimized queries
    async fn create_auth_token(&self, auth_token: &AuthToken) -> Result<()>;
    async fn get_auth_token(&self, token: &str) -> Result<Option<AuthToken>>;
    async fn validate_auth_token(&self, token: &str, account_hash: &str) -> Result<bool>;
    async fn update_auth_token(&self, auth_token: &AuthToken) -> Result<()>;
    async fn delete_auth_token(&self, token: &str) -> Result<()>;
    async fn cleanup_expired_tokens(&self) -> Result<u64>;
    
    // Device related methods with connection pooling
    async fn register_device(&self, device: &Device) -> Result<()>;
    async fn get_device(&self, account_hash: &str, device_hash: &str) -> Result<Option<Device>>;
    async fn list_devices(&self, account_hash: &str) -> Result<Vec<Device>>;
    async fn update_device(&self, device: &Device) -> Result<()>;
    async fn delete_device(&self, account_hash: &str, device_hash: &str) -> Result<()>;
    async fn validate_device(&self, account_hash: &str, device_hash: &str) -> Result<bool>;
    
    // File related methods with streaming support
    async fn store_file_info(&self, file: FileInfo) -> Result<u64>;
    async fn get_file_info(&self, file_id: u64) -> Result<Option<FileInfo>>;
    async fn get_file_info_include_deleted(&self, file_id: u64) -> Result<Option<(FileInfo, bool)>>;
    async fn get_file_info_by_path(&self, account_hash: &str, file_path: &str, group_id: i32) -> Result<Option<FileInfo>>;
    async fn get_file_by_hash(&self, account_hash: &str, file_hash: &str) -> Result<Option<FileInfo>>;
    async fn list_files(&self, account_hash: &str, group_id: i32, upload_time_from: Option<i64>) -> Result<Vec<FileInfo>>;
    async fn list_files_except_device(&self, account_hash: &str, group_id: i32, exclude_device_hash: &str, upload_time_from: Option<i64>) -> Result<Vec<FileInfo>>;
    async fn delete_file(&self, account_hash: &str, file_id: u64) -> Result<()>;
    
    // Optimized file search methods
    async fn find_file_by_path_and_name(&self, account_hash: &str, file_path: &str, filename: &str, revision: i64) -> Result<Option<FileInfo>>;
    async fn find_file_by_criteria(&self, account_hash: &str, group_id: i32, watcher_id: i32, file_path: &str, filename: &str) -> Result<Option<FileInfo>>;
    async fn check_file_exists(&self, file_id: u64) -> Result<(bool, bool)>;
    
    // Batch file operations
    async fn batch_store_files(&self, files: Vec<FileInfo>) -> Result<Vec<(u64, bool)>>;
    async fn batch_delete_files(&self, account_hash: &str, file_ids: Vec<u64>) -> Result<Vec<bool>>;
    
    // FileData related methods (optimized for large files)
    async fn store_file_data(&self, file_id: u64, data_bytes: Vec<u8>) -> Result<()>;
    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>>;
    
    // Streaming support for large files
    async fn store_file_data_stream(&self, file_id: u64, data_stream: Box<dyn futures::Stream<Item = Result<bytes::Bytes>> + Send + Unpin>) -> Result<()>;
    async fn get_file_data_stream(&self, file_id: u64) -> Result<Option<Box<dyn futures::Stream<Item = Result<bytes::Bytes>> + Send + Unpin>>>;
    
    // Encryption related methods with key caching
    async fn store_encryption_key(&self, account_hash: &str, encryption_key: &str) -> Result<()>;
    async fn get_encryption_key(&self, account_hash: &str) -> Result<Option<String>>;
    async fn update_encryption_key(&self, account_hash: &str, encryption_key: &str) -> Result<()>;
    async fn delete_encryption_key(&self, account_hash: &str) -> Result<()>;
    
    // WatcherGroup related methods with indexing
    async fn register_watcher_group(&self, account_hash: &str, device_hash: &str, watcher_group: &WatcherGroup) -> Result<i32>;
    async fn get_watcher_groups(&self, account_hash: &str) -> Result<Vec<WatcherGroup>>;
    async fn get_user_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<Option<WatcherGroup>>;
    async fn update_watcher_group(&self, account_hash: &str, watcher_group: &WatcherGroup) -> Result<()>;
    async fn delete_watcher_group(&self, account_hash: &str, group_id: i32) -> Result<()>;
    
    // WatcherPreset related methods (proto-based only)
    
    // WatcherCondition related methods
    async fn register_watcher_condition(&self, account_hash: &str, condition: &WatcherCondition) -> Result<i64>;
    async fn get_watcher_condition(&self, condition_id: i64) -> Result<Option<WatcherCondition>>;
    async fn get_watcher_conditions_by_watcher(&self, account_hash: &str, watcher_id: i32) -> Result<Vec<WatcherCondition>>;
    async fn update_watcher_condition(&self, condition: &WatcherCondition) -> Result<()>;
    async fn delete_watcher_condition(&self, condition_id: i64) -> Result<()>;
    
    // Optimized duplicate check methods
    async fn check_duplicate_watcher_group(&self, account_hash: &str, local_group_id: i32) -> Result<Option<i32>>;
    async fn check_duplicate_watcher(&self, account_hash: &str, local_watcher_id: i32) -> Result<Option<i32>>;
    
    // FileNotice related methods
    async fn store_file_notice(&self, file_notice: &FileNotice) -> Result<()>;
    async fn get_file_notices(&self, account_hash: &str, device_hash: &str) -> Result<Vec<FileNotice>>;
    async fn delete_file_notice(&self, account_hash: &str, device_hash: &str, file_id: u64) -> Result<()>;
    
    // Extended watcher methods
    async fn get_watcher_group_by_account_and_id(&self, account_hash: &str, group_id: i32) -> Result<Option<WatcherGroupData>>;
    async fn find_watcher_by_folder(&self, account_hash: &str, group_id: i32, folder: &str) -> Result<Option<i32>>;
    async fn create_watcher_with_conditions(&self, account_hash: &str, group_id: i32, watcher_data: &WatcherData, timestamp: i64) -> Result<i32>;
    async fn get_watcher_by_group_and_id(&self, account_hash: &str, group_id: i32, watcher_id: i32) -> Result<Option<WatcherData>>;
    async fn get_watcher_preset(&self, account_hash: &str) -> Result<Vec<String>>;
    async fn register_watcher_preset_proto(&self, account_hash: &str, device_hash: &str, presets: Vec<String>) -> Result<()>;
    async fn update_watcher_preset_proto(&self, account_hash: &str, device_hash: &str, presets: Vec<String>) -> Result<()>;
    async fn get_server_group_id(&self, account_hash: &str, client_group_id: i32) -> Result<Option<i32>>;
    async fn get_server_ids(&self, account_hash: &str, client_group_id: i32, client_watcher_id: i32) -> Result<Option<(i32, i32)>>;
    async fn get_client_watcher_id(&self, account_hash: &str, server_group_id: i32, server_watcher_id: i32) -> Result<Option<(i32, i32)>>;
    async fn create_watcher_condition(&self, condition: &WatcherCondition) -> Result<i64>;
    async fn get_watcher_conditions(&self, account_hash: &str, watcher_id: i32) -> Result<Vec<WatcherCondition>>;
    async fn delete_all_watcher_conditions(&self, watcher_id: i32) -> Result<()>;
    async fn save_watcher_conditions(&self, watcher_id: i32, conditions: &[WatcherCondition]) -> Result<()>;
    
    // Version management methods
    /// Get file history for a specific file path
    async fn get_file_history(&self, account_hash: &str, file_path: &str, group_id: i32) -> Result<Vec<crate::models::file::SyncFile>>;
    
    /// Get file versions by file ID
    async fn get_file_versions_by_id(&self, account_hash: &str, file_id: u64) -> Result<Vec<crate::models::file::SyncFile>>;
    
    /// Get specific file version by revision
    async fn get_file_by_revision(&self, account_hash: &str, file_id: u64, revision: i64) -> Result<crate::models::file::SyncFile>;
    
    /// Store a file version (for creating new versions)
    async fn store_file(&self, file: &crate::models::file::SyncFile) -> Result<()>;
    
    /// Get all devices for an account (for broadcasting)
    async fn get_devices_for_account(&self, account_hash: &str) -> Result<Vec<Device>>;
}

/// Optimized storage factory with connection pooling
pub struct StorageFactory;

impl StorageFactory {
    /// Create MySQL storage
    #[instrument(skip(config))]
    pub async fn create_mysql_storage(config: &DatabaseConfig) -> AppResult<impl Storage> {
        info!("Creating optimized MySQL storage");
        
        let host = config.host.clone();
        let port = config.port;
        let user = config.user.clone();
        let password = config.password.clone();
        let database = config.name.clone();
        
        // 연결 URL 생성 (secure_auth=false로 SSL 비활성화)
        let connection_url = format!("mysql://{}:{}@{}:{}/{}?secure_auth=false", user, password, host, port, database);
        
        // Opts 생성
        let opts = Opts::from_url(&connection_url)
            .map_err(|e| SyncError::Storage(format!("Failed to parse MySQL connection URL: {}", e)))?;
        
        info!("MySQL connection options: {:?}", opts);
        
        // MySqlStorage 생성
        let storage = MySqlStorage::new(opts)
            .map_err(|e| SyncError::Storage(format!("Failed to create MySQL storage: {}", e)))?;
        
        // 스키마 초기화 명시적 호출
        match storage.init_schema().await {
            Ok(_) => info!("✅ Database schema initialized successfully"),
            Err(e) => {
                error!("❌ Failed to initialize database schema: {}", e);
                // 스키마 초기화 실패는 경고만 하고 계속 진행
            }
        }
        
        // 트랜잭션 자동 커밋 설정 확인
        let pool = storage.get_pool();
        let mut conn = pool.get_conn().await
            .map_err(|e| SyncError::Storage(format!("Failed to get connection: {}", e)))?;
            
        match conn.query_first::<String, _>("SELECT @@autocommit").await {
            Ok(Some(value)) => info!("✅ MySQL autocommit setting: {}", value),
            Ok(None) => warn!("⚠️ Could not determine autocommit setting"),
            Err(e) => warn!("⚠️ Failed to check autocommit setting: {}", e),
        }
        
        info!("✅ MySQL storage created successfully");
        
        Ok(storage)
    }
    
    /// Create memory storage for testing
    pub fn create_memory_storage() -> memory::MemoryStorage {
        info!("Creating memory storage for testing");
        memory::MemoryStorage::new()
    }
}

/// Optimized storage initialization
#[instrument(skip(config))]
pub async fn init_storage(config: &DatabaseConfig) -> AppResult<Arc<dyn Storage>> {
    info!("Initializing optimized storage layer");
    
    let storage = StorageFactory::create_mysql_storage(config).await?;
    
    // 스키마 초기화는 create_mysql_storage에서 이미 수행됨
    
    // Validate storage health
    storage.health_check().await
        .map_err(|e| SyncError::Storage(format!("Storage health check failed: {}", e)))?;
    
    info!("✅ Storage layer initialized successfully");
    Ok(Arc::new(storage))
}

/// Create optimized file storage instance
#[instrument(skip(config))]
pub async fn create_file_storage(config: &StorageConfig) -> AppResult<Arc<dyn FileStorage>> {
    info!("Creating optimized file storage");
    
    let file_storage: Arc<dyn FileStorage> = match config.storage_type {
        StorageType::Database => {
            info!("Using database blob storage");
            Arc::new(file_storage::DatabaseFileStorage::new())
        },
        StorageType::S3 => {
            info!("Using S3 file storage");
            Arc::new(file_storage::S3FileStorage::new(&config.s3).await
                .map_err(|e| SyncError::Storage(format!("Failed to create S3 storage: {}", e)))?)
        },
    };
    
    // Validate file storage health
    file_storage.health_check().await
        .map_err(|e| SyncError::Storage(format!("File storage health check failed: {}", e)))?;
    
    info!("✅ File storage created successfully: {}", file_storage.storage_type());
    Ok(file_storage)
}

/// Create file storage with existing MySQL storage
#[instrument(skip(config, mysql_storage))]
pub async fn create_file_storage_with_mysql(
    config: &StorageConfig, 
    mysql_storage: Arc<mysql::MySqlStorage>
) -> AppResult<Arc<dyn FileStorage>> {
    info!("Creating file storage with existing MySQL connection");
    
    let file_storage: Arc<dyn FileStorage> = match config.storage_type {
        StorageType::Database => {
            Arc::new(file_storage::DatabaseFileStorage::with_mysql_storage(mysql_storage))
        },
        StorageType::S3 => {
            Arc::new(file_storage::S3FileStorage::new(&config.s3).await
                .map_err(|e| SyncError::Storage(format!("Failed to create S3 storage: {}", e)))?)
        },
    };
    
    info!("✅ File storage with MySQL created successfully");
    Ok(file_storage)
}

/// Performance monitoring for storage operations
pub struct StorageMonitor {
    metrics: std::sync::Arc<std::sync::RwLock<StorageMetrics>>,
}

impl StorageMonitor {
    pub fn new() -> Self {
        Self {
            metrics: std::sync::Arc::new(std::sync::RwLock::new(StorageMetrics::default())),
        }
    }
    
    pub fn record_query(&self, duration: Duration, success: bool) {
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.total_queries += 1;
            if success {
                metrics.successful_queries += 1;
            } else {
                metrics.failed_queries += 1;
            }
            
            // Update average query time using exponential moving average
            let duration_ms = duration.as_millis() as f64;
            metrics.average_query_time_ms = if metrics.total_queries == 1 {
                duration_ms
            } else {
                (metrics.average_query_time_ms * 0.9) + (duration_ms * 0.1)
            };
        }
    }
    
    pub fn get_metrics(&self) -> StorageMetrics {
        self.metrics.read().unwrap().clone()
    }
}

impl Default for StorageMonitor {
    fn default() -> Self {
        Self::new()
    }
}
