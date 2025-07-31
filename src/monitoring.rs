use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn, instrument};
use serde::{Serialize, Deserialize};

/// Performance monitoring for server operations
pub struct PerformanceMonitor {
    metrics: Arc<PerformanceMetrics>,
    start_time: Instant,
    system_info: SystemInfo,
}

/// Core performance metrics
#[derive(Debug, Default)]
pub struct PerformanceMetrics {
    // Request metrics
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
    pub active_requests: AtomicUsize,
    
    // Response time metrics (in milliseconds)
    pub total_response_time: AtomicU64,
    pub max_response_time: AtomicU64,
    pub min_response_time: AtomicU64,
    
    // Throughput metrics
    pub bytes_transferred: AtomicU64,
    pub files_processed: AtomicU64,
    
    // Resource usage
    pub memory_usage_bytes: AtomicU64,
    pub cpu_usage_percent: AtomicU64,
    
    // Database metrics
    pub db_connections_active: AtomicUsize,
    pub db_connections_idle: AtomicUsize,
    pub db_query_count: AtomicU64,
    pub db_slow_queries: AtomicU64,
    
    // Cache metrics
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub cache_evictions: AtomicU64,
    
    // Error tracking
    pub timeout_count: AtomicU64,
    pub rate_limit_hits: AtomicU64,
}

/// System information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub cpu_cores: usize,
    pub total_memory_bytes: u64,
    pub rust_version: String,
    pub server_version: String,
}

/// Performance summary for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    pub uptime_seconds: u64,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f64,
    pub requests_per_second: f64,
    pub avg_response_time_ms: u64,
    pub p95_response_time_ms: u64,
    pub p99_response_time_ms: u64,
    pub active_requests: usize,
    pub bytes_transferred: u64,
    pub files_processed: u64,
    pub memory_usage_bytes: u64,
    pub cpu_usage_percent: f64,
    pub db_connections_active: usize,
    pub db_connections_idle: usize,
    pub db_query_count: u64,
    pub db_slow_queries: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_rate: f64,
    pub cache_evictions: u64,
    pub active_alerts: usize,
}

impl PerformanceMonitor {
    /// Create new performance monitor
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(PerformanceMetrics::default()),
            start_time: Instant::now(),
            system_info: collect_system_info(),
        }
    }
    
    /// Record request start
    #[instrument(skip(self))]
    pub fn record_request_start(&self) {
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.active_requests.fetch_add(1, Ordering::Relaxed);
        
        debug!("Request started, active: {}", self.metrics.active_requests.load(Ordering::Relaxed));
    }
    
    /// Record request completion
    #[instrument(skip(self))]
    pub async fn record_request_complete(&self, duration: Duration, success: bool) {
        self.metrics.active_requests.fetch_sub(1, Ordering::Relaxed);
        
        if success {
            self.metrics.successful_requests.fetch_add(1, Ordering::Relaxed);
        } else {
            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        }
        
        // Update response time metrics
        let duration_ms = duration.as_millis() as u64;
        self.metrics.total_response_time.fetch_add(duration_ms, Ordering::Relaxed);
        
        // Update max response time
        let current_max = self.metrics.max_response_time.load(Ordering::Relaxed);
        if duration_ms > current_max {
            self.metrics.max_response_time.store(duration_ms, Ordering::Relaxed);
        }
        
        // Update min response time (initialize if zero)
        let current_min = self.metrics.min_response_time.load(Ordering::Relaxed);
        if current_min == 0 || duration_ms < current_min {
            self.metrics.min_response_time.store(duration_ms, Ordering::Relaxed);
        }
        
        debug!(
            "Request completed in {:?}, success: {}, active: {}",
            duration,
            success,
            self.metrics.active_requests.load(Ordering::Relaxed)
        );
    }
    
    /// Record bytes transferred
    pub fn record_bytes_transferred(&self, bytes: u64) {
        self.metrics.bytes_transferred.fetch_add(bytes, Ordering::Relaxed);
    }
    
    /// Record file processed
    pub fn record_file_processed(&self) {
        self.metrics.files_processed.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Update memory usage
    pub fn update_memory_usage(&self, bytes: u64) {
        self.metrics.memory_usage_bytes.store(bytes, Ordering::Relaxed);
    }
    
    /// Update CPU usage
    pub fn update_cpu_usage(&self, percent: f64) {
        self.metrics.cpu_usage_percent.store(percent as u64, Ordering::Relaxed);
    }
    
    /// Update database connection counts
    pub fn update_db_connections(&self, active: usize, idle: usize) {
        self.metrics.db_connections_active.store(active, Ordering::Relaxed);
        self.metrics.db_connections_idle.store(idle, Ordering::Relaxed);
    }
    
    /// Record database query
    pub fn record_db_query(&self, duration: Duration, is_slow: bool) {
        self.metrics.db_query_count.fetch_add(1, Ordering::Relaxed);
        
        if is_slow {
            self.metrics.db_slow_queries.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    /// Record cache hit
    pub fn record_cache_hit(&self) {
        self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record cache miss
    pub fn record_cache_miss(&self) {
        self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record cache eviction
    pub fn record_cache_eviction(&self) {
        self.metrics.cache_evictions.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record timeout
    pub fn record_timeout(&self) {
        self.metrics.timeout_count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record rate limit hit
    pub fn record_rate_limit_hit(&self) {
        self.metrics.rate_limit_hits.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Get performance summary
    pub async fn get_performance_summary(&self) -> PerformanceSummary {
        let uptime = self.start_time.elapsed();
        let total_requests = self.metrics.total_requests.load(Ordering::Relaxed);
        let successful_requests = self.metrics.successful_requests.load(Ordering::Relaxed);
        let failed_requests = self.metrics.failed_requests.load(Ordering::Relaxed);
        
        let success_rate = if total_requests > 0 {
            (successful_requests as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        };
        
        let requests_per_second = if uptime.as_secs() > 0 {
            total_requests as f64 / uptime.as_secs() as f64
        } else {
            0.0
        };
        
        let avg_response_time_ms = if total_requests > 0 {
            self.metrics.total_response_time.load(Ordering::Relaxed) / total_requests
        } else {
            0
        };
        
        let cache_hits = self.metrics.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.metrics.cache_misses.load(Ordering::Relaxed);
        let cache_hit_rate = if cache_hits + cache_misses > 0 {
            (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
        } else {
            0.0
        };
        
        PerformanceSummary {
            uptime_seconds: uptime.as_secs(),
            total_requests,
            successful_requests,
            failed_requests,
            success_rate,
            requests_per_second,
            avg_response_time_ms,
            p95_response_time_ms: self.calculate_percentile(95),
            p99_response_time_ms: self.calculate_percentile(99),
            active_requests: self.metrics.active_requests.load(Ordering::Relaxed),
            bytes_transferred: self.metrics.bytes_transferred.load(Ordering::Relaxed),
            files_processed: self.metrics.files_processed.load(Ordering::Relaxed),
            memory_usage_bytes: self.metrics.memory_usage_bytes.load(Ordering::Relaxed),
            cpu_usage_percent: self.metrics.cpu_usage_percent.load(Ordering::Relaxed) as f64,
            db_connections_active: self.metrics.db_connections_active.load(Ordering::Relaxed),
            db_connections_idle: self.metrics.db_connections_idle.load(Ordering::Relaxed),
            db_query_count: self.metrics.db_query_count.load(Ordering::Relaxed),
            db_slow_queries: self.metrics.db_slow_queries.load(Ordering::Relaxed),
            cache_hits,
            cache_misses,
            cache_hit_rate,
            cache_evictions: self.metrics.cache_evictions.load(Ordering::Relaxed),
            active_alerts: 0, // Placeholder for alerts
        }
    }
    
    /// Calculate response time percentile (simplified)
    fn calculate_percentile(&self, _percentile: u32) -> u64 {
        // Simplified implementation - in production would need histogram
        let max = self.metrics.max_response_time.load(Ordering::Relaxed);
        let min = self.metrics.min_response_time.load(Ordering::Relaxed);
        
        // Rough estimation
        min + ((max - min) * 95 / 100)
    }
    
    /// Export metrics in Prometheus format
    pub async fn to_prometheus(&self) -> String {
        let summary = self.get_performance_summary().await;
        
        format!(
            r#"# HELP cosmic_sync_requests_total Total number of requests
# TYPE cosmic_sync_requests_total counter
cosmic_sync_requests_total {}

# HELP cosmic_sync_requests_success Number of successful requests
# TYPE cosmic_sync_requests_success counter
cosmic_sync_requests_success {}

# HELP cosmic_sync_requests_failed Number of failed requests
# TYPE cosmic_sync_requests_failed counter
cosmic_sync_requests_failed {}

# HELP cosmic_sync_requests_active Current number of active requests
# TYPE cosmic_sync_requests_active gauge
cosmic_sync_requests_active {}

# HELP cosmic_sync_response_time_avg Average response time in milliseconds
# TYPE cosmic_sync_response_time_avg gauge
cosmic_sync_response_time_avg {}

# HELP cosmic_sync_memory_usage_bytes Memory usage in bytes
# TYPE cosmic_sync_memory_usage_bytes gauge
cosmic_sync_memory_usage_bytes {}

# HELP cosmic_sync_cpu_usage_percent CPU usage percentage
# TYPE cosmic_sync_cpu_usage_percent gauge
cosmic_sync_cpu_usage_percent {}

# HELP cosmic_sync_cache_hits_total Total cache hits
# TYPE cosmic_sync_cache_hits_total counter
cosmic_sync_cache_hits_total {}

# HELP cosmic_sync_cache_misses_total Total cache misses
# TYPE cosmic_sync_cache_misses_total counter
cosmic_sync_cache_misses_total {}

# HELP cosmic_sync_files_processed_total Total files processed
# TYPE cosmic_sync_files_processed_total counter
cosmic_sync_files_processed_total {}
"#,
            summary.total_requests,
            summary.successful_requests,
            summary.failed_requests,
            summary.active_requests,
            summary.avg_response_time_ms,
            summary.memory_usage_bytes,
            summary.cpu_usage_percent,
            summary.cache_hits,
            summary.cache_misses,
            summary.files_processed
        )
    }
    
    /// Reset metrics
    pub async fn reset_metrics(&self) {
        self.metrics.total_requests.store(0, Ordering::Relaxed);
        self.metrics.successful_requests.store(0, Ordering::Relaxed);
        self.metrics.failed_requests.store(0, Ordering::Relaxed);
        self.metrics.active_requests.store(0, Ordering::Relaxed);
        self.metrics.total_response_time.store(0, Ordering::Relaxed);
        self.metrics.max_response_time.store(0, Ordering::Relaxed);
        self.metrics.min_response_time.store(0, Ordering::Relaxed);
        self.metrics.bytes_transferred.store(0, Ordering::Relaxed);
        self.metrics.files_processed.store(0, Ordering::Relaxed);
        self.metrics.cache_hits.store(0, Ordering::Relaxed);
        self.metrics.cache_misses.store(0, Ordering::Relaxed);
        self.metrics.cache_evictions.store(0, Ordering::Relaxed);
        self.metrics.db_query_count.store(0, Ordering::Relaxed);
        self.metrics.db_slow_queries.store(0, Ordering::Relaxed);
        self.metrics.timeout_count.store(0, Ordering::Relaxed);
        self.metrics.rate_limit_hits.store(0, Ordering::Relaxed);
    }
}

/// Collect system information
fn collect_system_info() -> SystemInfo {
    SystemInfo {
        hostname: hostname::get()
            .unwrap_or_else(|_| "unknown".into())
            .to_string_lossy()
            .to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu_cores: num_cpus::get(),
        total_memory_bytes: get_total_memory(),
        rust_version: env!("CARGO_PKG_RUST_VERSION", "unknown").to_string(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Get total system memory (simplified)
fn get_total_memory() -> u64 {
    // In a real implementation, you would use a system info crate
    // For now, return a reasonable default
    8 * 1024 * 1024 * 1024 // 8GB
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}
