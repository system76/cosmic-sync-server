// Re-export core functionality for external use
pub use async_trait::async_trait;
pub use sqlx;
pub use tonic;

// Core module definitions with optimized structure
pub mod config;
pub mod models;
pub mod storage;
pub mod server;
pub mod sync;
pub mod services;
pub mod handlers;
pub mod auth;
pub mod utils;
pub mod error;

// New optimized modules
pub mod monitoring;
pub mod validation;
pub mod container;
pub mod domain;

// Unified error handling
pub use error::{SyncError, Result};
pub type AppResult<T> = Result<T>;

// Essential re-exports for convenience
pub use server::{
    startup::{start_server, start_server_with_storage},
    service::SyncServiceImpl,
    app_state::AppState,
};

// Container and dependency injection
pub use container::{
    AppContainer, ContainerBuilder, ServiceRegistry,
    ServiceProvider, StorageProvider, AuthServiceProvider,
    DeviceServiceProvider, FileServiceProvider,
    EncryptionServiceProvider,
};

pub use config::settings::{Config, ServerConfig, DatabaseConfig};

// Storage abstractions with performance traits
pub use storage::{
    Storage, StorageError, 
    Result as StorageResult,
    mysql::MySqlStorage,
    memory::MemoryStorage,
    init_storage,
};

// Model exports
pub use models::{
    account::Account,
    auth::AuthToken, 
    device::Device,
    file::FileInfo,
    watcher::{WatcherGroup, Watcher, Condition},
};

// gRPC service exports
pub use sync::sync_service_server::SyncService;

// Authentication services
pub use auth::oauth::OAuthService;

// Monitoring and metrics
pub use monitoring::{PerformanceMonitor, SystemInfo};

// Version and build information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NAME: &str = env!("CARGO_PKG_NAME");

// Feature flags for conditional compilation
pub mod features {
    pub const REDIS_CACHE: bool = cfg!(feature = "redis-cache");
    pub const METRICS: bool = cfg!(feature = "metrics");
    pub const COMPRESSION: bool = cfg!(feature = "compression");
    pub const REFLECTION: bool = cfg!(feature = "reflection");
}

// Prelude for common imports
pub mod prelude {
    pub use crate::{
        Result, SyncError, AppResult,
        Storage, Account, Device, FileInfo, AuthToken,
        ServerConfig, DatabaseConfig,
        AppContainer, ContainerBuilder,
        VERSION, NAME,
    };
    
    pub use async_trait::async_trait;
    pub use serde::{Serialize, Deserialize};
    pub use tracing::{debug, info, warn, error, instrument};
    pub use tokio;
}

// Configuration helpers
pub mod config_helpers {
    use std::env;
    use crate::{Result, SyncError};
    
    /// Parse environment variable with type conversion
    pub fn parse_env_var<T>(key: &str, default: T) -> Result<T>
    where
        T: std::str::FromStr + Clone,
        T::Err: std::fmt::Display,
    {
        match env::var(key) {
            Ok(value) => value.parse().map_err(|e| {
                SyncError::Config(format!("Failed to parse {}: {}", key, e))
            }),
            Err(_) => Ok(default),
        }
    }
    
    /// Get environment variable with default value
    pub fn get_env_var(key: &str, default: &str) -> String {
        env::var(key).unwrap_or_else(|_| default.to_string())
    }
    
    /// Get required environment variable
    pub fn get_required_env_var(key: &str) -> Result<String> {
        env::var(key).map_err(|_| {
            SyncError::Config(format!("Required environment variable {} is not set", key))
        })
    }
}

// Performance utilities
pub mod performance {
    use std::time::{Duration, Instant};
    
    /// Measure execution time of a function
    pub async fn measure_async<F, Fut, T>(f: F) -> (T, Duration)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let start = Instant::now();
        let result = f().await;
        let duration = start.elapsed();
        (result, duration)
    }
    
    /// Format duration in human readable form
    pub fn format_duration(duration: Duration) -> String {
        let ms = duration.as_millis();
        if ms < 1000 {
            format!("{}ms", ms)
        } else {
            format!("{:.2}s", duration.as_secs_f64())
        }
    }
}

// Common type aliases
pub type Timestamp = i64;
pub type FileId = u64;
pub type GroupId = i32;
pub type WatcherId = i32;
pub type AccountHash = String;
pub type DeviceHash = String;

// Constants
pub mod constants {
    /// Default page size for pagination
    pub const DEFAULT_PAGE_SIZE: u32 = 50;
    
    /// Maximum page size
    pub const MAX_PAGE_SIZE: u32 = 1000;
    
    /// Default cache TTL in seconds
    pub const DEFAULT_CACHE_TTL: u32 = 3600;
    
    /// Maximum file size (100MB)
    pub const MAX_FILE_SIZE: usize = 100 * 1024 * 1024;
    
    /// Authentication token expiry (24 hours)
    pub const DEFAULT_TOKEN_EXPIRY_HOURS: i64 = 24;
    
    /// Maximum concurrent connections
    pub const MAX_CONCURRENT_CONNECTIONS: usize = 10000;
    
    /// Request timeout in seconds
    pub const REQUEST_TIMEOUT_SECONDS: u64 = 120;
}

// Health check utilities
pub mod health {
    use crate::Result;
    use serde::{Serialize, Deserialize};
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct HealthStatus {
        pub status: String,
        pub timestamp: i64,
        pub uptime_seconds: u64,
        pub version: String,
        pub components: std::collections::HashMap<String, ComponentHealth>,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ComponentHealth {
        pub healthy: bool,
        pub response_time_ms: Option<u64>,
        pub error: Option<String>,
    }
    
    impl HealthStatus {
        pub fn new() -> Self {
            Self {
                status: "unknown".to_string(),
                timestamp: chrono::Utc::now().timestamp(),
                uptime_seconds: 0,
                version: crate::VERSION.to_string(),
                components: std::collections::HashMap::new(),
            }
        }
        
        pub fn is_healthy(&self) -> bool {
            self.status == "healthy" && 
            self.components.values().all(|c| c.healthy)
        }
    }
    
    impl Default for HealthStatus {
        fn default() -> Self {
            Self::new()
        }
    }
}
