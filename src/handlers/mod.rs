// Handler modules - new implementation
pub mod auth_handler;
pub use auth_handler::AuthHandler;

pub mod device_handler;
pub use device_handler::DeviceHandler;

pub mod file_handler;
pub use file_handler::FileHandler;

pub mod watcher_handler;
pub use watcher_handler::WatcherHandler;

pub mod sync_handler;
pub use sync_handler::SyncHandler;

// Health check handler
pub mod health;

// API handlers
pub mod api;

// Metrics handlers
pub mod metrics;

use tonic::{Request, Response, Status};
use crate::sync::HealthCheckRequest;
use crate::sync::HealthCheckResponse;

#[tonic::async_trait]
pub trait HealthHandler {
    async fn handle_health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status>;
}

// OAuth handler  
pub mod oauth;
pub use oauth::OAuthHandler;