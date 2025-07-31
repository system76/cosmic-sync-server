use chrono::{DateTime, Utc, TimeZone};
use serde::{Deserialize, Serialize, Deserializer, Serializer};
use uuid::Uuid;
use crate::sync;
use prost_types::Timestamp;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::utils::time::{timestamp_to_datetime, datetime_to_timestamp};
use crate::utils::time::timestamp_serde;
use crate::utils::crypto::generate_file_id;

/// Information about a file for synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFile {
    pub id: String,
    pub user_id: String,
    pub device_hash: String,
    pub group_id: i32,
    pub watcher_id: i32,
    pub file_id: u64,
    pub filename: String,
    pub file_hash: String,
    pub file_path: String,
    pub file_size: i64,
    pub mime_type: String,
    pub modified_time: i64,
    pub is_encrypted: bool,
    pub upload_time: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub is_deleted: bool,
    pub revision: i64,
}

impl SyncFile {
    /// create new file info
    pub fn new(
        user_id: String,
        device_hash: String,
        group_id: i32,
        watcher_id: i32,
        filename: String,
        file_hash: String,
        file_path: String,
        file_size: i64,
        mime_type: String,
        modified_time: i64,
        is_encrypted: bool,
        revision: i64,
    ) -> Self {
        let now = Utc::now();
        let file_id = generate_file_id(&user_id, &filename, &file_hash);
        
        Self {
            id: Uuid::new_v4().to_string(),
            user_id,
            device_hash,
            group_id,
            watcher_id,
            file_id,
            filename,
            file_hash,
            file_path,
            file_size,
            mime_type,
            modified_time,
            is_encrypted,
            upload_time: now,
            last_updated: now,
            is_deleted: false,
            revision,
        }
    }
    
    // add simplified constructor method
    pub fn new_simple(
        filename: String,
        account_hash: String,
        device_hash: String,
        file_id: u64,
        file_size: usize,
        is_encrypted: bool,
    ) -> Self {
        let now = Utc::now();
        
        Self {
            id: Uuid::new_v4().to_string(),
            user_id: account_hash,
            device_hash: device_hash,
            group_id: 0,
            watcher_id: 0,
            file_id,
            filename,
            file_hash: "".to_string(),  // actual hash is calculated by client
            file_path: "".to_string(),
            file_size: file_size as i64,
            mime_type: "".to_string(),
            modified_time: now.timestamp_millis(),
            is_encrypted,
            upload_time: now,
            last_updated: now,
            is_deleted: false,
            revision: 0,
        }
    }
    
    /// update file metadata
    pub fn update_metadata(
        &mut self,
        file_hash: String,
        file_size: i64,
        mime_type: String,
        modified_time: i64,
    ) {
        self.file_hash = file_hash;
        self.file_size = file_size;
        self.mime_type = mime_type;
        self.modified_time = modified_time;
        self.last_updated = Utc::now();
    }
    
    /// mark file as deleted
    pub fn mark_as_deleted(&mut self) {
        self.is_deleted = true;
        self.last_updated = Utc::now();
    }
    
    /// restore file
    pub fn restore(&mut self) {
        self.is_deleted = false;
        self.last_updated = Utc::now();
    }
}

/// protobuf compatible file info structure
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    pub file_id: u64,
    pub filename: String,
    pub file_hash: String,
    pub device_hash: String,
    pub group_id: i32,
    pub watcher_id: i32,
    pub is_encrypted: bool,
    pub file_path: String,
    #[serde(with = "timestamp_serde")]
    pub updated_time: Timestamp,
    pub revision: i64,
    pub account_hash: String,
    pub size: u64,
}

impl FileInfo {
    pub fn to_sync_file(&self) -> sync::FileInfo {
        self.into()
    }
}

// FileInfo → sync::FileInfo conversion implementation
impl From<&FileInfo> for sync::FileInfo {
    fn from(file_info: &FileInfo) -> Self {
        Self {
            file_id: file_info.file_id,
            filename: file_info.filename.clone(),
            file_hash: file_info.file_hash.clone(),
            device_hash: file_info.device_hash.clone(),
            group_id: file_info.group_id,
            watcher_id: file_info.watcher_id,
            is_encrypted: file_info.is_encrypted,
            file_path: file_info.file_path.clone(),
            updated_time: Some(file_info.updated_time.clone()),
            revision: file_info.revision,
            file_size: file_info.size,
        }
    }
}

// sync::FileInfo → FileInfo conversion implementation
impl From<sync::FileInfo> for FileInfo {
    fn from(proto: sync::FileInfo) -> Self {
        Self {
            file_id: proto.file_id,
            filename: proto.filename,
            file_hash: proto.file_hash,
            device_hash: proto.device_hash,
            group_id: proto.group_id,
            watcher_id: proto.watcher_id,
            is_encrypted: proto.is_encrypted,
            file_path: proto.file_path,
            updated_time: proto.updated_time.unwrap_or_else(|| {
                Timestamp {
                    seconds: Utc::now().timestamp(),
                    nanos: 0,
                }
            }),
            revision: proto.revision,
            account_hash: String::new(),
            size: proto.file_size,
        }
    }
}

/// proto ListFilesResponse structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesResponse {
    pub success: bool,
    pub files: Vec<FileInfo>,
    pub return_message: String,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: i32,
    pub user_id: String,
    pub description: String,
    pub updated_at: DateTime<Utc>,
    pub registered_at: DateTime<Utc>,
    pub device_hash: String,
    // remove ip_address, port fields
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfoData {
    pub file_id: u64,
    pub filename: String,
    pub file_hash: String,
    #[serde(with = "timestamp_serde")]
    pub updated_time: Timestamp,
    pub device_hash: String,
    pub group_id: i32,
    pub watcher_id: i32,
    pub is_encrypted: bool,
    pub file_path: String,
    pub revision: i64,
}

// ModelFileInfo -> FileInfoData conversion
impl From<FileInfo> for FileInfoData {
    fn from(file: FileInfo) -> Self {
        Self {
            file_id: file.file_id,
            filename: file.filename,
            file_hash: file.file_hash,
            updated_time: file.updated_time,
            device_hash: file.device_hash,
            group_id: file.group_id,
            watcher_id: file.watcher_id,
            is_encrypted: file.is_encrypted,
            file_path: file.file_path,
            revision: file.revision,
        }
    }
}

// FileInfoData -> SyncFileInfo conversion
impl From<FileInfoData> for SyncFileInfo {
    fn from(file: FileInfoData) -> Self {
        let upload_time = match Utc.timestamp_opt(file.updated_time.seconds, file.updated_time.nanos as u32) {
            chrono::LocalResult::Single(dt) => dt,
            _ => Utc::now(),
        };        
        Self {
            file_id: file.file_id,
            filename: file.filename,
            file_hash: file.file_hash,
            device_hash: file.device_hash,
            group_id: file.group_id,
            watcher_id: file.watcher_id,
            is_encrypted: file.is_encrypted,
            file_path: file.file_path,
            upload_time,
        }
    }
}

// SyncFileInfo -> FileInfoData conversion
impl From<SyncFileInfo> for FileInfoData {
    fn from(file: SyncFileInfo) -> Self {
        let updated_time = Timestamp {
            seconds: file.upload_time.timestamp(),
            nanos: file.upload_time.timestamp_subsec_nanos() as i32,
        };
        
        Self {
            file_id: file.file_id,
            filename: file.filename,
            file_hash: file.file_hash,
            updated_time,
            device_hash: file.device_hash,
            group_id: file.group_id,
            watcher_id: file.watcher_id,
            is_encrypted: file.is_encrypted,
            file_path: file.file_path,
            revision: 0,
        }
    }
}

/// sync API compatible file info structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileInfo {
    pub file_id: u64,
    pub filename: String,
    pub file_hash: String,
    pub device_hash: String,
    pub group_id: i32,
    pub watcher_id: i32,
    pub is_encrypted: bool,
    pub file_path: String,
    pub upload_time: DateTime<Utc>,
}

/// Represents a file entry in storage
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// User account hash
    pub account_hash: String,
    /// File path
    pub path: String,
    /// File content
    pub data: Vec<u8>,
    /// File checksum
    pub checksum: String,
    /// Last modified timestamp
    pub modified: i64,
    /// File size in bytes
    pub size: i64,
    /// Group ID
    pub group_id: i32,
    /// Watcher ID 
    pub watcher_id: i32,
    /// Revision number
    pub revision: i64,
    /// File ID
    pub file_id: u64,
    /// Device hash
    pub device_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileData {
    pub file_id: u64,
    pub filename: String,
    pub file_size: i64,
    pub file_type: String,
    pub folder_path: String,
    #[serde(with = "timestamp_serde")]
    pub updated_time: Timestamp,
    pub account_hash: String,
    pub device_hash: String,
    pub group_id: i32,
    pub watcher_id: i32,
    pub revision: i64,
}

/// Represents a file notice for synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNotice {
    /// User account hash
    pub account_hash: String,
    /// Device hash
    pub device_hash: String,
    /// File path
    pub path: String,
    /// File action (create, update, delete)
    pub action: String,
    /// Timestamp of the notice
    pub timestamp: i64,
    /// File ID if applicable
    pub file_id: u64,
    /// Group ID
    pub group_id: i32,
    /// Watcher ID
    pub watcher_id: i32,
    /// Revision number
    pub revision: i64,
}
