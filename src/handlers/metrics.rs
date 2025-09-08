//! Metrics handlers for monitoring

use actix_web::{web, HttpResponse, Result};
use serde_json::json;

/// Get Prometheus-formatted metrics
pub async fn prometheus_metrics() -> Result<HttpResponse> {
    // Basic metrics in Prometheus format
    let uptime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let metrics = format!(
        "# HELP cosmic_sync_server_info Server information\n\
         # TYPE cosmic_sync_server_info gauge\n\
         cosmic_sync_server_info{{version=\"{}\"}} 1\n\
         \n\
         # HELP cosmic_sync_server_uptime_seconds Server uptime in seconds\n\
         # TYPE cosmic_sync_server_uptime_seconds gauge\n\
         cosmic_sync_server_uptime_seconds {}\n",
        env!("CARGO_PKG_VERSION"),
        uptime
    );

    Ok(HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4; charset=utf-8")
        .body(metrics))
}

/// Get detailed metrics in JSON format
pub async fn detailed_metrics() -> Result<HttpResponse> {
    let uptime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(HttpResponse::Ok().json(json!({
        "server": {
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": uptime,
            "rust_version": env!("CARGO_PKG_RUST_VERSION", "unknown")
        },
        "system": {
            "cpu_cores": num_cpus::get(),
            "timestamp": chrono::Utc::now().to_rfc3339()
        }
    })))
}
