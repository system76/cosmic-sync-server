use async_trait::async_trait;
use tonic::{Request, Response, Status};
use crate::sync::{
    UploadFileRequest, UploadFileResponse,
    DownloadFileRequest, DownloadFileResponse,
    ListFilesRequest, ListFilesResponse,
    DeleteFileRequest, DeleteFileResponse,
    AuthUpdateNotification, DeviceUpdateNotification, 
    EncryptionKeyUpdateNotification, FileUpdateNotification,
    WatcherPresetUpdateNotification, WatcherGroupUpdateNotification,
    VersionUpdateNotification,
    FindFileRequest, FindFileResponse,
    CheckFileExistsRequest, CheckFileExistsResponse
};
use std::sync::Arc;
// use std::panic; // not used
use crate::server::app_state::AppState;
use crate::services::Handler;
// use crate::sync; // not used
use crate::utils::response;
use tracing::{info, warn, error, debug};
// use chrono::Utc; // not used
// use prost_types; // not used
// use rand; // not used

/// Handler for file-related requests
pub struct FileHandler {
    pub app_state: Arc<AppState>,
}

impl FileHandler {
    /// Create a new file handler
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }
    
    /// Handle file upload request with enhanced error handling for stream errors
    pub async fn upload_file(&self, request: Request<UploadFileRequest>) -> Result<Response<UploadFileResponse>, Status> {
        let req = request.into_inner();
        super::file::upload::handle_upload_file(self, req).await
    }
    
    /// Log upload request details
    pub(crate) fn log_upload_request(&self, req: &UploadFileRequest) {
        info!("File upload request started:");
        info!("   filename: {}", req.filename);
        info!("   file_size: {} bytes", req.file_size);
        info!("   account_hash: {}", req.account_hash);
        info!("   device_hash: {}", req.device_hash);
        info!("   group_id: {}", req.group_id);
        info!("   watcher_id: {}", req.watcher_id);
        info!("   file_path: {}", req.file_path);
        info!("   file_data length: {} bytes", req.file_data.len());
        let key_id_log = if req.key_id.is_empty() { "<empty>" } else { "<present>" };
        info!("   is_encrypted: {}, key_id: {}", req.is_encrypted, key_id_log);
    }
    
    /// Validate upload input
    pub(crate) fn validate_upload_input(&self, req: &UploadFileRequest) -> Result<(), String> {
        if req.account_hash.is_empty() || req.device_hash.is_empty() || req.file_hash.is_empty() {
            return Err("Missing required fields: account_hash/device_hash/file_hash".to_string());
        }
        if req.group_id <= 0 || req.watcher_id <= 0 {
            return Err("Invalid group_id/watcher_id".to_string());
        }
        if req.filename.trim().is_empty() || req.file_path.trim().is_empty() {
            return Err("filename/file_path must not be empty".to_string());
        }
        if req.is_encrypted && req.key_id.trim().is_empty() {
            return Err("key_id is required when is_encrypted is true".to_string());
        }
        if !req.is_encrypted && !req.key_id.trim().is_empty() {
            return Err("key_id must be empty when is_encrypted is false".to_string());
        }
        Ok(())
    }
    
    /// Normalize file path with error handling
    pub(crate) fn normalize_file_path(&self, file_path: &str) -> Result<String, String> {
        debug!("Normalizing file path...");
        
        let normalized_file_path = if file_path.is_empty() {
            warn!("file_path is empty, using default");
            "~/".to_string()
        } else {
            match std::panic::catch_unwind(|| {
                crate::utils::helpers::normalize_path_preserve_tilde(file_path)
            }) {
                Ok(path) => path,
                Err(_) => {
                    error!("Panic during path normalization: {}", file_path);
                    return Err("File path processing error".to_string());
                }
            }
        };

        if normalized_file_path.len() > 1024 {
            error!("Normalized path too long: {} chars (max 1024)", normalized_file_path.len());
            return Err("File path too long".to_string());
        }
        
        debug!("Path normalized: {}", normalized_file_path);
        Ok(normalized_file_path)
    }
    
    /// Generate file ID with error handling
    pub(crate) fn generate_file_id(&self, req: &UploadFileRequest) -> Result<u64, String> {
        debug!("Generating file ID...");
        
        match std::panic::catch_unwind(|| {
            crate::utils::crypto::generate_file_id(&req.account_hash, &req.filename, &req.file_hash)
        }) {
            Ok(id) => {
                debug!("File ID generated: {}", id);
                Ok(id)
            },
            Err(_) => {
                error!("Panic during file ID generation");
                Err("File ID generation error".to_string())
            }
        }
    }
    
    // Removed legacy create_file_info / create_file_info_with_server_ids / convert_to_server_ids
    
    /// validate if file path is within watcher folder (single attempt, fail immediately)
    pub(crate) async fn validate_file_path_with_watcher(&self, req: &crate::sync::UploadFileRequest, normalized_file_path: &str) -> Result<(), String> {
        debug!("Starting single-attempt file path validation: group_id={}, watcher_id={}, file_path={}", 
               req.group_id, req.watcher_id, normalized_file_path);
        
        // get watcher info once (no retry)
        let watcher = match self.get_watcher_info_once(&req.account_hash, req.group_id, req.watcher_id).await {
            Ok(watcher) => watcher,
            Err(e) => {
                error!("Failed to get watcher info (no retry): {}", e);
                return Err("Watcher not found or validation failed".to_string());
            }
        };
        
        // normalize watcher folder path
        let normalized_watcher_folder = crate::utils::helpers::normalize_path_preserve_tilde(&watcher.folder);
        debug!("Normalized watcher folder: {}", normalized_watcher_folder);
        
        // validate path: check if file path is within watcher folder
        let file_is_in_watcher_folder = if watcher.recursive_path {
            // if watcher is recursive, check if file path starts with watcher folder
            normalized_file_path.starts_with(&normalized_watcher_folder) ||
            normalized_file_path == normalized_watcher_folder
        } else {
            // if watcher is not recursive, check if file path is exactly the same as watcher folder
            let file_parent = std::path::Path::new(normalized_file_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "".to_string());
            file_parent == normalized_watcher_folder
        };
        
        if !file_is_in_watcher_folder {
            error!("File path validation failed: file '{}' is not within watcher folder '{}' (recursive: {})", 
                   normalized_file_path, normalized_watcher_folder, watcher.recursive_path);
            return Err(format!("File path '{}' is not within watcher folder '{}'", 
                             normalized_file_path, normalized_watcher_folder));
        }
        
        debug!("File path validation successful: file is within watcher folder");
        Ok(())
    }
    
    /// get watcher info once (no retry)
    async fn get_watcher_info_once(&self, account_hash: &str, group_id: i32, watcher_id: i32) -> Result<crate::sync::WatcherData, String> {
        debug!("Getting watcher info once: account={}, group_id={}, watcher_id={}", 
               account_hash, group_id, watcher_id);
        
        match self.app_state.storage.get_watcher_by_group_and_id(account_hash, group_id, watcher_id).await {
            Ok(Some(watcher)) => {
                debug!("Watcher found: folder={}, recursive={}", watcher.folder, watcher.recursive_path);
                Ok(watcher)
            },
            Ok(None) => {
                error!("Watcher not found: group_id={}, watcher_id={}", group_id, watcher_id);
                Err(format!("Watcher not found: group_id={}, watcher_id={}", group_id, watcher_id))
            },
            Err(e) => {
                error!("Database error while getting watcher: {}", e);
                Err(format!("Database error: {}", e))
            }
        }
    }
    
    /// Handle file download request
    pub async fn download_file(&self, request: Request<DownloadFileRequest>) -> Result<Response<DownloadFileResponse>, Status> {
        let req = request.into_inner();
        super::file::download::handle_download_file(self, req).await
    }
    
    /// Handle file list request with optional time-based filtering for recovery sync
    pub async fn list_files(&self, request: Request<ListFilesRequest>) -> Result<Response<ListFilesResponse>, Status> {
        let req = request.into_inner();
        super::file::list::handle_list_files(self, req).await
    }

    /// Delete file - internal implementation  
    pub async fn delete_file_internal(&self, request: Request<DeleteFileRequest>) -> Result<Response<DeleteFileResponse>, Status> {
        let req = request.into_inner();
        super::file::delete::handle_delete_file(self, req).await
    }
    
    /// Validate file for deletion
    pub(crate) async fn validate_file_for_deletion(&self, file_id: u64) -> Result<u64, Response<DeleteFileResponse>> {
        debug!("Deleting directly by file ID: file_id={}", file_id);
        
        match self.app_state.file.check_file_exists(file_id).await {
            Ok((exists, is_deleted)) => {
                info!("File status check: file_id={}, exists={}, is_deleted={}", file_id, exists, is_deleted);
                if !exists {
                    warn!("File does not exist: file_id={}", file_id);
                    return Err(Response::new(response::file_delete_error("File not found")));
                }
                if is_deleted {
                    warn!("File already deleted: file_id={}", file_id);
                    return Err(Response::new(response::file_delete_success("File already deleted")));
                }
                Ok(file_id)
            }
            Err(e) => {
                error!("Failed to check file existence: {}", e);
                Err(Response::new(response::file_delete_error(format!("Failed to check file status: {}", e))))
            }
        }
    }
    
    /// Find file by path
    pub(crate) async fn find_file_by_path(&self, req: &DeleteFileRequest) -> Result<u64, Response<DeleteFileResponse>> {
        debug!("Searching file by name: filename={}, path={}", req.filename, req.file_path);
        
        let normalized_file_path = crate::utils::helpers::normalize_path_preserve_tilde(&req.file_path);
        debug!("Path normalized: original={}, normalized={}", req.file_path, normalized_file_path);
        
        match self.app_state.file.find_file_by_local_path(
            &req.account_hash,
            &normalized_file_path,
            &req.filename,
            req.revision
        ).await {
            Ok(Some(info)) => {
                debug!("File found: file_id={}, filename={}", info.file_id, info.filename);
                Ok(info.file_id)
            },
            Ok(None) => {
                warn!("File not found: filename={}, path={}", req.filename, normalized_file_path);
                Err(Response::new(response::file_delete_error(format!("File not found: {}", req.filename))))
            }
            Err(e) => {
                error!("Failed to search file: {}", e);
                Err(Response::new(response::file_delete_error(format!("Failed to search file: {}", e))))
            }
        }
    }

    /// Handle find file by criteria request
    pub async fn find_file_by_criteria(&self, request: Request<FindFileRequest>) -> Result<Response<FindFileResponse>, Status> {
        let req = request.into_inner();
        super::file::find::handle_find_file_by_criteria(self, req).await
    }

    /// Handle check file exists request
    pub async fn check_file_exists(&self, request: Request<CheckFileExistsRequest>) -> Result<Response<CheckFileExistsResponse>, Status> {
        let req = request.into_inner();
        super::file::exists::handle_check_file_exists(self, req).await
    }
}

#[async_trait]
impl Handler for FileHandler {
    // 스트리밍 반환 타입 정의
    type SubscribeToAuthUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<AuthUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToDeviceUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<DeviceUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToEncryptionKeyUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<EncryptionKeyUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToFileUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<FileUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherPresetUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherPresetUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToWatcherGroupUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<WatcherGroupUpdateNotification, Status>> + Send + 'static>>;
    type SubscribeToVersionUpdatesStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<VersionUpdateNotification, Status>> + Send + 'static>>;

    async fn handle_upload_file(
        &self,
        request: Request<UploadFileRequest>,
    ) -> Result<Response<UploadFileResponse>, Status> {
        self.upload_file(request).await
    }

    async fn handle_download_file(
        &self,
        request: Request<DownloadFileRequest>,
    ) -> Result<Response<DownloadFileResponse>, Status> {
        self.download_file(request).await
    }

    async fn handle_list_files(
        &self,
        request: Request<ListFilesRequest>,
    ) -> Result<Response<ListFilesResponse>, Status> {
        self.list_files(request).await
    }

    async fn handle_delete_file(
        &self,
        request: Request<DeleteFileRequest>,
    ) -> Result<Response<DeleteFileResponse>, Status> {
        self.delete_file_internal(request).await
    }

    async fn handle_find_file_by_criteria(
        &self,
        request: Request<FindFileRequest>,
    ) -> Result<Response<FindFileResponse>, Status> {
        self.find_file_by_criteria(request).await
    }

    async fn handle_check_file_exists(
        &self,
        request: Request<CheckFileExistsRequest>,
    ) -> Result<Response<CheckFileExistsResponse>, Status> {
        self.check_file_exists(request).await
    }
} 