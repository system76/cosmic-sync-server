// Version management service for file history and restoration

use std::sync::Arc;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::{debug, info, warn, error};

use crate::{
    error::AppError,
    models::file::SyncFile,
    storage::Storage,
    services::file_service::FileService,
    sync::{
        GetFileHistoryRequest, GetFileHistoryResponse,
        RestoreFileVersionRequest, RestoreFileVersionResponse,
        BroadcastFileRestoreRequest, BroadcastFileRestoreResponse,
        FileVersionInfo, FileInfo, VersionUpdateNotification,
    },
    utils::time::{datetime_to_timestamp, timestamp_to_datetime},
};

pub type Result<T> = std::result::Result<T, AppError>;

#[async_trait]
pub trait VersionService: Send + Sync {
    /// Get file history with filtering options
    async fn get_file_history(&self, request: GetFileHistoryRequest) -> Result<GetFileHistoryResponse>;
    
    /// Restore a specific version of a file
    async fn restore_file_version(&self, request: RestoreFileVersionRequest) -> Result<RestoreFileVersionResponse>;
    
    /// Broadcast file restoration to all devices
    async fn broadcast_file_restore(&self, request: BroadcastFileRestoreRequest) -> Result<BroadcastFileRestoreResponse>;
    
    /// Get all file versions for a specific file
    async fn get_file_versions(&self, account_hash: &str, file_id: u64) -> Result<Vec<FileVersionInfo>>;
    
    /// Create a new version when file is updated
    async fn create_file_version(&self, file: &SyncFile, change_description: Option<String>) -> Result<i64>;
}

#[derive(Clone)]
pub struct VersionServiceImpl {
    storage: Arc<dyn Storage>,
    file_service: FileService,
}

impl VersionServiceImpl {
    pub fn new(storage: Arc<dyn Storage>, file_service: FileService) -> Self {
        Self { storage, file_service }
    }

    /// Convert SyncFile to FileVersionInfo
    fn sync_file_to_version_info(&self, file: &SyncFile, change_description: Option<String>) -> FileVersionInfo {
        FileVersionInfo {
            file_id: file.file_id,
            revision: file.revision,
            filename: file.filename.clone(),
            file_path: file.file_path.clone(),
            file_hash: file.file_hash.clone(),
            created_time: Some(datetime_to_timestamp(&file.upload_time)),
            device_hash: file.device_hash.clone(),
            group_id: file.group_id,
            watcher_id: file.watcher_id,
            file_size: file.file_size as u64,
            change_description: change_description.unwrap_or_default(),
            is_deleted: file.is_deleted,
        }
    }

    /// Convert SyncFile to FileInfo (proto)
    fn sync_file_to_file_info(&self, file: &SyncFile) -> FileInfo {
        FileInfo {
            file_id: file.file_id,
            filename: file.filename.clone(),
            file_hash: file.file_hash.clone(),
            updated_time: Some(datetime_to_timestamp(&file.last_updated)),
            device_hash: file.device_hash.clone(),
            group_id: file.group_id,
            watcher_id: file.watcher_id,
            is_encrypted: file.is_encrypted,
            file_path: file.file_path.clone(),
            revision: file.revision,
            file_size: file.file_size as u64,
        }
    }

    /// Convert SyncFile to FileInfo model (for FileService)
    fn sync_file_to_file_info_model(&self, file: &SyncFile) -> crate::models::file::FileInfo {
        use crate::utils::time::datetime_to_timestamp;
        
        crate::models::file::FileInfo {
            file_id: file.file_id,
            filename: file.filename.clone(),
            file_hash: file.file_hash.clone(),
            file_path: file.file_path.clone(),
            size: file.file_size as u64,
            updated_time: datetime_to_timestamp(&file.last_updated),
            account_hash: file.user_id.clone(), // user_id is account_hash in SyncFile
            device_hash: file.device_hash.clone(),
            group_id: file.group_id,
            watcher_id: file.watcher_id,
            revision: file.revision,
            is_encrypted: file.is_encrypted,
        }
    }

    /// Apply filters to file history based on request parameters
    fn apply_history_filters(&self, files: &mut Vec<SyncFile>, request: &GetFileHistoryRequest) {
        // Filter by time range
        if let Some(start_time) = &request.start_time {
            let start_dt = timestamp_to_datetime(start_time);
            files.retain(|f| f.upload_time >= start_dt);
        }

        if let Some(end_time) = &request.end_time {
            let end_dt = timestamp_to_datetime(end_time);
            files.retain(|f| f.upload_time <= end_dt);
        }

        // Sort by revision descending (newest first)
        files.sort_by(|a, b| b.revision.cmp(&a.revision));

        // Limit results
        if let Some(max_versions) = request.max_versions {
            if max_versions > 0 {
                files.truncate(max_versions as usize);
            }
        } else {
            files.truncate(50); // Default limit
        }
    }
}

#[async_trait]
impl VersionService for VersionServiceImpl {
    async fn get_file_history(&self, request: GetFileHistoryRequest) -> Result<GetFileHistoryResponse> {
        debug!("Getting file history for path: {}", request.file_path);

        // Validate required fields
        if request.account_hash.is_empty() || request.file_path.is_empty() {
            return Ok(GetFileHistoryResponse {
                success: false,
                return_message: "Account hash and file path are required".to_string(),
                versions: vec![],
                total_versions: 0,
                has_more: false,
            });
        }

        // Get file history from storage
        let mut files = self.storage
            .get_file_history(&request.account_hash, &request.file_path, request.group_id)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to get file history: {}", e)))?;

        let total_before_filter = files.len() as i32;

        // Apply filters
        self.apply_history_filters(&mut files, &request);

        let has_more = files.len() < total_before_filter as usize;

        // Convert to FileVersionInfo
        let versions: Vec<FileVersionInfo> = files
            .iter()
            .map(|f| self.sync_file_to_version_info(f, None))
            .collect();

        info!("Retrieved {} file versions for path: {}", versions.len(), request.file_path);

        Ok(GetFileHistoryResponse {
            success: true,
            return_message: format!("Found {} versions", versions.len()),
            versions,
            total_versions: total_before_filter,
            has_more,
        })
    }

    async fn restore_file_version(&self, request: RestoreFileVersionRequest) -> Result<RestoreFileVersionResponse> {
        debug!("Restoring file version: file_id={}, revision={}", request.file_id, request.target_revision);

        // Validate required fields
        if request.account_hash.is_empty() || request.file_id == 0 {
            return Ok(RestoreFileVersionResponse {
                success: false,
                return_message: "Account hash and file ID are required".to_string(),
                new_revision: 0,
                restored_file_info: None,
                notified_devices: 0,
            });
        }

        // Get the target version to restore
        let target_file = self.storage
            .get_file_by_revision(&request.account_hash, request.file_id, request.target_revision)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to get file version: {}", e)))?;

        // Get current latest version to determine new revision number
        let current_files = self.storage
            .get_file_history(&request.account_hash, &target_file.file_path, target_file.group_id)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to get current file versions: {}", e)))?;

        let new_revision = current_files
            .iter()
            .map(|f| f.revision)
            .max()
            .unwrap_or(0) + 1;

        // Create a new version based on the target version but with new revision and metadata
        let mut restored_file = target_file.clone();
        restored_file.revision = new_revision;
        restored_file.device_hash = request.device_hash.clone();
        restored_file.upload_time = Utc::now();
        restored_file.last_updated = Utc::now();
        restored_file.is_deleted = false;

        // Convert SyncFile to FileInfo for FileService
        let source_file_info = self.sync_file_to_file_info_model(&target_file);
        let target_file_info = self.sync_file_to_file_info_model(&restored_file);

        // Step 1: Copy file data using FileService (no DB storage yet)
        self.file_service
            .restore_file_version(&source_file_info, &target_file_info)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to restore file data: {}", e)))?;

        // Step 2: Store the restored version in the files table (only once!)
        self.storage
            .store_file(&restored_file)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to store restored file version: {}", e)))?;

        // Step 3: Send notification to all devices including the requester
        self.file_service.send_file_restore_notification(&target_file_info).await;

        let restored_file_info = self.sync_file_to_file_info(&restored_file);

        // If broadcast is requested, notify other devices
        let mut notified_devices = 0;
        if request.broadcast_to_all_devices {
            let broadcast_request = BroadcastFileRestoreRequest {
                account_hash: request.account_hash.clone(),
                source_device_hash: request.device_hash.clone(),
                auth_token: request.auth_token.clone(),
                file_id: request.file_id,
                restored_revision: request.target_revision,
                new_revision,
                file_info: Some(restored_file_info.clone()),
                restore_reason: request.restore_reason.clone(),
            };

            match self.broadcast_file_restore(broadcast_request).await {
                Ok(broadcast_response) => {
                    notified_devices = broadcast_response.total_notified;
                }
                Err(e) => {
                    warn!("Failed to broadcast file restore: {}", e);
                }
            }
        }

        info!(
            "Successfully restored file version: file_id={}, old_revision={}, new_revision={}",
            request.file_id, request.target_revision, new_revision
        );

        Ok(RestoreFileVersionResponse {
            success: true,
            return_message: format!("File restored to revision {}", new_revision),
            new_revision,
            restored_file_info: Some(restored_file_info),
            notified_devices,
        })
    }

    async fn broadcast_file_restore(&self, request: BroadcastFileRestoreRequest) -> Result<BroadcastFileRestoreResponse> {
        debug!("Broadcasting file restore for file_id: {}", request.file_id);

        // Get all devices for this account (excluding the source device)
        let devices = self.storage
            .get_devices_for_account(&request.account_hash)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to get devices: {}", e)))?;

        let mut notified_device_hashes = Vec::new();

        for device in devices {
            if device.device_hash != request.source_device_hash {
                // Here you would implement the actual notification mechanism
                // For now, we'll just log and add to the list
                debug!("Notifying device {} about file restore", device.device_hash);
                notified_device_hashes.push(device.device_hash.clone());
            }
        }

        let total_notified = notified_device_hashes.len() as i32;

        info!("Broadcasted file restore to {} devices", total_notified);

        Ok(BroadcastFileRestoreResponse {
            success: true,
            return_message: format!("Notified {} devices", total_notified),
            notified_device_hashes,
            total_notified,
        })
    }

    async fn get_file_versions(&self, account_hash: &str, file_id: u64) -> Result<Vec<FileVersionInfo>> {
        debug!("Getting all versions for file_id: {}", file_id);

        let files = self.storage
            .get_file_versions_by_id(account_hash, file_id)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to get file versions: {}", e)))?;

        let versions: Vec<FileVersionInfo> = files
            .iter()
            .map(|f| self.sync_file_to_version_info(f, None))
            .collect();

        Ok(versions)
    }

    async fn create_file_version(&self, file: &SyncFile, change_description: Option<String>) -> Result<i64> {
        debug!("Creating new file version for file_id: {}", file.file_id);

        // Store the file version
        self.storage
            .store_file(file)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to create file version: {}", e)))?;

        info!("Created new file version: file_id={}, revision={}", file.file_id, file.revision);

        Ok(file.revision)
    }
} 