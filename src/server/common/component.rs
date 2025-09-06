use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Component status enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ComponentStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

impl Default for ComponentStatus {
    fn default() -> Self {
        ComponentStatus::Stopped
    }
}

/// Component metrics trait for extensible metrics collection
pub trait ComponentMetrics: Debug + Clone + Send + Sync + 'static {
    fn reset(&mut self);
    fn merge(&mut self, other: &Self);
}

/// Component health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub is_healthy: bool,
    pub message: Option<String>,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub dependencies: std::collections::HashMap<String, bool>,
}

impl Default for ComponentHealth {
    fn default() -> Self {
        Self {
            is_healthy: false,
            message: None,
            last_check: chrono::Utc::now(),
            dependencies: std::collections::HashMap::new(),
        }
    }
}

/// Generic component trait that all services should implement
#[async_trait]
pub trait Component: Send + Sync + Debug + 'static {
    type Config: Clone + Debug + Send + Sync + 'static;
    type Metrics: ComponentMetrics;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get component name for logging and identification
    fn name(&self) -> &'static str;

    /// Get current component configuration
    fn config(&self) -> &Self::Config;

    /// Start the component
    async fn start(&mut self) -> Result<(), Self::Error>;

    /// Stop the component gracefully
    async fn stop(&mut self) -> Result<(), Self::Error>;

    /// Check if component is healthy
    async fn health_check(&self) -> ComponentHealth;

    /// Get current status
    fn status(&self) -> ComponentStatus;

    /// Get current metrics
    fn metrics(&self) -> Self::Metrics;

    /// Handle configuration update (optional)
    async fn update_config(&mut self, config: Self::Config) -> Result<(), Self::Error> {
        let _ = config;
        Ok(())
    }

    /// Restart the component (default implementation)
    async fn restart(&mut self) -> Result<(), Self::Error> {
        if self.status() != ComponentStatus::Stopped {
            self.stop().await?;
        }
        self.start().await
    }
}
