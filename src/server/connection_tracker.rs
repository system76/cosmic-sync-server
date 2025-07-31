//! Connection state tracking for improved reconnection handling

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use tracing::{debug, info, warn};

/// Connection state for a client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionState {
    pub device_hash: String,
    pub account_hash: String,
    pub connected_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub disconnected_at: Option<DateTime<Utc>>,
    pub connection_count: u64,
    pub is_active: bool,
    pub last_sync_time: Option<DateTime<Utc>>,
}

impl ConnectionState {
    pub fn new(device_hash: String, account_hash: String) -> Self {
        let now = Utc::now();
        Self {
            device_hash,
            account_hash,
            connected_at: now,
            last_activity: now,
            disconnected_at: None,
            connection_count: 1,
            is_active: true,
            last_sync_time: None,
        }
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
    }

    pub fn mark_disconnected(&mut self) {
        self.disconnected_at = Some(Utc::now());
        self.is_active = false;
    }

    pub fn mark_reconnected(&mut self) {
        let now = Utc::now();
        self.connected_at = now;
        self.last_activity = now;
        self.disconnected_at = None;
        self.connection_count += 1;
        self.is_active = true;
    }

    pub fn update_sync_time(&mut self) {
        self.last_sync_time = Some(Utc::now());
    }
}

/// Connection tracking manager
#[derive(Debug)]
pub struct ConnectionTracker {
    connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new connection
    pub async fn register_connection(&self, device_hash: String, account_hash: String) -> String {
        let connection_key = format!("{}:{}", account_hash, device_hash);
        let mut connections = self.connections.write().await;
        
        if let Some(existing) = connections.get_mut(&connection_key) {
            existing.mark_reconnected();
            info!("📡 Client reconnected: {} (connection #{} for {})", 
                  device_hash, existing.connection_count, account_hash);
        } else {
            let state = ConnectionState::new(device_hash.clone(), account_hash.clone());
            connections.insert(connection_key.clone(), state);
            info!("🔌 New client connected: {} for account {}", device_hash, account_hash);
        }
        
        connection_key
    }

    /// Update activity for a connection
    pub async fn update_activity(&self, connection_key: &str) {
        let mut connections = self.connections.write().await;
        if let Some(state) = connections.get_mut(connection_key) {
            state.update_activity();
        }
    }

    /// Mark connection as disconnected
    pub async fn mark_disconnected(&self, connection_key: &str) {
        let mut connections = self.connections.write().await;
        if let Some(state) = connections.get_mut(connection_key) {
            state.mark_disconnected();
            info!("🔌❌ Client disconnected: {} for account {}", 
                  state.device_hash, state.account_hash);
        }
    }

    /// Update sync time for a connection
    pub async fn update_sync_time(&self, connection_key: &str) {
        let mut connections = self.connections.write().await;
        if let Some(state) = connections.get_mut(connection_key) {
            state.update_sync_time();
            debug!("🔄 Sync time updated for: {}", connection_key);
        }
    }

    /// Get connection state
    pub async fn get_connection_state(&self, connection_key: &str) -> Option<ConnectionState> {
        let connections = self.connections.read().await;
        connections.get(connection_key).cloned()
    }

    /// Get last disconnect time for recovery sync
    pub async fn get_last_disconnect_time(&self, device_hash: &str, account_hash: &str) -> Option<DateTime<Utc>> {
        let connection_key = format!("{}:{}", account_hash, device_hash);
        let connections = self.connections.read().await;
        
        if let Some(state) = connections.get(&connection_key) {
            state.disconnected_at
        } else {
            None
        }
    }

    /// Get all active connections for an account
    pub async fn get_active_connections(&self, account_hash: &str) -> Vec<ConnectionState> {
        let connections = self.connections.read().await;
        connections
            .values()
            .filter(|state| state.account_hash == account_hash && state.is_active)
            .cloned()
            .collect()
    }

    /// Get connection statistics
    pub async fn get_stats(&self) -> ConnectionStats {
        let connections = self.connections.read().await;
        let total = connections.len();
        let active = connections.values().filter(|s| s.is_active).count();
        let inactive = total - active;
        
        ConnectionStats {
            total_connections: total,
            active_connections: active,
            inactive_connections: inactive,
        }
    }

    /// Cleanup old inactive connections
    pub async fn cleanup_old_connections(&self, max_age_hours: i64) {
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);
        let mut connections = self.connections.write().await;
        
        let initial_count = connections.len();
        connections.retain(|_, state| {
            if let Some(disconnected_at) = state.disconnected_at {
                disconnected_at > cutoff
            } else {
                true // Keep active connections
            }
        });
        
        let removed = initial_count - connections.len();
        if removed > 0 {
            info!("🧹 Cleaned up {} old connection records", removed);
        }
    }
    
    /// Get total number of active connections (for all accounts)
    pub async fn get_total_active_connections(&self) -> usize {
        let connections = self.connections.read().await;
        connections.values().filter(|state| state.is_active).count()
    }
}

/// Connection statistics
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub inactive_connections: usize,
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
} 