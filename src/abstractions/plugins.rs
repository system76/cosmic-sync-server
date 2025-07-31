// Plugin system implementation for extensible architecture
// Provides a robust plugin framework that supports dynamic loading,
// lifecycle management, and dependency injection

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{
    abstractions::{
        Plugin, PluginMetadata, PluginConfig, PluginHealth, HealthStatus,
        Lifecycle, LifecycleState, Registry, Factory,
    },
    error::{Result, SyncError},
};

/// Plugin manager for handling plugin lifecycle and dependencies
pub struct PluginManager {
    plugins: Arc<RwLock<HashMap<String, PluginContainer>>>,
    registry: Arc<RwLock<PluginRegistry>>,
    factory: Arc<RwLock<PluginFactory>>,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            registry: Arc::new(RwLock::new(PluginRegistry::new())),
            factory: Arc::new(RwLock::new(PluginFactory::new())),
        }
    }
    
    /// Register a plugin type with the factory
    pub async fn register_plugin_type<T: Plugin + 'static>(
        &self,
        plugin_type: String,
        factory_fn: impl Fn() -> Result<T> + Send + Sync + 'static,
    ) -> Result<()> {
        let mut factory = self.factory.write().await;
        factory.register(plugin_type, Box::new(factory_fn));
        Ok(())
    }
    
    /// Load a plugin by type
    pub async fn load_plugin(
        &self,
        plugin_type: &str,
        plugin_name: String,
        config: PluginConfig,
    ) -> Result<()> {
        // Create plugin instance
        let factory = self.factory.read().await;
        let mut plugin = factory.create(plugin_type)?;
        
        // Initialize plugin
        plugin.initialize(config).await?;
        
        let metadata = plugin.metadata();
        
        // Check dependencies
        self.check_dependencies(&metadata.dependencies).await?;
        
        // Store plugin
        let container = PluginContainer::new(plugin);
        let mut plugins = self.plugins.write().await;
        plugins.insert(plugin_name.clone(), container);
        
        // Register in registry
        let mut registry = self.registry.write().await;
        registry.register(plugin_name, metadata).await?;
        
        Ok(())
    }
    
    /// Start a plugin
    pub async fn start_plugin(&self, plugin_name: &str) -> Result<()> {
        let mut plugins = self.plugins.write().await;
        if let Some(container) = plugins.get_mut(plugin_name) {
            container.start().await?;
            Ok(())
        } else {
            Err(SyncError::NotFound(format!("Plugin not found: {}", plugin_name)))
        }
    }
    
    /// Stop a plugin
    pub async fn stop_plugin(&self, plugin_name: &str) -> Result<()> {
        let mut plugins = self.plugins.write().await;
        if let Some(container) = plugins.get_mut(plugin_name) {
            container.stop().await?;
            Ok(())
        } else {
            Err(SyncError::NotFound(format!("Plugin not found: {}", plugin_name)))
        }
    }
    
    /// Get plugin health
    pub async fn get_plugin_health(&self, plugin_name: &str) -> Result<PluginHealth> {
        let plugins = self.plugins.read().await;
        if let Some(container) = plugins.get(plugin_name) {
            container.get_health().await
        } else {
            Err(SyncError::NotFound(format!("Plugin not found: {}", plugin_name)))
        }
    }
    
    /// List all loaded plugins
    pub async fn list_plugins(&self) -> Vec<String> {
        let plugins = self.plugins.read().await;
        plugins.keys().cloned().collect()
    }
    
    /// Get plugin by name
    pub async fn get_plugin(&self, plugin_name: &str) -> Option<Arc<RwLock<dyn Plugin>>> {
        let plugins = self.plugins.read().await;
        plugins.get(plugin_name).map(|container| container.plugin.clone())
    }
    
    /// Check plugin dependencies
    async fn check_dependencies(&self, dependencies: &[String]) -> Result<()> {
        let registry = self.registry.read().await;
        for dep in dependencies {
            if !registry.contains(dep).await {
                return Err(SyncError::Validation(format!(
                    "Missing dependency: {}",
                    dep
                )));
            }
        }
        Ok(())
    }
    
    /// Start all plugins in dependency order
    pub async fn start_all_plugins(&self) -> Result<()> {
        let plugin_names = self.list_plugins().await;
        
        // TODO: Implement topological sort for dependencies
        // For now, start plugins in registration order
        for plugin_name in plugin_names {
            if let Err(e) = self.start_plugin(&plugin_name).await {
                tracing::error!("Failed to start plugin {}: {}", plugin_name, e);
                // Continue starting other plugins
            }
        }
        
        Ok(())
    }
    
    /// Stop all plugins
    pub async fn stop_all_plugins(&self) -> Result<()> {
        let plugin_names = self.list_plugins().await;
        
        // Stop plugins in reverse order
        for plugin_name in plugin_names.iter().rev() {
            if let Err(e) = self.stop_plugin(plugin_name).await {
                tracing::error!("Failed to stop plugin {}: {}", plugin_name, e);
                // Continue stopping other plugins
            }
        }
        
        Ok(())
    }
}

/// Plugin container for managing individual plugin lifecycle
struct PluginContainer {
    plugin: Arc<RwLock<dyn Plugin>>,
    state: LifecycleState,
}

impl PluginContainer {
    /// Create a new plugin container
    fn new(plugin: impl Plugin + 'static) -> Self {
        Self {
            plugin: Arc::new(RwLock::new(plugin)),
            state: LifecycleState::Created,
        }
    }
    
    /// Start the plugin
    async fn start(&mut self) -> Result<()> {
        self.state = LifecycleState::Starting;
        
        match self.plugin.write().await.start().await {
            Ok(_) => {
                self.state = LifecycleState::Started;
                Ok(())
            }
            Err(e) => {
                self.state = LifecycleState::Failed(e.to_string());
                Err(e)
            }
        }
    }
    
    /// Stop the plugin
    async fn stop(&mut self) -> Result<()> {
        self.state = LifecycleState::Stopping;
        
        match self.plugin.write().await.stop().await {
            Ok(_) => {
                self.state = LifecycleState::Stopped;
                Ok(())
            }
            Err(e) => {
                self.state = LifecycleState::Failed(e.to_string());
                Err(e)
            }
        }
    }
    
    /// Get plugin health
    async fn get_health(&self) -> Result<PluginHealth> {
        match &self.state {
            LifecycleState::Started => {
                self.plugin.read().await.health_check().await
            }
            LifecycleState::Failed(msg) => {
                Ok(PluginHealth {
                    status: HealthStatus::Unhealthy,
                    message: Some(msg.clone()),
                    last_check: chrono::Utc::now().timestamp(),
                })
            }
            _ => {
                Ok(PluginHealth {
                    status: HealthStatus::Warning,
                    message: Some(format!("Plugin in state: {:?}", self.state)),
                    last_check: chrono::Utc::now().timestamp(),
                })
            }
        }
    }
}

/// Plugin registry for tracking loaded plugins
struct PluginRegistry {
    plugins: HashMap<String, PluginMetadata>,
}

impl PluginRegistry {
    fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }
}

#[async_trait]
impl Registry<PluginMetadata> for PluginRegistry {
    async fn register(&mut self, key: String, item: PluginMetadata) -> Result<()> {
        self.plugins.insert(key, item);
        Ok(())
    }
    
    async fn unregister(&mut self, key: &str) -> Result<Option<PluginMetadata>> {
        Ok(self.plugins.remove(key))
    }
    
    async fn get(&self, key: &str) -> Option<&PluginMetadata> {
        self.plugins.get(key)
    }
    
    async fn keys(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }
    
    async fn contains(&self, key: &str) -> bool {
        self.plugins.contains_key(key)
    }
    
    async fn clear(&mut self) -> Result<()> {
        self.plugins.clear();
        Ok(())
    }
}

/// Plugin factory for creating plugin instances
struct PluginFactory {
    factories: HashMap<String, Box<dyn Fn() -> Result<Box<dyn Plugin>> + Send + Sync>>,
}

impl PluginFactory {
    fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }
    
    fn register<F>(&mut self, plugin_type: String, factory: F)
    where
        F: Fn() -> Result<Box<dyn Plugin>> + Send + Sync + 'static,
    {
        self.factories.insert(plugin_type, Box::new(factory));
    }
    
    fn create(&self, plugin_type: &str) -> Result<Box<dyn Plugin>> {
        if let Some(factory) = self.factories.get(plugin_type) {
            factory()
        } else {
            Err(SyncError::NotFound(format!(
                "Plugin type not found: {}",
                plugin_type
            )))
        }
    }
}

/// Base plugin trait implementation for common functionality
pub struct BasePlugin {
    metadata: PluginMetadata,
    config: PluginConfig,
    state: LifecycleState,
}

impl BasePlugin {
    /// Create a new base plugin
    pub fn new(metadata: PluginMetadata) -> Self {
        Self {
            metadata,
            config: HashMap::new(),
            state: LifecycleState::Created,
        }
    }
    
    /// Get current state
    pub fn state(&self) -> &LifecycleState {
        &self.state
    }
    
    /// Set state
    pub fn set_state(&mut self, state: LifecycleState) {
        self.state = state;
    }
}

#[async_trait]
impl Plugin for BasePlugin {
    fn metadata(&self) -> PluginMetadata {
        self.metadata.clone()
    }
    
    async fn initialize(&mut self, config: PluginConfig) -> Result<()> {
        self.config = config;
        self.state = LifecycleState::Initialized;
        Ok(())
    }
    
    async fn start(&mut self) -> Result<()> {
        self.state = LifecycleState::Started;
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<()> {
        self.state = LifecycleState::Stopped;
        Ok(())
    }
    
    async fn health_check(&self) -> Result<PluginHealth> {
        Ok(PluginHealth {
            status: match self.state {
                LifecycleState::Started => HealthStatus::Healthy,
                LifecycleState::Failed(_) => HealthStatus::Unhealthy,
                _ => HealthStatus::Warning,
            },
            message: None,
            last_check: chrono::Utc::now().timestamp(),
        })
    }
    
    async fn configure(&mut self, config: PluginConfig) -> Result<()> {
        self.config = config;
        Ok(())
    }
}

/// Plugin discovery trait for automatic plugin loading
#[async_trait]
pub trait PluginDiscovery: Send + Sync {
    /// Discover available plugins
    async fn discover(&self) -> Result<Vec<PluginDescriptor>>;
    
    /// Load plugin from descriptor
    async fn load_plugin(&self, descriptor: &PluginDescriptor) -> Result<Box<dyn Plugin>>;
}

/// Plugin descriptor for discovered plugins
#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub plugin_type: String,
    pub path: Option<String>,
    pub metadata: PluginMetadata,
    pub config: PluginConfig,
}

/// File system plugin discovery implementation
pub struct FileSystemPluginDiscovery {
    plugin_directories: Vec<String>,
}

impl FileSystemPluginDiscovery {
    /// Create a new file system plugin discovery
    pub fn new(directories: Vec<String>) -> Self {
        Self {
            plugin_directories: directories,
        }
    }
}

#[async_trait]
impl PluginDiscovery for FileSystemPluginDiscovery {
    async fn discover(&self) -> Result<Vec<PluginDescriptor>> {
        let mut descriptors = Vec::new();
        
        for directory in &self.plugin_directories {
            // TODO: Implement actual file system scanning
            // This would scan for plugin manifests (e.g., plugin.toml files)
            // and create descriptors based on the found files
            tracing::info!("Scanning directory for plugins: {}", directory);
        }
        
        Ok(descriptors)
    }
    
    async fn load_plugin(&self, descriptor: &PluginDescriptor) -> Result<Box<dyn Plugin>> {
        // TODO: Implement dynamic loading of plugins from files
        // This would involve loading shared libraries (.so, .dll, .dylib)
        // and calling plugin factory functions
        Err(SyncError::NotSupported(
            "Dynamic plugin loading not yet implemented".to_string()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        base: BasePlugin,
    }

    impl TestPlugin {
        fn new() -> Self {
            let metadata = PluginMetadata {
                name: "test-plugin".to_string(),
                version: "1.0.0".to_string(),
                description: "Test plugin".to_string(),
                author: "Test Author".to_string(),
                dependencies: vec![],
                capabilities: vec!["test".to_string()],
            };
            
            Self {
                base: BasePlugin::new(metadata),
            }
        }
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn metadata(&self) -> PluginMetadata {
            self.base.metadata()
        }
        
        async fn initialize(&mut self, config: PluginConfig) -> Result<()> {
            self.base.initialize(config).await
        }
        
        async fn start(&mut self) -> Result<()> {
            self.base.start().await
        }
        
        async fn stop(&mut self) -> Result<()> {
            self.base.stop().await
        }
        
        async fn health_check(&self) -> Result<PluginHealth> {
            self.base.health_check().await
        }
        
        async fn configure(&mut self, config: PluginConfig) -> Result<()> {
            self.base.configure(config).await
        }
    }

    #[tokio::test]
    async fn test_plugin_manager() {
        let manager = PluginManager::new();
        
        // Register plugin type
        manager.register_plugin_type(
            "test".to_string(),
            || Ok(TestPlugin::new()),
        ).await.unwrap();
        
        // Load plugin
        let config = HashMap::new();
        manager.load_plugin("test", "test-instance".to_string(), config)
            .await.unwrap();
        
        // Start plugin
        manager.start_plugin("test-instance").await.unwrap();
        
        // Check health
        let health = manager.get_plugin_health("test-instance").await.unwrap();
        assert_eq!(health.status, HealthStatus::Healthy);
        
        // Stop plugin
        manager.stop_plugin("test-instance").await.unwrap();
    }

    #[tokio::test]
    async fn test_plugin_registry() {
        let mut registry = PluginRegistry::new();
        
        let metadata = PluginMetadata {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Author".to_string(),
            dependencies: vec![],
            capabilities: vec![],
        };
        
        registry.register("test".to_string(), metadata).await.unwrap();
        assert!(registry.contains("test").await);
        
        let keys = registry.keys().await;
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], "test");
    }
}
