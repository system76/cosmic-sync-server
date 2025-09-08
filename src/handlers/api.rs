//! API handlers for system information

use actix_web::{web, HttpResponse, Result};
use serde_json::json;

/// Get API information
pub async fn api_info() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "name": "Cosmic Sync Server",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "High-performance synchronization server for COSMIC Desktop Environment"
    })))
}

/// Get API version
pub async fn api_version() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "api_version": "v1"
    })))
}

/// Get API status
pub async fn api_status() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}
