use crate::handlers::HealthHandler;
use crate::server::service::SyncServiceImpl;
use crate::sync::{HealthCheckRequest, HealthCheckResponse};
use actix_web::{web, HttpResponse, Result as ActixResult};
use serde_json::json;
use tonic::{Request, Response, Status};
use tracing::info;

#[tonic::async_trait]
impl HealthHandler for SyncServiceImpl {
    /// Handle health check requests
    async fn handle_health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        info!("HealthCheck request received");

        // Create response with status "SERVING" and package version
        let reply = HealthCheckResponse {
            status: "SERVING".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        Ok(Response::new(reply))
    }
}

/// HTTP health check endpoint
pub async fn health_check() -> ActixResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

/// HTTP readiness check endpoint
pub async fn readiness_check() -> ActixResult<HttpResponse> {
    // TODO: Add actual readiness checks (database, storage, etc.)
    Ok(HttpResponse::Ok().json(json!({
        "status": "ready",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

/// HTTP liveness check endpoint
pub async fn liveness_check() -> ActixResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "alive",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}
