// Trait-based abstractions for enhanced flexibility and extensibility
// This module provides a comprehensive set of traits and interfaces
// that enable plugin systems, middleware, and extensible architectures

pub mod plugins;
pub mod middleware;
pub mod services;
pub mod providers;
pub mod interceptors;
pub mod extensibility;

// Re-export core abstractions
pub use plugins::*;
pub use middleware::*;
pub use services::*;
pub use providers::*;
pub use interceptors::*;
pub use extensibility::*;

use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use crate::error::Result;

/// Core plugin trait that all plugins must implement
/// Provides lifecycle management and metadata access
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get plugin metadata
    fn metadata(&self) -> PluginMetadata;
    
    /// Initialize the plugin with configuration
    async fn initialize(&mut self, config: PluginConfig) -> Result<()>;
    
    /// Start the plugin (called after all plugins are initialized)
    async fn start(&mut self) -> Result<()>;
    
    /// Stop the plugin gracefully
    async fn stop(&mut self) -> Result<()>;
    
    /// Get plugin health status
    async fn health_check(&self) -> Result<PluginHealth>;
    
    /// Handle configuration updates
    async fn configure(&mut self, config: PluginConfig) -> Result<()>;
}

/// Plugin metadata containing essential information
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub dependencies: Vec<String>,
    pub capabilities: Vec<String>,
}

/// Plugin configuration passed during initialization
pub type PluginConfig = HashMap<String, serde_json::Value>;

/// Plugin health information
#[derive(Debug, Clone)]
pub struct PluginHealth {
    pub status: HealthStatus,
    pub message: Option<String>,
    pub last_check: i64,
}

/// Health status enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Unhealthy,
    Unknown,
}

/// Service provider trait for dependency injection
#[async_trait]
pub trait ServiceProvider: Send + Sync {
    /// Get service by type identifier
    async fn get_service(&self, service_type: &str) -> Result<Box<dyn Any + Send + Sync>>;
    
    /// Check if service is available
    async fn has_service(&self, service_type: &str) -> bool;
    
    /// Register a service implementation
    async fn register_service(&mut self, service_type: String, service: Box<dyn Any + Send + Sync>) -> Result<()>;
    
    /// List all available services
    async fn list_services(&self) -> Vec<String>;
}

/// Configuration provider trait for dynamic configuration management
#[async_trait]
pub trait ConfigurationProvider: Send + Sync {
    /// Get configuration value by key
    async fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>>;
    
    /// Set configuration value
    async fn set<T: serde::Serialize>(&mut self, key: String, value: T) -> Result<()>;
    
    /// Check if configuration key exists
    async fn exists(&self, key: &str) -> bool;
    
    /// Get all configuration keys
    async fn keys(&self) -> Vec<String>;
    
    /// Watch for configuration changes
    async fn watch(&self, key: &str) -> Result<Box<dyn ConfigurationWatcher>>;
}

/// Configuration change watcher
#[async_trait]
pub trait ConfigurationWatcher: Send + Sync {
    /// Wait for next configuration change
    async fn next_change(&mut self) -> Result<ConfigurationChange>;
}

/// Configuration change event
#[derive(Debug, Clone)]
pub struct ConfigurationChange {
    pub key: String,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub timestamp: i64,
}

/// Event system trait for decoupled communication
#[async_trait]
pub trait EventSystem: Send + Sync {
    /// Publish an event
    async fn publish<T: Send + Sync + Clone>(&self, event: T) -> Result<()>;
    
    /// Subscribe to events of a specific type
    async fn subscribe<T: Send + Sync + Clone + 'static>(
        &self,
        handler: Box<dyn EventHandler<T>>,
    ) -> Result<SubscriptionHandle>;
    
    /// Unsubscribe from events
    async fn unsubscribe(&self, handle: SubscriptionHandle) -> Result<()>;
}

/// Event handler trait
#[async_trait]
pub trait EventHandler<T>: Send + Sync {
    /// Handle an event
    async fn handle(&self, event: T) -> Result<()>;
}

/// Subscription handle for event system
pub type SubscriptionHandle = String;

/// Registry trait for managing collections of items
#[async_trait]
pub trait Registry<T>: Send + Sync {
    /// Register an item
    async fn register(&mut self, key: String, item: T) -> Result<()>;
    
    /// Unregister an item
    async fn unregister(&mut self, key: &str) -> Result<Option<T>>;
    
    /// Get an item by key
    async fn get(&self, key: &str) -> Option<&T>;
    
    /// List all registered keys
    async fn keys(&self) -> Vec<String>;
    
    /// Check if key is registered
    async fn contains(&self, key: &str) -> bool;
    
    /// Clear all registrations
    async fn clear(&mut self) -> Result<()>;
}

/// Factory trait for creating instances
pub trait Factory<T> {
    /// Create a new instance
    fn create(&self) -> Result<T>;
    
    /// Create with configuration
    fn create_with_config(&self, config: PluginConfig) -> Result<T>;
}

/// Lifecycle management trait
#[async_trait]
pub trait Lifecycle: Send + Sync {
    /// Initialize the component
    async fn initialize(&mut self) -> Result<()>;
    
    /// Start the component
    async fn start(&mut self) -> Result<()>;
    
    /// Stop the component
    async fn stop(&mut self) -> Result<()>;
    
    /// Get current lifecycle state
    fn state(&self) -> LifecycleState;
}

/// Lifecycle states
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleState {
    Created,
    Initialized,
    Starting,
    Started,
    Stopping,
    Stopped,
    Failed(String),
}

/// Configurable trait for components that can be configured
#[async_trait]
pub trait Configurable: Send + Sync {
    /// Apply configuration
    async fn configure(&mut self, config: PluginConfig) -> Result<()>;
    
    /// Get current configuration
    async fn get_configuration(&self) -> Result<PluginConfig>;
    
    /// Validate configuration
    async fn validate_configuration(&self, config: &PluginConfig) -> Result<()>;
}

/// Observable trait for components that can be monitored
#[async_trait]
pub trait Observable: Send + Sync {
    /// Get metrics
    async fn get_metrics(&self) -> Result<HashMap<String, f64>>;
    
    /// Get health status
    async fn get_health(&self) -> Result<PluginHealth>;
    
    /// Subscribe to metric updates
    async fn subscribe_metrics(&self, callback: Box<dyn MetricsCallback>) -> Result<SubscriptionHandle>;
}

/// Metrics callback trait
#[async_trait]
pub trait MetricsCallback: Send + Sync {
    /// Called when metrics are updated
    async fn on_metrics_update(&self, metrics: HashMap<String, f64>) -> Result<()>;
}

/// Extension point trait for providing extension capabilities
#[async_trait]
pub trait ExtensionPoint: Send + Sync {
    /// Get extension point name
    fn name(&self) -> &'static str;
    
    /// Register an extension
    async fn register_extension(&mut self, extension: Box<dyn Extension>) -> Result<()>;
    
    /// Get all registered extensions
    async fn get_extensions(&self) -> Vec<&dyn Extension>;
    
    /// Invoke extensions with context
    async fn invoke_extensions(&self, context: ExtensionContext) -> Result<Vec<ExtensionResult>>;
}

/// Extension trait for implementing extensions
#[async_trait]
pub trait Extension: Send + Sync {
    /// Get extension metadata
    fn metadata(&self) -> ExtensionMetadata;
    
    /// Execute the extension
    async fn execute(&self, context: ExtensionContext) -> Result<ExtensionResult>;
    
    /// Check if extension supports the given context
    async fn supports(&self, context: &ExtensionContext) -> bool;
}

/// Extension metadata
#[derive(Debug, Clone)]
pub struct ExtensionMetadata {
    pub name: String,
    pub version: String,
    pub priority: i32,
    pub capabilities: Vec<String>,
}

/// Extension execution context
pub type ExtensionContext = HashMap<String, serde_json::Value>;

/// Extension execution result
#[derive(Debug, Clone)]
pub struct ExtensionResult {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub message: Option<String>,
}

/// Resource manager trait for managing resources
#[async_trait]
pub trait ResourceManager: Send + Sync {
    /// Allocate a resource
    async fn allocate(&mut self, resource_type: &str, config: PluginConfig) -> Result<ResourceHandle>;
    
    /// Release a resource
    async fn release(&mut self, handle: ResourceHandle) -> Result<()>;
    
    /// Get resource status
    async fn get_status(&self, handle: ResourceHandle) -> Result<ResourceStatus>;
    
    /// List all allocated resources
    async fn list_resources(&self) -> Vec<ResourceHandle>;
}

/// Resource handle for tracking allocated resources
pub type ResourceHandle = String;

/// Resource status information
#[derive(Debug, Clone)]
pub struct ResourceStatus {
    pub handle: ResourceHandle,
    pub resource_type: String,
    pub status: String,
    pub allocated_at: i64,
    pub last_accessed: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_metadata() {
        let metadata = PluginMetadata {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "Test plugin".to_string(),
            author: "Test Author".to_string(),
            dependencies: vec!["dep1".to_string()],
            capabilities: vec!["cap1".to_string()],
        };
        
        assert_eq!(metadata.name, "test-plugin");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.dependencies.len(), 1);
        assert_eq!(metadata.capabilities.len(), 1);
    }

    #[test]
    fn test_plugin_health() {
        let health = PluginHealth {
            status: HealthStatus::Healthy,
            message: Some("All good".to_string()),
            last_check: chrono::Utc::now().timestamp(),
        };
        
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.message.is_some());
        assert!(health.last_check > 0);
    }

    #[test]
    fn test_lifecycle_state() {
        let state = LifecycleState::Started;
        assert_eq!(state, LifecycleState::Started);
        
        let failed_state = LifecycleState::Failed("Error message".to_string());
        match failed_state {
            LifecycleState::Failed(msg) => assert_eq!(msg, "Error message"),
            _ => panic!("Expected Failed state"),
        }
    }

    #[test]
    fn test_configuration_change() {
        let change = ConfigurationChange {
            key: "test.key".to_string(),
            old_value: Some(serde_json::json!("old")),
            new_value: Some(serde_json::json!("new")),
            timestamp: chrono::Utc::now().timestamp(),
        };
        
        assert_eq!(change.key, "test.key");
        assert!(change.old_value.is_some());
        assert!(change.new_value.is_some());
        assert!(change.timestamp > 0);
    }
}
