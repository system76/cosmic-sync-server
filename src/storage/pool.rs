//! Connection pool management

use std::sync::Arc;
use std::time::Duration;

use sqlx::mysql::MySqlPoolOptions;
use sqlx::{MySqlPool, Error as SqlxError};

/// Database connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600), // 10 minutes
            max_lifetime: Duration::from_secs(1800), // 30 minutes
        }
    }
}

/// Connection pool manager
#[derive(Debug, Clone)]
pub struct PoolManager {
    pool: MySqlPool,
    config: PoolConfig,
}

impl PoolManager {
    /// Create a new pool manager with the given database URL and configuration
    pub async fn new(database_url: &str, config: PoolConfig) -> Result<Self, SqlxError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .max_lifetime(config.max_lifetime)
            .connect(database_url)
            .await?;

        Ok(Self { pool, config })
    }

    /// Create a new pool manager with default configuration
    pub async fn with_defaults(database_url: &str) -> Result<Self, SqlxError> {
        Self::new(database_url, PoolConfig::default()).await
    }

    /// Get the underlying connection pool
    pub fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    /// Get pool configuration
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Get current pool statistics
    pub async fn stats(&self) -> PoolStats {
        PoolStats {
            size: self.pool.size(),
            idle: self.pool.num_idle() as u32,
            max_connections: self.config.max_connections,
            min_connections: self.config.min_connections,
        }
    }

    /// Close all connections in the pool
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Check if the pool is closed
    pub fn is_closed(&self) -> bool {
        self.pool.is_closed()
    }
}

/// Pool statistics for monitoring
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub size: u32,
    pub idle: u32,
    pub max_connections: u32,
    pub min_connections: u32,
}

impl PoolStats {
    pub fn active_connections(&self) -> u32 {
        self.size - self.idle
    }

    pub fn utilization_percentage(&self) -> f64 {
        if self.max_connections == 0 {
            0.0
        } else {
            (self.size as f64 / self.max_connections as f64) * 100.0
        }
    }
} 