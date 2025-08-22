use crate::server::common::{Component, ComponentHealth, ComponentStatus, ComponentMetrics};
use crate::server::app_state::AppState;
use crate::config::settings::ServerConfig;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error, debug};
use serde::{Serialize, Deserialize};

/// Metrics specific to the sync server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncServerMetrics {
    pub total_requests: u64,
    pub active_connections: u32,
    pub uptime_seconds: u64,
    pub sync_operations: u64,
    pub failed_operations: u64,
    pub data_transferred_bytes: u64,
}

impl Default for SyncServerMetrics {
    fn default() -> Self {
        Self {
            total_requests: 0,
            active_connections: 0,
            uptime_seconds: 0,
            sync_operations: 0,
            failed_operations: 0,
            data_transferred_bytes: 0,
        }
    }
}

impl ComponentMetrics for SyncServerMetrics {
    fn reset(&mut self) {
        *self = Self::default();
    }
    
    fn merge(&mut self, other: &Self) {
        self.total_requests += other.total_requests;
        self.active_connections = other.active_connections; // Use latest value
        self.uptime_seconds = other.uptime_seconds; // Use latest value
        self.sync_operations += other.sync_operations;
        self.failed_operations += other.failed_operations;
        self.data_transferred_bytes += other.data_transferred_bytes;
    }
}

/// Sync server component error types
#[derive(Debug, thiserror::Error)]
pub enum SyncServerError {
    #[error("Server startup failed: {0}")]
    StartupError(String),
    #[error("Server shutdown failed: {0}")]
    ShutdownError(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Internal error: {0}")]
    InternalError(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Sync server component that implements the Component trait
pub struct SyncServerComponent {
    config: ServerConfig,
    app_state: Arc<AppState>,
    status: ComponentStatus,
    metrics: SyncServerMetrics,
    start_time: Option<chrono::DateTime<chrono::Utc>>,
}

impl std::fmt::Debug for SyncServerComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncServerComponent")
            .field("config", &self.config)
            .field("status", &self.status)
            .field("metrics", &self.metrics)
            .field("start_time", &self.start_time)
            .field("app_state", &"AppState { ... }")
            .finish()
    }
}

impl SyncServerComponent {
    pub async fn new(config: ServerConfig) -> Result<Self, SyncServerError> {
        info!("🔧 Creating SyncServerComponent");
        
        let app_state = Arc::new(
            AppState::new_with_storage_and_server_config(
                // In component mode we don't have external storage; fall back to legacy helper to honor config
                crate::server::startup::init_storage_legacy(config.storage_path.clone()).await,
                &config,
            )
            .await
            .map_err(|e| SyncServerError::ConfigError(e.to_string()))?
        );
        
        Ok(Self {
            config,
            app_state,
            status: ComponentStatus::Stopped,
            metrics: SyncServerMetrics::default(),
            start_time: None,
        })
    }
    
    /// Update metrics based on current state
    async fn update_metrics(&mut self) {
        if let Some(start_time) = self.start_time {
            let uptime = chrono::Utc::now() - start_time;
            self.metrics.uptime_seconds = uptime.num_seconds() as u64;
        }
        
        // Get active connections from app state
        let stats = self.app_state.client_store.get_connection_stats().await;
        self.metrics.active_connections = stats.values().sum::<usize>() as u32;
    }
}

#[async_trait]
impl Component for SyncServerComponent {
    type Config = ServerConfig;
    type Metrics = SyncServerMetrics;
    type Error = SyncServerError;
    
    fn name(&self) -> &'static str {
        "SyncServer"
    }
    
    fn config(&self) -> &Self::Config {
        &self.config
    }
    
    async fn start(&mut self) -> Result<(), Self::Error> {
        info!("🚀 Starting SyncServerComponent");
        
        self.status = ComponentStatus::Starting;
        self.start_time = Some(chrono::Utc::now());
        
        // Start client cleanup task
        self.app_state.start_client_cleanup_task();
        // Start retention cleanup task
        self.app_state.start_retention_cleanup_task();
        
        // Here we would start the actual gRPC server
        // For now, we'll simulate a successful start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        self.status = ComponentStatus::Running;
        
        info!("✅ SyncServerComponent started successfully");
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<(), Self::Error> {
        info!("🛑 Stopping SyncServerComponent");
        
        self.status = ComponentStatus::Stopping;
        
        // Graceful shutdown logic would go here
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        
        self.status = ComponentStatus::Stopped;
        self.start_time = None;
        
        info!("✅ SyncServerComponent stopped successfully");
        Ok(())
    }
    
    async fn health_check(&self) -> ComponentHealth {
        let mut health = ComponentHealth::default();
        health.last_check = chrono::Utc::now();
        
        // Check if component is running
        let is_running = self.status == ComponentStatus::Running;
        
        // Check dependencies
        health.dependencies.insert("app_state".to_string(), true);
        health.dependencies.insert("database".to_string(), true); // Would check actual DB
        health.dependencies.insert("storage".to_string(), true);  // Would check actual storage
        
        health.is_healthy = is_running && health.dependencies.values().all(|&v| v);
        
        if !health.is_healthy {
            health.message = Some("One or more dependencies are unhealthy".to_string());
        }
        
        health
    }
    
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }
    
    fn metrics(&self) -> Self::Metrics {
        let mut metrics = self.metrics.clone();
        
        // Update uptime if running
        if let Some(start_time) = self.start_time {
            let uptime = chrono::Utc::now() - start_time;
            metrics.uptime_seconds = uptime.num_seconds() as u64;
        }
        
        // Note: active_connections will be updated by background tasks
        // We return the cached value here
        
        metrics
    }
    
    async fn update_config(&mut self, config: Self::Config) -> Result<(), Self::Error> {
        info!("🔧 Updating SyncServerComponent configuration");
        
        // Validate new configuration
        if let Err(e) = config.address() {
            return Err(SyncServerError::ConfigError(format!("Invalid address: {}", e)));
        }
        
        self.config = config;
        
        info!("✅ SyncServerComponent configuration updated");
        Ok(())
    }
} 