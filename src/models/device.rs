use uuid::Uuid;
use chrono::{DateTime, Utc, TimeZone};
use serde::{Serialize, Deserialize};
use crate::sync;
use crate::utils::time::{timestamp_to_datetime, datetime_to_timestamp};
use crate::utils::crypto::generate_device_hash;

/// User device information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Device name (should be unique within the system)
    pub user_id: String,
    pub account_hash: String,
    /// Device unique identifier
    pub device_hash: String,
    pub updated_at: DateTime<Utc>, // last updated time
    pub registered_at: DateTime<Utc>, // Registered time
    /// Last time device synced data
    pub last_sync: DateTime<Utc>,
    /// Whether device is active
    pub is_active: bool,
    /// OS version
    pub os_version: String,
    /// App version
    pub app_version: String,
}

/// Proto-compatible device info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device identifier
    pub device_hash: String,
    pub account_hash: String,
    /// Device name
    pub is_active: bool,
    
    /// Device hash
    pub os_version: String, 
    pub app_version: String,
    pub registered_at: DateTime<Utc>,
    /// Last sync time as ISO string
    pub last_sync_time: Option<DateTime<Utc>>,
}

impl From<&Device> for DeviceInfo {
    fn from(device: &Device) -> Self {
        Self {
            device_hash: device.device_hash.clone(),
            account_hash: device.account_hash.clone(),
            is_active: device.is_active,
            os_version: device.os_version.clone(),
            app_version: device.app_version.clone(),
            registered_at: device.registered_at,
            last_sync_time: Some(device.last_sync),
        }
    }
}

// Implementation of conversion from DeviceInfo to Device
impl From<&DeviceInfo> for Device {
    fn from(info: &DeviceInfo) -> Self {
        let now = Utc::now();
        Self {
            user_id: String::new(),
            updated_at: info.last_sync_time.unwrap_or(now),
            registered_at: info.registered_at,
            device_hash: info.device_hash.clone(),
            account_hash: info.account_hash.clone(),
            last_sync: info.last_sync_time.unwrap_or(now),
            is_active: info.is_active,
            os_version: info.os_version.clone(),
            app_version: info.app_version.clone(),
        }
    }
}

// Implementation of conversion from sync::DeviceInfo to Device
impl From<sync::DeviceInfo> for Device {
    fn from(info: sync::DeviceInfo) -> Self {
        let now = Utc::now();
        
        // improve timestamp conversion
        let registered_at = info.registered_at
            .map(|ts| match Utc.timestamp_opt(ts.seconds, ts.nanos as u32) {
                chrono::LocalResult::Single(dt) => dt,
                _ => now,
            }).unwrap_or(now);
            
        let last_sync = info.last_sync_time
            .map(|ts| match Utc.timestamp_opt(ts.seconds, ts.nanos as u32) {
                chrono::LocalResult::Single(dt) => dt,
                _ => now,
            }).unwrap_or(now);
            
        Self {
            user_id: String::new(),
            updated_at: last_sync,
            registered_at,
            device_hash: info.device_hash,
            account_hash: info.account_hash,
            last_sync,
            is_active: info.is_active,
            os_version: info.os_version,
            app_version: info.app_version,
        }
    }
}

/// List of devices response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDevicesResponse {
    pub success: bool,
    pub devices: Vec<DeviceInfo>,
    pub return_message: String,
}

impl Device {
    /// Create a new device
    pub fn new(
        account_hash: String,
        device_hash: String,
        is_active: bool,
        os_version: String,
        app_version: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            user_id: account_hash.clone(),
            account_hash,
            device_hash,
            is_active,
            os_version,
            app_version,
            updated_at: now,
            registered_at: now,
            last_sync: now,
        }
    }
    
    /// Update device information
    pub fn update_info(
        &mut self,
        is_active: Option<bool>,
        os_version: Option<String>,
        app_version: Option<String>,
    ) {
        if let Some(active) = is_active {
            self.is_active = active;
        }
        
        if let Some(os) = os_version {
            self.os_version = os;
        }
        
        if let Some(app) = app_version {
            self.app_version = app;
        }
        
        self.updated_at = Utc::now();
    }
    
    /// Update last sync time
    pub fn update_last_sync(&mut self) {
        self.last_sync = Utc::now();
        self.updated_at = Utc::now();
    }
    
    /// Deactivate device
    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.updated_at = Utc::now();
    }
}

/// Represents a user device
#[derive(Debug, Clone)]
pub struct UserDevice {
    /// User account hash
    pub account_hash: String,
    /// Device hash
    pub device_hash: String,
    /// Device name
    pub name: String,
    /// Device type
    pub device_type: String,
    /// Last active timestamp
    pub last_active: i64,
}

// Database model to proto message conversion (based on prost-generated sync::DeviceInfo)
impl From<&Device> for sync::DeviceInfo {
    fn from(device: &Device) -> Self {
        use prost_types::Timestamp;
        let registered_at = Some(Timestamp {
            seconds: device.registered_at.timestamp(),
            nanos: 0,
        });
        let last_sync_time = Some(Timestamp {
            seconds: device.last_sync.timestamp(),
            nanos: 0,
        });
        
        Self {
            account_hash: device.account_hash.clone(),
            is_active: device.is_active,
            device_hash: device.device_hash.clone(),
            os_version: device.os_version.clone(),
            app_version: device.app_version.clone(),
            registered_at,
            last_sync_time,
        }
    }
}