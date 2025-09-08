use super::{Component, ComponentHealth, ComponentStatus};
use super::{HealthMonitor, MetricsCollector, StatusManager};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Generic service manager that can manage any component implementing the Component trait
pub struct ServiceManager<T: Component> {
    /// The actual component being managed
    component: Arc<RwLock<T>>,
    /// Status management
    status_manager: StatusManager<T::Config>,
    /// Metrics collection
    metrics_collector: MetricsCollector<T::Metrics>,
    /// Health monitoring
    health_monitor: HealthMonitor,
    /// Background task management
    task_manager: TaskManager,
    /// Component name for logging
    name: String,
}

impl<T: Component> ServiceManager<T> {
    /// Create a new service manager for the given component
    pub async fn new(component: T) -> Result<Self, Box<dyn std::error::Error>> {
        let name = component.name().to_string();
        let config = component.config().clone();

        info!("🚀 Initializing {} with ServiceManager", name);

        let component = Arc::new(RwLock::new(component));
        let status_manager = StatusManager::new(config);
        let metrics_collector = MetricsCollector::new();
        let health_monitor = HealthMonitor::new();
        let task_manager = TaskManager::new();

        Ok(Self {
            component,
            status_manager,
            metrics_collector,
            health_monitor,
            task_manager,
            name,
        })
    }

    /// Start the managed component and all monitoring services
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("🔧 Starting {} service...", self.name);

        // Update status to starting
        self.status_manager
            .set_status(ComponentStatus::Starting)
            .await;

        // Start background monitoring tasks
        self.start_monitoring_tasks().await;

        // Start the actual component
        {
            let mut component = self.component.write().await;
            if let Err(e) = component.start().await {
                let error_msg = format!("Failed to start {}: {}", self.name, e);
                error!("{}", error_msg);
                self.status_manager
                    .set_status(ComponentStatus::Error(error_msg))
                    .await;
                return Err(e.into());
            }
        }

        // Update status to running
        self.status_manager
            .set_status(ComponentStatus::Running)
            .await;

        info!("🎉 {} service started successfully!", self.name);
        Ok(())
    }

    /// Stop the managed component gracefully
    pub async fn stop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("🛑 Stopping {} service...", self.name);

        self.status_manager
            .set_status(ComponentStatus::Stopping)
            .await;

        // Stop background tasks
        self.task_manager.stop_all().await;

        // Stop the component
        {
            let mut component = self.component.write().await;
            if let Err(e) = component.stop().await {
                let error_msg = format!("Failed to stop {}: {}", self.name, e);
                error!("{}", error_msg);
                self.status_manager
                    .set_status(ComponentStatus::Error(error_msg))
                    .await;
                return Err(e.into());
            }
        }

        self.status_manager
            .set_status(ComponentStatus::Stopped)
            .await;

        info!("✅ {} service stopped successfully", self.name);
        Ok(())
    }

    /// Restart the component
    pub async fn restart(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("🔄 Restarting {} service...", self.name);

        if self.status_manager.get_status().await != ComponentStatus::Stopped {
            self.stop().await?;
        }

        self.start().await
    }

    /// Get current status
    pub async fn status(&self) -> ComponentStatus {
        self.status_manager.get_status().await
    }

    /// Get current health
    pub async fn health(&self) -> ComponentHealth {
        let component = self.component.read().await;
        component.health_check().await
    }

    /// Get current metrics
    pub async fn metrics(&self) -> T::Metrics {
        let component = self.component.read().await;
        component.metrics()
    }

    /// Update component configuration
    pub async fn update_config(
        &mut self,
        config: T::Config,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("🔧 Updating {} configuration...", self.name);

        {
            let mut component = self.component.write().await;
            component.update_config(config.clone()).await?;
        }

        self.status_manager.update_config(config).await;

        info!("✅ {} configuration updated", self.name);
        Ok(())
    }

    /// Start monitoring background tasks
    async fn start_monitoring_tasks(&mut self) {
        let component_clone = Arc::clone(&self.component);
        let metrics_clone = self.metrics_collector.clone();
        let health_clone = self.health_monitor.clone();
        let name_clone = self.name.clone();

        // Start metrics collection task
        let metrics_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;

                let component = component_clone.read().await;
                let current_metrics = component.metrics();
                metrics_clone.update(current_metrics).await;

                debug!("📊 {} metrics updated", name_clone);
            }
        });

        self.task_manager
            .add_task("metrics_collection", metrics_task)
            .await;

        // Start health monitoring task
        let component_clone = Arc::clone(&self.component);
        let health_clone = self.health_monitor.clone();
        let name_clone = self.name.clone();

        let health_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let component = component_clone.read().await;
                let health_status = component.health_check().await;
                health_clone.update(health_status).await;

                debug!("🏥 {} health checked", name_clone);
            }
        });

        self.task_manager
            .add_task("health_monitoring", health_task)
            .await;
    }
}

/// Task manager for handling background tasks
pub struct TaskManager {
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_task(&self, name: &str, handle: JoinHandle<()>) {
        let mut tasks = self.tasks.lock().await;
        tasks.insert(name.to_string(), handle);
    }

    pub async fn stop_task(&self, name: &str) -> bool {
        let mut tasks = self.tasks.lock().await;
        if let Some(handle) = tasks.remove(name) {
            handle.abort();
            true
        } else {
            false
        }
    }

    pub async fn stop_all(&self) {
        let mut tasks = self.tasks.lock().await;
        for (name, handle) in tasks.drain() {
            debug!("🛑 Stopping task: {}", name);
            handle.abort();
        }
    }
}
