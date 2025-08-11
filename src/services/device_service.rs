use std::sync::Arc;
use tracing::debug;
use uuid::Uuid;
use crate::models::device::Device;
use crate::sync;
// remove unused alias import
use crate::storage::{Storage, StorageError};
use crate::models::account::Account;
use chrono;
use prost_types;

/// Service for managing device registrations and operations
#[derive(Clone)]
pub struct DeviceService {
    // Device registrations
    device_registrations: Arc<std::sync::Mutex<std::collections::HashMap<String, Device>>>,
    
    // Database storage
    storage: Arc<dyn Storage>,
}

impl DeviceService {
    /// Create a new DeviceService instance
    pub fn new() -> Self {
        Self {
            device_registrations: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            storage: Arc::new(crate::storage::memory::MemoryStorage::new()),
        }
    }
    
    /// Create DeviceService with storage
    pub fn with_storage(storage: Arc<dyn Storage>) -> Self {
        Self {
            device_registrations: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            storage,
        }
    }
    
    /// Create a new account with specified name (if needed)
    pub fn create_account(&self, name: Option<String>) -> Account {
        let now = chrono::Utc::now();
        let account_id = Uuid::new_v4().to_string();
        let user_email = format!("user_{}@example.com", Uuid::new_v4());
        
        Account {
            id: account_id,
            account_hash: Uuid::new_v4().to_string(),
            name: name.unwrap_or_else(|| "New User".to_string()),
            email: user_email.clone(),
            user_id: user_email,
            user_type: String::new(),
            password_hash: String::new(),
            salt: String::new(),
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login: now,
        }
    }
    
    /// Get account information
    pub async fn get_account(&self, account_hash: &str) -> Result<Option<Account>, StorageError> {
        debug!("Getting account: account_hash={}", account_hash);
        self.storage.get_account_by_hash(account_hash).await
    }
    
    /// Save encryption key for account
    pub async fn save_encryption_key(&self, account_hash: &str, encryption_key: &str) -> Result<(), StorageError> {
        debug!("Saving encryption key: account_hash={}", account_hash);
        self.storage.store_encryption_key(account_hash, encryption_key).await
    }
    
    /// Register a device
    pub async fn register_device(&self, device: &Device) -> Result<(), StorageError> {
        debug!("Registering device: device_hash={}", device.device_hash);
        self.storage.register_device(device).await
    }
    
    /// Get device information
    pub async fn get_device(&self, device_hash: &str, account_hash: &str) -> Result<Option<Device>, StorageError> {
        debug!("Getting device: device_hash={}, account_hash={}", device_hash, account_hash);
        
        // Check in storage
        let device_result = self.storage.get_device(account_hash, device_hash).await?;
        
        if let Some(device) = device_result {
            return Ok(Some(device));
        }
        
        // Check in memory
        let registrations = self.device_registrations.lock().unwrap();
        if let Some(device) = registrations.get(device_hash) {
            if device.account_hash == account_hash {
                return Ok(Some(device.clone()));
            }
        }
        
        Ok(None)
    }
    
    /// Update device information
    pub async fn update_device(&self, device: &Device) -> Result<(), StorageError> {
        debug!("Updating device: device_hash={}", device.device_hash);
        self.storage.update_device(device).await
    }
    
    /// Get list of devices for an account
    pub async fn list_devices(&self, account_hash: &str) -> Result<Vec<Device>, StorageError> {
        debug!("Listing devices: account_hash={}", account_hash);
        self.storage.list_devices(account_hash).await
    }
    
    /// Delete device
    pub async fn delete_device(&self, account_hash: &str, device_hash: &str) -> Result<(), StorageError> {
        debug!("Deleting device: device_hash={}, account_hash={}", device_hash, account_hash);
        self.storage.delete_device(account_hash, device_hash).await
    }
    
    // Conversion methods between proto and model
    
    /// Convert model to proto
    pub fn to_proto_device_info(&self, device: &Device) -> sync::DeviceInfo {
        sync::DeviceInfo {
            device_hash: device.device_hash.clone(),
            account_hash: device.account_hash.clone(),
            is_active: device.is_active,
            os_version: device.os_version.clone(),
            app_version: device.app_version.clone(),
            registered_at: Some(prost_types::Timestamp {
                seconds: device.registered_at.timestamp(),
                nanos: 0,
            }),
            last_sync_time: Some(prost_types::Timestamp {
                seconds: device.last_sync.timestamp(),
                nanos: 0,
            }),
        }
    }
    
    /// Convert proto to model
    pub fn from_proto_device_info(&self, device_info: sync::DeviceInfo) -> Device {
        let now = chrono::Utc::now();
        let registered_at = device_info.registered_at
            .map(|ts| chrono::DateTime::<chrono::Utc>::from(std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts.seconds as u64)))
            .unwrap_or(now);
        let last_sync = device_info.last_sync_time
            .map(|ts| chrono::DateTime::<chrono::Utc>::from(std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts.seconds as u64)))
            .unwrap_or(now);
        Device {
            user_id: String::new(),
            updated_at: last_sync,
            registered_at,
            device_hash: device_info.device_hash,
            account_hash: device_info.account_hash,
            last_sync,
            is_active: device_info.is_active,
            os_version: device_info.os_version,
            app_version: device_info.app_version,
        }
    }
} 