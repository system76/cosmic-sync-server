use std::sync::Arc;
use tracing::{debug, info, error, warn};
use crate::models::file::FileInfo as ModelFileInfo;
use crate::models::device::Device;
use crate::sync;
use crate::sync::FileUpdateNotification;
use crate::storage::{Storage, StorageError, Result as StorageResult, FileStorage};
use crate::models::file::FileInfo as FileInfoData;
// removed unused prost_types
use chrono::Utc;
use crate::server::notification_manager::NotificationManager;
use std::io::{Error, ErrorKind};

use crate::config::settings::Config as AppConfig;
use base64::Engine as _;

/// Service for managing file operations
#[derive(Clone)]
pub struct FileService {
    // In-memory storage for files
    files: Arc<std::sync::Mutex<std::collections::HashMap<u64, Vec<u8>>>>,
    
    // Database storage
    storage: Arc<dyn Storage>,
    
    // File storage (separate from metadata storage)
    file_storage: Option<Arc<dyn FileStorage>>,
    
    // Notification manager for event broadcasting
    notification_manager: Option<Arc<NotificationManager>>,
}

impl FileService {
    /// Create a new FileService instance
    pub fn new() -> Self {
        Self {
            files: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            storage: Arc::new(crate::storage::memory::MemoryStorage::new()),
            file_storage: None,
            notification_manager: None,
        }
    }
    
    /// Create a FileService with database storage
    pub fn with_storage(storage: Arc<dyn Storage>) -> Self {
        Self {
            files: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            storage,
            file_storage: None,
            notification_manager: None,
        }
    }
    
    /// Create a FileService with database storage and notification manager
    pub fn with_storage_and_notifications(storage: Arc<dyn Storage>, notification_manager: Arc<NotificationManager>) -> Self {
        Self {
            files: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            storage,
            file_storage: None,
            notification_manager: Some(notification_manager),
        }
    }
    
    /// Create a FileService with storage, file storage, and notification manager
    pub fn with_storage_file_storage_and_notifications(
        storage: Arc<dyn Storage>, 
        file_storage: Arc<dyn FileStorage>,
        notification_manager: Arc<NotificationManager>
    ) -> Self {
        Self {
            files: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            storage,
            file_storage: Some(file_storage),
            notification_manager: Some(notification_manager),
        }
    }
    
    /// Prepare metadata: compute eq_index/token_path and encrypt path/name
    fn prepare_metadata(&self, cfg: &AppConfig, mut fi: ModelFileInfo) -> ModelFileInfo {
        let key_vec_opt = cfg.server_encode_key.as_ref();
        let aad = format!("{}:{}:{}", fi.account_hash, fi.group_id, fi.watcher_id);
        if let Some(kv) = key_vec_opt {
            if kv.len() == 32 {
                let key: &[u8;32] = kv.as_slice().try_into().expect("len checked");
                // compute indices from normalized plaintext path
                let _eq = crate::utils::crypto::make_eq_index(key, &fi.file_path);
                let _tp = crate::utils::crypto::make_token_path(key, &fi.file_path);
                // encrypt path/name
                let ct_path = crate::utils::crypto::aead_encrypt(key, fi.file_path.as_bytes(), aad.as_bytes());
                let ct_name = crate::utils::crypto::aead_encrypt(key, fi.filename.as_bytes(), aad.as_bytes());
                fi.file_path = base64::engine::general_purpose::STANDARD_NO_PAD.encode(ct_path);
                fi.filename = base64::engine::general_purpose::STANDARD_NO_PAD.encode(ct_name);
                debug!("metadata prepared with encryption");
            } else {
                debug!("server_encode_key length != 32; skipping encryption");
            }
        } else {
            debug!("server_encode_key not set; storing plaintext (dev only)");
        }
        fi
    }

    pub async fn store_file(&self, file_info: &ModelFileInfo, data: &Vec<u8>) -> Result<(), StorageError> {
        self.store_file_with_update_type(file_info, data, sync::file_update_notification::UpdateType::Uploaded).await
    }

    /// Convert client group_id to server group_id
    pub async fn convert_client_group_to_server(
        &self,
        account_hash: &str,
        client_group_id: i32,
    ) -> Result<Option<i32>, StorageError> {
        self.storage.get_server_group_id(account_hash, client_group_id).await
    }

    /// Convert client (group_id, watcher_id) to server (group_id, watcher_id)
    pub async fn convert_client_ids_to_server(
        &self,
        account_hash: &str,
        client_group_id: i32,
        client_watcher_id: i32,
    ) -> Result<Option<(i32, i32)>, StorageError> {
        self.storage
            .get_server_ids(account_hash, client_group_id, client_watcher_id)
            .await
    }

    /// Ensure server-side IDs for upload context or fallback to mapping
    pub async fn ensure_server_ids_for_upload(
        &self,
        account_hash: &str,
        device_hash: &str,
        client_group_id: i32,
        client_watcher_id: i32,
        normalized_file_path: Option<&str>,
    ) -> Result<(i32, i32), StorageError> {
        // If underlying storage is MySqlStorage, use ensure_server_ids_for
        if let Some(mysql) = self.storage.as_any().downcast_ref::<crate::storage::mysql::MySqlStorage>() {
            let (server_group_id, server_watcher_id) = mysql
                .ensure_server_ids_for(
                    account_hash,
                    device_hash,
                    client_group_id,
                    client_watcher_id,
                    normalized_file_path,
                )
                .await?;
            return Ok((server_group_id, server_watcher_id));
        }

        // Fallback: try existing mapping from generic Storage
        match self
            .storage
            .get_server_ids(account_hash, client_group_id, client_watcher_id)
            .await?
        {
            Some((group_id, watcher_id)) => Ok((group_id, watcher_id)),
            None => Ok((0, 0)),
        }
    }

    /// Build file metadata for upload using server IDs
    pub fn build_file_info_for_upload(
        &self,
        req: &crate::sync::UploadFileRequest,
        file_id: u64,
        normalized_file_path: String,
        server_group_id: i32,
        server_watcher_id: i32,
    ) -> ModelFileInfo {
        ModelFileInfo {
            file_id,
            filename: req.filename.clone(),
            file_hash: req.file_hash.clone(),
            device_hash: req.device_hash.clone(),
            group_id: server_group_id,
            watcher_id: server_watcher_id,
            is_encrypted: req.is_encrypted,
            file_path: normalized_file_path,
            updated_time: prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            },
            revision: req.revision,
            account_hash: req.account_hash.clone(),
            size: req.file_size,
            key_id: if req.key_id.is_empty() { None } else { Some(req.key_id.clone()) },
        }
    }

    /// Store file with custom update type for notifications
    pub async fn store_file_with_update_type(
        &self, 
        file_info: &ModelFileInfo, 
        data: &Vec<u8>, 
        update_type: sync::file_update_notification::UpdateType
    ) -> Result<(), StorageError> {
        debug!("🔄 FileService::store_file_with_update_type started: file_id={}, filename={}, size={} bytes, update_type={:?}", 
               file_info.file_id, file_info.filename, data.len(), update_type);
        
        // Store file metadata
        debug!("📄 Storing file metadata...");
        match self.storage.store_file_info(file_info.clone()).await {
            Ok(_) => debug!("✅ File metadata stored successfully"),
            Err(e) => {
                error!("❌ Failed to store file metadata: {}", e);
                return Err(e);
            }
        }
        
        // Store file data
        debug!("💾 Storing file data...");
        self.store_file_data_internal(file_info.file_id, data).await?;
        
        // Update in-memory cache
        debug!("🗂️ Updating in-memory cache...");
        self.files.lock().unwrap().insert(file_info.file_id, data.clone());
        debug!("✅ In-memory cache updated");
        
        // Send notification
        self.send_file_update_notification(file_info, update_type).await;
        
        debug!("🎉 FileService::store_file_with_update_type completed successfully for file_id={}", file_info.file_id);
        Ok(())
    }

    /// Internal method to store file data
    async fn store_file_data_internal(&self, file_id: u64, data: &Vec<u8>) -> Result<(), StorageError> {
        match self.file_storage.as_ref() {
            Some(file_storage) => {
                match file_storage.store_file_data(file_id, data.clone()).await {
                    Ok(_) => {
                        debug!("✅ File data stored successfully in file storage");
                        Ok(())
                    },
                    Err(e) => {
                        error!("❌ Failed to store file data in file storage: {}", e);
                        Err(e)
                    }
                }
            }
            None => {
                // Fallback to legacy storage
                debug!("Using legacy storage for file data");
                match self.storage.store_file_data(file_id, data.clone()).await {
                    Ok(_) => {
                        debug!("✅ File data stored successfully");
                        Ok(())
                    },
                    Err(e) => {
                        error!("❌ Failed to store file data: {}", e);
                        Err(e)
                    }
                }
            }
        }
    }

    /// Send file update notification
    async fn send_file_update_notification(
        &self, 
        file_info: &ModelFileInfo, 
        update_type: sync::file_update_notification::UpdateType
    ) {
        self.send_file_update_notification_with_options(file_info, update_type, false).await
    }
    
    /// Send file update notification with options
    async fn send_file_update_notification_with_options(
        &self, 
        file_info: &ModelFileInfo, 
        update_type: sync::file_update_notification::UpdateType,
        include_source_device: bool
    ) {
        debug!("📡 Preparing file update notification...");
        if let Some(notification_manager) = &self.notification_manager {
            // 파일 업데이트 알림 생성
            let file_update_notification = FileUpdateNotification {
                account_hash: file_info.account_hash.clone(),
                device_hash: file_info.device_hash.clone(),
                file_info: Some(file_info.to_sync_file()),
                update_type: update_type as i32,
                timestamp: Utc::now().timestamp(),
            };
            
            // 알림 전송 방식 선택
            debug!("📤 Broadcasting file update notification (include_source: {})...", include_source_device);
            let result = if include_source_device {
                // 소스 장치도 포함해서 전송 (파일 복원 등)
                notification_manager.broadcast_file_update_including_source(file_update_notification).await
            } else {
                // 소스 장치 제외하고 전송 (일반적인 파일 업로드)
                notification_manager.broadcast_file_update(file_update_notification).await
            };
            
            match result {
                Ok(sent_count) => {
                    if sent_count > 0 {
                        info!("✅ File sync notification delivered to {} clients: {} ({}KB, rev {})", 
                              sent_count, file_info.filename, file_info.size / 1024, file_info.revision);
                        debug!("   → Notification details: file_id={}, account={}, path={}", 
                               file_info.file_id, file_info.account_hash, file_info.file_path);
                    } else {
                        warn!("⚠️ No active subscribers for file notification: {} (file_id={})", 
                              file_info.filename, file_info.file_id);
                        info!("📝 File saved but not synced - clients offline:");
                        info!("   → File: {} ({}KB, revision {})", file_info.filename, file_info.size / 1024, file_info.revision);
                        info!("   → Account: {}", file_info.account_hash);
                        info!("   → 💡 File will be synchronized when clients reconnect and subscribe");
                    }
                },
                Err(e) => warn!("❌ Failed to broadcast file update to clients: {}", e),
                }
            }


    }

    /// Copy file data from one file to another (for version restoration)
    pub async fn copy_file_data(&self, source_file_id: u64, target_file_id: u64) -> Result<(), StorageError> {
        debug!("🔄 Copying file data from {} to {}", source_file_id, target_file_id);
        
        // Get source file data
        let source_data = self.get_file_data(source_file_id).await?;
        
        match source_data {
            Some(data) => {
                // Store data to target file
                self.store_file_data_internal(target_file_id, &data).await?;
                
                // Update in-memory cache
                self.files.lock().unwrap().insert(target_file_id, data);
                
                debug!("✅ File data copied successfully from {} to {}", source_file_id, target_file_id);
                Ok(())
            },
            None => {
                error!("❌ Source file data not found: {}", source_file_id);
                Err(StorageError::NotFound(format!("Source file data not found: {}", source_file_id)))
            }
        }
    }

    /// Restore file version - combines metadata and data restoration
    /// Note: This method only handles file data copying and notification.
    /// The actual database storage should be handled by the calling service.
    pub async fn restore_file_version(
        &self,
        source_file_info: &ModelFileInfo,
        target_file_info: &ModelFileInfo
    ) -> Result<(), StorageError> {
        debug!("🔄 Restoring file version: source={}, target={}", 
               source_file_info.file_id, target_file_info.file_id);
        
        // Copy file data from source to target
        self.copy_file_data(source_file_info.file_id, target_file_info.file_id).await?;
        
        // Update in-memory cache
        let file_data = self.get_file_data(target_file_info.file_id).await?.unwrap_or_default();
        debug!("🗂️ Updating in-memory cache for restored file...");
        self.files.lock().unwrap().insert(target_file_info.file_id, file_data);
        debug!("✅ In-memory cache updated for restored file");
        
        debug!("✅ File version data restoration completed");
        Ok(())
    }

    /// Send file restore notification (복원 전용)
    pub async fn send_file_restore_notification(&self, file_info: &ModelFileInfo) {
        // Send notification including the requesting device (파일 복원 시에는 요청한 장치도 포함)
        self.send_file_update_notification_with_options(
            file_info, 
            sync::file_update_notification::UpdateType::Updated,
            true // include_source_device = true for file restoration
        ).await;
    }
    
    /// Get file metadata (returns only proto FileInfo)
    pub async fn get_file_info(&self, file_id: u64) -> Result<Option<ModelFileInfo>, StorageError> {
        debug!("Getting file info: file_id={}", file_id);
        self.storage.get_file_info(file_id).await
    }
    
    /// Get file data
    /// Get file data
    pub async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>, StorageError> {
        debug!("Getting file data: file_id={}", file_id);

        // Check in-memory cache first (optional)
        let cached_data = self.files.lock().unwrap().get(&file_id).cloned();
        
        if let Some(data) = cached_data {
            return Ok(Some(data));
        }
        
        // 캐시에 없으면 스토리지에서 가져옴
        self.storage.get_file_data(file_id).await
    }
    
    /// List files for an account/device/group (returns only proto FileInfo)
    pub async fn list_files(
        &self,
        group_id: i32,
        account_hash: &str,
    ) -> StorageResult<Vec<FileInfoData>> {
        debug!("Listing files: group_id={}, account_hash={}", group_id, account_hash);
        // list_files 메서드는 account_hash, group_id, upload_time_from 매개변수가 필요합니다
        let files = self.storage.list_files(account_hash, group_id, None).await?;
        let result = files.into_iter().map(|f| f.into()).collect();
        Ok(result)
    }
    
    /// List files for an account/device/group but filter out files uploaded by the same device
    /// This prevents sending files back to the device that uploaded them
    pub async fn list_files_filtered_by_device(
        &self,
        group_id: i32,
        account_hash: &str,
        device_hash: &str,
    ) -> StorageResult<Vec<FileInfoData>> {
        debug!("Listing files with device filter: group_id={}, account_hash={}, device_hash={}", 
               group_id, account_hash, device_hash);
        
        // 스토리지 레이어의 list_files_except_device 메서드 사용
        let files = self.storage.list_files_except_device(account_hash, group_id, device_hash, None).await?;
        let result: Vec<FileInfoData> = files.into_iter().map(|f| f.into()).collect();
        
        debug!("Filtered files for device {}: total={}", device_hash, result.len());
        
        Ok(result)
    }
    
    /// 파일 정보 조회 (삭제된 파일 포함)
    pub async fn get_file_info_include_deleted(&self, file_id: u64) -> Result<Option<(ModelFileInfo, bool)>, StorageError> {
        debug!("파일 정보 조회 (삭제 여부 포함): file_id={}", file_id);
        self.storage.get_file_info_include_deleted(file_id).await
    }

    /// 파일 삭제 처리
    pub async fn delete_file(&self, file_id: u64) -> Result<(), StorageError> {
        info!("파일 삭제 처리 시작: file_id={}", file_id);
        
        // 파일 정보 조회 (삭제 여부 포함)
        let file_info_result = self.storage.get_file_info_include_deleted(file_id).await?;
        
        match file_info_result {
            Some((file_info, is_deleted)) => {
                debug!("파일 정보 조회됨: file_id={}, account_hash={}, file_path={}, filename={}, is_deleted={}", 
                       file_id, file_info.account_hash, file_info.file_path, file_info.filename, is_deleted);
                
                if is_deleted {
                    info!("파일이 이미 삭제되어 있음: file_id={}", file_id);
                    // 이미 삭제된 경우 성공으로 처리
                    return Ok(());
                }
                
                // 파일 삭제 처리
                debug!("파일 삭제 처리 시작: file_id={}, account_hash={}, file_path={}, filename={}", 
                       file_id, file_info.account_hash, file_info.file_path, file_info.filename);
                
                match self.storage.delete_file(&file_info.account_hash, file_id).await {
                    Ok(_) => {
                        // 메모리 캐시에서 삭제
                        self.files.lock().unwrap().remove(&file_id);
                        
                        // 파일 업데이트 알림 전송 (선택적)
                        if let Some(nm) = &self.notification_manager {
                            let notification = FileUpdateNotification {
                                account_hash: file_info.account_hash.clone(),
                                device_hash: file_info.device_hash.clone(),
                                file_info: Some(file_info.to_sync_file()),
                                update_type: crate::sync::file_update_notification::UpdateType::Deleted as i32,
                                timestamp: chrono::Utc::now().timestamp(),
                            };
                            
                            match nm.broadcast_file_update(notification).await {
                                Ok(sent) => debug!("파일 삭제 알림 {}개 전송 완료: file_id={}", sent, file_id),
                                Err(e) => warn!("파일 삭제 알림 전송 실패: {}", e),
                            }
                        }
                        
                        info!("파일 삭제 완료: file_id={}, account_hash={}, file_path={}, filename={}", 
                              file_id, file_info.account_hash, file_info.file_path, file_info.filename);
                        Ok(())
                    },
                    Err(e) => {
                        error!("파일 삭제 실패: file_id={}, 오류={}", file_id, e);
                        Err(e)
                    }
                }
            },
            None => {
                error!("삭제할 파일을 찾을 수 없음: file_id={}", file_id);
                Err(StorageError::NotFound(format!("파일을 찾을 수 없음: {}", file_id)))
            }
        }
    }

    /// Device registration: use register_device everywhere
    pub async fn register_device(&self, device: &Device) -> Result<(), StorageError> {
        debug!("Registering device: device_hash={}", device.device_hash);
        
        self.storage.register_device(device).await
    }

    /// File storage: do not use encryption_keys
    pub async fn store_file_info(&self, file_info: &ModelFileInfo) -> Result<(), StorageError> {
        debug!("Storing file info: file_id={}, filename={}", file_info.file_id, file_info.filename);
        self.storage.store_file_info(file_info.clone()).await?;
        Ok(())
    }
    
    pub async fn store_file_data(&self, file_id: u64, file_data: &Vec<u8>) -> Result<(), StorageError> {
        debug!("Storing file data: file_id={}", file_id);
        self.storage.store_file_data(file_id, file_data.clone()).await
    }

    pub async fn get_file(&self, file_id: u64) -> Result<(ModelFileInfo, Vec<u8>), StorageError> {
        debug!("Getting file: file_id={}", file_id);
        
        // Check in-memory cache first (optional)
        let cached_data = self.files.lock().unwrap().get(&file_id).cloned();
        
        if let Some(data) = cached_data {
            // Get file metadata from storage
            let file_info = self.storage.get_file_info(file_id).await?;
            if let Some(info) = file_info {
                return Ok((info, data));
            }
        }
        
        // Get file metadata from storage
        let file_info = self.storage.get_file_info(file_id).await?;
        if let Some(info) = file_info {
            // Get file data from file storage or fallback to legacy storage
            let data = match self.file_storage.as_ref() {
                Some(file_storage) => {
                    debug!("Getting file data from file storage");
                    file_storage.get_file_data(file_id).await?
                }
                None => {
                    debug!("Getting file data from legacy storage");
                    self.storage.get_file_data(file_id).await?
                }
            };
            
            if let Some(file_data) = data {
                // Update in-memory cache (optional)
                self.files.lock().unwrap().insert(file_id, file_data.clone());
                return Ok((info, file_data));
            }
        }
        
        Err(StorageError::NotFound("File not found".to_string()))
    }
    
    /// 로컬 경로와 파일 이름으로 파일 정보 찾기
    pub async fn find_file_by_local_path(
        &self,
        account_hash: &str,
        file_path: &str,
        filename: &str,
        revision: i64,
    ) -> Result<Option<ModelFileInfo>, StorageError> {
        debug!("Finding file by local path: account={}, path={}, filename={}, revision={}", 
              account_hash, file_path, filename, revision);
        
        // Normalize file path to preserve tilde (~) prefix for home directory
        let normalized_file_path = crate::utils::helpers::normalize_path_preserve_tilde(file_path);
        debug!("Service find_file_by_local_path - Original path: {}, Normalized path: {}", file_path, normalized_file_path);
        
        // Storage에서 파일 정보 조회
        self.storage.find_file_by_path_and_name(account_hash, &normalized_file_path, filename, revision).await
    }

    /// 경로와 이름으로 파일 찾기
    pub async fn find_file_by_criteria(
        &self, 
        account_hash: &str,
        group_id: i32,
        watcher_id: i32,
        file_path: &str,
        file_name: &str
    ) -> StorageResult<Option<FileInfoData>> {
        debug!("Searching file by criteria: account={}, group_id={}, watcher_id={}, path={}, name={}", 
               account_hash, group_id, watcher_id, file_path, file_name);

        // Normalize file path to preserve tilde (~) prefix for home directory
        let normalized_file_path = crate::utils::helpers::normalize_path_preserve_tilde(file_path);
        debug!("Service find_file_by_criteria - Original path: {}, Normalized path: {}", file_path, normalized_file_path);

        // 경로가 이미 파일명을 포함하고 있는지 확인
        let search_path: String;
        let search_name: &str = file_name;
        
        if normalized_file_path.ends_with(&format!("/{}", file_name)) || normalized_file_path.ends_with(file_name) {
            // 경로에 이미 파일명이 포함되어 있는 경우
            debug!("경로가 이미 파일명을 포함하고 있습니다: {}", normalized_file_path);
            
            // 파일명을 포함한 전체 경로를 검색 경로로 사용
            search_path = normalized_file_path.to_string();
        } else {
            // 경로와 파일명 결합
            search_path = if normalized_file_path.ends_with('/') {
                format!("{}{}", normalized_file_path, file_name)
            } else if !normalized_file_path.is_empty() {
                format!("{}/{}", normalized_file_path, file_name)
            } else {
                file_name.to_string()
            };
        }
        
        debug!("Full path for search: {}", search_path);

        // Storage 레이어에서 파일 검색 - 존재하는 최신 파일만 조회 (is_deleted = false)
        // group_id와 watcher_id 조건 추가
        match self.storage.find_file_by_criteria(account_hash, group_id, watcher_id, &search_path, search_name).await {
            Ok(Some(file_info)) => {
                info!("파일을 찾았습니다: ID={}, 경로={}, 이름={}, 리비전={}, group_id={}, watcher_id={}", 
                      file_info.file_id, search_path, search_name, file_info.revision, group_id, watcher_id);
                Ok(Some(file_info.into()))
            },
            Ok(None) => {
                debug!("파일을 찾을 수 없습니다: 경로={}, 이름={}, group_id={}, watcher_id={}", 
                      search_path, search_name, group_id, watcher_id);
                Ok(None)
            },
            Err(e) => {
                error!("파일 검색 중 오류: {}", e);
                Err(e)
            }
        }
    }

    /// 파일 ID로 존재 여부와 삭제 상태 확인
    pub async fn check_file_exists(&self, file_id: u64) -> Result<(bool, bool), Error> {
        debug!("Checking if file exists with file_id: {}", file_id);
        
        match self.storage.check_file_exists(file_id).await {
            Ok((exists, is_deleted)) => {
                debug!(
                    "File check result - exists: {}, is_deleted: {}, file_id: {}",
                    exists, is_deleted, file_id
                );
                Ok((exists, is_deleted))
            }
            Err(e) => {
                error!("Error checking file existence: {}", e);
                Err(Error::new(ErrorKind::Other, format!("Failed to check file existence: {}", e)))
            }
        }
    }

    /// 파일 ID로 존재 여부 확인 (CheckFileExists RPC용)
    pub async fn check_file_exists_rpc(&self, file_id: u64) -> Result<bool, StorageError> {
        debug!("Checking if file exists for RPC with file_id: {}", file_id);
        
        match self.storage.check_file_exists(file_id).await {
            Ok((exists, is_deleted)) => {
                debug!("File check result - exists: {}, is_deleted: {}, file_id: {}", exists, is_deleted, file_id);
                
                // 파일이 존재하는지 여부만 반환 (is_deleted 상태 무시)
                // CheckFileExists RPC에서는 삭제된 파일을 포함해 모든 파일의 존재 여부를 확인
                Ok(exists)
            }
            Err(e) => {
                error!("Error checking file existence: {}", e);
                Err(StorageError::General(format!("Failed to check file existence: {}", e)))
            }
        }
    }
} 