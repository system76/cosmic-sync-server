use tonic::{Response, Status};
use tracing::{debug, error};

use crate::sync::{UploadFileRequest, UploadFileResponse};
use crate::utils::response;
// use crate::services::file_service::FileService; // not used directly
use super::super::file_handler::FileHandler;

pub async fn handle_upload_file(handler: &FileHandler, req: UploadFileRequest) -> Result<Response<UploadFileResponse>, Status> {
    // Log request details
    handler.log_upload_request(&req);

    // Validate file data size
    if req.file_data.len() != req.file_size as usize {
        error!("File data size mismatch: declared={}, actual= {}", req.file_size, req.file_data.len());
        return Ok(Response::new(response::file_upload_error("File data size mismatch")));
    }

    // 1. Verify authentication and normalize account_hash
    let verified = match handler.app_state.oauth.verify_token(&req.auth_token).await {
        Ok(v) if v.valid => v,
        _ => return Ok(Response::new(response::file_upload_error("Authentication failed")))
    };
    let server_account_hash = verified.account_hash;

    // 2. Validate input
    if let Err(msg) = handler.validate_upload_input(&req) {
        return Ok(Response::new(response::file_upload_error(msg)));
    }

    // 3. Normalize file path
    let normalized_file_path = match handler.normalize_file_path(&req.file_path) {
        Ok(path) => path,
        Err(msg) => return Ok(Response::new(response::file_upload_error(msg))),
    };

    // 4. Validate file path with watcher (single attempt)
    if let Err(msg) = handler.validate_file_path_with_watcher(&req, &normalized_file_path).await {
        debug!("Skipping strict watcher path validation: {}", msg);
    }

    // 5. Generate file ID
    let file_id = match handler.generate_file_id(&req) {
        Ok(id) => id,
        Err(msg) => return Ok(Response::new(response::file_upload_error(msg))),
    };

    // 6. Convert client IDs to server IDs via FileService helper
    let (server_group_id, server_watcher_id) = match handler.app_state.file
        .ensure_server_ids_for_upload(&server_account_hash, &req.device_hash, req.group_id, req.watcher_id, Some(&normalized_file_path))
        .await {
        Ok(ids) => ids,
        Err(e) => {
            error!("Failed to ensure server IDs: {}", e);
            return Ok(Response::new(response::file_upload_error(format!("Failed to ensure server IDs: {}", e))));
        }
    };

    // 7. Create file info with server IDs
    let mut req_server = req.clone();
    req_server.account_hash = server_account_hash.clone();
    let file_info = handler.app_state.file.build_file_info_for_upload(&req_server, file_id, normalized_file_path, server_group_id, server_watcher_id);

    // 8. Store file via FileService
    match handler.app_state.file.store_file(&file_info, &req.file_data).await {
        Ok(_) => Ok(Response::new(response::file_upload_success(file_id, req.revision + 1))),
        Err(e) => {
            error!("File storage failed: {}", e);
            Ok(Response::new(response::file_upload_error(&format!("File storage failed: {}", e))))
        }
    }
}


