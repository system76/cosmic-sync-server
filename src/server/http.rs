use actix_web::{get, HttpResponse, Responder};
use serde::Serialize;
use tracing::debug;

/// Health check response structure
#[derive(Debug, Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub version: String,
}

/// Health check endpoint
#[get("/health")]
pub async fn handle_health_check() -> impl Responder {
    debug!("Health check requested");
    
    let response = HealthCheckResponse {
        status: "SERVING".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    
    HttpResponse::Ok().json(response)
}