use async_trait::async_trait;
use tonic::{Request, Response, Status};
use crate::sync::{
    UploadFileRequest, UploadFileResponse,
    DownloadFileRequest, DownloadFileResponse,
    ListFilesRequest, ListFilesResponse,
    DeleteFileRequest, DeleteFileResponse,
    FileInfo,
    AuthUpdateNotification, DeviceUpdateNotification, 
    EncryptionKeyUpdateNotification, FileUpdateNotification,
    WatcherPresetUpdateNotification, WatcherGroupUpdateNotification,
    VersionUpdateNotification,
    FindFileRequest, FindFileResponse,
    SubscribeRequest,
    CheckFileExistsRequest, CheckFileExistsResponse
};
use std::sync::Arc;
use std::panic;
use crate::server::app_state::AppState;
use crate::services::Handler;
use crate::sync;
use crate::utils::{auth, response};
use tracing::{info, warn, error, debug};
use chrono::Utc;
use prost_types;
use rand;

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
        
        // Log request details
        self.log_upload_request(&req);
        
        // Validate file data size
        if req.file_data.len() != req.file_size as usize {
            error!("File data size mismatch: declared={}, actual={}", req.file_size, req.file_data.len());
            return Ok(Response::new(response::file_upload_error("File data size mismatch")));
        }
        
        // 1. Verify authentication
        if let Err(_) = auth::verify_auth_token(&self.app_state.oauth, &req.auth_token, &req.account_hash).await {
            return Ok(Response::new(response::file_upload_error("Authentication failed")));
        }
        
        // 2. Validate input
        if let Err(msg) = self.validate_upload_input(&req) {
            return Ok(Response::new(response::file_upload_error(msg)));
        }
        
        // 3. Normalize file path
        let normalized_file_path = match self.normalize_file_path(&req.file_path) {
            Ok(path) => path,
            Err(msg) => return Ok(Response::new(response::file_upload_error(msg))),
        };
        
        // 4. Validate file path with watcher (single attempt, fail immediately if not valid)
        if let Err(msg) = self.validate_file_path_with_watcher(&req, &normalized_file_path).await {
            return Ok(Response::new(response::file_upload_error(msg)));
        }
        
        // 5. Generate file ID
        let file_id = match self.generate_file_id(&req) {
            Ok(id) => id,
            Err(msg) => return Ok(Response::new(response::file_upload_error(msg))),
        };
        
        // 6. Convert client IDs to server IDs
        let (server_group_id, server_watcher_id) = match self.convert_to_server_ids(&req).await {
            Ok(ids) => ids,
            Err(msg) => return Ok(Response::new(response::file_upload_error(msg))),
        };
        
        // 7. Create file info with server IDs
        let file_info = self.create_file_info_with_server_ids(
            &req, 
            file_id, 
            normalized_file_path,
            server_group_id,
            server_watcher_id
        );
        
        // 8. Store file
        match self.app_state.file.store_file(&file_info, &req.file_data).await {
            Ok(_) => {
                Ok(Response::new(response::file_upload_success(file_id, req.revision + 1)))
            }
            Err(e) => {
                error!("File storage failed: {}", e);
                Ok(Response::new(response::file_upload_error(&format!("File storage failed: {}", e))))
            }
        }
    }
    
    /// Log upload request details
    fn log_upload_request(&self, req: &UploadFileRequest) {
        info!("File upload request started:");
        info!("   filename: {}", req.filename);
        info!("   file_size: {} bytes", req.file_size);
        info!("   account_hash: {}", req.account_hash);
        info!("   device_hash: {}", req.device_hash);
        info!("   group_id: {}", req.group_id);
        info!("   watcher_id: {}", req.watcher_id);
        info!("   file_path: {}", req.file_path);
        info!("   file_data length: {} bytes", req.file_data.len());
    }
    
    /// Validate upload input
    fn validate_upload_input(&self, req: &UploadFileRequest) -> Result<(), String> {
        debug!("Validating input data...");
        
        if req.account_hash.is_empty() {
            error!("account_hash is empty");
            return Err("Account hash is required".to_string());
        }

        if req.filename.is_empty() {
            error!("filename is empty");
            return Err("Filename is required".to_string());
        }

        if req.file_hash.is_empty() {
            error!("file_hash is empty");
            return Err("File hash is required".to_string());
        }
        
        debug!("Input validation complete");
        Ok(())
    }
    
    /// Normalize file path with error handling
    fn normalize_file_path(&self, file_path: &str) -> Result<String, String> {
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
    fn generate_file_id(&self, req: &UploadFileRequest) -> Result<u64, String> {
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
    
    /// Create file info structure
    fn create_file_info(&self, req: &UploadFileRequest, file_id: u64, normalized_file_path: String) -> crate::models::file::FileInfo {
        debug!("Creating file info structure...");
        
        let file_info = crate::models::file::FileInfo {
            file_id,
            filename: req.filename.clone(),
            file_hash: req.file_hash.clone(),
            device_hash: req.device_hash.clone(),
            group_id: req.group_id,
            watcher_id: req.watcher_id,
            is_encrypted: req.is_encrypted,
            file_path: normalized_file_path,
            updated_time: prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            },
            revision: req.revision,
            account_hash: req.account_hash.clone(),
            size: req.file_size,
        };
        
        debug!("File info structure created");
        file_info
    }
    
    /// Create file info structure with server IDs
    fn create_file_info_with_server_ids(
        &self, 
        req: &UploadFileRequest, 
        file_id: u64, 
        normalized_file_path: String,
        server_group_id: i32,
        server_watcher_id: i32
    ) -> crate::models::file::FileInfo {
        debug!("Creating file info structure with server IDs...");
        debug!("Client IDs: group_id={}, watcher_id={}", req.group_id, req.watcher_id);
        debug!("Server IDs: group_id={}, watcher_id={}", server_group_id, server_watcher_id);
        
        let file_info = crate::models::file::FileInfo {
            file_id,
            filename: req.filename.clone(),
            file_hash: req.file_hash.clone(),
            device_hash: req.device_hash.clone(),
            group_id: server_group_id,      // 서버 ID 사용
            watcher_id: server_watcher_id,  // 서버 ID 사용
            is_encrypted: req.is_encrypted,
            file_path: normalized_file_path,
            updated_time: prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            },
            revision: req.revision,
            account_hash: req.account_hash.clone(),
            size: req.file_size,
        };
        
        debug!("File info structure created with server IDs");
        file_info
    }
    
    /// Convert client IDs to server IDs
    async fn convert_to_server_ids(&self, req: &UploadFileRequest) -> Result<(i32, i32), String> {
        debug!("Converting client IDs to server IDs: group_id={}, watcher_id={}", 
               req.group_id, req.watcher_id);
        
        match self.app_state.storage.get_server_ids(&req.account_hash, req.group_id, req.watcher_id).await {
            Ok(Some((server_group_id, server_watcher_id))) => {
                debug!("Converted to server IDs: group_id={}, watcher_id={}", 
                       server_group_id, server_watcher_id);
                Ok((server_group_id, server_watcher_id))
            },
            Ok(None) => {
                error!("No matching group/watcher found for client IDs: group_id={}, watcher_id={}", 
                       req.group_id, req.watcher_id);
                Err("Watcher group or watcher not found".to_string())
            },
            Err(e) => {
                error!("Failed to convert IDs: {}", e);
                Err(format!("Failed to convert IDs: {}", e))
            }
        }
    }
    
    /// validate if file path is within watcher folder (single attempt, fail immediately)
    async fn validate_file_path_with_watcher(&self, req: &crate::sync::UploadFileRequest, normalized_file_path: &str) -> Result<(), String> {
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
        let file_id = req.file_id;
        
        // Verify authentication
        if let Err(_) = auth::verify_auth_token(&self.app_state.oauth, &req.auth_token, &req.account_hash).await {
            return Ok(Response::new(response::file_download_error("Authentication failed")));
        }
        
        // Get file info
        let file_info = match self.app_state.file.get_file_info(file_id).await {
            Ok(Some(info)) => info,
            Ok(None) => {
                return Ok(Response::new(response::file_download_error("File not found")));
            }
            Err(e) => {
                error!("Failed to get file info: {}", e);
                return Ok(Response::new(response::file_download_error(format!("Failed to get file info: {}", e))));
            }
        };
        
        // Get file data
        match self.app_state.file.get_file_data(file_id).await {
            Ok(Some(data)) => {
                info!("Sending file download response: file_id={}, filename={}, file_path={}", 
                      file_id, file_info.filename, file_info.file_path);
                Ok(Response::new(DownloadFileResponse {
                    success: true,
                    file_data: data,
                    file_hash: file_info.file_hash,
                    is_encrypted: file_info.is_encrypted,
                    return_message: "".to_string(),
                    filename: file_info.filename.clone(),
                    file_path: file_info.file_path.clone(),
                    updated_time: Some(prost_types::Timestamp {
                        seconds: file_info.updated_time.seconds,
                        nanos: 0,
                    }),
                }))
            }
            Ok(None) => {
                Ok(Response::new(response::file_download_error("File data not found")))
            }
            Err(e) => {
                error!("Failed to get file data: {}", e);
                Ok(Response::new(response::file_download_error(format!("Failed to get file data: {}", e))))
            }
        }
    }
    
    /// Handle file list request with optional time-based filtering for recovery sync
    pub async fn list_files(&self, request: Request<ListFilesRequest>) -> Result<Response<ListFilesResponse>, Status> {
        let req = request.into_inner();
        debug!("File list request: account={}, device={}, group_id={}, time_filter={:?}", 
               req.account_hash, req.device_hash, req.group_id, req.upload_time_from);
        
        // Verify authentication
        if let Err(_) = auth::verify_auth_token(&self.app_state.oauth, &req.auth_token, &req.account_hash).await {
            return Ok(Response::new(ListFilesResponse {
                success: false,
                files: Vec::new(),
                return_message: "Authentication failed".to_string(),
            }));
        }
        
        // Convert client group_id to server group_id
        let server_group_id = match self.app_state.storage.get_server_group_id(&req.account_hash, req.group_id).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                debug!("No matching group found for client group_id={}", req.group_id);
                return Ok(Response::new(ListFilesResponse {
                    success: true,
                    files: Vec::new(),
                    return_message: "No files found".to_string(),
                }));
            },
            Err(e) => {
                error!("Failed to convert group_id: {}", e);
                return Err(Status::internal(format!("Failed to convert group_id: {}", e)));
            }
        };
        
        debug!("Converted client group_id={} to server group_id={}", req.group_id, server_group_id);
        
        // Convert protobuf timestamp to chrono DateTime for time filtering
        let time_filter = req.upload_time_from.map(|ts| {
            chrono::DateTime::<chrono::Utc>::from_utc(
                chrono::NaiveDateTime::from_timestamp_opt(ts.seconds, ts.nanos as u32)
                    .unwrap_or_else(|| chrono::Utc::now().naive_utc()),
                chrono::Utc
            )
        });
        
        if let Some(ref filter_time) = time_filter {
            info!("🔍 Recovery sync requested: filtering files updated since {}", filter_time);
        }
        
        // Get filtered files using server group_id
        match self.app_state.file.list_files_filtered_by_device(server_group_id, &req.account_hash, &req.device_hash).await {
            Ok(files) => {
                // Convert server IDs back to client IDs for response
                let mut sync_files = Vec::new();
                
                let mut files_processed = 0;
                let mut files_filtered = 0;
                
                for file in files.iter() {
                    files_processed += 1;
                    
                    // Apply time filter if provided (for recovery sync)
                    if let Some(ref filter_time) = time_filter {
                        let file_updated_time = chrono::DateTime::<chrono::Utc>::from_utc(
                            chrono::NaiveDateTime::from_timestamp_opt(file.updated_time.seconds, 0)
                                .unwrap_or_else(|| chrono::Utc::now().naive_utc()),
                            chrono::Utc
                        );
                        
                        // Skip files that haven't been updated since the filter time
                        if file_updated_time <= *filter_time {
                            files_filtered += 1;
                            continue;
                        }
                        
                        debug!("🔄 Including file {} (updated: {}) in recovery sync", file.filename, file_updated_time);
                    }
                    
                    // Convert server watcher_id to client watcher_id
                    let client_watcher_id = match self.app_state.storage.get_client_watcher_id(
                        &req.account_hash, 
                        server_group_id, 
                        file.watcher_id
                    ).await {
                        Ok(Some((_, watcher_id))) => watcher_id, // Extract watcher_id from tuple
                        Ok(None) => {
                            warn!("No client watcher_id found for server watcher_id={}", file.watcher_id);
                            file.watcher_id // Fallback to server ID
                        },
                        Err(e) => {
                            error!("Failed to get client watcher_id: {}", e);
                            file.watcher_id // Fallback to server ID
                        }
                    };
                    
                    let file_info = sync::FileInfo {
                        file_id: file.file_id,
                        filename: file.filename.clone(),
                        file_hash: file.file_hash.clone(),
                        device_hash: file.device_hash.clone(),
                        group_id: req.group_id,         // Return client group_id
                        watcher_id: client_watcher_id,  // Return client watcher_id
                        is_encrypted: file.is_encrypted,
                        file_path: file.file_path.clone(),
                        updated_time: Some(prost_types::Timestamp {
                            seconds: file.updated_time.seconds,
                            nanos: 0,
                        }),
                        revision: file.revision,
                        file_size: file.size,
                    };
                    
                    sync_files.push(file_info);
                }
                
                // Log filtering results
                if let Some(_) = time_filter {
                    info!("📊 Recovery sync results: {} files processed, {} filtered out, {} files returned", 
                          files_processed, files_filtered, sync_files.len());
                } else {
                    debug!("📋 File list: {} files returned", sync_files.len());
                }
                
                Ok(Response::new(ListFilesResponse {
                    success: true,
                    files: sync_files,
                    return_message: String::new(),
                }))
            },
            Err(e) => {
                error!("File list retrieval failed: {}", e);
                Err(Status::internal(format!("File list retrieval failed: {}", e)))
            }
        }
    }

    /// Delete file - internal implementation  
    pub async fn delete_file_internal(&self, request: Request<DeleteFileRequest>) -> Result<Response<DeleteFileResponse>, Status> {
        let req = request.into_inner();
        info!("File deletion request received:");
        info!("   account_hash: {}", req.account_hash);
        info!("   file_id: {}", req.file_id);
        info!("   file_path: {}", req.file_path);
        info!("   filename: {}", req.filename);
        info!("   revision: {}", req.revision);
        
        // Verify authentication
        if let Err(_) = auth::verify_auth_token(&self.app_state.oauth, &req.auth_token, &req.account_hash).await {
            return Ok(Response::new(response::file_delete_error("Authentication failed")));
        }
        
        // Find file ID
        let file_id = if req.file_id > 0 {
            match self.validate_file_for_deletion(req.file_id).await {
                Ok(id) => id,
                Err(resp) => return Ok(resp),
            }
        } else {
            match self.find_file_by_path(&req).await {
                Ok(id) => id,
                Err(resp) => return Ok(resp),
            }
        };

        // Delete file
        debug!("Executing file deletion: file_id={}", file_id);
        match self.app_state.file.delete_file(file_id).await {
            Ok(_) => {
                info!("File deleted successfully: filename={}, file_id={}", req.filename, file_id);
                Ok(Response::new(response::file_delete_success("File deleted successfully")))
            }
            Err(e) => {
                error!("File deletion failed: file_id={}, error={}", file_id, e);
                Ok(Response::new(response::file_delete_error(format!("File deletion failed: {}", e))))
            }
        }
    }
    
    /// Validate file for deletion
    async fn validate_file_for_deletion(&self, file_id: u64) -> Result<u64, Response<DeleteFileResponse>> {
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
    async fn find_file_by_path(&self, req: &DeleteFileRequest) -> Result<u64, Response<DeleteFileResponse>> {
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
        debug!("FindFileByCriteria request: account={}, file_path={}, file_name={}", 
               req.account_hash, req.file_path, req.file_name);
        
        // Verify authentication
        if let Err(_) = auth::verify_auth_token(&self.app_state.oauth, &req.auth_token, &req.account_hash).await {
            return Ok(Response::new(FindFileResponse {
                success: false,
                return_message: "Authentication failed".to_string(),
                file_id: 0,
                revision: 0,
                file_info: None,
            }));
        }
        
        // Convert client IDs to server IDs
        let (server_group_id, server_watcher_id) = if req.group_id > 0 && req.watcher_id > 0 {
            match self.app_state.storage.get_server_ids(&req.account_hash, req.group_id, req.watcher_id).await {
                Ok(Some((gid, wid))) => {
                    debug!("Converted client IDs (group={}, watcher={}) to server IDs (group={}, watcher={})", 
                           req.group_id, req.watcher_id, gid, wid);
                    (gid, wid)
                },
                Ok(None) => {
                    debug!("No matching group/watcher found for client IDs: group={}, watcher={}", 
                           req.group_id, req.watcher_id);
                    return Ok(Response::new(FindFileResponse {
                        success: false,
                        return_message: "Group or watcher not found".to_string(),
                        file_id: 0,
                        revision: 0,
                        file_info: None,
                    }));
                },
                Err(e) => {
                    error!("Failed to convert IDs: {}", e);
                    return Ok(Response::new(FindFileResponse {
                        success: false,
                        return_message: format!("Failed to convert IDs: {}", e),
                        file_id: 0,
                        revision: 0,
                        file_info: None,
                    }));
                }
            }
        } else {
            (0, 0) // No specific group/watcher filtering
        };
        
        // Normalize file path
        let normalized_file_path = crate::utils::helpers::normalize_path_preserve_tilde(&req.file_path);
        debug!("Find - Original path: {}, Normalized path: {}", req.file_path, normalized_file_path);
        
        match self.app_state.file.find_file_by_criteria(
            &req.account_hash,
            server_group_id,
            server_watcher_id,
            &normalized_file_path,
            &req.file_name,
        ).await {
            Ok(Some(file_info)) => {
                // File found - convert server IDs back to client IDs
                let proto_file_info = sync::FileInfo {
                    file_id: file_info.file_id,
                    filename: file_info.filename.clone(),
                    file_hash: file_info.file_hash.clone(),
                    device_hash: file_info.device_hash.clone(),
                    group_id: req.group_id,     // Return client group_id
                    watcher_id: req.watcher_id, // Return client watcher_id
                    is_encrypted: file_info.is_encrypted,
                    file_path: file_info.file_path.clone(),
                    updated_time: Some(prost_types::Timestamp {
                        seconds: file_info.updated_time.seconds,
                        nanos: 0,
                    }),
                    revision: file_info.revision,
                    file_size: file_info.size,
                };
                
                Ok(Response::new(FindFileResponse {
                    success: true,
                    return_message: "File found".to_string(),
                    file_id: file_info.file_id,
                    revision: file_info.revision,
                    file_info: Some(proto_file_info),
                }))
            },
            Ok(None) => {
                // File not found
                debug!("File not found: path={}, name={}", req.file_path, req.file_name);
                Ok(Response::new(FindFileResponse {
                    success: false,
                    return_message: "File not found".to_string(),
                    file_id: 0,
                    revision: 0,
                    file_info: None,
                }))
            },
            Err(e) => {
                // Search error
                error!("File search error: {}", e);
                Ok(Response::new(FindFileResponse {
                    success: false,
                    return_message: format!("File search error: {}", e),
                    file_id: 0,
                    revision: 0,
                    file_info: None,
                }))
            }
        }
    }

    /// Handle check file exists request
    pub async fn check_file_exists(&self, request: Request<CheckFileExistsRequest>) -> Result<Response<CheckFileExistsResponse>, Status> {
        let req = request.into_inner();
        debug!("CheckFileExists request: account={}, file_id={}", req.account_hash, req.file_id);
        
        // Verify authentication
        if let Err(_) = auth::verify_auth_token(&self.app_state.oauth, &req.auth_token, &req.account_hash).await {
            return Ok(Response::new(CheckFileExistsResponse {
                success: false,
                exists: false,
                is_deleted: false,
                return_message: "Authentication failed".to_string(),
            }));
        }

        // Check file existence
        match self.app_state.file.check_file_exists(req.file_id).await {
            Ok((exists, is_deleted)) => {
                debug!("File existence check: file_id={}, exists={}, is_deleted={}", req.file_id, exists, is_deleted);
                Ok(Response::new(CheckFileExistsResponse {
                    success: true,
                    exists,
                    is_deleted,
                    return_message: if exists {
                        if is_deleted {
                            "File exists but is deleted".to_string()
                        } else {
                            "File exists and is active".to_string()
                        }
                    } else {
                        "File does not exist".to_string()
                    },
                }))
            }
            Err(e) => {
                error!("Failed to check file existence: {}", e);
                Ok(Response::new(CheckFileExistsResponse {
                    success: false,
                    exists: false,
                    is_deleted: false,
                    return_message: format!("Failed to check file existence: {}", e),
                }))
            }
        }
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