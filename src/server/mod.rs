pub mod service;
pub mod http;
pub mod startup;
pub mod app_state;
pub mod notification_manager;
pub mod event_bus;
pub mod connection_handler;
pub mod connection_tracker;
pub mod connection_cleanup;

// New reusable architecture modules
pub mod common;
pub mod components;

// Re-export the new architecture
pub use common::*;
pub use components::*;

use crate::config::settings::ServerConfig;
use tracing::info;

/// Main server type using the new reusable architecture
pub type Server = ServiceManager<SyncServerComponent>;

/// Helper function to create a new server with the SyncServerComponent
pub async fn create_sync_server(config: ServerConfig) -> Result<Server, Box<dyn std::error::Error>> {
    info!("🚀 Creating Cosmic Sync Server with reusable architecture");
    
    let component = SyncServerComponent::new(config).await?;
    let server = ServiceManager::new(component).await?;
    
    info!("✅ Sync server created successfully");
    Ok(server)
}

// Example functions for future service types (commented out until implemented)
/*
/// Example of how easy it is to add a new service type
pub async fn create_file_service(config: FileServiceConfig) -> Result<ServiceManager<FileServiceComponent>, Box<dyn std::error::Error>> {
    let component = FileServiceComponent::new(config).await?;
    let server = ServiceManager::new(component).await?;
    Ok(server)
}

/// Example of how easy it is to add a notification service
pub async fn create_notification_service(config: NotificationConfig) -> Result<ServiceManager<NotificationComponent>, Box<dyn std::error::Error>> {
    let component = NotificationComponent::new(config).await?;
    let server = ServiceManager::new(component).await?;
    Ok(server)
}
*/

