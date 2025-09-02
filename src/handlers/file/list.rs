use tonic::{Response, Status};
use tracing::{debug, error, info, warn};

use crate::sync::{ListFilesRequest, ListFilesResponse};
use crate::utils::time::timestamp_to_datetime;
use super::super::file_handler::FileHandler;
use base64::Engine as _;

fn parse_account_key(s: &str) -> Option<[u8;32]> {
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(s) {
        if bytes.len() == 32 {
            return bytes.try_into().ok();
        }
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s) {
        if bytes.len() == 32 {
            return bytes.try_into().ok();
        }
    }
    None
}

pub async fn handle_list_files(handler: &FileHandler, req: ListFilesRequest) -> Result<Response<ListFilesResponse>, Status> {
    debug!("File list request: account={}, device={}, group_id={}, time_filter={:?}", req.account_hash, req.device_hash, req.group_id, req.upload_time_from);

    // Verify authentication
    match handler.app_state.oauth.verify_token(&req.auth_token).await {
        Ok(v) if v.valid => {}
        _ => {
            return Ok(Response::new(ListFilesResponse {
                success: false,
                files: Vec::new(),
                return_message: "Authentication failed".to_string(),
            }));
        }
    }

    // Warn if storage backend is memory (diagnostic)
    if handler.app_state.storage.as_any().downcast_ref::<crate::storage::memory::MemoryStorage>().is_some() {
        warn!("ListFiles using in-memory storage backend - data may appear empty if previous uploads were to MySQL");
    }

    // Convert client group_id to server group_id via FileService
    let server_group_id = match handler.app_state.file.convert_client_group_to_server(&req.account_hash, req.group_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            debug!("No matching group found for client group_id={}", req.group_id);
            return Ok(Response::new(ListFilesResponse {
                success: true,
                files: Vec::new(),
                return_message: "No files found".to_string(),
            }));
        }
        Err(e) => {
            error!("Failed to convert group_id: {}", e);
            return Err(Status::internal(format!("Failed to convert group_id: {}", e)));
        }
    };

    debug!("Converted client group_id={} to server group_id={}", req.group_id, server_group_id);

    // Time filter
    let time_filter = req.upload_time_from.map(|ts| timestamp_to_datetime(&ts));
    if let Some(ref filter_time) = time_filter {
        info!("🔍 Recovery sync requested: filtering files updated since {}", filter_time);
    }

    // Try to get account transport encryption key (for path/name)
    let account_key: Option<[u8;32]> = if handler.app_state.config.features.transport_encrypt_metadata {
        match handler.app_state.storage.get_encryption_key(&req.account_hash).await {
            Ok(Some(k)) => parse_account_key(&k),
            _ => None,
        }
    } else { None };
    let aad = format!("{}:{}", req.account_hash, req.device_hash);

    // Get filtered files using server group_id
    match handler.app_state.file.list_files_filtered_by_device(server_group_id, &req.account_hash, &req.device_hash).await {
        Ok(files) => {
            let mut sync_files = Vec::new();
            let mut files_processed = 0;
            let mut files_filtered = 0;

            for file in files.iter() {
                files_processed += 1;

                if let Some(ref filter_time) = time_filter {
                    let file_updated_time = timestamp_to_datetime(&prost_types::Timestamp { seconds: file.updated_time.seconds, nanos: 0 });
                    if file_updated_time <= *filter_time {
                        files_filtered += 1;
                        continue;
                    }
                    debug!("🔄 Including file {} (updated: {}) in recovery sync", file.filename, file_updated_time);
                }

                let client_watcher_id = match handler.app_state.storage.get_client_watcher_id(&req.account_hash, server_group_id, file.watcher_id).await {
                    Ok(Some((_, watcher_id))) => watcher_id,
                    Ok(None) => {
                        warn!("No client watcher_id found for server watcher_id={}", file.watcher_id);
                        file.watcher_id
                    }
                    Err(e) => {
                        error!("Failed to get client watcher_id: {}", e);
                        file.watcher_id
                    }
                };

                // Encrypt path/name for transport if account key is available
                let (enc_path, enc_name) = if let Some(key) = account_key.as_ref() {
                    let ct_path = crate::utils::crypto::aead_encrypt(key, file.file_path.as_bytes(), aad.as_bytes());
                    let ct_name = crate::utils::crypto::aead_encrypt(key, file.filename.as_bytes(), aad.as_bytes());
                    (
                        base64::engine::general_purpose::STANDARD_NO_PAD.encode(ct_path),
                        base64::engine::general_purpose::STANDARD_NO_PAD.encode(ct_name),
                    )
                } else {
                    (file.file_path.clone(), file.filename.clone())
                };

                let file_info = crate::sync::FileInfo {
                    file_id: file.file_id,
                    filename: enc_name,
                    file_hash: file.file_hash.clone(),
                    device_hash: file.device_hash.clone(),
                    group_id: req.group_id,
                    watcher_id: client_watcher_id,
                    is_encrypted: file.is_encrypted,
                    file_path: enc_path,
                    updated_time: Some(prost_types::Timestamp { seconds: file.updated_time.seconds, nanos: 0 }),
                    revision: file.revision,
                    file_size: file.size,
                };
                sync_files.push(file_info);
            }

            if let Some(_) = time_filter {
                info!("📊 Recovery sync results: {} files processed, {} filtered out, {} files returned", files_processed, files_filtered, sync_files.len());
            } else {
                debug!("📋 File list: {} files returned", sync_files.len());
            }

            Ok(Response::new(ListFilesResponse { success: true, files: sync_files, return_message: String::new() }))
        }
        Err(e) => {
            error!("File list retrieval failed: {}", e);
            Err(Status::internal(format!("File list retrieval failed: {}", e)))
        }
    }
}


