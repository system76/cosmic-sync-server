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
pub async fn readiness_check(app_state: web::Data<crate::server::app_state::AppState>) -> ActixResult<HttpResponse> {
    // Perform basic dependency checks
    let storage_ok = app_state
        .storage
        .health_check()
        .await
        .unwrap_or(false);

    let message_broker_enabled = crate::server::app_state::AppState::get_config().message_broker.enabled;

    let status = if storage_ok { "ready" } else { "degraded" };
    let body = json!({
        "status": status,
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "dependencies": {
            "database": storage_ok,
            "message_broker_enabled": message_broker_enabled
        }
    });

    if storage_ok {
        Ok(HttpResponse::Ok().json(body))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(body))
    }
}

/// HTTP liveness check endpoint
pub async fn liveness_check() -> ActixResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "alive",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

/// Detailed health for external debugging
pub async fn health_details(app_state: web::Data<crate::server::app_state::AppState>) -> ActixResult<HttpResponse> {
    let cfg = crate::server::app_state::AppState::get_config();

    // Check storage availability
    let storage_ok = app_state
        .storage
        .health_check()
        .await
        .unwrap_or(false);

    // Summarize configuration flags (safe subset)
    let details = json!({
        "app": {
            "version": env!("CARGO_PKG_VERSION"),
            "time": chrono::Utc::now().to_rfc3339(),
        },
        "config": {
            "host": cfg.server.host,
            "grpc_port": cfg.server.port,
            "http_port": crate::config::constants::DEFAULT_HTTP_PORT,
            "storage_type": format!("{:?}", cfg.storage.storage_type),
            "message_broker_enabled": cfg.message_broker.enabled,
        },
        "dependencies": {
            "database": storage_ok,
            "redis_enabled": cfg.redis.enabled,
        }
    });

    if storage_ok {
        Ok(HttpResponse::Ok().json(details))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(details))
    }
}
