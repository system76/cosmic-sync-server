//! Cache implementation for improved performance

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc, Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    pub data: T,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl<T> CacheEntry<T> {
    pub fn new(data: T, ttl: Option<Duration>) -> Self {
        let created_at = Utc::now();
        let expires_at = ttl.map(|duration| created_at + duration);
        
        Self {
            data,
            created_at,
            expires_at,
        }
    }
    
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }
}

/// In-memory cache with TTL support
#[derive(Debug)]
pub struct MemoryCache<T> {
    storage: Arc<RwLock<HashMap<String, CacheEntry<T>>>>,
    default_ttl: Option<Duration>,
}

impl<T: Clone> MemoryCache<T> {
    pub fn new(default_ttl: Option<Duration>) -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
            default_ttl,
        }
    }
    
    /// Get value from cache
    pub async fn get(&self, key: &str) -> Option<T> {
        let mut storage = self.storage.write().await;
        
        if let Some(entry) = storage.get(key) {
            if entry.is_expired() {
                storage.remove(key);
                None
            } else {
                Some(entry.data.clone())
            }
        } else {
            None
        }
    }
    
    /// Set value in cache with optional TTL
    pub async fn set(&self, key: String, value: T, ttl: Option<Duration>) {
        let mut storage = self.storage.write().await;
        let entry = CacheEntry::new(value, ttl.or(self.default_ttl));
        storage.insert(key, entry);
    }
    
    /// Remove value from cache
    pub async fn remove(&self, key: &str) -> bool {
        let mut storage = self.storage.write().await;
        storage.remove(key).is_some()
    }
    
    /// Clear all expired entries
    pub async fn cleanup_expired(&self) {
        let mut storage = self.storage.write().await;
        let now = Utc::now();
        
        storage.retain(|_, entry| {
            if let Some(expires_at) = entry.expires_at {
                now <= expires_at
            } else {
                true
            }
        });
    }
    
    /// Get cache size
    pub async fn size(&self) -> usize {
        let storage = self.storage.read().await;
        storage.len()
    }
    
    /// Clear all entries
    pub async fn clear(&self) {
        let mut storage = self.storage.write().await;
        storage.clear();
    }
}

/// File metadata cache for improved performance
pub type FileMetadataCache = MemoryCache<crate::models::file::FileInfo>;

/// Device information cache
pub type DeviceCache = MemoryCache<crate::models::device::Device>;

/// Authentication token cache
pub type AuthTokenCache = MemoryCache<crate::models::auth::AuthResult>; 